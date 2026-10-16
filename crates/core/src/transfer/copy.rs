//! Pipelined copy of one file: up to `IN_FLIGHT` read+write pairs run at once.

use futures::{StreamExt, stream};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::side::{Reader, Writer};
use crate::error::CoreError;

/// OpenSSH's sftp-server answers at most 261 120 bytes per read; 64 KiB stays well below.
pub(crate) const CHUNK: usize = 64 * 1024;
/// 2 MiB in flight per session (shared by all files through the queue's semaphore): enough to fill
/// long links, and few enough that a queued request is answered within the SFTP request timeout
/// even on a slow upstream.
pub(crate) const IN_FLIGHT: usize = 32;

/// Copies `[from, size)` and returns the end offset. `progress` sees the contiguous prefix that is
/// confirmed written; on error that prefix comes back with the error (resume point).
pub(crate) async fn copy_range(
    reader: &dyn Reader,
    writer: &dyn Writer,
    from: u64,
    size: u64,
    limit: &Semaphore,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(u64) + Send),
) -> std::result::Result<u64, (CoreError, u64)> {
    let mut done = from;
    if cancel.is_cancelled() {
        return Err((CoreError::Cancelled, done));
    }
    // `buffered` runs the pairs concurrently but yields them in order: each yielded pair extends
    // the contiguous prefix.
    let mut pairs = stream::iter((from..size).step_by(CHUNK))
        .map(|offset| async move {
            let _permit = limit.acquire().await.map_err(|_| CoreError::Cancelled)?;
            let len = (size - offset).min(CHUNK as u64) as usize;
            let data = reader.read_at(offset, len).await?;
            let read = data.len();
            if read > 0 {
                writer.write_at(offset, data).await?;
            }
            Ok::<_, CoreError>((offset, read, len))
        })
        .buffered(IN_FLIGHT);
    loop {
        let next = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err((CoreError::Cancelled, done)),
            next = pairs.next() => next,
        };
        match next {
            None => return Ok(done),
            Some(Ok((offset, read, len))) => {
                done = offset + read as u64;
                progress(done);
                if read < len {
                    // The source ended early (it shrank): stop here.
                    return Ok(done);
                }
            }
            Some(Err(error)) => return Err((error, done)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::transfer::{LocalSide, Side};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::Semaphore;

    fn p(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_string()
    }

    async fn copy_file(data: &[u8], from: u64) -> (Vec<u8>, Vec<u64>, std::result::Result<u64, (CoreError, u64)>) {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, data).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, from > 0).await.unwrap();
        let mut seen = Vec::new();
        let result = copy_range(
            &*r,
            &*w,
            from,
            data.len() as u64,
            &Semaphore::new(IN_FLIGHT),
            &CancellationToken::new(),
            &mut |d| seen.push(d),
        )
        .await;
        w.close().await.unwrap();
        (std::fs::read(&dst).unwrap(), seen, result)
    }

    #[tokio::test]
    async fn copies_multi_chunk_files_and_reports_a_rising_watermark() {
        let data: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| (i % 251) as u8).collect();
        let (out, seen, result) = copy_file(&data, 0).await;
        assert_eq!(out, data);
        assert_eq!(result.unwrap(), data.len() as u64);
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert_eq!(*seen.last().unwrap(), data.len() as u64);
    }

    #[tokio::test]
    async fn empty_files_copy_nothing() {
        let (out, seen, result) = copy_file(b"", 0).await;
        assert!(out.is_empty() && seen.is_empty());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn resumes_from_an_offset_reading_only_the_rest() {
        struct Counting<'a>(&'a dyn Reader, AtomicU64);
        #[async_trait::async_trait]
        impl Reader for Counting<'_> {
            async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
                let bytes = self.0.read_at(offset, len).await?;
                self.1.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                Ok(bytes)
            }
            async fn close(&self) {}
        }
        let d = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(CHUNK * 2 + 5)).map(|i| (i % 13) as u8).collect();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, &data).unwrap();
        std::fs::write(&dst, &data[..CHUNK]).unwrap();
        let side = LocalSide;
        let inner = side.open_read(&src).await.unwrap();
        let r = Counting(&*inner, AtomicU64::new(0));
        let w = side.open_write(&dst, true).await.unwrap();
        let done = copy_range(
            &r,
            &*w,
            CHUNK as u64,
            data.len() as u64,
            &Semaphore::new(IN_FLIGHT),
            &CancellationToken::new(),
            &mut |_| {},
        )
        .await;
        assert_eq!(done.unwrap(), data.len() as u64);
        assert_eq!(r.1.load(Ordering::SeqCst), (data.len() - CHUNK) as u64);
        assert_eq!(std::fs::read(&dst).unwrap(), data);
    }

    #[tokio::test]
    async fn copy_stops_at_a_short_read() {
        // The source shrank after its size was taken: copy what is there, report it, do not hang.
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, vec![7u8; CHUNK + 10]).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, false).await.unwrap();
        let done = copy_range(
            &*r,
            &*w,
            0,
            (CHUNK * 4) as u64,
            &Semaphore::new(IN_FLIGHT),
            &CancellationToken::new(),
            &mut |_| {},
        )
        .await;
        assert_eq!(done.unwrap(), (CHUNK + 10) as u64);
    }

    #[tokio::test]
    async fn a_cancel_returns_cancelled_with_the_watermark() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, vec![1u8; CHUNK * 8]).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, false).await.unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = copy_range(&*r, &*w, 0, (CHUNK * 8) as u64, &Semaphore::new(IN_FLIGHT), &cancel, &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(err, (CoreError::Cancelled, 0));
    }

    #[tokio::test]
    async fn parallel_files_share_one_in_flight_limit() {
        // Requests queued in russh-sftp start their timeout at once: the session-wide limit keeps a
        // slow link from timing out the tail of 4 files x 32 chunks.
        struct Tracking<'a>(&'a dyn Reader, Arc<(AtomicU64, AtomicU64)>);
        #[async_trait::async_trait]
        impl Reader for Tracking<'_> {
            async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
                let now = self.1.0.fetch_add(1, Ordering::SeqCst) + 1;
                self.1.1.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                let bytes = self.0.read_at(offset, len).await;
                self.1.0.fetch_sub(1, Ordering::SeqCst);
                bytes
            }
            async fn close(&self) {}
        }
        let d = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(CHUNK * 10)).map(|i| (i % 7) as u8).collect();
        let side = LocalSide;
        let limit = Semaphore::new(3);
        let counts = Arc::new((AtomicU64::new(0), AtomicU64::new(0)));
        let mut copies = Vec::new();
        for name in ["a", "b"] {
            let (src, dst) = (p(&d.path().join(format!("{name}.s"))), p(&d.path().join(name)));
            std::fs::write(&src, &data).unwrap();
            copies.push((side.open_read(&src).await.unwrap(), side.open_write(&dst, false).await.unwrap(), dst));
        }
        let (ra, rb) = (Tracking(&*copies[0].0, counts.clone()), Tracking(&*copies[1].0, counts.clone()));
        let cancel = CancellationToken::new();
        let size = data.len() as u64;
        let (mut pa, mut pb) = (|_| {}, |_| {});
        let (a, b) = tokio::join!(
            copy_range(&ra, &*copies[0].1, 0, size, &limit, &cancel, &mut pa),
            copy_range(&rb, &*copies[1].1, 0, size, &limit, &cancel, &mut pb),
        );
        assert_eq!((a.unwrap(), b.unwrap()), (size, size));
        assert!(counts.1.load(Ordering::SeqCst) <= 3, "peak {}", counts.1.load(Ordering::SeqCst));
        for (_, w, dst) in &copies {
            w.close().await.unwrap();
            assert!(std::fs::read(dst).unwrap() == data);
        }
    }
}

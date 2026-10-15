//! Local terminal session backed by a pseudo terminal (portable-pty).

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::{Mutex, mpsc};

use tokio::sync::oneshot;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{OutputSink, Session, SessionEvent, panicked};
use crate::error::{CoreError, Result};

const FALLBACK_SHELL: &str = "/bin/bash";

/// Picks the login shell: `$SHELL` if set and non-empty, else `/bin/bash`.
pub(crate) fn default_shell(shell_env: Option<String>) -> String {
    match shell_env {
        Some(s) if !s.trim().is_empty() => s,
        _ => FALLBACK_SHELL.to_string(),
    }
}

fn pty_error(context: &str, err: impl std::fmt::Display) -> CoreError {
    CoreError::Io { message: format!("{context}: {err}") }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// One queued input chunk plus the channel that receives the outcome of writing it.
type WriteJob = (Vec<u8>, oneshot::Sender<io::Result<()>>);

/// Waits until `fd` reports one of `events`, or `wake` (if any) becomes readable or hung up.
/// Returns `false` when woken.
fn poll(fd: &File, events: libc::c_short, wake: Option<&OwnedFd>) -> io::Result<bool> {
    let mut fds = [
        libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 },
        libc::pollfd { fd: wake.map_or(-1, |w| w.as_raw_fd()), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        // SAFETY: `fds` is a valid array of two pollfd structs for the duration of the call.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } >= 0 {
            return Ok(fds[1].revents == 0);
        }
        let err = io::Error::last_os_error();
        if err.kind() != ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// Writes all of `data` to the non-blocking pty, waiting for room in between. Gives up as soon
/// as `wake` fires (the session was closed): a pty nobody reads would block a write forever.
fn write_until_closed(pty: &File, wake: &OwnedFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match (&*pty).write(data) {
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if !poll(pty, libc::POLLOUT, Some(wake))? {
                    return Err(io::Error::new(ErrorKind::BrokenPipe, "terminal closed"));
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Own handle on the pty master, switched to non-blocking mode (shared by all handles of it).
fn nonblocking_handle(master: &dyn MasterPty) -> Result<File> {
    let fd = master.as_raw_fd().ok_or_else(|| pty_error("cannot use pty", "no file descriptor"))?;
    // SAFETY: `fd` belongs to `master`, which outlives this call; it is duplicated right away.
    let owned =
        unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().map_err(|e| pty_error("cannot use pty", e))?;
    // SAFETY: plain fcntl calls on a valid descriptor.
    let ok = unsafe {
        let flags = libc::fcntl(owned.as_raw_fd(), libc::F_GETFL);
        flags >= 0 && libc::fcntl(owned.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
    };
    if !ok {
        return Err(pty_error("cannot use pty", io::Error::last_os_error()));
    }
    Ok(File::from(owned))
}

/// A pipe whose write end is dropped to wake the writer thread.
fn wake_pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors pipe2 writes.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(pty_error("cannot create pipe", io::Error::last_os_error()));
    }
    // SAFETY: pipe2 succeeded, so both descriptors are open and owned by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// A shell running in a local pseudo terminal.
pub struct LocalPtySession {
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// Queue of the dedicated writer thread. Blocking pty writes never run on an async worker,
    /// and the single queue keeps the input order of one session.
    writer: mpsc::Sender<WriteJob>,
    /// Write end of the wake pipe; dropped on close so a stalled write gives up.
    closing: Mutex<Option<OwnedFd>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl LocalPtySession {
    /// Spawns `$SHELL` (fallback `/bin/bash`) in the home directory with
    /// `TERM=xterm-256color`.
    pub fn spawn(cols: u16, rows: u16, sink: OutputSink) -> Result<Self> {
        Self::spawn_shell(&default_shell(std::env::var("SHELL").ok()), cols, rows, sink)
    }

    /// Like `spawn` with a given shell (hermetic tests use `/bin/sh`).
    pub fn spawn_shell(shell: &str, cols: u16, rows: u16, sink: OutputSink) -> Result<Self> {
        let mut cmd = CommandBuilder::new(shell);
        if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }
        Self::spawn_with_command(cmd, cols, rows, sink)
    }

    /// Spawns an arbitrary command in a new PTY. Used by `spawn` and tests.
    pub(crate) fn spawn_with_command(mut cmd: CommandBuilder, cols: u16, rows: u16, sink: OutputSink) -> Result<Self> {
        cmd.env("TERM", "xterm-256color");
        let pair = native_pty_system().openpty(pty_size(cols, rows)).map_err(|e| pty_error("cannot open pty", e))?;
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| pty_error("cannot spawn shell", e))?;
        // Close our copy of the slave so EOF is seen once the child exits.
        drop(pair.slave);

        let reader = nonblocking_handle(pair.master.as_ref())?;
        let writer = reader.try_clone().map_err(|e| pty_error("cannot write pty", e))?;
        let (wake, closing) = wake_pipe()?;
        let killer = child.clone_killer();

        let (writer_tx, writer_rx) = mpsc::channel::<WriteJob>();
        std::thread::Builder::new()
            .name("linascp-pty-writer".into())
            .spawn(move || {
                // Ends once the session (the only sender) is dropped.
                while let Ok((data, reply)) = writer_rx.recv() {
                    let _ = reply.send(write_until_closed(&writer, &wake, &data));
                }
            })
            .map_err(|e| pty_error("cannot start writer thread", e))?;

        std::thread::Builder::new()
            .name("linascp-pty-reader".into())
            .spawn(move || {
                let forwarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut buf = [0u8; 8192];
                    loop {
                        match (&reader).read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => sink(SessionEvent::Output(buf[..n].to_vec())),
                            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                                if poll(&reader, libc::POLLIN, None).is_err() {
                                    break;
                                }
                            }
                            // EIO on Linux once the slave side is closed.
                            Err(_) => break,
                        }
                    }
                }));
                if forwarded.is_err() {
                    // The consumer drops the session on this event, which kills the shell. If the
                    // sink panics again there is no way left to report; still reap the child.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        sink(SessionEvent::Error(panicked()));
                    }));
                    let _ = child.wait();
                    return;
                }
                let code = child.wait().ok().map(|status| status.exit_code());
                sink(SessionEvent::Exit { code });
            })
            .map_err(|e| pty_error("cannot start reader thread", e))?;

        Ok(Self {
            master: Mutex::new(pair.master),
            writer: writer_tx,
            closing: Mutex::new(Some(closing)),
            killer: Mutex::new(killer),
        })
    }

    /// Kills the shell and wakes a stalled write.
    fn shut_down(&self) {
        self.closing.lock().unwrap_or_else(|e| e.into_inner()).take();
        // Fails with ESRCH if the child already exited; that is fine.
        let _ = self.killer.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

#[async_trait::async_trait]
impl Session for LocalPtySession {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let closed = || CoreError::Io { message: "terminal input is closed".into() };
        if self.closing.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            return Err(closed());
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.writer.send((data.to_vec(), reply_tx)).map_err(|_| closed())?;
        reply_rx.await.map_err(|_| closed())??;
        Ok(())
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner());
        master.resize(pty_size(cols, rows)).map_err(|e| pty_error("cannot resize pty", e))
    }

    async fn close(&self) -> Result<()> {
        self.shut_down();
        Ok(())
    }
}

impl Drop for LocalPtySession {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;

    fn channel_sink() -> (OutputSink, mpsc::UnboundedReceiver<SessionEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let sink: OutputSink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        (sink, rx)
    }

    #[test]
    fn default_shell_uses_env_or_bash() {
        assert_eq!(default_shell(Some("/usr/bin/zsh".into())), "/usr/bin/zsh");
        assert_eq!(default_shell(Some("  ".into())), "/bin/bash");
        assert_eq!(default_shell(None), "/bin/bash");
    }

    #[tokio::test]
    async fn local_pty_echo_resize_close() {
        let (sink, mut rx) = channel_sink();
        let session =
            LocalPtySession::spawn_with_command(CommandBuilder::new("/bin/sh"), 80, 24, sink).expect("spawn /bin/sh");

        // The quotes keep the typed command (echoed back by the tty) from
        // matching; only the command output contains "hello".
        session.write(b"echo hel\"\"lo\n").await.unwrap();
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Output(bytes) = event {
                    output.extend_from_slice(&bytes);
                    if String::from_utf8_lossy(&output).contains("hello") {
                        break;
                    }
                }
            }
        })
        .await
        .expect("output contains hello");

        session.resize(120, 40).await.expect("resize");

        session.close().await.unwrap();
        let exited = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Exit { .. } = event {
                    return true;
                }
            }
            false
        })
        .await
        .expect("exit within timeout");
        assert!(exited);
    }

    #[tokio::test]
    async fn panic_in_the_reader_is_reported_once_as_error() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sink: OutputSink = Arc::new(move |event| {
            if matches!(event, SessionEvent::Output(_)) {
                panic!("sink bug");
            }
            let _ = tx.send(event);
        });
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "echo hi; sleep 1"]);
        let _session = LocalPtySession::spawn_with_command(cmd, 80, 24, sink).expect("spawn");

        let first = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("event in time");
        assert!(matches!(first, Some(SessionEvent::Error(CoreError::Internal { .. }))), "{first:?}");
        assert!(
            tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.unwrap().is_none(),
            "nothing after the error"
        );
    }

    /// A process that never reads stdin fills the pty input buffer; the write must then wait off the
    /// async runtime instead of blocking its (here: only) worker thread.
    #[test]
    fn stalled_pty_write_does_not_block_the_runtime() {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let (sink, _rx) = channel_sink();
                let mut cmd = CommandBuilder::new("sleep");
                cmd.arg("30");
                let session = Arc::new(LocalPtySession::spawn_with_command(cmd, 80, 24, sink).unwrap());
                let writer = session.clone();
                let big = b"xxxxxxxxxxxxxxx\n".repeat(256 * 1024);
                let pending = tokio::spawn(async move { writer.write(&big).await });
                // Only fires if the runtime thread is not stuck inside the write.
                tokio::time::sleep(Duration::from_millis(300)).await;
                session.close().await.unwrap();
                pending.abort();
            });
            let _ = done_tx.send(());
        });
        assert!(done_rx.recv_timeout(Duration::from_secs(15)).is_ok(), "pty write blocked the runtime");
    }

    /// A write stuck on a full pty must end (with an error) once the session is closed, so the
    /// writer thread is not parked forever.
    #[tokio::test]
    async fn stalled_write_fails_once_the_session_is_closed() {
        let (sink, _rx) = channel_sink();
        let mut cmd = CommandBuilder::new("sleep");
        cmd.arg("30");
        let session = Arc::new(LocalPtySession::spawn_with_command(cmd, 80, 24, sink).unwrap());
        let writer = session.clone();
        let big = b"xxxxxxxxxxxxxxx\n".repeat(256 * 1024);
        let pending = tokio::spawn(async move { writer.write(&big).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!pending.is_finished(), "the write should be stalled on the full pty");

        session.close().await.unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), pending).await.expect("write ends after close");
        assert!(matches!(result.unwrap(), Err(CoreError::Io { .. })));
        assert!(session.write(b"late").await.is_err(), "writes after close fail fast");
    }

    /// Writes issued concurrently reach the pty in the order they were issued.
    #[test]
    fn concurrent_writes_keep_their_order() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let (sink, _rx) = channel_sink();
            let mut cmd = CommandBuilder::new("/bin/sh");
            cmd.args(["-c", &format!("cat > '{}'", out.display())]);
            let session = Arc::new(LocalPtySession::spawn_with_command(cmd, 80, 24, sink).unwrap());
            let handles: Vec<_> = (0..200)
                .map(|i| {
                    let session = session.clone();
                    tokio::spawn(async move { session.write(format!("line-{i}\n").as_bytes()).await })
                })
                .collect();
            for handle in handles {
                handle.await.unwrap().unwrap();
            }
            let expected: String = (0..200).map(|i| format!("line-{i}\n")).collect();
            let got = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let got = std::fs::read_to_string(&out).unwrap_or_default();
                    if got.len() >= expected.len() {
                        return got;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("cat wrote all lines");
            assert_eq!(got, expected);
            session.close().await.unwrap();
        });
    }

    #[tokio::test]
    async fn local_pty_reports_exit_code() {
        let (sink, mut rx) = channel_sink();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "exit 3"]);
        let _session = LocalPtySession::spawn_with_command(cmd, 80, 24, sink).unwrap();
        let code = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = rx.recv().await {
                if let SessionEvent::Exit { code } = event {
                    return code;
                }
            }
            None
        })
        .await
        .expect("exit within timeout");
        assert_eq!(code, Some(3));
    }
}

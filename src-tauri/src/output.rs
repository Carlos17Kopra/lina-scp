//! Terminal output transport: coalescing (8 ms / 64 KiB) and ack-based flow control.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};

use crate::util::lock;

/// A pending chunk is emitted as soon as it reaches this size.
pub const MAX_CHUNK: usize = 64 * 1024;
/// Pending output is flushed at least this often.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(8);
/// The session reader is paused when this many bytes are unacknowledged by the UI …
pub const HIGH_WATERMARK: u64 = 1024 * 1024;
/// … and resumed once the unacknowledged bytes dropped to this value.
pub const LOW_WATERMARK: u64 = 256 * 1024;

/// Raw terminal bytes, sent through a `Channel` without JSON encoding (JS receives an `ArrayBuffer`).
pub struct RawChunk(pub Vec<u8>);

impl IpcResponse for RawChunk {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl specta::Type for RawChunk {
    fn definition(_types: &mut specta::Types) -> specta::datatype::DataType {
        specta::datatype::DataType::Reference(specta_typescript::define("ArrayBuffer"))
    }
}

/// Coalesces small writes into larger chunks.
#[derive(Debug, Default)]
pub struct OutputBuffer {
    buf: Vec<u8>,
}

impl OutputBuffer {
    /// Appends `data`; returns the whole buffer once it reached `MAX_CHUNK`.
    pub fn push(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.buf.extend_from_slice(data);
        (self.buf.len() >= MAX_CHUNK).then(|| std::mem::take(&mut self.buf))
    }

    /// Takes everything pending; `None` when empty.
    pub fn take(&mut self) -> Option<Vec<u8>> {
        (!self.buf.is_empty()).then(|| std::mem::take(&mut self.buf))
    }
}

/// Hysteresis between the high and the low watermark.
#[derive(Debug, Clone, Copy)]
pub struct Watermark {
    unacked: u64,
    paused: bool,
    high: u64,
    low: u64,
}

impl Watermark {
    pub fn new(high: u64, low: u64) -> Self {
        Self { unacked: 0, paused: false, high, low }
    }

    pub fn sent(&mut self, bytes: u64) {
        self.unacked += bytes;
        if self.unacked >= self.high {
            self.paused = true;
        }
    }

    pub fn acked(&mut self, bytes: u64) {
        self.unacked = self.unacked.saturating_sub(bytes);
        if self.paused && self.unacked <= self.low {
            self.paused = false;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn unacked(&self) -> u64 {
        self.unacked
    }
}

struct GateState {
    mark: Watermark,
    closed: bool,
}

/// Thread-blocking wrapper around [`Watermark`].
pub struct FlowGate {
    state: Mutex<GateState>,
    resumed: Condvar,
}

impl FlowGate {
    pub fn new(high: u64, low: u64) -> Self {
        Self {
            state: Mutex::new(GateState { mark: Watermark::new(high, low), closed: false }),
            resumed: Condvar::new(),
        }
    }

    pub fn sent(&self, bytes: u64) {
        lock(&self.state).mark.sent(bytes);
    }

    pub fn ack(&self, bytes: u64) {
        lock(&self.state).mark.acked(bytes);
        self.resumed.notify_all();
    }

    pub fn close(&self) {
        lock(&self.state).closed = true;
        self.resumed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        lock(&self.state).mark.is_paused()
    }

    pub fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    /// Blocks the calling thread while paused and not closed.
    pub fn wait_until_open(&self) {
        let mut state = lock(&self.state);
        while state.mark.is_paused() && !state.closed {
            state = self.resumed.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

pub type ChunkSender = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Per-session output pipeline: session reader → buffer → IPC channel, with back-pressure.
pub struct OutputPump {
    buffer: Mutex<OutputBuffer>,
    send: ChunkSender,
    gate: FlowGate,
}

impl OutputPump {
    pub fn new(send: ChunkSender) -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(OutputBuffer::default()),
            send,
            gate: FlowGate::new(HIGH_WATERMARK, LOW_WATERMARK),
        })
    }

    /// Pump writing into a raw IPC channel, flushed by a timer. Must be called inside a Tokio runtime.
    pub fn for_channel(channel: Channel<RawChunk>) -> Arc<Self> {
        let pump = Self::new(Arc::new(move |chunk: Vec<u8>| {
            if let Err(error) = channel.send(RawChunk(chunk)) {
                tracing::debug!(%error, "terminal output dropped: channel closed");
            }
        }));
        pump.start_ticker();
        pump
    }

    /// Spawns the periodic flush task; it ends when the pump is closed or dropped.
    pub fn start_ticker(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(pump) = weak.upgrade() else { break };
                pump.flush();
                if pump.is_closed() {
                    break;
                }
            }
        });
    }

    /// Called by the session reader for every output chunk. Blocks while the UI is too far behind.
    pub fn push(&self, data: &[u8]) {
        {
            let mut buffer = lock(&self.buffer);
            if let Some(chunk) = buffer.push(data) {
                self.emit(chunk);
            }
        }
        self.wait_for_capacity();
    }

    pub fn flush(&self) {
        let mut buffer = lock(&self.buffer);
        if let Some(chunk) = buffer.take() {
            self.emit(chunk);
        }
    }

    /// The UI consumed `bytes` (xterm write callback).
    pub fn ack(&self, bytes: u64) {
        self.gate.ack(bytes);
    }

    /// Flushes what is pending and releases a blocked reader for good.
    pub fn close(&self) {
        self.flush();
        self.gate.close();
    }

    pub fn is_closed(&self) -> bool {
        self.gate.is_closed()
    }

    /// Called with the buffer lock held, so chunks can never overtake each other.
    fn emit(&self, chunk: Vec<u8>) {
        self.gate.sent(chunk.len() as u64);
        (self.send)(chunk);
    }

    fn wait_for_capacity(&self) {
        if !self.gate.is_paused() {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| self.gate.wait_until_open())
            }
            // Blocking a current-thread runtime would dead-lock it; skip back-pressure there.
            Ok(_) => tracing::debug!("output flow control skipped on a current-thread runtime"),
            Err(_) => self.gate.wait_until_open(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    type Chunks = Arc<Mutex<Vec<Vec<u8>>>>;

    fn collecting_pump() -> (Arc<OutputPump>, Chunks) {
        let chunks: Chunks = Arc::default();
        let sink = chunks.clone();
        let pump = OutputPump::new(Arc::new(move |chunk: Vec<u8>| sink.lock().unwrap().push(chunk)));
        (pump, chunks)
    }

    fn wait_for(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(start.elapsed() < Duration::from_secs(5), "condition not met within 5 s");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn buffer_coalesces_until_max_chunk() {
        let mut buffer = OutputBuffer::default();
        assert_eq!(buffer.push(b"abc"), None);
        assert_eq!(buffer.push(&vec![0u8; MAX_CHUNK - 4]), None);
        let chunk = buffer.push(b"z").expect("a full chunk");
        assert_eq!(chunk.len(), MAX_CHUNK);
        assert_eq!(buffer.take(), None);
        buffer.push(b"tail");
        assert_eq!(buffer.take(), Some(b"tail".to_vec()));
    }

    #[test]
    fn watermark_pauses_at_high_and_resumes_at_low() {
        let mut mark = Watermark::new(100, 25);
        mark.sent(60);
        assert!(!mark.is_paused());
        mark.sent(40);
        assert!(mark.is_paused());
        mark.acked(50);
        assert!(mark.is_paused(), "50 unacked bytes are still above the low watermark");
        mark.acked(25);
        assert!(!mark.is_paused());
        assert_eq!(mark.unacked(), 25);
        mark.acked(1_000);
        assert_eq!(mark.unacked(), 0);
    }

    #[test]
    fn gate_blocks_until_enough_bytes_are_acked() {
        let gate = Arc::new(FlowGate::new(10, 2));
        gate.sent(10);
        let done = Arc::new(AtomicBool::new(false));
        let waiter = {
            let (gate, done) = (gate.clone(), done.clone());
            std::thread::spawn(move || {
                gate.wait_until_open();
                done.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!done.load(Ordering::SeqCst));
        gate.ack(5);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!done.load(Ordering::SeqCst), "5 unacked bytes are still above the low watermark");
        gate.ack(3);
        waiter.join().unwrap();
        assert!(done.load(Ordering::SeqCst));
    }

    #[test]
    fn closing_the_gate_releases_waiters() {
        let gate = Arc::new(FlowGate::new(10, 2));
        gate.sent(50);
        let waiter = {
            let gate = gate.clone();
            std::thread::spawn(move || gate.wait_until_open())
        };
        std::thread::sleep(Duration::from_millis(20));
        gate.close();
        waiter.join().unwrap();
        assert!(gate.is_closed());
    }

    #[test]
    fn pump_emits_full_chunks_immediately_and_small_ones_on_flush() {
        let (pump, chunks) = collecting_pump();
        pump.push(&vec![1u8; MAX_CHUNK]);
        assert_eq!(chunks.lock().unwrap().len(), 1);
        pump.push(b"x");
        assert_eq!(chunks.lock().unwrap().len(), 1);
        pump.flush();
        assert_eq!(chunks.lock().unwrap()[1], b"x".to_vec());
    }

    #[test]
    fn close_flushes_pending_output() {
        let (pump, chunks) = collecting_pump();
        pump.push(b"bye");
        pump.close();
        assert_eq!(*chunks.lock().unwrap(), vec![b"bye".to_vec()]);
        assert!(pump.is_closed());
    }

    #[test]
    fn pump_blocks_the_reader_above_the_high_watermark_until_acked() {
        let (pump, chunks) = collecting_pump();
        let chunks_per_high_watermark = HIGH_WATERMARK as usize / MAX_CHUNK;
        let reader = {
            let pump = pump.clone();
            std::thread::spawn(move || {
                for _ in 0..=chunks_per_high_watermark {
                    pump.push(&vec![7u8; MAX_CHUNK]);
                }
            })
        };
        wait_for(|| chunks.lock().unwrap().len() == chunks_per_high_watermark);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(chunks.lock().unwrap().len(), chunks_per_high_watermark, "reader must be paused");
        assert!(!reader.is_finished());

        pump.ack(HIGH_WATERMARK - LOW_WATERMARK);
        reader.join().unwrap();
        assert_eq!(chunks.lock().unwrap().len(), chunks_per_high_watermark + 1);
    }

    #[tokio::test(start_paused = true)]
    async fn ticker_flushes_small_output() {
        let (pump, chunks) = collecting_pump();
        pump.start_ticker();
        pump.push(b"prompt$ ");
        assert!(chunks.lock().unwrap().is_empty());
        tokio::time::sleep(FLUSH_INTERVAL * 2).await;
        assert_eq!(*chunks.lock().unwrap(), vec![b"prompt$ ".to_vec()]);
    }

    #[test]
    fn raw_chunk_is_sent_without_json_encoding() {
        let body = RawChunk(vec![1, 2, 3]).body().unwrap();
        assert!(matches!(body, InvokeResponseBody::Raw(ref bytes) if bytes == &vec![1, 2, 3]));
    }
}

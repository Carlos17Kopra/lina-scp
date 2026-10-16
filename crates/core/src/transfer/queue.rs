pub type TransferEmit = std::sync::Arc<dyn Fn(super::TransferEvent) + Send + Sync>;

pub struct TransferQueue;

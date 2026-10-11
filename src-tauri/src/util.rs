use std::sync::{Mutex, MutexGuard};

/// Locks a mutex, recovering the data if a previous holder panicked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

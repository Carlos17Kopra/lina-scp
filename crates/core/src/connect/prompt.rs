//! Interactive prompts during connection setup (host key, secrets, 2FA).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::ConnectEvent;
use crate::known_hosts::HostKeyInfo;

/// How long the pipeline waits for an answer before treating it as `Cancel`.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Event callback used to publish connect events (and thus prompts).
pub type EmitFn = Arc<dyn Fn(ConnectEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KbdPrompt {
    pub text: String,
    pub echo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Prompt {
    HostKeyUnknown {
        host: String,
        port: u16,
        info: HostKeyInfo,
        /// Key types known_hosts holds for this host other than the offered one: the host is
        /// known, but with a different key type. Never a reason to trust automatically.
        other_key_types_known: Vec<String>,
    },
    HostKeyChanged {
        host: String,
        port: u16,
        stored: HostKeyInfo,
        current: HostKeyInfo,
    },
    KeyboardInteractive {
        name: String,
        instructions: String,
        prompts: Vec<KbdPrompt>,
    },
    Password {
        user: String,
        host: String,
        retry: bool,
    },
    Passphrase {
        key_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyDecision {
    Reject,
    AcceptOnce,
    Trust,
    Replace,
}

/// Answer sent by the UI. `Debug` is implemented manually so secrets never
/// end up in logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PromptAnswer {
    HostKey { decision: HostKeyDecision },
    Responses { values: Vec<String> },
    Secret { value: String, save: bool },
    Cancel,
}

impl std::fmt::Debug for PromptAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptAnswer::HostKey { decision } => f.debug_struct("HostKey").field("decision", decision).finish(),
            PromptAnswer::Responses { values } => f.debug_struct("Responses").field("count", &values.len()).finish(),
            PromptAnswer::Secret { save, .. } => {
                f.debug_struct("Secret").field("value", &"***").field("save", save).finish()
            }
            PromptAnswer::Cancel => f.write_str("Cancel"),
        }
    }
}

/// Process-wide unique prompt ids, so one broker can serve many connects.
static NEXT_PROMPT_ID: AtomicU64 = AtomicU64::new(1);

/// Routes prompts to the UI and answers back to the waiting pipeline.
pub struct PromptBroker {
    emit: EmitFn,
    pending: Mutex<HashMap<u64, oneshot::Sender<PromptAnswer>>>,
    timeout: Duration,
}

impl PromptBroker {
    /// Broker whose `ask` publishes prompts through `emit`.
    pub fn new(emit: EmitFn) -> Self {
        Self::with_timeout(emit, PROMPT_TIMEOUT)
    }

    /// Like `new` with a custom answer timeout (tests).
    pub fn with_timeout(emit: EmitFn, timeout: Duration) -> Self {
        Self { emit, pending: Mutex::new(HashMap::new()), timeout }
    }

    /// Ask via the broker's own emitter; never cancelled externally.
    pub async fn ask(&self, prompt: Prompt) -> PromptAnswer {
        let emit = self.emit.clone();
        self.ask_cancellable(&emit, prompt, &CancellationToken::new()).await
    }

    /// Publish `prompt` through `emit` (the per-connect event channel) and
    /// wait for `answer`. Timeout or cancellation yields `PromptAnswer::Cancel`.
    pub async fn ask_cancellable(&self, emit: &EmitFn, prompt: Prompt, cancel: &CancellationToken) -> PromptAnswer {
        let prompt_id = NEXT_PROMPT_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        // Register before emitting so a synchronous answer is not lost.
        self.lock().insert(prompt_id, tx);
        emit(ConnectEvent::Prompt { prompt_id, prompt });

        let answer = tokio::select! {
            _ = cancel.cancelled() => PromptAnswer::Cancel,
            res = tokio::time::timeout(self.timeout, rx) => match res {
                Ok(Ok(answer)) => answer,
                _ => PromptAnswer::Cancel,
            },
        };
        self.lock().remove(&prompt_id);
        answer
    }

    /// Deliver an answer. Returns `false` if the id is unknown or expired.
    pub fn answer(&self, prompt_id: u64, answer: PromptAnswer) -> bool {
        match self.lock().remove(&prompt_id) {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<PromptAnswer>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_emit() -> EmitFn {
        Arc::new(|_| {})
    }

    /// Emitter that answers every prompt synchronously with `answer`.
    fn answering_emit(broker: Arc<PromptBroker>, answer: PromptAnswer, seen: Arc<Mutex<Vec<(u64, Prompt)>>>) -> EmitFn {
        Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, prompt } = event {
                seen.lock().unwrap().push((prompt_id, prompt));
                assert!(broker.answer(prompt_id, answer.clone()));
            }
        })
    }

    fn passphrase_prompt() -> Prompt {
        Prompt::Passphrase { key_path: "~/.ssh/id_ed25519".into() }
    }

    #[tokio::test]
    async fn ask_returns_answer_and_emits_prompt() {
        let broker = Arc::new(PromptBroker::new(noop_emit()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let emit =
            answering_emit(broker.clone(), PromptAnswer::Secret { value: "pw".into(), save: true }, seen.clone());
        let answer = broker.ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new()).await;
        assert_eq!(answer, PromptAnswer::Secret { value: "pw".into(), save: true });
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, passphrase_prompt());
    }

    #[tokio::test]
    async fn prompt_ids_are_unique_across_brokers() {
        let ids = Arc::new(Mutex::new(Vec::new()));
        for _ in 0..2 {
            let broker = Arc::new(PromptBroker::new(noop_emit()));
            let ids2 = ids.clone();
            let b2 = broker.clone();
            let emit: EmitFn = Arc::new(move |event| {
                if let ConnectEvent::Prompt { prompt_id, .. } = event {
                    ids2.lock().unwrap().push(prompt_id);
                    b2.answer(prompt_id, PromptAnswer::Cancel);
                }
            });
            broker.ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new()).await;
            broker.ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new()).await;
        }
        let mut ids = ids.lock().unwrap().clone();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before);
        assert_eq!(before, 4);
    }

    #[test]
    fn answer_unknown_id_returns_false() {
        let broker = PromptBroker::new(noop_emit());
        assert!(!broker.answer(u64::MAX, PromptAnswer::Cancel));
    }

    #[tokio::test]
    async fn timeout_yields_cancel_and_forgets_id() {
        let last_id = Arc::new(Mutex::new(None));
        let last2 = last_id.clone();
        let emit: EmitFn = Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, .. } = event {
                *last2.lock().unwrap() = Some(prompt_id);
            }
        });
        let broker = PromptBroker::with_timeout(noop_emit(), Duration::from_millis(20));
        let answer = broker.ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new()).await;
        assert_eq!(answer, PromptAnswer::Cancel);
        let id = last_id.lock().unwrap().expect("prompt emitted");
        assert!(!broker.answer(id, PromptAnswer::Cancel));
    }

    #[tokio::test]
    async fn cancellation_yields_cancel() {
        let broker = PromptBroker::new(noop_emit());
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let emit: EmitFn = Arc::new(move |_| c2.cancel());
        let answer = broker.ask_cancellable(&emit, passphrase_prompt(), &cancel).await;
        assert_eq!(answer, PromptAnswer::Cancel);
    }

    #[tokio::test]
    async fn ask_uses_own_emitter() {
        let slot: Arc<Mutex<Option<Arc<PromptBroker>>>> = Arc::new(Mutex::new(None));
        let slot2 = slot.clone();
        let emit: EmitFn = Arc::new(move |event| {
            if let ConnectEvent::Prompt { prompt_id, .. } = event {
                let broker = slot2.lock().unwrap().clone().unwrap();
                broker.answer(prompt_id, PromptAnswer::HostKey { decision: HostKeyDecision::Trust });
            }
        });
        let broker = Arc::new(PromptBroker::new(emit));
        *slot.lock().unwrap() = Some(broker.clone());
        let answer = broker.ask(passphrase_prompt()).await;
        assert_eq!(answer, PromptAnswer::HostKey { decision: HostKeyDecision::Trust });
    }

    #[test]
    fn debug_never_prints_secret() {
        let answer = PromptAnswer::Secret { value: "hunter2".into(), save: false };
        assert!(!format!("{answer:?}").contains("hunter2"));
    }

    #[test]
    fn prompt_serializes_with_kind_tag() {
        let json = serde_json::to_value(passphrase_prompt()).unwrap();
        assert_eq!(json["kind"], "passphrase");
        assert_eq!(json["keyPath"], "~/.ssh/id_ed25519");
    }
}

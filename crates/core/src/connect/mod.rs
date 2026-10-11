//! SSH connection pipeline: [JumpHost] -> Tcp -> HostKey -> Auth -> Channel -> Shell.

pub mod classify;
pub mod prompt;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use classify::{classify_io, classify_resolve_error, map_russh_error, network_error, sanitize_log_line};
pub use prompt::{EmitFn, HostKeyDecision, KbdPrompt, Prompt, PromptAnswer, PromptBroker};

use crate::error::CoreError;
use crate::known_hosts::KnownHosts;
use crate::model::{Host, LoginProfile};
use crate::secrets::SecretStore;
use crate::session::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    JumpHost,
    Tcp,
    HostKey,
    Auth,
    Channel,
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConnectEvent {
    Steps {
        steps: Vec<Step>,
    },
    StepStarted {
        step: Step,
    },
    StepDone {
        step: Step,
    },
    StepFailed {
        step: Step,
        error: CoreError,
    },
    /// Sanitized `ssh -v` style log line.
    Log {
        line: String,
    },
    Prompt {
        prompt_id: u64,
        prompt: Prompt,
    },
    Connected {
        session_id: SessionId,
    },
    Failed {
        error: CoreError,
    },
}

/// Build a `Log` event; the line is always sanitized.
pub fn log_line(line: impl Into<String>) -> ConnectEvent {
    ConnectEvent::Log { line: sanitize_log_line(&line.into()) }
}

#[derive(Debug, Clone)]
pub struct ConnectTarget {
    pub host: Host,
    pub profile: Option<LoginProfile>,
}

#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub target: ConnectTarget,
    pub jump: Option<ConnectTarget>,
    pub cols: u16,
    pub rows: u16,
}

pub struct ConnectDeps {
    pub known_hosts: KnownHosts,
    pub secrets: Arc<dyn SecretStore>,
    pub agent_sock: Option<PathBuf>,
}

/// Steps shown in the loading screen. `JumpHost` only with a jump host.
pub fn plan_steps(req: &ConnectRequest) -> Vec<Step> {
    let mut steps = Vec::with_capacity(6);
    if req.jump.is_some() {
        steps.push(Step::JumpHost);
    }
    steps.extend([Step::Tcp, Step::HostKey, Step::Auth, Step::Channel, Step::Shell]);
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(with_jump: bool) -> ConnectRequest {
        let target = ConnectTarget { host: Host::new("web", "web.example.org"), profile: None };
        ConnectRequest {
            jump: with_jump.then(|| ConnectTarget { host: Host::new("bastion", "bastion.example.org"), profile: None }),
            target,
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn plan_steps_without_jump() {
        assert_eq!(plan_steps(&request(false)), vec![Step::Tcp, Step::HostKey, Step::Auth, Step::Channel, Step::Shell]);
    }

    #[test]
    fn plan_steps_with_jump() {
        let steps = plan_steps(&request(true));
        assert_eq!(steps.first(), Some(&Step::JumpHost));
        assert_eq!(steps.len(), 6);
    }

    #[test]
    fn log_line_is_sanitized() {
        match log_line("password: hunter2") {
            ConnectEvent::Log { line } => assert_eq!(line, "password: ***"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let json = serde_json::to_value(ConnectEvent::StepStarted { step: Step::HostKey }).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "stepStarted", "step": "hostKey"}));

        let json = serde_json::to_value(ConnectEvent::Prompt {
            prompt_id: 7,
            prompt: Prompt::Passphrase { key_path: "k".into() },
        })
        .unwrap();
        assert_eq!(json["kind"], "prompt");
        assert_eq!(json["promptId"], 7);
        assert_eq!(json["prompt"]["kind"], "passphrase");

        let json = serde_json::to_value(ConnectEvent::Failed { error: CoreError::Cancelled }).unwrap();
        assert_eq!(json["kind"], "failed");
        assert!(json["error"].is_object());
    }
}

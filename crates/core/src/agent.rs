//! Identities of the running SSH agent (TP6c), read-only.

use std::path::Path;
use std::time::Duration;

use russh::keys::HashAlg;
use russh::keys::agent::client::AgentClient;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentity {
    /// E.g. `ssh-ed25519`, `ssh-rsa`.
    pub algorithm: String,
    /// `SHA256:…`
    pub fingerprint: String,
    pub comment: String,
    /// The OpenSSH line of the public key.
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// `None` when `SSH_AUTH_SOCK` is not set; then there are no identities.
    pub socket: Option<String>,
    /// In the agent's order.
    pub identities: Vec<AgentIdentity>,
}

fn agent_error(e: russh::keys::Error) -> CoreError {
    CoreError::config(format!("ssh-agent: {e}"))
}

/// A local agent answers at once; a stale forwarded socket may accept and never answer.
const AGENT_TIMEOUT: Duration = Duration::from_secs(5);

/// Asks the agent at `socket` for its identities. No socket is not an error; an unreachable or silent one is.
pub async fn list_agent_identities(socket: Option<&Path>) -> Result<AgentStatus> {
    list_within(socket, AGENT_TIMEOUT).await
}

async fn list_within(socket: Option<&Path>, timeout: Duration) -> Result<AgentStatus> {
    let Some(socket) = socket else { return Ok(AgentStatus { socket: None, identities: Vec::new() }) };
    let request = async {
        let mut agent = AgentClient::connect_uds(socket).await.map_err(agent_error)?;
        agent.request_identities().await.map_err(agent_error)
    };
    let identities =
        tokio::time::timeout(timeout, request).await.map_err(|_| CoreError::config("ssh-agent: no answer"))??;
    let identities = identities
        .iter()
        .map(|identity| {
            let public = identity.public_key();
            AgentIdentity {
                algorithm: public.algorithm().as_str().to_string(),
                fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
                comment: public.comment().as_str_lossy().to_string(),
                public_key: public.to_openssh().unwrap_or_default(),
            }
        })
        .collect();
    Ok(AgentStatus { socket: Some(socket.display().to_string()), identities })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_socket_means_no_agent_and_no_error() {
        assert_eq!(list_agent_identities(None).await.unwrap(), AgentStatus { socket: None, identities: vec![] });
    }

    #[tokio::test]
    async fn silent_agent_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silent.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let _held = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
            drop(stream);
        });
        let err = list_within(Some(&path), Duration::from_millis(100)).await.unwrap_err();
        assert!(err.to_string().contains("no answer"), "{err}");
    }

    #[tokio::test]
    async fn unreachable_socket_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = list_agent_identities(Some(&dir.path().join("missing.sock"))).await.unwrap_err();
        assert!(err.to_string().contains("ssh-agent"), "{err}");
    }
}

//! Read-only import of `~/.ssh/config` host aliases.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, ErrorKind};
use std::path::Path;

use ssh2_config::{ParseRule, SshConfig};

use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, AuthMethod, Host, HostAuth, HostSource, Id};

/// Deterministic id of an imported alias: uuid v5(NAMESPACE_URL, "ssh-config:" + alias).
pub fn import_id(alias: &str) -> Id {
    Id::new_v5(&Id::NAMESPACE_URL, format!("ssh-config:{alias}").as_bytes())
}

/// Imports every concrete `Host` alias (no wildcards, no negation) with its effective
/// settings. A missing file yields an empty list.
pub fn import_ssh_config(path: &Path) -> Result<Vec<Host>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut reader = BufReader::new(file);
    let config = SshConfig::default()
        .parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS)
        .map_err(|e| CoreError::config(format!("cannot parse {}: {e}", path.display())))?;

    let aliases = concrete_aliases(&config);
    let ids: HashMap<&str, Id> = aliases.iter().map(|a| (a.as_str(), import_id(a))).collect();
    Ok(aliases.iter().map(|alias| to_host(&config, alias, &ids)).collect())
}

/// Stored hosts win over imported hosts with the same name. Result is sorted by name
/// (case-insensitive).
pub fn merge_hosts(stored: Vec<Host>, imported: Vec<Host>) -> Vec<Host> {
    // A stored name also hides the imported alias it is mirrored as (see `Mirror::render`).
    let names: HashSet<String> =
        stored.iter().flat_map(|h| [h.name.clone(), super::mirror::host_alias(&h.name)]).collect();
    let mut merged = stored;
    merged.extend(imported.into_iter().filter(|h| !names.contains(&h.name)));
    merged.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    merged
}

fn concrete_aliases(config: &SshConfig) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut aliases = Vec::new();
    for host in config.get_hosts() {
        for clause in &host.pattern {
            let pattern = clause.pattern.as_str();
            let concrete = !clause.negated && !pattern.is_empty() && !pattern.contains(['*', '?', '!']);
            if concrete && seen.insert(pattern.to_string()) {
                aliases.push(pattern.to_string());
            }
        }
    }
    aliases
}

fn to_host(config: &SshConfig, alias: &str, ids: &HashMap<&str, Id>) -> Host {
    let params = config.query(alias);
    let mut host = Host::new(alias, params.host_name.as_deref().unwrap_or(alias));
    host.id = ids[alias];
    host.source = HostSource::SshConfig;
    host.created_at = 0;
    host.updated_at = 0;
    host.port = params.port.unwrap_or(22);
    host.user = params.user.clone();
    if let Some(interval) = params.server_alive_interval {
        host.keepalive_s = interval.as_secs() as u32;
    }
    if let Some(timeout) = params.connect_timeout {
        host.timeout_s = timeout.as_secs() as u32;
    }
    let identity = params.identity_file.as_ref().and_then(|files| files.first());
    host.auth = HostAuth::Custom {
        auth: match identity {
            Some(path) => AuthConfig {
                method: AuthMethod::Key,
                key_path: Some(path.to_string_lossy().into_owned()),
                has_secret: false,
            },
            None => AuthConfig::new(AuthMethod::Agent),
        },
    };
    if let Some(jumps) = &params.proxy_jump {
        let resolved = match jumps.as_slice() {
            [single] => ids.get(jump_alias(single)).copied(),
            _ => None,
        };
        match resolved {
            Some(id) => host.jump_host_id = Some(id),
            None if jumps.iter().all(|j| j.eq_ignore_ascii_case("none")) => {}
            None => host.notes = format!("ProxyJump {} (not imported)", jumps.join(",")),
        }
    }
    host
}

/// `user@alias:port` -> `alias`.
fn jump_alias(jump: &str) -> &str {
    let without_user = jump.rsplit_once('@').map_or(jump, |(_, rest)| rest);
    without_user.split_once(':').map_or(without_user, |(alias, _)| alias)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
Host *
    ServerAliveInterval 45

Host bastion
    HostName bastion.example.com
    User jump
    IdentityFile /keys/id_bastion

Host web web-alias
    HostName 10.0.0.5
    Port 2222
    User deploy
    ProxyJump bastion

Host chained
    HostName 10.0.0.6
    ProxyJump bastion,web

Host db-* !db-old
    User dbadmin

Host plain
    UnknownOption yes
";

    fn import(content: &str) -> Vec<Host> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, content).unwrap();
        import_ssh_config(&path).unwrap()
    }

    fn by_name<'a>(hosts: &'a [Host], name: &str) -> &'a Host {
        hosts.iter().find(|h| h.name == name).unwrap()
    }

    #[test]
    fn imports_concrete_aliases_only() {
        let hosts = import(CONFIG);
        let names: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["bastion", "web", "web-alias", "chained", "plain"]);
        assert!(hosts.iter().all(|h| h.source == HostSource::SshConfig));
    }

    #[test]
    fn maps_effective_parameters() {
        let hosts = import(CONFIG);
        let bastion = by_name(&hosts, "bastion");
        assert_eq!(bastion.id, import_id("bastion"));
        assert_eq!(bastion.address, "bastion.example.com");
        assert_eq!(bastion.port, 22);
        assert_eq!(bastion.user.as_deref(), Some("jump"));
        assert_eq!(bastion.keepalive_s, 45);
        assert_eq!(
            bastion.auth,
            HostAuth::Custom {
                auth: AuthConfig {
                    method: AuthMethod::Key,
                    key_path: Some("/keys/id_bastion".into()),
                    has_secret: false
                }
            }
        );

        let web = by_name(&hosts, "web");
        assert_eq!(web.address, "10.0.0.5");
        assert_eq!(web.port, 2222);
        assert_eq!(web.auth, HostAuth::Custom { auth: AuthConfig::new(AuthMethod::Agent) });
        assert_eq!(web.jump_host_id, Some(import_id("bastion")));

        let plain = by_name(&hosts, "plain");
        assert_eq!(plain.address, "plain");
        assert!(plain.user.is_none());
    }

    #[test]
    fn multi_hop_proxy_jump_is_noted_not_linked() {
        let hosts = import(CONFIG);
        let chained = by_name(&hosts, "chained");
        assert_eq!(chained.jump_host_id, None);
        assert_eq!(chained.notes, "ProxyJump bastion,web (not imported)");
    }

    #[test]
    fn ids_are_deterministic() {
        assert_eq!(import_id("web"), import_id("web"));
        assert_ne!(import_id("web"), import_id("web2"));
        assert_eq!(import_id("web").get_version_num(), 5);
    }

    #[test]
    fn missing_file_yields_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(import_ssh_config(&dir.path().join("none")).unwrap().is_empty());
    }

    #[test]
    fn jump_alias_strips_user_and_port() {
        assert_eq!(jump_alias("me@bastion:2200"), "bastion");
        assert_eq!(jump_alias("bastion"), "bastion");
    }

    #[test]
    fn merge_prefers_stored_hosts_and_sorts_by_name() {
        let stored = vec![Host::new("web", "override.example"), Host::new("Zeta", "z")];
        let imported = import(CONFIG);
        let merged = merge_hosts(stored, imported);
        let names: Vec<&str> = merged.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["bastion", "chained", "plain", "web", "web-alias", "Zeta"]);
        let web = by_name(&merged, "web");
        assert_eq!(web.address, "override.example");
        assert_eq!(web.source, HostSource::Linascp);
    }
}

#[cfg(test)]
mod review_focus_tests {
    use super::*;

    #[test]
    fn config_with_include_and_match_imports_without_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("extra.conf"), "Host extra\n    HostName 10.0.0.9\n").unwrap();
        let config = format!(
            "Include {}/extra.conf\n\nHost main\n    HostName 10.0.0.1\n\nMatch host *.internal\n    User ops\n",
            dir.path().display()
        );
        let path = dir.path().join("config");
        std::fs::write(&path, config).unwrap();

        let hosts = import_ssh_config(&path).expect("Include/Match must not break the import");
        assert!(hosts.iter().any(|h| h.name == "main" && h.address == "10.0.0.1"));
    }

    #[test]
    fn stored_host_hides_the_imported_alias_it_is_mirrored_as() {
        let stored = vec![Host::new("O'Reilly server", "a.example")];
        let mut mirrored = Host::new("O-Reilly-server", "a.example");
        mirrored.source = HostSource::SshConfig;
        let other = Host::new("other", "o.example");
        let merged = merge_hosts(stored, vec![mirrored, other]);
        let names: Vec<_> = merged.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["O'Reilly server", "other"]);
    }
}

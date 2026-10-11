//! Reading and writing OpenSSH `known_hosts` (plain, hashed `|1|salt|hash`, `[host]:port`).

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use russh::keys::{HashAlg, PublicKey};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

use crate::error::{CoreError, Result};
use crate::fsutil;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    /// OpenSSH algorithm name, e.g. `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:<base64 without padding>`.
    pub sha256: String,
    /// `MD5:aa:bb:...`.
    pub md5: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Changed { stored: HostKeyInfo },
}

pub fn fingerprints(key: &PublicKey) -> HostKeyInfo {
    // Encoding an already parsed key into its wire blob cannot fail in practice.
    let blob = key.to_bytes().unwrap_or_default();
    let md5 = Md5::digest(&blob).iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":");
    HostKeyInfo {
        algorithm: key.algorithm().as_str().to_string(),
        sha256: key.fingerprint(HashAlg::Sha256).to_string(),
        md5: format!("MD5:{md5}"),
    }
}

pub struct KnownHosts {
    path: PathBuf,
}

impl KnownHosts {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `Known` if any matching entry has exactly this key, `Changed` if a matching entry has
    /// a different key of the same algorithm, otherwise `Unknown`.
    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
        let content = self.read()?;
        let name = lookup_name(host, port);
        let mut changed: Option<PublicKey> = None;
        for line in content.lines() {
            let Some(entry) = parse_line(line) else { continue };
            if !patterns_match(entry.patterns, &name) {
                continue;
            }
            if entry.key.key_data() == key.key_data() {
                return Ok(HostKeyStatus::Known);
            }
            if changed.is_none() && entry.key.algorithm() == key.algorithm() {
                changed = Some(entry.key);
            }
        }
        Ok(match changed {
            Some(stored) => HostKeyStatus::Changed { stored: fingerprints(&stored) },
            None => HostKeyStatus::Unknown,
        })
    }

    /// Appends an entry. Creates the file (0600) and its directory (0700) if missing.
    pub fn add(&self, host: &str, port: u16, key: &PublicKey, hashed: bool) -> Result<()> {
        let name = lookup_name(host, port);
        let host_field = if hashed { hash_name(&name)? } else { name };
        let line = format!("{host_field} {}\n", key_text(key)?);

        if let Some(parent) = self.path.parent() {
            fsutil::create_private_dir(parent)?;
        }
        let needs_newline = match fs::read(&self.path) {
            Ok(bytes) => bytes.last().is_some_and(|b| *b != b'\n'),
            Err(err) if err.kind() == ErrorKind::NotFound => false,
            Err(err) => return Err(err.into()),
        };
        let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path)?;
        if needs_newline {
            file.write_all(b"\n")?;
        }
        file.write_all(line.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    /// Removes the entries for this host that hold a key of the same algorithm,
    /// then adds the new key as a hashed entry. Entries of other hosts are kept;
    /// on plain multi-host lines only the matching host pattern is removed.
    pub fn replace(&self, host: &str, port: u16, key: &PublicKey) -> Result<()> {
        let content = self.read()?;
        let name = lookup_name(host, port);
        let mut out = String::with_capacity(content.len());
        for line in content.lines() {
            if let Some(kept) = rewrite_line(line, &name, key) {
                out.push_str(&kept);
                out.push('\n');
            }
        }
        if out != content {
            fsutil::write_private(&self.path, out.as_bytes())?;
        }
        self.add(host, port, key, true)
    }

    fn read(&self) -> Result<String> {
        match fs::read_to_string(&self.path) {
            Ok(content) => Ok(content),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(err) => Err(err.into()),
        }
    }
}

struct Entry<'a> {
    patterns: &'a str,
    key: PublicKey,
}

/// Parses a known_hosts line. Comments, blank lines, marker lines
/// (`@cert-authority`, `@revoked`) and unparsable keys yield `None`.
fn parse_line(line: &str) -> Option<Entry<'_>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let patterns = parts.next()?;
    let algorithm = parts.next()?;
    let data = parts.next()?;
    let key = PublicKey::from_openssh(&format!("{algorithm} {data}")).ok()?;
    Some(Entry { patterns, key })
}

/// Returns the line to keep (possibly with fewer host patterns) or `None` to drop it.
fn rewrite_line(line: &str, name: &str, key: &PublicKey) -> Option<String> {
    let Some(entry) = parse_line(line) else { return Some(line.to_string()) };
    if entry.key.algorithm() != key.algorithm() || !patterns_match(entry.patterns, name) {
        return Some(line.to_string());
    }
    if entry.patterns.starts_with("|1|") {
        return None;
    }
    let remaining: Vec<&str> = entry
        .patterns
        .split(',')
        .filter(|p| p.starts_with('!') || !wildcard_match(&p.to_ascii_lowercase(), name))
        .collect();
    if remaining.iter().all(|p| p.starts_with('!')) {
        return None;
    }
    let rest = line.trim_start()[entry.patterns.len()..].to_string();
    Some(format!("{}{rest}", remaining.join(",")))
}

/// Name as stored by OpenSSH: lowercase host, `[host]:port` when port != 22.
fn lookup_name(host: &str, port: u16) -> String {
    let host = host.to_ascii_lowercase();
    if port == 22 { host } else { format!("[{host}]:{port}") }
}

fn patterns_match(patterns: &str, name: &str) -> bool {
    if let Some(rest) = patterns.strip_prefix("|1|") {
        return hashed_match(rest, name);
    }
    let mut matched = false;
    for pattern in patterns.split(',') {
        let pattern = pattern.to_ascii_lowercase();
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard_match(negated, name) {
                return false;
            }
        } else if wildcard_match(&pattern, name) {
            matched = true;
        }
    }
    matched
}

fn hashed_match(rest: &str, name: &str) -> bool {
    let Some((salt, hash)) = rest.split_once('|') else { return false };
    let (Ok(salt), Ok(hash)) = (B64.decode(salt), B64.decode(hash)) else { return false };
    hmac_sha1(&salt, name).as_slice() == hash.as_slice()
}

fn hash_name(name: &str) -> Result<String> {
    let mut salt = [0u8; 20];
    getrandom::fill(&mut salt).map_err(|e| CoreError::internal(format!("random salt: {e}")))?;
    Ok(format!("|1|{}|{}", B64.encode(salt), B64.encode(hmac_sha1(&salt, name))))
}

fn hmac_sha1(salt: &[u8], name: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(salt).expect("HMAC accepts keys of any length");
    mac.update(name.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `<algorithm> <base64 blob>` without comment.
fn key_text(key: &PublicKey) -> Result<String> {
    let blob = key.to_bytes().map_err(|e| CoreError::internal(format!("encode host key: {e}")))?;
    Ok(format!("{} {}", key.algorithm().as_str(), B64.encode(blob)))
}

/// OpenSSH-style glob: `*` matches any sequence, `?` matches one character.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            backtrack = Some((pi, ti));
            pi += 1;
        } else if let Some((star_pi, star_ti)) = backtrack {
            pi = star_pi + 1;
            ti = star_ti + 1;
            backtrack = Some((star_pi, star_ti + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // Generated with `ssh-keygen -t ed25519|ecdsa -C ""`; fingerprints from `ssh-keygen -l [-E md5]`.
    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICi2vUL2Z58TUNsW6eB7m9Vm3RgGtVqEf1QNfW24jbho";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHtwBGjTF/mmhwN4utNSNgAgcIRdABlX4NFv5Pvskb0S";
    const KEY_C: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBGlX0YFfeEtmpRikznN2qniQ1IyC7kaRaI2WTk76J4+jKC7gzqVPyYdfTLLgo3B/i11vZ53woAkukHOLm1kXsPE=";
    // `ssh-keygen -H` output for "example.com" and "[example.com]:2222" with KEY_A.
    const HASHED_22: &str = "|1|pD3/KWOuOWYIdPFBZoa3rqv/k60=|fUYbOJ2iF30CwYg1GZvmBr/l6Xw=";
    const HASHED_2222: &str = "|1|zwh7GbAXhP7NxCON2SJIl2sIg8I=|qG1O0+loDUTsKoROOLb7Yk5EcjQ=";

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    fn kh_with(content: &str) -> (tempfile::TempDir, KnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        fs::write(&path, content).unwrap();
        (dir, KnownHosts::new(path))
    }

    #[test]
    fn fingerprints_match_ssh_keygen() {
        let a = fingerprints(&key(KEY_A));
        assert_eq!(a.algorithm, "ssh-ed25519");
        assert_eq!(a.sha256, "SHA256:jNtjcKMOvysttEhG3S6Ai4dCzblEgDjHFgmZE2lh2yI");
        assert_eq!(a.md5, "MD5:86:ea:16:e3:2a:b9:62:c1:70:5c:7c:13:71:85:ca:27");
        let c = fingerprints(&key(KEY_C));
        assert_eq!(c.algorithm, "ecdsa-sha2-nistp256");
        assert_eq!(c.sha256, "SHA256:NYjSAeB7moVPnKeW19QrTTWScnma4eKKubjc1FJ2GVA");
        assert_eq!(c.md5, "MD5:a8:cd:b3:d5:b4:56:9d:43:a4:2c:13:59:3b:55:81:86");
    }

    #[test]
    fn missing_file_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let kh = KnownHosts::new(dir.path().join("nope"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn plain_entries_known_changed_unknown() {
        let (_d, kh) = kh_with(&format!("# comment\n\nfoo.example,Example.COM {KEY_A} me@box\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("EXAMPLE.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("example.com", 22, &key(KEY_B)).unwrap(),
            HostKeyStatus::Changed { stored: fingerprints(&key(KEY_A)) }
        );
        assert_eq!(kh.check("other.example", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn different_algorithm_is_unknown_not_changed() {
        let (_d, kh) = kh_with(&format!("example.com {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn multiple_keys_per_host() {
        let (_d, kh) = kh_with(&format!("example.com {KEY_C}\nexample.com {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn hashed_entries_from_ssh_keygen() {
        let (_d, kh) = kh_with(&format!("{HASHED_22} {KEY_A}\n{HASHED_2222} {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert!(matches!(kh.check("example.com", 22, &key(KEY_B)).unwrap(), HostKeyStatus::Changed { .. }));
        assert_eq!(kh.check("example.org", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn wildcards_negation_and_markers() {
        let (_d, kh) = kh_with(&format!("*.example.com,!bad.example.com {KEY_A}\n@revoked * {KEY_B}\n"));
        assert_eq!(kh.check("web.example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("bad.example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
        assert!(wildcard_match("w?b*", "web01"));
        assert!(!wildcard_match("w?b", "wb"));
    }

    #[test]
    fn add_creates_private_file_and_is_found_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ssh/known_hosts");
        let kh = KnownHosts::new(path.clone());
        kh.add("example.com", 22, &key(KEY_A), true).unwrap();
        kh.add("example.com", 2222, &key(KEY_C), false).unwrap();

        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[0].starts_with("|1|"));
        assert!(!lines[0].contains("example.com"));
        assert!(lines[1].starts_with("[example.com]:2222 ecdsa-sha2-nistp256 "));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn add_inserts_missing_trailing_newline() {
        let (_d, kh) = kh_with(&format!("other.host {KEY_B}"));
        kh.add("example.com", 22, &key(KEY_A), false).unwrap();
        let content = fs::read_to_string(&kh.path).unwrap();
        assert_eq!(content, format!("other.host {KEY_B}\nexample.com {KEY_A}\n"));
    }

    #[test]
    fn replace_removes_only_same_algorithm_entries_of_that_host() {
        let content = format!(
            "{HASHED_22} {KEY_A}\nexample.com,keep.example {KEY_A}\nexample.com {KEY_C}\nkeep.example {KEY_A}\n"
        );
        let (_d, kh) = kh_with(&content);
        kh.replace("example.com", 22, &key(KEY_B)).unwrap();

        let after = fs::read_to_string(&kh.path).unwrap();
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], format!("keep.example {KEY_A}"));
        assert_eq!(lines[1], format!("example.com {KEY_C}"));
        assert_eq!(lines[2], format!("keep.example {KEY_A}"));
        assert!(lines[3].starts_with("|1|") && lines[3].contains(KEY_B));
        assert_eq!(kh.check("example.com", 22, &key(KEY_B)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("keep.example", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(fs::metadata(&kh.path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[cfg(test)]
mod review_focus_tests {
    use super::*;

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICi2vUL2Z58TUNsW6eB7m9Vm3RgGtVqEf1QNfW24jbho";

    #[test]
    fn ipv6_hosts_use_bracket_form_with_port_and_bare_form_without() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::new(path.clone());
        let key = PublicKey::from_openssh(KEY_A).unwrap();

        kh.add("::1", 2222, &key, false).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("[::1]:2222 ssh-ed25519 "), "{content}");
        assert!(matches!(kh.check("::1", 2222, &key).unwrap(), HostKeyStatus::Known));
        assert!(matches!(kh.check("::1", 22, &key).unwrap(), HostKeyStatus::Unknown));

        kh.add("fe80::1", 22, &key, true).unwrap();
        assert!(matches!(kh.check("fe80::1", 22, &key).unwrap(), HostKeyStatus::Known));
    }
}

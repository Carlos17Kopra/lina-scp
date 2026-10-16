//! The user's SSH key files (TP5b): list them, generate new ones. Private keys stay files; nothing is stored.

use std::path::{Path, PathBuf};

use russh::keys::ssh_key::private::{Ed25519Keypair, KeypairData, RsaKeypair};
use russh::keys::ssh_key::{HashAlg, LineEnding, PublicKey};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::fsutil::create_private_dir;
use crate::sftp::local::io_error;
use crate::validate::validate_file_name;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub path: String,
    pub name: String,
    /// E.g. `ssh-ed25519`, `ssh-rsa`.
    pub algorithm: String,
    /// `SHA256:…`
    pub fingerprint: String,
    pub comment: String,
    /// The OpenSSH line of the public key (`.pub` contents).
    pub public_key: String,
    /// `None` when the private file is not in the OpenSSH format (e.g. PEM).
    pub encrypted: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KeyKind {
    Ed25519,
    Rsa4096,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyDefaults {
    pub name: String,
    pub comment: String,
}

/// Reads `<dir>` and `<dir>/linascp`: every file `X` with a valid `X.pub` beside it, sorted by name.
pub fn list_keys(ssh_dir: &Path) -> Result<Vec<KeyInfo>> {
    let mut keys = Vec::new();
    for dir in [ssh_dir.to_path_buf(), ssh_dir.join("linascp")] {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(e, &dir.display().to_string())),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "pub") || !path.is_file() {
                continue;
            }
            if let Some(info) = read_pair(&path) {
                keys.push(info);
            }
        }
    }
    keys.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    Ok(keys)
}

fn pub_path(private: &Path) -> PathBuf {
    let mut name = private.as_os_str().to_os_string();
    name.push(".pub");
    PathBuf::from(name)
}

fn read_pair(private: &Path) -> Option<KeyInfo> {
    let text = std::fs::read_to_string(pub_path(private)).ok()?;
    let public = parse_public_key(&text).ok()?;
    let encrypted = std::fs::read_to_string(private)
        .ok()
        .and_then(|pem| russh::keys::PrivateKey::from_openssh(&pem).ok())
        .map(|key| key.is_encrypted());
    Some(info(private, &public, encrypted))
}

fn info(private: &Path, public: &PublicKey, encrypted: Option<bool>) -> KeyInfo {
    KeyInfo {
        path: private.display().to_string(),
        name: private.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        algorithm: public.algorithm().as_str().to_string(),
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        comment: public.comment().as_str_lossy().to_string(),
        public_key: public.to_openssh().unwrap_or_default(),
        encrypted,
    }
}

/// One line with a valid OpenSSH public key (surrounding whitespace is ignored).
pub fn parse_public_key(text: &str) -> Result<PublicKey> {
    let line = text.trim();
    if line.is_empty() || line.contains(['\n', '\r']) {
        return Err(CoreError::config("Öffentlicher Schlüssel: genau eine Zeile angeben"));
    }
    PublicKey::from_openssh(line).map_err(|e| CoreError::config(format!("Kein gültiger öffentlicher Schlüssel: {e}")))
}

fn base_name(kind: KeyKind) -> &'static str {
    match kind {
        KeyKind::Ed25519 => "id_ed25519",
        KeyKind::Rsa4096 => "id_rsa",
    }
}

fn taken(dir: &Path, name: &str) -> bool {
    let private = dir.join(name);
    private.exists() || pub_path(&private).exists()
}

/// `id_ed25519` / `id_rsa` when free (neither the file nor its `.pub`), else `…_linascp`, `…_linascp2`, …
pub fn suggest_name(ssh_dir: &Path, kind: KeyKind) -> String {
    let base = base_name(kind);
    if !taken(ssh_dir, base) {
        return base.to_string();
    }
    (1..)
        .map(|n| if n == 1 { format!("{base}_linascp") } else { format!("{base}_linascp{n}") })
        .find(|name| !taken(ssh_dir, name))
        .expect("an unbounded range finds a free name")
}

/// Writes `<dir>/<name>` (0600) and `<name>.pub` (0644); `dir` is created with 0700. Never overwrites.
pub fn generate_key(
    dir: &Path,
    kind: KeyKind,
    name: &str,
    comment: &str,
    passphrase: Option<&SecretString>,
) -> Result<KeyInfo> {
    use secrecy::ExposeSecret;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    validate_file_name(name)?;
    if name.starts_with('.') || name.ends_with(".pub") || name.chars().any(char::is_control) {
        return Err(CoreError::config("Dateiname: ohne führenden Punkt und nicht auf .pub endend"));
    }
    if comment.chars().any(char::is_control) || comment.len() > 255 {
        return Err(CoreError::config("Kommentar: eine Zeile, höchstens 255 Zeichen"));
    }
    if taken(dir, name) {
        return Err(CoreError::config(format!("{name} existiert bereits")));
    }
    let mut rng = rand::rng();
    let keypair = match kind {
        KeyKind::Ed25519 => KeypairData::from(Ed25519Keypair::random(&mut rng)),
        KeyKind::Rsa4096 => KeypairData::from(RsaKeypair::random(&mut rng, 4096).map_err(key_error)?),
    };
    let mut key = russh::keys::PrivateKey::new(keypair, comment).map_err(key_error)?;
    // Before encrypting: an encrypted key no longer exposes its comment.
    let public_key = key.public_key().clone();
    if let Some(passphrase) = passphrase.filter(|p| !p.expose_secret().is_empty()) {
        key = key.encrypt(&mut rng, passphrase.expose_secret()).map_err(key_error)?;
    }
    let private_text = key.to_openssh(LineEnding::LF).map_err(key_error)?;
    let public_text = format!("{}\n", public_key.to_openssh().map_err(key_error)?);

    create_private_dir(dir)?;
    let private = dir.join(name);
    let public = pub_path(&private);
    let write = |path: &Path, text: &[u8], mode: u32| -> Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(path)?;
        let written = (|| {
            file.write_all(text)?;
            // On the open file: the umask may have narrowed the mode.
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
            file.sync_all()
        })();
        if let Err(error) = written {
            // A half-written key must not block the name.
            let _ = std::fs::remove_file(path);
            return Err(error.into());
        }
        Ok(())
    };
    write(&private, private_text.as_bytes(), 0o600)?;
    if let Err(error) = write(&public, public_text.as_bytes(), 0o644) {
        let _ = std::fs::remove_file(&private);
        return Err(error);
    }
    Ok(info(&private, &public_key, Some(key.is_encrypted())))
}

fn key_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::internal(format!("Schlüssel erzeugen: {error}"))
}

/// Default name and comment (`user@hostname`) for a new key.
pub fn key_defaults(ssh_dir: &Path, kind: KeyKind) -> KeyDefaults {
    let user = std::env::var("USER").unwrap_or_default();
    let host = std::fs::read_to_string("/etc/hostname").map(|h| h.trim().to_string()).unwrap_or_default();
    let comment = match (user.is_empty(), host.is_empty()) {
        (false, false) => format!("{user}@{host}"),
        (false, true) => user,
        _ => String::new(),
    };
    KeyDefaults { name: suggest_name(ssh_dir, kind), comment }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::os::unix::fs::PermissionsExt;

    const PUB: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF1w1k1W6vnfRuRnMRhx9WheXyFs7vy1sTrQ47LcDBbV alice@laptop";

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn lists_key_pairs_of_both_dirs_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("linascp")).unwrap();
        std::fs::write(ssh.join("id_pem"), "-----BEGIN RSA PRIVATE KEY-----\nx\n-----END RSA PRIVATE KEY-----\n")
            .unwrap();
        std::fs::write(ssh.join("id_pem.pub"), format!("{PUB}\n")).unwrap();
        std::fs::write(ssh.join("lonely"), "private").unwrap();
        std::fs::write(ssh.join("broken"), "private").unwrap();
        std::fs::write(ssh.join("broken.pub"), "not a key").unwrap();
        std::fs::write(ssh.join("known_hosts"), "").unwrap();
        generate_key(&ssh.join("linascp"), KeyKind::Ed25519, "imported", "bob@x", None).unwrap();

        let keys = list_keys(&ssh).unwrap();
        let names: Vec<&str> = keys.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["id_pem", "imported"]);
        let pem = &keys[0];
        assert_eq!(pem.algorithm, "ssh-ed25519");
        assert_eq!(pem.comment, "alice@laptop");
        assert!(pem.fingerprint.starts_with("SHA256:"));
        assert_eq!(pem.public_key, PUB);
        assert_eq!(pem.encrypted, None);
        assert_eq!(pem.path, ssh.join("id_pem").display().to_string());
        assert_eq!(keys[1].encrypted, Some(false));

        assert_eq!(list_keys(&dir.path().join("missing")).unwrap(), Vec::new());
    }

    #[test]
    fn generates_a_private_and_a_public_file_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        let pass = SecretString::from("geheim".to_string());
        let key = generate_key(&ssh, KeyKind::Ed25519, "id_test", "me@box", Some(&pass)).unwrap();
        let private = ssh.join("id_test");
        assert_eq!((mode(&ssh), mode(&private), mode(&ssh.join("id_test.pub"))), (0o700, 0o600, 0o644));
        assert_eq!(key.encrypted, Some(true));
        assert_eq!(key.comment, "me@box");
        let parsed = russh::keys::PrivateKey::read_openssh_file(&private).unwrap();
        assert!(parsed.is_encrypted());
        let decrypted = parsed.decrypt(pass.expose_secret()).unwrap();
        assert_eq!(decrypted.public_key().to_openssh().unwrap(), key.public_key);

        let before = std::fs::read(&private).unwrap();
        assert!(matches!(generate_key(&ssh, KeyKind::Ed25519, "id_test", "", None), Err(CoreError::Config { .. })));
        assert_eq!(std::fs::read(&private).unwrap(), before);
        std::fs::write(ssh.join("only_pub.pub"), PUB).unwrap();
        assert!(generate_key(&ssh, KeyKind::Ed25519, "only_pub", "", None).is_err(), "a lone .pub also blocks");
        assert!(!ssh.join("only_pub").exists());

        for bad in ["", ".hidden", "a/b", "..", "x.pub", "a\nb"] {
            assert!(
                matches!(generate_key(&ssh, KeyKind::Ed25519, bad, "", None), Err(CoreError::Config { .. })),
                "{bad:?}"
            );
        }
        assert!(generate_key(&ssh, KeyKind::Ed25519, "id_c", "two\nlines", None).is_err());
    }

    #[test]
    fn generates_rsa_4096() {
        let dir = tempfile::tempdir().unwrap();
        let key = generate_key(dir.path(), KeyKind::Rsa4096, "id_rsa", "", None).unwrap();
        assert_eq!(key.algorithm, "ssh-rsa");
        let public = parse_public_key(&key.public_key).unwrap();
        assert_eq!(public.key_data().rsa().unwrap().n().as_positive_bytes().unwrap().len(), 512);
    }

    #[test]
    fn suggests_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        assert_eq!(suggest_name(&ssh, KeyKind::Ed25519), "id_ed25519");
        assert_eq!(suggest_name(&ssh, KeyKind::Rsa4096), "id_rsa");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "").unwrap();
        assert_eq!(suggest_name(&ssh, KeyKind::Ed25519), "id_ed25519_linascp");
        std::fs::write(ssh.join("id_ed25519_linascp.pub"), "").unwrap();
        assert_eq!(suggest_name(&ssh, KeyKind::Ed25519), "id_ed25519_linascp2");
    }

    #[test]
    fn checks_a_public_key_line() {
        assert_eq!(parse_public_key(&format!("  {PUB}\n")).unwrap().comment().as_str_lossy(), "alice@laptop");
        for bad in ["", "ssh-ed25519", "ssh-ed25519 !!!", &format!("{PUB}\n{PUB}")] {
            assert!(matches!(parse_public_key(bad), Err(CoreError::Config { .. })), "{bad:?}");
        }
    }
}

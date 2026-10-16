//! Validation of host and login-profile fields that end up in ssh_config lines (mirror mode).
//!
//! One rule set for every write path: host/profile save, import and mirror render. A value
//! that contains a line break, another control character or a double quote could break out of
//! its `~/.ssh/config.d/linascp.conf` line and inject directives such as `ProxyCommand`.

use std::ops::RangeInclusive;

use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, Host, HostAuth, LoginProfile, Snippet};

/// True when `value` can be written into one ssh_config line: no control characters
/// (including `\n`, `\r`, `\t`) and no `"`.
pub fn is_safe_text(value: &str) -> bool {
    !value.chars().any(|c| c.is_control() || c == '"')
}

/// True when `value` can be written as an ssh_config *value* token. On top of
/// [`is_safe_text`]: OpenSSH (>= 8.7) treats `'` as a quote and `\` as an escape even in
/// unquoted words, and a token starting with `#` starts a comment, so none of these may occur.
pub fn is_safe_value(value: &str) -> bool {
    is_safe_text(value) && !value.contains(['\'', '\\']) && !value.trim_start().starts_with('#')
}

/// A single path component typed by the user (new folder, rename) or sent back from a listing.
/// U+FFFD marks a name that was lossily decoded from a non-UTF-8 local name: it is rejected so a
/// lossy name never addresses a (different) real file.
// ponytail: also rejects real names containing U+FFFD; add raw-byte names if that ever matters.
pub fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0', '\u{FFFD}']) {
        return Err(CoreError::config(format!("ungültiger Dateiname: {name:?}")));
    }
    Ok(())
}

fn check_value(field: &str, value: &str) -> Result<()> {
    check_text(field, value)?;
    if is_safe_value(value) {
        Ok(())
    } else {
        Err(CoreError::config(format!("{field} darf kein ' oder \\ enthalten und nicht mit # beginnen")))
    }
}

fn check_text(field: &str, value: &str) -> Result<()> {
    if is_safe_text(value) {
        Ok(())
    } else {
        Err(CoreError::config(format!(
            "{field} enthält ein Steuerzeichen (z. B. Zeilenumbruch) oder ein Anführungszeichen"
        )))
    }
}

fn check_auth(auth: &AuthConfig) -> Result<()> {
    match &auth.key_path {
        Some(path) => check_value("Schlüsseldatei", path),
        None => Ok(()),
    }
}

/// Allowed terminal font sizes in px (`schemes.json` `fontSize.min/max`).
pub const TERMINAL_FONT_SIZES: RangeInclusive<u16> = 10..=20;

/// `"auto"` or a scheme id from `schemes.json`. The core only checks the format; the UI maps
/// ids it does not know (e.g. from a newer build) to `"auto"`.
pub fn is_valid_terminal_scheme(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphabetic())
}

/// Checks the per-host terminal overrides. They are cosmetic: unlike [`validate_host`] a failure
/// here never drops a host from the mirror or an import; imports clear invalid overrides instead.
pub fn validate_terminal_overrides(host: &Host) -> Result<()> {
    if let Some(scheme) = &host.terminal_scheme
        && !is_valid_terminal_scheme(scheme)
    {
        return Err(CoreError::config("Ungültiges Farbschema"));
    }
    if let Some(size) = host.terminal_font_size
        && !TERMINAL_FONT_SIZES.contains(&size)
    {
        return Err(CoreError::config("Schriftgröße muss zwischen 10 und 20 px liegen"));
    }
    Ok(())
}

/// Checks a host's own fields. A referenced login profile is checked by [`validate_profile`].
pub fn validate_host(host: &Host) -> Result<()> {
    check_text("Name", &host.name)?;
    check_value("Adresse", &host.address)?;
    if host.address.chars().any(char::is_whitespace) {
        return Err(CoreError::config("Adresse darf keine Leerzeichen enthalten"));
    }
    if host.port == 0 {
        return Err(CoreError::config("Port muss zwischen 1 und 65535 liegen"));
    }
    if let Some(user) = &host.user {
        check_value("Benutzer", user)?;
    }
    if host.sftp.parallel == 0 {
        return Err(CoreError::config("Parallele Übertragungen: mindestens 1"));
    }
    if let Some(perms) = &host.sftp.upload_perms
        && !is_upload_mode(perms)
    {
        return Err(CoreError::config("Upload-Rechte: Oktal angeben, z. B. 0644"));
    }
    match &host.auth {
        HostAuth::Custom { auth } => check_auth(auth),
        HostAuth::Profile { .. } => Ok(()),
    }
}

/// File mode for uploads: three octal digits, optionally with a leading 0 (no setuid/setgid/sticky).
fn is_upload_mode(text: &str) -> bool {
    let digits = text.strip_prefix('0').filter(|rest| rest.len() == 3).unwrap_or(text);
    digits.len() == 3 && digits.bytes().all(|b| (b'0'..=b'7').contains(&b))
}

/// Checks the profile fields that are written to ssh_config (user, key path).
pub fn validate_profile(profile: &LoginProfile) -> Result<()> {
    check_value("Benutzer", &profile.user)?;
    check_auth(&profile.auth)
}

pub(crate) const SNIPPET_NAME_MAX_CHARS: usize = 64;
const SNIPPET_COMMAND_MAX_BYTES: usize = 8192;

/// Snippets never reach ssh_config; the limits keep the UI and the terminal sane.
pub fn validate_snippet(snippet: &Snippet) -> Result<()> {
    if snippet.name.trim().is_empty() {
        return Err(CoreError::config("Name darf nicht leer sein"));
    }
    if snippet.name.chars().count() > SNIPPET_NAME_MAX_CHARS {
        return Err(CoreError::config("Name darf höchstens 64 Zeichen lang sein"));
    }
    if snippet.name.chars().any(char::is_control) {
        return Err(CoreError::config("Name darf keine Steuerzeichen enthalten"));
    }
    if snippet.command.trim().is_empty() {
        return Err(CoreError::config("Befehl darf nicht leer sein"));
    }
    if snippet.command.len() > SNIPPET_COMMAND_MAX_BYTES {
        return Err(CoreError::config("Befehl darf höchstens 8192 Bytes lang sein"));
    }
    if snippet.command.contains('\0') {
        return Err(CoreError::config("Befehl darf keine NUL-Zeichen enthalten"));
    }
    Ok(())
}

/// Bookmark folders: absolute, at most 4096 bytes, no control characters. Not normalised.
pub fn validate_bookmark_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(CoreError::config("Ungültiger Pfad für ein Lesezeichen"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AuthMethod;

    #[test]
    fn file_names_must_be_single_path_components() {
        for bad in ["", ".", "..", "a/b", "/", "a\0b", "bad\u{FFFD}name"] {
            assert!(matches!(validate_file_name(bad), Err(CoreError::Config { .. })), "{bad:?} accepted");
        }
        for good in ["a", ".hidden", "with space", "ümlaut.txt", "a..b"] {
            assert!(validate_file_name(good).is_ok(), "{good:?} rejected");
        }
    }

    fn key_auth(path: &str) -> AuthConfig {
        AuthConfig { method: AuthMethod::Key, key_path: Some(path.into()), has_secret: false }
    }

    #[test]
    fn sftp_needs_a_parallel_transfer_and_octal_upload_permissions() {
        let mut host = Host::new("web", "h.example");
        host.sftp.parallel = 0;
        assert!(validate_host(&host).is_err());
        host.sftp.parallel = 4;
        for bad in ["0844", "rw-r--r--", "12", "07777"] {
            host.sftp.upload_perms = Some(bad.into());
            assert!(validate_host(&host).is_err(), "{bad:?}");
        }
        for good in ["644", "0644", "0750"] {
            host.sftp.upload_perms = Some(good.into());
            assert!(validate_host(&host).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn plain_values_are_valid() {
        let mut host = Host::new("Kunde Müller Shop", "shop.example.de");
        host.user = Some("deploy".into());
        host.auth = HostAuth::Custom { auth: key_auth("~/.ssh/id ed25519") };
        assert!(validate_host(&host).is_ok());
        let profile = LoginProfile::new("ops", "ops", key_auth("/keys/ops key"));
        assert!(validate_profile(&profile).is_ok());
    }

    #[test]
    fn control_characters_and_quotes_are_rejected_in_every_field() {
        for bad in ["a\nb", "a\rb", "a\tb", "a\"b", "a\u{0}b", "a\u{7f}b", "a\u{85}b"] {
            let mut host = Host::new(bad, "h.example");
            assert!(validate_host(&host).is_err(), "name {bad:?}");
            host.name = "ok".into();
            host.address = bad.into();
            assert!(validate_host(&host).is_err(), "address {bad:?}");
            host.address = "h.example".into();
            host.user = Some(bad.into());
            assert!(validate_host(&host).is_err(), "user {bad:?}");
            host.user = None;
            host.auth = HostAuth::Custom { auth: key_auth(bad) };
            assert!(validate_host(&host).is_err(), "key path {bad:?}");

            let profile = LoginProfile::new("p", bad, key_auth("/k"));
            assert!(validate_profile(&profile).is_err(), "profile user {bad:?}");
            let profile = LoginProfile::new("p", "u", key_auth(bad));
            assert!(validate_profile(&profile).is_err(), "profile key path {bad:?}");
        }
    }

    #[test]
    fn address_without_whitespace_and_port_at_least_one() {
        let host = Host::new("h", "a.example ProxyCommand=x");
        assert!(matches!(validate_host(&host), Err(CoreError::Config { .. })));
        let mut host = Host::new("h", "a.example");
        host.port = 0;
        assert!(matches!(validate_host(&host), Err(CoreError::Config { .. })));
        host.port = 1;
        assert!(validate_host(&host).is_ok());
    }

    #[test]
    fn value_fields_reject_quote_backslash_and_leading_hash() {
        for bad in ["o'brien", "a\\b", "dir\\", "#comment", "  #x"] {
            let mut host = Host::new("ok", "h.example");
            host.address = bad.replace(' ', "");
            assert!(validate_host(&host).is_err(), "address {bad:?}");
            host.address = "h.example".into();
            host.user = Some(bad.into());
            assert!(validate_host(&host).is_err(), "user {bad:?}");
            host.user = None;
            host.auth = HostAuth::Custom { auth: key_auth(bad) };
            assert!(validate_host(&host).is_err(), "key path {bad:?}");
            assert!(validate_profile(&LoginProfile::new("p", bad, key_auth("/k"))).is_err(), "profile user {bad:?}");
            assert!(validate_profile(&LoginProfile::new("p", "u", key_auth(bad))).is_err(), "profile key {bad:?}");
        }
    }

    #[test]
    fn display_names_may_contain_quotes_backslashes_and_hash() {
        let mut host = Host::new("O'Reilly #1 \\ server", "h.example");
        host.user = Some("a#b".into());
        assert!(validate_host(&host).is_ok());
    }

    #[test]
    fn terminal_overrides_are_range_and_format_checked() {
        let mut host = Host::new("h", "h.example");
        for size in [10, 20] {
            host.terminal_font_size = Some(size);
            assert!(validate_terminal_overrides(&host).is_ok(), "size {size}");
        }
        for size in [9, 21] {
            host.terminal_font_size = Some(size);
            assert!(validate_terminal_overrides(&host).is_err(), "size {size}");
        }
        host.terminal_font_size = None;
        let longest = "x".repeat(32);
        for scheme in ["auto", "nord", "solDark", longest.as_str()] {
            host.terminal_scheme = Some(scheme.into());
            assert!(validate_terminal_overrides(&host).is_ok(), "scheme {scheme:?}");
        }
        let too_long = "x".repeat(33);
        for scheme in ["", "a b", "../x", "nörd", too_long.as_str()] {
            host.terminal_scheme = Some(scheme.into());
            assert!(validate_terminal_overrides(&host).is_err(), "scheme {scheme:?}");
            // Cosmetic: never a reason to drop the host from the mirror or an import.
            assert!(validate_host(&host).is_ok(), "scheme {scheme:?}");
        }
    }

    fn snippet(name: &str, command: &str) -> Snippet {
        Snippet::new(name, command, None)
    }

    #[test]
    fn snippet_allows_multiline_commands_up_to_the_limits() {
        assert!(validate_snippet(&snippet("Deploy", "cd /srv\n\tgit pull\r\n")).is_ok());
        assert!(validate_snippet(&snippet(&"n".repeat(64), &"c".repeat(8192))).is_ok());
        assert!(validate_snippet(&snippet(&"ä".repeat(64), "x")).is_ok(), "64 characters, not bytes");
    }

    #[test]
    fn snippet_rejects_bad_names_and_commands() {
        let long_name = "n".repeat(65);
        let long_command = "c".repeat(8193);
        let cases = [
            ("", "ls"),
            ("   ", "ls"),
            (long_name.as_str(), "ls"),
            ("a\nb", "ls"),
            ("a\tb", "ls"),
            ("ok", ""),
            ("ok", " \n\t "),
            ("ok", long_command.as_str()),
            ("ok", "ls\0"),
        ];
        for (name, command) in cases {
            assert!(
                matches!(validate_snippet(&snippet(name, command)), Err(CoreError::Config { .. })),
                "{name:?} / {command:?}"
            );
        }
    }

    #[test]
    fn bookmark_paths_must_be_absolute_and_plain() {
        assert!(validate_bookmark_path("/").is_ok());
        assert!(validate_bookmark_path("/srv/www").is_ok());
        for bad in ["", "x", "rel/x", "/a\u{7}", "/a\nb"] {
            assert!(validate_bookmark_path(bad).is_err(), "{bad:?}");
        }
        assert!(validate_bookmark_path(&format!("/{}", "a".repeat(4096))).is_err());
    }
}

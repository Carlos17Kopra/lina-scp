//! Validation of host and login-profile fields that end up in ssh_config lines (mirror mode).
//!
//! One rule set for every write path: host/profile save, import and mirror render. A value
//! that contains a line break, another control character or a double quote could break out of
//! its `~/.ssh/config.d/linascp.conf` line and inject directives such as `ProxyCommand`.

use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, Host, HostAuth, LoginProfile};

/// True when `value` can be written into one ssh_config line: no control characters
/// (including `\n`, `\r`, `\t`) and no `"`.
pub fn is_safe_text(value: &str) -> bool {
    !value.chars().any(|c| c.is_control() || c == '"')
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
        Some(path) => check_text("Schlüsseldatei", path),
        None => Ok(()),
    }
}

/// Checks a host's own fields. A referenced login profile is checked by [`validate_profile`].
pub fn validate_host(host: &Host) -> Result<()> {
    check_text("Name", &host.name)?;
    check_text("Adresse", &host.address)?;
    if host.address.chars().any(char::is_whitespace) {
        return Err(CoreError::config("Adresse darf keine Leerzeichen enthalten"));
    }
    if host.port == 0 {
        return Err(CoreError::config("Port muss zwischen 1 und 65535 liegen"));
    }
    if let Some(user) = &host.user {
        check_text("Benutzer", user)?;
    }
    match &host.auth {
        HostAuth::Custom { auth } => check_auth(auth),
        HostAuth::Profile { .. } => Ok(()),
    }
}

/// Checks the profile fields that are written to ssh_config (user, key path).
pub fn validate_profile(profile: &LoginProfile) -> Result<()> {
    check_text("Benutzer", &profile.user)?;
    check_auth(&profile.auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AuthMethod;

    fn key_auth(path: &str) -> AuthConfig {
        AuthConfig { method: AuthMethod::Key, key_path: Some(path.into()), has_secret: false }
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
}

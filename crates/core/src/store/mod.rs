//! SQLite persistence for hosts, login profiles, groups, snippets and settings.

mod migrations;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::error::{CoreError, Result};
use crate::model::{
    AuthConfig, AuthMethod, ForwardKind, Group, Host, HostAuth, HostSource, Id, LoginProfile, PortForward, Snippet,
    now_unix,
};

/// Setting key: `"dark"` | `"light"`.
pub const THEME: &str = "theme";
/// Setting key: `"integrate"` | `"mirror"`.
pub const SSH_MODE: &str = "ssh_mode";
/// Setting key: `"auto"` | `"webgl"` | `"dom"`.
pub const TERMINAL_RENDERER: &str = "terminal_renderer";
/// Setting key: font size in px as decimal string.
pub const TERMINAL_FONT_SIZE: &str = "terminal_font_size";
/// Setting key: terminal color scheme id (`"auto"` or a `schemes.json` id).
pub const TERMINAL_SCHEME: &str = "terminal_scheme";
/// Setting key: `"1"` while the terminal sidebar is open.
pub const TERMINAL_SIDEBAR_OPEN: &str = "terminal_sidebar_open";
/// Setting key: `"0"` turns automatic SSH reconnects off (default on).
pub const TERMINAL_AUTO_RECONNECT: &str = "terminal_auto_reconnect";
/// Setting key: `"true"` once the first-run dialog was completed.
pub const FIRST_RUN_DONE: &str = "first_run_done";

const HOST_COLUMNS: &str = "id, name, address, port, user, group_id, color, notes, protocol_ssh, protocol_sftp, \
    login_profile_id, auth_method, key_path, has_secret, jump_host_id, keepalive_s, timeout_s, start_command, \
    env, sftp_settings, source, created_at, updated_at, terminal_scheme, terminal_font_size";

const PROFILE_COLUMNS: &str = "id, name, user, method, key_path, has_secret, created_at, updated_at";

const SNIPPET_COLUMNS: &str = "id, name, command, host_id, created_at, updated_at";

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (and creates/migrates) the database file. Creates the parent directory.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::run(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // ---- hosts ----

    pub fn list_hosts(&self) -> Result<Vec<Host>> {
        let conn = self.conn();
        let sql = format!("SELECT {HOST_COLUMNS} FROM hosts ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let mut hosts = stmt.query_map([], host_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for host in &mut hosts {
            load_host_children(&conn, host)?;
        }
        Ok(hosts)
    }

    pub fn get_host(&self, id: Id) -> Result<Option<Host>> {
        let conn = self.conn();
        let sql = format!("SELECT {HOST_COLUMNS} FROM hosts WHERE id = ?1");
        let host = conn.query_row(&sql, [id.to_string()], host_from_row).optional()?;
        match host {
            Some(mut host) => {
                load_host_children(&conn, &mut host)?;
                Ok(Some(host))
            }
            None => Ok(None),
        }
    }

    /// Upserts the host including tags and port forwards. `updated_at` is set to now.
    pub fn save_host(&self, host: &Host) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        upsert_host(&tx, host)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes the host; other hosts using it as jump host get `jump_host_id = NULL`.
    pub fn delete_host(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("UPDATE hosts SET jump_host_id = NULL WHERE jump_host_id = ?1", [id.to_string()])?;
        tx.execute("DELETE FROM hosts WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    // ---- login profiles ----

    pub fn list_profiles(&self) -> Result<Vec<LoginProfile>> {
        let conn = self.conn();
        let sql = format!("SELECT {PROFILE_COLUMNS} FROM login_profiles ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let profiles = stmt.query_map([], profile_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(profiles)
    }

    pub fn get_profile(&self, id: Id) -> Result<Option<LoginProfile>> {
        let conn = self.conn();
        let sql = format!("SELECT {PROFILE_COLUMNS} FROM login_profiles WHERE id = ?1");
        Ok(conn.query_row(&sql, [id.to_string()], profile_from_row).optional()?)
    }

    /// Upserts the profile. `updated_at` is set to now.
    pub fn save_profile(&self, p: &LoginProfile) -> Result<()> {
        upsert_profile(&self.conn(), p)
    }

    /// Fails with `Store{"profile in use"}` while any host references the profile.
    pub fn delete_profile(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if profile_usage_in(&tx, id)? > 0 {
            return Err(CoreError::store("profile in use"));
        }
        tx.execute("DELETE FROM login_profiles WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// Number of hosts that use the profile.
    pub fn profile_usage(&self, id: Id) -> Result<u32> {
        let conn = self.conn();
        profile_usage_in(&conn, id)
    }

    // ---- groups ----

    pub fn list_groups(&self) -> Result<Vec<Group>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id, name, sort_order FROM groups ORDER BY sort_order, name COLLATE NOCASE, id")?;
        let groups = stmt
            .query_map([], |row| Ok(Group { id: id_col(row, 0)?, name: row.get(1)?, sort_order: row.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(groups)
    }

    pub fn save_group(&self, g: &Group) -> Result<()> {
        upsert_group(&self.conn(), g)
    }

    /// Deletes the group; its hosts get `group_id = NULL`.
    pub fn delete_group(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("UPDATE hosts SET group_id = NULL WHERE group_id = ?1", [id.to_string()])?;
        tx.execute("DELETE FROM groups WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    // ---- snippets ----

    pub fn list_snippets(&self) -> Result<Vec<Snippet>> {
        let conn = self.conn();
        let sql = format!("SELECT {SNIPPET_COLUMNS} FROM snippets ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let snippets = stmt.query_map([], snippet_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(snippets)
    }

    pub fn get_snippet(&self, id: Id) -> Result<Option<Snippet>> {
        let conn = self.conn();
        let sql = format!("SELECT {SNIPPET_COLUMNS} FROM snippets WHERE id = ?1");
        Ok(conn.query_row(&sql, [id.to_string()], snippet_from_row).optional()?)
    }

    /// Upserts the snippet (`created_at` of an existing row is kept, `updated_at` set to now).
    /// Fails with `Config` when `host_id` names no stored host.
    pub fn save_snippet(&self, s: &Snippet) -> Result<()> {
        let conn = self.conn();
        if let Some(host_id) = s.host_id {
            let known = conn
                .query_row("SELECT 1 FROM hosts WHERE id = ?1", [host_id.to_string()], |_| Ok(()))
                .optional()?
                .is_some();
            if !known {
                return Err(CoreError::config("Der Host des Snippets existiert nicht"));
            }
        }
        upsert_snippet(&conn, s)
    }

    pub fn delete_snippet(&self, id: Id) -> Result<()> {
        self.conn().execute("DELETE FROM snippets WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn();
        Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        upsert_setting(&self.conn(), key, value)
    }

    // ---- import ----

    /// Upserts everything an import writes in one transaction: all of it or nothing.
    pub fn save_import(
        &self,
        groups: &[Group],
        profiles: &[LoginProfile],
        hosts: &[Host],
        snippets: &[Snippet],
        settings: &[(&str, &str)],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for g in groups {
            upsert_group(&tx, g)?;
        }
        for p in profiles {
            upsert_profile(&tx, p)?;
        }
        for h in hosts {
            upsert_host(&tx, h)?;
        }
        for s in snippets {
            upsert_snippet(&tx, s)?;
        }
        for (key, value) in settings {
            upsert_setting(&tx, key, value)?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn upsert_group(conn: &Connection, g: &Group) -> Result<()> {
    conn.execute(
        "INSERT INTO groups (id, name, sort_order) VALUES (?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, sort_order = excluded.sort_order",
        params![g.id.to_string(), g.name, g.sort_order],
    )?;
    Ok(())
}

fn upsert_profile(conn: &Connection, p: &LoginProfile) -> Result<()> {
    conn.execute(
        "INSERT INTO login_profiles (id, name, user, method, key_path, has_secret, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, user = excluded.user, method = excluded.method, \
         key_path = excluded.key_path, has_secret = excluded.has_secret, updated_at = excluded.updated_at",
        params![
            p.id.to_string(),
            p.name,
            p.user,
            auth_method_to_db(p.auth.method),
            p.auth.key_path,
            p.auth.has_secret,
            p.created_at,
            now_unix(),
        ],
    )?;
    Ok(())
}

fn upsert_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn upsert_snippet(conn: &Connection, s: &Snippet) -> Result<()> {
    conn.execute(
        "INSERT INTO snippets (id, name, command, host_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, command = excluded.command, \
         host_id = excluded.host_id, updated_at = excluded.updated_at",
        params![s.id.to_string(), s.name, s.command, s.host_id.map(|id| id.to_string()), s.created_at, now_unix()],
    )?;
    Ok(())
}

fn snippet_from_row(row: &Row<'_>) -> rusqlite::Result<Snippet> {
    Ok(Snippet {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        host_id: opt_id_col(row, 3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn upsert_host(tx: &Transaction<'_>, host: &Host) -> Result<()> {
    let (profile_id, method, key_path, has_secret) = match &host.auth {
        HostAuth::Profile { profile_id } => (Some(profile_id.to_string()), None, None, false),
        HostAuth::Custom { auth } => {
            (None, Some(auth_method_to_db(auth.method)), auth.key_path.clone(), auth.has_secret)
        }
    };
    let env = serde_json::to_string(&host.env).map_err(|e| CoreError::internal(e.to_string()))?;
    let sftp = serde_json::to_string(&host.sftp).map_err(|e| CoreError::internal(e.to_string()))?;
    let sql = format!(
        "INSERT INTO hosts ({HOST_COLUMNS}) VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, address = excluded.address, port = excluded.port, \
         user = excluded.user, group_id = excluded.group_id, color = excluded.color, notes = excluded.notes, \
         protocol_ssh = excluded.protocol_ssh, protocol_sftp = excluded.protocol_sftp, \
         login_profile_id = excluded.login_profile_id, auth_method = excluded.auth_method, \
         key_path = excluded.key_path, has_secret = excluded.has_secret, jump_host_id = excluded.jump_host_id, \
         keepalive_s = excluded.keepalive_s, timeout_s = excluded.timeout_s, \
         start_command = excluded.start_command, env = excluded.env, sftp_settings = excluded.sftp_settings, \
         source = excluded.source, updated_at = excluded.updated_at, \
         terminal_scheme = excluded.terminal_scheme, terminal_font_size = excluded.terminal_font_size"
    );
    tx.execute(
        &sql,
        params![
            host.id.to_string(),
            host.name,
            host.address,
            host.port,
            host.user,
            host.group_id.map(|g| g.to_string()),
            host.color,
            host.notes,
            host.protocol_ssh,
            host.protocol_sftp,
            profile_id,
            method,
            key_path,
            has_secret,
            host.jump_host_id.map(|j| j.to_string()),
            host.keepalive_s,
            host.timeout_s,
            host.start_command,
            env,
            sftp,
            source_to_db(host.source),
            host.created_at,
            now_unix(),
            host.terminal_scheme,
            host.terminal_font_size,
        ],
    )?;

    let host_id = host.id.to_string();
    tx.execute("DELETE FROM host_tags WHERE host_id = ?1", [&host_id])?;
    for tag in &host.tags {
        tx.execute("INSERT OR IGNORE INTO host_tags (host_id, tag) VALUES (?1, ?2)", params![host_id, tag])?;
    }
    tx.execute("DELETE FROM port_forwards WHERE host_id = ?1", [&host_id])?;
    for (position, fwd) in host.forwards.iter().enumerate() {
        tx.execute(
            "INSERT INTO port_forwards (id, host_id, type, bind, target, autostart, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                fwd.id.to_string(),
                host_id,
                forward_kind_to_db(fwd.kind),
                fwd.bind,
                fwd.target,
                fwd.autostart,
                position as i64,
            ],
        )?;
    }
    Ok(())
}

fn load_host_children(conn: &Connection, host: &mut Host) -> Result<()> {
    let host_id = host.id.to_string();
    let mut stmt = conn.prepare("SELECT tag FROM host_tags WHERE host_id = ?1 ORDER BY rowid")?;
    host.tags = stmt.query_map([&host_id], |row| row.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, type, bind, target, autostart FROM port_forwards WHERE host_id = ?1 ORDER BY position, rowid",
    )?;
    host.forwards = stmt
        .query_map([&host_id], |row| {
            Ok(PortForward {
                id: id_col(row, 0)?,
                kind: forward_kind_from_db(&row.get::<_, String>(1)?).map_err(|e| conv_err(1, e))?,
                bind: row.get(2)?,
                target: row.get(3)?,
                autostart: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(())
}

fn profile_usage_in(conn: &Connection, id: Id) -> Result<u32> {
    Ok(conn.query_row("SELECT COUNT(*) FROM hosts WHERE login_profile_id = ?1", [id.to_string()], |row| row.get(0))?)
}

fn host_from_row(row: &Row<'_>) -> rusqlite::Result<Host> {
    let profile_id = opt_id_col(row, 10)?;
    let auth = match profile_id {
        Some(profile_id) => HostAuth::Profile { profile_id },
        None => {
            let method: Option<String> = row.get(11)?;
            let method = method.unwrap_or_else(|| "password".to_string());
            HostAuth::Custom {
                auth: AuthConfig {
                    method: auth_method_from_db(&method).map_err(|e| conv_err(11, e))?,
                    key_path: row.get(12)?,
                    has_secret: row.get(13)?,
                },
            }
        }
    };
    let env: String = row.get(18)?;
    let sftp: String = row.get(19)?;
    let source: String = row.get(20)?;
    Ok(Host {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        address: row.get(2)?,
        port: row.get(3)?,
        user: row.get(4)?,
        group_id: opt_id_col(row, 5)?,
        color: row.get(6)?,
        notes: row.get(7)?,
        tags: Vec::new(),
        protocol_ssh: row.get(8)?,
        protocol_sftp: row.get(9)?,
        auth,
        jump_host_id: opt_id_col(row, 14)?,
        keepalive_s: row.get(15)?,
        timeout_s: row.get(16)?,
        start_command: row.get(17)?,
        env: serde_json::from_str(&env).map_err(|e| conv_err(18, e))?,
        sftp: serde_json::from_str(&sftp).map_err(|e| conv_err(19, e))?,
        forwards: Vec::new(),
        terminal_scheme: row.get(23)?,
        terminal_font_size: row.get(24)?,
        source: source_from_db(&source).map_err(|e| conv_err(20, e))?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

fn profile_from_row(row: &Row<'_>) -> rusqlite::Result<LoginProfile> {
    let method: String = row.get(3)?;
    Ok(LoginProfile {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        user: row.get(2)?,
        auth: AuthConfig {
            method: auth_method_from_db(&method).map_err(|e| conv_err(3, e))?,
            key_path: row.get(4)?,
            has_secret: row.get(5)?,
        },
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn conv_err(idx: usize, err: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err))
}

fn id_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Id> {
    let s: String = row.get(idx)?;
    Id::parse_str(&s).map_err(|e| conv_err(idx, e))
}

fn opt_id_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<Id>> {
    let s: Option<String> = row.get(idx)?;
    s.map(|s| Id::parse_str(&s).map_err(|e| conv_err(idx, e))).transpose()
}

fn auth_method_to_db(method: AuthMethod) -> &'static str {
    match method {
        AuthMethod::Password => "password",
        AuthMethod::Key => "key",
        AuthMethod::Agent => "agent",
        AuthMethod::KeyboardInteractive => "keyboard_interactive",
    }
}

fn auth_method_from_db(s: &str) -> Result<AuthMethod> {
    match s {
        "password" => Ok(AuthMethod::Password),
        "key" => Ok(AuthMethod::Key),
        "agent" => Ok(AuthMethod::Agent),
        "keyboard_interactive" => Ok(AuthMethod::KeyboardInteractive),
        other => Err(CoreError::store(format!("unknown auth method {other:?}"))),
    }
}

fn forward_kind_to_db(kind: ForwardKind) -> &'static str {
    match kind {
        ForwardKind::Local => "L",
        ForwardKind::Remote => "R",
        ForwardKind::Dynamic => "D",
    }
}

fn forward_kind_from_db(s: &str) -> Result<ForwardKind> {
    match s {
        "L" => Ok(ForwardKind::Local),
        "R" => Ok(ForwardKind::Remote),
        "D" => Ok(ForwardKind::Dynamic),
        other => Err(CoreError::store(format!("unknown forward type {other:?}"))),
    }
}

fn source_to_db(source: HostSource) -> &'static str {
    match source {
        HostSource::Linascp => "linascp",
        HostSource::SshConfig => "ssh_config",
    }
}

fn source_from_db(s: &str) -> Result<HostSource> {
    match s {
        "linascp" => Ok(HostSource::Linascp),
        "ssh_config" => Ok(HostSource::SshConfig),
        other => Err(CoreError::store(format!("unknown host source {other:?}"))),
    }
}

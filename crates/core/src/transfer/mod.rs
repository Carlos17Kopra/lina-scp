//! File transfers between the local file system and an SFTP session (TP3b).

mod copy;
mod local_side;
mod queue;
mod side;
#[cfg(test)]
mod test_support;

pub use local_side::LocalSide;
pub use queue::{TransferEmit, TransferQueue};
pub use side::{Child, Node, NodeKind, Reader, Side, Writer};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::model::SftpSettings;

pub type JobId = u64;
pub type BatchId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    File,
    Dir,
}

/// Why a job was skipped without a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobNote {
    DirLink,
    BrokenLink,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    WaitingDecision,
    /// The connection is gone; resumes after a reconnect.
    Paused,
    Done,
    Skipped,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Done | Self::Skipped | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: JobId,
    pub batch_id: BatchId,
    pub direction: Direction,
    /// Absolute path on the source side.
    pub source: String,
    /// Absolute path on the target side.
    pub target: String,
    /// Path relative to the batch, for display.
    pub rel: String,
    pub kind: JobKind,
    pub size: u64,
    /// Bytes confirmed written (contiguous from 0).
    pub done: u64,
    pub state: JobState,
    pub note: Option<JobNote>,
    pub error: Option<CoreError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub size: u64,
    pub mtime: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Overwrite,
    Skip,
    /// Overwrite only when the source has a newer mtime.
    Newer,
    CancelBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CancelTarget {
    Job { id: JobId },
    Batch { id: BatchId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TransferEvent {
    /// Full job on creation and on every state change.
    Job { job: Job },
    /// Throttled byte progress of a running file.
    Progress { job_id: JobId, done: u64 },
    /// The target exists; answer with `TransferQueue::resolve`.
    Conflict { job_id: JobId, source: FileStamp, target: FileStamp },
    /// Every job of the batch reached a final state.
    BatchDone { batch_id: BatchId, target_dir: String, direction: Direction },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferOptions {
    /// Files transferred at the same time (directories do not count).
    pub parallel: usize,
    /// Host setting "Rechte beim Upload".
    pub upload_mode: Option<u32>,
}

impl TransferOptions {
    /// Values are validated on save; fall back anyway (1 file, no mode) so a bad row never breaks transfers.
    pub fn from_settings(settings: &SftpSettings) -> Self {
        let upload_mode = settings
            .upload_perms
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty() && m.len() <= 4 && m.chars().all(|c| ('0'..='7').contains(&c)))
            .and_then(|m| u32::from_str_radix(m, 8).ok());
        Self { parallel: (settings.parallel as usize).max(1), upload_mode }
    }

    pub fn file_mode(&self, direction: Direction, source_mode: u32) -> u32 {
        match direction {
            Direction::Upload => self.upload_mode.unwrap_or(source_mode & 0o777),
            Direction::Download => (source_mode & 0o777) | 0o600,
        }
    }

    pub fn dir_mode(&self, direction: Direction, source_mode: u32) -> u32 {
        match direction {
            // x wherever r is set: 0644 -> 0755.
            Direction::Upload => self.upload_mode.map(|m| m | ((m & 0o444) >> 2)).unwrap_or(source_mode & 0o777),
            Direction::Download => (source_mode & 0o777) | 0o700,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SftpSettings;

    #[test]
    fn upload_mode_applies_to_files_and_adds_x_for_dirs() {
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o644) };
        assert_eq!(o.file_mode(Direction::Upload, 0o100600), 0o644);
        assert_eq!(o.dir_mode(Direction::Upload, 0o40700), 0o755);
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o640) };
        assert_eq!(o.dir_mode(Direction::Upload, 0o40700), 0o750);
    }

    #[test]
    fn without_upload_mode_the_local_mode_is_kept() {
        let o = TransferOptions { parallel: 4, upload_mode: None };
        assert_eq!(o.file_mode(Direction::Upload, 0o100751), 0o751);
        assert_eq!(o.dir_mode(Direction::Upload, 0o40711), 0o711);
    }

    #[test]
    fn downloads_keep_the_remote_mode_but_stay_usable() {
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o666) };
        assert_eq!(o.file_mode(Direction::Download, 0o100600), 0o600);
        assert_eq!(o.file_mode(Direction::Download, 0o100444), 0o644);
        assert_eq!(o.dir_mode(Direction::Download, 0o40500), 0o700);
    }

    #[test]
    fn options_from_settings_fall_back_on_invalid_values() {
        let mut s = SftpSettings { parallel: 3, upload_perms: Some("0640".into()), ..SftpSettings::default() };
        assert_eq!(TransferOptions::from_settings(&s), TransferOptions { parallel: 3, upload_mode: Some(0o640) });
        s.parallel = 0;
        s.upload_perms = Some("rwx".into());
        assert_eq!(TransferOptions::from_settings(&s), TransferOptions { parallel: 1, upload_mode: None });
        s.upload_perms = Some("  ".into());
        assert_eq!(TransferOptions::from_settings(&s).upload_mode, None);
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let event = TransferEvent::Progress { job_id: 7, done: 10 };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "progress", "jobId": 7, "done": 10 })
        );
        let target = CancelTarget::Batch { id: 2 };
        assert_eq!(serde_json::to_value(target).unwrap(), serde_json::json!({ "type": "batch", "id": 2 }));
    }

    #[test]
    fn final_states() {
        use JobState::*;
        for s in [Done, Skipped, Failed, Cancelled] {
            assert!(s.is_final());
        }
        for s in [Queued, Running, WaitingDecision, Paused] {
            assert!(!s.is_final());
        }
    }
}

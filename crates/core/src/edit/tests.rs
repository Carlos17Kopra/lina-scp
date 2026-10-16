use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::transfer::test_support::{FaultySide, Recorder, p};
use crate::transfer::{LocalSide, Side, TransferEvent, TransferOptions};

#[derive(Default)]
struct FakeLauncher {
    calls: Mutex<Vec<PathBuf>>,
    apps: Mutex<Vec<Option<String>>>,
    fail: AtomicBool,
}

impl Launcher for FakeLauncher {
    fn open(&self, path: &Path, app: Option<&str>) -> Result<()> {
        self.calls.lock().unwrap().push(path.to_path_buf());
        self.apps.lock().unwrap().push(app.map(str::to_string));
        if self.fail.load(Ordering::SeqCst) {
            return Err(CoreError::config("no text editor found"));
        }
        Ok(())
    }
}

struct Env {
    _dir: tempfile::TempDir,
    remote: PathBuf,
    root: PathBuf,
    queue: TransferQueue,
    transfers: Recorder,
    events: Arc<Mutex<Vec<EditEvent>>>,
    launcher: Arc<FakeLauncher>,
    mgr: EditManager,
}

impl Env {
    fn launches(&self) -> usize {
        self.launcher.calls.lock().unwrap().len()
    }
    fn copy(&self) -> PathBuf {
        self.launcher.calls.lock().unwrap().last().cloned().expect("launched")
    }
    fn last(&self, id: EditId) -> Option<EditFile> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|EditEvent::File { file }| (file.id == id).then(|| file.clone()))
    }
    async fn wait_state(&self, id: EditId, state: EditState) -> EditFile {
        for _ in 0..500 {
            if let Some(file) = self.last(id).filter(|f| f.state == state) {
                return file;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {state:?}: {:#?}", self.events.lock().unwrap());
    }
    fn uploads(&self) -> usize {
        self.transfers
            .events()
            .iter()
            .filter(|e| matches!(e, TransferEvent::BatchDone { direction: crate::transfer::Direction::Upload, .. }))
            .count()
    }
}

fn env_with(remote: Arc<dyn Side>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let (remote_dir, root) = (base.join("remote"), base.join("cache/edit"));
    std::fs::create_dir_all(&remote_dir).unwrap();
    let transfers = Recorder::default();
    let queue = TransferQueue::new(
        Arc::new(LocalSide),
        TransferOptions { parallel: 4, upload_mode: Some(0o600), bandwidth: None },
        transfers.emit(),
    );
    queue.set_remote(Some(remote));
    let events: Arc<Mutex<Vec<EditEvent>>> = Arc::default();
    let sink = events.clone();
    let launcher = Arc::new(FakeLauncher::default());
    let mgr = EditManager::new(
        queue.clone(),
        root.clone(),
        launcher.clone(),
        Arc::new(move |e| sink.lock().unwrap().push(e)),
        Duration::from_millis(20),
    );
    Env { _dir: dir, remote: remote_dir, root, queue, transfers, events, launcher, mgr }
}

fn env() -> Env {
    env_with(Arc::new(LocalSide))
}

fn remote_file(e: &Env, name: &str, content: &[u8], mode: u32) -> PathBuf {
    let path = e.remote.join(name);
    std::fs::write(&path, content).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Editors write within the same clock tick as the download otherwise.
async fn save(path: &Path, content: &[u8]) {
    tokio::time::sleep(Duration::from_millis(15)).await;
    std::fs::write(path, content).unwrap();
}

#[tokio::test]
async fn open_downloads_a_private_copy_and_launches_once() {
    let e = env();
    let remote = remote_file(&e, "run.sh", b"echo hi", 0o755);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    let copy = e.copy();
    assert_eq!(std::fs::read(&copy).unwrap(), b"echo hi");
    assert_eq!(copy.file_name().unwrap(), "run.sh");
    assert_eq!(mode_of(&copy), 0o644);
    assert_eq!(mode_of(copy.parent().unwrap()), 0o700);
    assert_eq!(mode_of(&e.root), 0o700);
    assert_eq!(e.launches(), 1);
    let file = e.last(id).unwrap();
    assert_eq!((file.state, file.dirty, file.name.as_str()), (EditState::Synced, false, "run.sh"));
    assert_eq!(file.remote_path, p(&remote));
}

#[tokio::test]
async fn opening_the_same_file_again_only_relaunches() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"x", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    assert_eq!(e.mgr.open(&p(&remote)).await.unwrap(), id);
    assert_eq!(e.launches(), 2);
    let downloads = e.transfers.events().iter().filter(|ev| matches!(ev, TransferEvent::BatchDone { .. })).count();
    assert_eq!(downloads, 1);
}

#[tokio::test]
async fn the_chosen_app_opens_the_copy_and_is_kept_for_reopen() {
    let e = env();
    let remote = remote_file(&e, "a.png", b"x", 0o644);
    let id = e.mgr.open_with(&p(&remote), Some("viewer.desktop".into())).await.unwrap();
    e.mgr.reopen(id).unwrap();
    e.mgr.open_with(&p(&remote), Some("gimp.desktop".into())).await.unwrap();
    e.mgr.reopen(id).unwrap();
    e.mgr.open(&p(&remote)).await.unwrap();
    let apps = e.launcher.apps.lock().unwrap().clone();
    let apps: Vec<_> = apps.iter().map(Option::as_deref).collect();
    assert_eq!(
        apps,
        [Some("viewer.desktop"), Some("viewer.desktop"), Some("gimp.desktop"), Some("gimp.desktop"), None]
    );
}

#[tokio::test]
async fn local_change_is_uploaded_and_keeps_the_remote_mode() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o640);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    save(&e.copy(), b"new content").await;
    e.wait_state(id, EditState::Changed).await;
    let file = e.wait_state(id, EditState::Synced).await;
    assert!(!file.dirty);
    assert_eq!(std::fs::read(&remote).unwrap(), b"new content");
    assert_eq!(mode_of(&remote), 0o640);
}

#[tokio::test]
async fn rename_replace_save_is_detected() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    let copy = e.copy();
    let tmp = copy.with_file_name(".a.conf.swp");
    save(&tmp, b"renamed in").await;
    std::fs::rename(&tmp, &copy).unwrap();
    e.wait_state(id, EditState::Changed).await;
    e.wait_state(id, EditState::Synced).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"renamed in");
}

#[tokio::test]
async fn change_during_upload_uploads_again() {
    let side = FaultySide::new();
    let e = env_with(side.clone());
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    side.delay_renames(300);
    save(&e.copy(), b"first").await;
    e.wait_state(id, EditState::Uploading).await;
    save(&e.copy(), b"second, longer").await;
    for _ in 0..500 {
        if std::fs::read(&remote).unwrap() == b"second, longer" && e.last(id).unwrap().state == EditState::Synced {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(std::fs::read(&remote).unwrap(), b"second, longer");
    assert_eq!(e.last(id).unwrap().state, EditState::Synced);
    assert_eq!(e.uploads(), 2);
}

#[tokio::test]
async fn remote_change_asks_and_overwrite_uploads() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    std::fs::write(&remote, b"changed by someone").unwrap();
    save(&e.copy(), b"mine").await;
    let file = e.wait_state(id, EditState::Conflict).await;
    assert!(file.dirty);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"changed by someone");
    e.mgr.resolve(id, true).unwrap();
    e.wait_state(id, EditState::Synced).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"mine");
}

#[tokio::test]
async fn keep_does_not_upload_and_asks_again_on_next_change() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    std::fs::write(&remote, b"changed by someone").unwrap();
    save(&e.copy(), b"mine").await;
    e.wait_state(id, EditState::Conflict).await;
    e.mgr.resolve(id, false).unwrap();
    let file = e.wait_state(id, EditState::Changed).await;
    assert!(file.dirty);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(e.last(id).unwrap().state, EditState::Changed, "asked again without a new change");
    assert_eq!(std::fs::read(&remote).unwrap(), b"changed by someone");
    save(&e.copy(), b"mine, again").await;
    e.wait_state(id, EditState::Conflict).await;
    assert_eq!(e.uploads(), 0);
}

#[tokio::test]
async fn deleted_remote_file_is_recreated() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o640);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    std::fs::remove_file(&remote).unwrap();
    save(&e.copy(), b"back").await;
    e.wait_state(id, EditState::Changed).await;
    e.wait_state(id, EditState::Synced).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"back");
    assert_eq!(mode_of(&remote), 0o640);
}

#[tokio::test]
async fn symlinked_remote_file_stays_a_link() {
    let e = env();
    let real = remote_file(&e, "real.txt", b"old", 0o644);
    let link = e.remote.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let id = e.mgr.open(&p(&link)).await.unwrap();
    assert_eq!(e.last(id).unwrap().remote_path, p(&real));
    save(&e.copy(), b"through the link").await;
    e.wait_state(id, EditState::Changed).await;
    e.wait_state(id, EditState::Synced).await;
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&real).unwrap(), b"through the link");
}

#[tokio::test]
async fn upload_waits_while_disconnected() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    e.queue.set_remote(None);
    save(&e.copy(), b"offline edit").await;
    e.wait_state(id, EditState::Changed).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"old");
    e.queue.set_remote(Some(Arc::new(LocalSide)));
    e.wait_state(id, EditState::Synced).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"offline edit");
}

#[tokio::test]
async fn stop_after_upload_removes_the_copy() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    let copy = e.copy();
    assert_eq!(e.mgr.stop(id).await.unwrap(), None);
    assert!(!copy.parent().unwrap().exists());
    assert!(e.mgr.stop(id).await.is_err(), "unknown id");
}

#[tokio::test]
async fn stop_with_dirty_copy_keeps_it_and_returns_its_path() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    e.queue.set_remote(None);
    save(&e.copy(), b"not uploaded").await;
    assert_eq!(e.mgr.stop(id).await.unwrap(), Some(e.copy()));
    assert_eq!(std::fs::read(e.copy()).unwrap(), b"not uploaded");
}

#[tokio::test]
async fn close_keeps_only_dirty_copies() {
    let e = env();
    let clean = remote_file(&e, "clean.conf", b"c", 0o644);
    let dirty = remote_file(&e, "dirty.conf", b"d", 0o644);
    e.mgr.open(&p(&clean)).await.unwrap();
    let clean_copy = e.copy();
    e.mgr.open(&p(&dirty)).await.unwrap();
    let dirty_copy = e.copy();
    e.queue.set_remote(None);
    save(&dirty_copy, b"dd").await;
    assert_eq!(e.mgr.close().await, vec![dirty_copy.clone()]);
    assert!(!clean_copy.exists());
    assert!(dirty_copy.exists());
}

#[tokio::test]
async fn opening_a_directory_fails() {
    let e = env();
    std::fs::create_dir(e.remote.join("d")).unwrap();
    assert!(e.mgr.open(&p(&e.remote.join("d"))).await.is_err());
    assert_eq!(e.launches(), 0);
}

#[tokio::test]
async fn a_failing_launcher_still_watches_the_copy() {
    let e = env();
    e.launcher.fail.store(true, Ordering::SeqCst);
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    assert!(e.mgr.open(&p(&remote)).await.is_err());
    let id = e.last_id();
    e.mgr.reopen(id).unwrap_err();
    e.launcher.fail.store(false, Ordering::SeqCst);
    e.mgr.reopen(id).unwrap();
    save(&e.copy(), b"later").await;
    e.wait_state(id, EditState::Changed).await;
    e.wait_state(id, EditState::Synced).await;
    assert_eq!(std::fs::read(&remote).unwrap(), b"later");
}

impl Env {
    fn last_id(&self) -> EditId {
        self.events.lock().unwrap().last().map(|EditEvent::File { file }| file.id).expect("an edit event")
    }
}

#[tokio::test]
async fn opening_while_downloading_does_not_launch_early() {
    let side = FaultySide::new();
    let e = env_with(side.clone());
    let remote = remote_file(&e, "a.conf", b"content", 0o644);
    side.delay_reads(200);
    let first = {
        let (mgr, path) = (e.mgr.clone(), p(&remote));
        tokio::spawn(async move { mgr.open(&path).await })
    };
    for _ in 0..100 {
        if e.last_id_opt().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let again = e.mgr.open(&p(&remote)).await.unwrap();
    assert_eq!(e.launches(), 0, "no editor on a file that is not there yet");
    assert_eq!(first.await.unwrap().unwrap(), again);
    assert_eq!(e.launches(), 1);
    assert_eq!(std::fs::read(e.copy()).unwrap(), b"content");
}

#[tokio::test]
async fn opening_after_a_failed_download_downloads_again() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"content", 0o000);
    assert!(e.mgr.open(&p(&remote)).await.is_err());
    assert_eq!(e.launches(), 0);
    std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o644)).unwrap();
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    assert_eq!(std::fs::read(e.copy()).unwrap(), b"content");
    assert_eq!(e.last(id).unwrap().state, EditState::Synced);
}

#[tokio::test]
async fn stop_keeps_the_dir_while_the_copy_is_moved_away() {
    let e = env();
    let remote = remote_file(&e, "a.conf", b"old", 0o644);
    let id = e.mgr.open(&p(&remote)).await.unwrap();
    let copy = e.copy();
    std::fs::rename(&copy, copy.with_file_name("a.conf~")).unwrap();
    assert_eq!(e.mgr.stop(id).await.unwrap(), Some(copy.clone()));
    assert!(copy.with_file_name("a.conf~").exists());
}

impl Env {
    fn last_id_opt(&self) -> Option<EditId> {
        self.events.lock().unwrap().last().map(|EditEvent::File { file }| file.id)
    }
}

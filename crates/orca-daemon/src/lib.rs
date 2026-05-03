//! Orca daemon runtime and IPC entrypoints.

use std::collections::HashMap;
use std::ffi::c_int;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use orca_core::{Task, TaskId};
use thiserror::Error;
use tracing::{debug, info, warn};

static TERMINATE: AtomicBool = AtomicBool::new(false);

const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;
const LOCK_EX: c_int = 2;
const LOCK_NB: c_int = 4;

unsafe extern "C" {
    fn flock(fd: c_int, operation: c_int) -> c_int;
    fn signal(sig: c_int, handler: extern "C" fn(c_int)) -> usize;
    fn getpid() -> c_int;
}

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub project_dir: PathBuf,
}

impl DaemonOptions {
    /// Creates a default options set rooted at the current working directory.
    pub fn from_current_dir() -> std::io::Result<Self> {
        Ok(Self {
            project_dir: std::env::current_dir()?,
        })
    }
}

pub struct DaemonGuard {
    _pid_file: File,
    pid_path: PathBuf,
    sock_path: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.sock_path);
        let _ = fs::remove_file(&self.pid_path);
    }
}

/// Runs the daemon loop until SIGINT/SIGTERM is received.
pub fn run_daemon(options: &DaemonOptions) -> std::io::Result<()> {
    info!(project_dir = %options.project_dir.display(), "starting daemon");
    register_signal_handlers();

    let state_dir = options.project_dir.join(".orca/state");
    fs::create_dir_all(&state_dir)?;

    let pid_path = state_dir.join("daemon.pid");
    let pid_file = acquire_pid_lock(&pid_path)?;
    write_pid(&pid_file)?;

    let sock_path = state_dir.join("daemon.sock");
    if sock_path.exists() {
        fs::remove_file(&sock_path)?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    listener.set_nonblocking(true)?;

    let _guard = DaemonGuard {
        _pid_file: pid_file,
        pid_path,
        sock_path,
    };

    while !TERMINATE.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => handle_client(stream)?,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(err),
        }
    }

    info!("stopping daemon");
    Ok(())
}

/// Durable daemon state store backed by `.orca/state/tasks`.
#[derive(Debug)]
pub struct StateStore {
    root: PathBuf,
    tasks: HashMap<TaskId, Task>,
}

impl StateStore {
    /// Loads all tasks from `.orca/state/tasks` if present.
    pub fn load(project_dir: &Path) -> Result<Self, StateStoreError> {
        let root = project_dir.join(".orca/state");
        let mut store = Self {
            root: root.clone(),
            tasks: HashMap::new(),
        };

        let tasks_dir = root.join("tasks");
        if !tasks_dir.exists() {
            return Ok(store);
        }

        for entry in fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let file_path = path.join("task.toml");
            if !file_path.exists() {
                continue;
            }
            let task = Task::from_file(&file_path).map_err(StateStoreError::Task)?;
            store.tasks.insert(task.id.clone(), task);
        }

        Ok(store)
    }

    pub fn create_task(&mut self, task: Task) -> Result<(), StateStoreError> {
        if self.tasks.contains_key(&task.id) {
            return Err(StateStoreError::TaskAlreadyExists(
                task.id.as_str().to_owned(),
            ));
        }
        self.write_task_to_disk(&task)?;
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn update_task(&mut self, task: Task) -> Result<(), StateStoreError> {
        self.write_task_to_disk(&task)?;
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn get_task(&self, task_id: &TaskId) -> Option<&Task> {
        self.tasks.get(task_id)
    }

    pub fn list_tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    pub fn reconcile_task_file(
        &mut self,
        file_path: &Path,
    ) -> Result<Option<String>, StateStoreError> {
        let task = Task::from_file(file_path).map_err(StateStoreError::Task)?;
        let task_id = task.id.clone();
        let message = if let Some(previous) = self.tasks.get(&task_id) {
            if previous != &task {
                Some(format!(
                    "external update detected for task {}",
                    task_id.as_str()
                ))
            } else {
                None
            }
        } else {
            Some(format!("task {} ingested", task_id.as_str()))
        };
        self.tasks.insert(task_id, task);
        Ok(message)
    }

    fn write_task_to_disk(&self, task: &Task) -> Result<(), StateStoreError> {
        let task_dir = self.root.join("tasks").join(
            task.id
                .as_str()
                .trim_matches(|c: char| c == '/' || c == '\\'),
        );
        fs::create_dir_all(&task_dir)?;
        task.write_atomic(&task_dir.join("task.toml"))
            .map_err(StateStoreError::Task)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StateStoreError {
    #[error("state store I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("task serialization error: {0}")]
    Task(#[from] orca_core::TaskError),
    #[error("task `{0}` already exists")]
    TaskAlreadyExists(String),
    #[error("notify watcher error: {0}")]
    Notify(String),
}

pub struct FilesystemWatcher {
    _watcher: RecommendedWatcher,
    _thread: JoinHandle<()>,
}

pub fn start_filesystem_watcher(
    store: Arc<Mutex<StateStore>>,
    project_dir: &Path,
) -> Result<FilesystemWatcher, StateStoreError> {
    let (sender, receiver) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = sender.send(event);
        },
        NotifyConfig::default(),
    )
    .map_err(|err| StateStoreError::Notify(err.to_string()))?;

    let tasks_dir = project_dir.join(".orca/state/tasks");
    let config_path = project_dir.join(".orca/config.toml");
    fs::create_dir_all(&tasks_dir)?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !config_path.exists() {
        fs::write(&config_path, "")?;
    }

    watcher
        .watch(&tasks_dir, RecursiveMode::Recursive)
        .map_err(|err| StateStoreError::Notify(err.to_string()))?;
    watcher
        .watch(&config_path, RecursiveMode::NonRecursive)
        .map_err(|err| StateStoreError::Notify(err.to_string()))?;

    let thread = std::thread::spawn(move || {
        while let Ok(event_result) = receiver.recv() {
            let Ok(event) = event_result else {
                continue;
            };

            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any
            ) {
                continue;
            }

            for path in event.paths {
                if path.ends_with("task.toml") {
                    if let Ok(mut guard) = store.lock() {
                        match guard.reconcile_task_file(&path) {
                            Ok(Some(message)) => info!("{message}"),
                            Ok(None) => {}
                            Err(error) => warn!(%error, "failed to reconcile changed task file"),
                        }
                    }
                } else if path.ends_with(".orca/config.toml") {
                    warn!("external config change detected; reconciliation not yet implemented");
                }
            }
        }
    });

    Ok(FilesystemWatcher {
        _watcher: watcher,
        _thread: thread,
    })
}

fn handle_client(stream: UnixStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }

        let response = match line.trim() {
            "{\"type\":\"ping\"}" => {
                debug!("received ping request");
                "{\"type\":\"pong\"}\n"
            }
            _ => {
                warn!(request = %line.trim(), "received unknown request");
                "{\"type\":\"error\",\"message\":\"unknown request\"}\n"
            }
        };
        writer.write_all(response.as_bytes())?;
        writer.flush()?;
    }

    Ok(())
}

fn acquire_pid_lock(pid_path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .read(true)
        .open(pid_path)?;
    // SAFETY: flock is called with a valid file descriptor owned by `file`.
    let code = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if code != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "daemon is already running",
        ));
    }
    Ok(file)
}

fn write_pid(file: &File) -> std::io::Result<()> {
    // SAFETY: getpid takes no arguments and has no side effects beyond returning current PID.
    let pid = unsafe { getpid() };
    let mut clone = file.try_clone()?;
    clone.set_len(0)?;
    clone.write_all(format!("{pid}\n").as_bytes())?;
    clone.flush()?;
    Ok(())
}

extern "C" fn signal_handler(_: c_int) {
    TERMINATE.store(true, Ordering::SeqCst);
}

fn register_signal_handlers() {
    // SAFETY: registering a process signal handler with function pointer of correct ABI.
    unsafe {
        signal(SIGINT, signal_handler);
        signal(SIGTERM, signal_handler);
    }
}

#[cfg(test)]
mod tests {
    use super::StateStore;
    use orca_core::{Task, TaskId, TaskState};

    #[test]
    fn state_store_persists_tasks_across_reload() {
        let root = std::env::temp_dir().join(format!(
            "orca-state-store-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let mut store = StateStore::load(&root).expect("state store should load");

        let task = Task {
            id: TaskId::new("T-001").expect("task id should be valid"),
            title: "Test persistence".to_owned(),
            description: "Task to validate state store persistence".to_owned(),
            state: TaskState::Drafted,
            created_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            assigned_to: None,
            reviewer: None,
            capabilities: vec!["surgical_edit".to_owned()],
            parent: None,
            subtasks: Vec::new(),
            context_files: Vec::new(),
            worktree: None,
            branch: None,
            acceptance: Vec::new(),
            notes: String::new(),
        };

        store
            .create_task(task.clone())
            .expect("task creation should persist");

        let reloaded = StateStore::load(&root).expect("state store should reload");
        let loaded_task = reloaded
            .get_task(&TaskId::new("T-001").expect("task id should be valid"))
            .expect("task should exist after reload");
        assert_eq!(loaded_task, &task);
        assert_eq!(reloaded.list_tasks().len(), 1);
    }

    #[test]
    fn reconcile_task_file_ingests_external_task() {
        let root = std::env::temp_dir().join(format!(
            "orca-reconcile-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be monotonic")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let mut store = StateStore::load(&root).expect("state store should load");

        let task_path = root.join(".orca/state/tasks/T-999/task.toml");
        let task = Task {
            id: TaskId::new("T-999").expect("task id should be valid"),
            title: "External ingest".to_owned(),
            description: "Task created outside daemon".to_owned(),
            state: TaskState::Drafted,
            created_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_at: "2026-04-22T00:00:00Z".to_owned(),
            assigned_to: None,
            reviewer: None,
            capabilities: vec!["surgical_edit".to_owned()],
            parent: None,
            subtasks: Vec::new(),
            context_files: Vec::new(),
            worktree: None,
            branch: None,
            acceptance: Vec::new(),
            notes: String::new(),
        };
        task.write_atomic(&task_path)
            .expect("external task file should be written");

        let message = store
            .reconcile_task_file(&task_path)
            .expect("reconcile should succeed");
        assert_eq!(message, Some("task T-999 ingested".to_owned()));
        assert!(
            store
                .get_task(&TaskId::new("T-999").expect("task id should be valid"))
                .is_some()
        );
    }
}

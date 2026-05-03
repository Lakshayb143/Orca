use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONNECT_RETRIES: usize = 20;
const CONNECT_RETRY_DELAY_MS: u64 = 100;

/// Sends a raw JSON request to the daemon and returns one raw JSON response line.
pub fn request(project_dir: Option<&Path>, request: &str) -> std::io::Result<String> {
    let project_dir = resolve_project_dir(project_dir)?;
    let socket_path = project_dir.join(".orca/state/daemon.sock");

    if !socket_path.exists() {
        launch_daemon(&project_dir)?;
    }

    let mut last_error = None;
    for _ in 0..CONNECT_RETRIES {
        match UnixStream::connect(&socket_path) {
            Ok(mut stream) => {
                stream.write_all(request.as_bytes())?;
                if !request.ends_with('\n') {
                    stream.write_all(b"\n")?;
                }
                stream.flush()?;

                let mut response = String::new();
                let mut reader = BufReader::new(stream);
                reader.read_line(&mut response)?;
                return Ok(response.trim().to_owned());
            }
            Err(err) => {
                last_error = Some(err);
                std::thread::sleep(Duration::from_millis(CONNECT_RETRY_DELAY_MS));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("failed to connect to daemon with unknown error")))
}

fn resolve_project_dir(project_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    match project_dir {
        Some(path) => Ok(path.to_path_buf()),
        None => std::env::current_dir(),
    }
}

fn launch_daemon(project_dir: &Path) -> std::io::Result<()> {
    let current_exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(current_exe);
    command.arg("--project").arg(project_dir).arg("daemon");
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    command.spawn()?;
    Ok(())
}

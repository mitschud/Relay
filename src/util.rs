//! Small shared filesystem/process helpers used by the compiled-in helpers.
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Build a command for a helper compiled into this binary. `env_key` keeps the
/// historical single-helper override; otherwise the current executable is used
/// with its hidden subcommand.
pub fn helper_command(env_key: &str, subcommand: &str) -> std::process::Command {
    if let Ok(path) = std::env::var(env_key) {
        if !path.is_empty() {
            return std::process::Command::new(path);
        }
    }
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("relay"));
    let mut command = std::process::Command::new(executable);
    command.arg(subcommand);
    command
}

pub fn expand_tilde(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        home().join(rest)
    } else if value == "~" {
        home()
    } else {
        PathBuf::from(value)
    }
}

pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

pub fn which(name: &str) -> Option<PathBuf> {
    let expanded = expand_tilde(name);
    if name.contains('/') {
        return is_executable(&expanded).then_some(expanded);
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let candidate = Path::new(dir).join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}


pub fn read_json_file(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn open_lock(path: &Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())
}

pub fn flock(file: &File, operation: libc::c_int) -> Result<(), String> {
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}


pub fn lock_exclusive_nonblocking(path: &Path) -> Result<File, String> {
    let file = open_lock(path)?;
    flock(&file, libc::LOCK_EX | libc::LOCK_NB)?;
    Ok(file)
}

pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path.parent().ok_or("Path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("relay");
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.to_string());
    }
    Ok(())
}

pub fn atomic_write_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    atomic_write(path, text.as_bytes(), 0o644)
}

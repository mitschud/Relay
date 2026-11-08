//! Run one selected local model in its user service; never downloads at launch.
//! This is the former `helpers/relay-local-model` Python helper.
use serde_json::Value;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

fn settings() -> Value {
    crate::util::read_json_file(&crate::util::home().join(".config/relay/settings.json"))
        .unwrap_or_else(|_| serde_json::json!({}))
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            collect_files(&path, files);
        } else if kind.is_file() {
            files.push(path);
        }
    }
}

pub fn main() -> ! {
    match run() {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let configured = args
        .iter()
        .position(|arg| arg == "--config")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let path = match configured {
        Some(path) => path,
        None => settings()
            .get("local_model_config")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or("Set local_model_config in Settings first.")?,
    };
    let path = crate::util::expand_tilde(&path);
    let config = crate::util::read_json_file(&path)?;
    for required in config
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !Path::new(required).is_file() {
            return Err(format!("Model preparation is unfinished: {required}"));
        }
    }
    let root = config
        .get("runtime_root")
        .and_then(Value::as_str)
        .ok_or("local model config has no runtime_root")?;
    let root = crate::util::expand_tilde(root);
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let servers: Vec<&PathBuf> = files
        .iter()
        .filter(|path| path.file_name().and_then(|n| n.to_str()) == Some("llama-server"))
        .collect();
    if servers.len() != 1 {
        return Err(format!(
            "Expected one verified llama-server in {}",
            root.display()
        ));
    }
    let exe = servers[0];
    let mut libdirs: Vec<String> = files
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".so"))
        })
        .filter_map(|path| path.parent().map(|parent| parent.display().to_string()))
        .collect();
    libdirs.sort();
    libdirs.dedup();
    let mut library_path = vec![exe
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default()];
    library_path.extend(libdirs);
    if let Ok(existing) = std::env::var("LD_LIBRARY_PATH") {
        library_path.push(existing);
    }
    let mut command = std::process::Command::new(exe);
    for arg in config
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        command.arg(arg);
    }
    for key in [
        "__GLX_VENDOR_LIBRARY_NAME",
        "LIBGL_ALWAYS_SOFTWARE",
        "GALLIUM_DRIVER",
        "LP_NUM_THREADS",
        "RELAY_SOFTWARE_FRONTEND",
    ] {
        command.env_remove(key);
    }
    command.env("LD_LIBRARY_PATH", library_path.join(":"));
    let error = command.exec();
    Err(format!("Could not start local model: {error}"))
}

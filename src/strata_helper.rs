//! Compiled adapter to Strata's signed updater and drafting planner. This is
//! the former `helpers/relay-strata` Python script.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

const MODES: [&str; 4] = ["auto", "mtp", "dflash2", "off"];

fn emit(kind: &str, fields: Value) {
    let mut object = serde_json::Map::new();
    object.insert("kind".into(), json!(kind));
    if let Value::Object(fields) = fields {
        object.extend(fields);
    }
    println!("{}", Value::Object(object));
    let _ = std::io::stdout().flush();
}

fn settings() -> Value {
    crate::util::read_json_file(&crate::util::home().join(".config/relay/settings.json"))
        .unwrap_or_else(|_| json!({}))
}

fn components_root() -> PathBuf {
    std::env::var("STRATA_COMPONENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::util::home().join(".local/share/strata/components"))
}

fn cli(cfg: &Value) -> Result<Vec<String>, String> {
    if let Some(command) = cfg.get("strata_command") {
        if !command.is_null() {
            let values = command
                .as_array()
                .ok_or("strata_command must be an array of executable and arguments")?;
            if values.iter().any(|v| !v.is_string()) {
                return Err("strata_command must be an array of executable and arguments".into());
            }
            let values: Vec<String> = values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if !values.is_empty() {
                return Ok(values);
            }
        }
    }
    let root = components_root();
    let pointer = root.join("control/active.json");
    if pointer.exists() {
        let active = crate::util::read_json_file(&pointer)?;
        let folder = active
            .get("folder")
            .and_then(Value::as_str)
            .ok_or("Strata active-Control pointer has no folder")?;
        let folder = std::fs::canonicalize(root.join("control").join(folder))
            .map_err(|e| e.to_string())?;
        let control = std::fs::canonicalize(root.join("control")).map_err(|e| e.to_string())?;
        if folder.parent() != Some(control.as_path()) {
            return Err("Invalid Strata active-Control path".into());
        }
        let mut candidates = vec![folder.join("python/bin/python3")];
        if let Ok(entries) = std::fs::read_dir(folder.join("python/bin")) {
            let mut rest: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("python3."))
                })
                .collect();
            rest.sort();
            candidates.extend(rest);
        }
        let python = candidates.into_iter().find(|path| crate::util::is_executable(path));
        let entry = folder.join("strata.py");
        if let (Some(python), true) = (python, entry.is_file()) {
            return Ok(vec![
                python.display().to_string(),
                entry.display().to_string(),
            ]);
        }
    }
    crate::util::which("strata")
        .map(|path| vec![path.display().to_string()])
        .ok_or_else(|| "Strata CLI not found. Set strata_command in Connection configuration.".into())
}

fn model_path(cfg: &Value) -> Result<PathBuf, String> {
    let value = cfg
        .get("strata_model_config")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("Set strata_model_config to the Strata model run-config JSON in Connection configuration.")?;
    Ok(crate::util::expand_tilde(value))
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:09}", now.as_secs(), now.subsec_nanos())
}

fn save_mode(path: &Path, mode: &str) -> Result<Value, String> {
    if !MODES.contains(&mode) {
        return Err("Unknown drafting mode".into());
    }
    use std::os::unix::fs::OpenOptionsExt;
    let mut source = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    crate::util::flock(&source, libc::LOCK_EX)?;
    let mut before = String::new();
    source.read_to_string(&mut before).map_err(|e| e.to_string())?;
    let mut config: Value = serde_json::from_str(&before).map_err(|e| e.to_string())?;
    config["drafting"] = json!(mode);
    if mode != "off" && config.get("mtp").and_then(Value::as_bool) == Some(false) {
        config["mtp"] = json!(true);
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model-run-config.json");
    let backup = path.with_file_name(format!(
        "{name}.relay-before-drafting-{}",
        timestamp()
    ));
    let mut backup_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&backup)
        .map_err(|e| e.to_string())?;
    backup_file
        .write_all(before.as_bytes())
        .map_err(|e| e.to_string())?;
    backup_file.sync_all().map_err(|e| e.to_string())?;
    let mut text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    text.push('\n');
    crate::util::atomic_write(path, text.as_bytes(), 0o600)?;
    Ok(json!({
        "drafting": mode,
        "backup": backup.display().to_string(),
        "note": "Saved for the next model load. The current model is unchanged."
    }))
}

fn command_for(action: &str, cfg: &Value, mode: Option<&str>) -> Result<Vec<String>, String> {
    let mut args = cli(cfg)?;
    match action {
        "check" => args.extend(["update", "--check", "--output", "json"].map(str::to_owned)),
        "update" => args.extend(["update", "--yes", "--output", "jsonl"].map(str::to_owned)),
        "rollback" => {
            args.extend(["components", "rollback", "--output", "jsonl"].map(str::to_owned))
        }
        "plan" | "fetch" => {
            let config_path = model_path(cfg)?.display().to_string();
            args.extend(
                [
                    "drafting",
                    if action == "plan" { "plan" } else { "fetch" },
                    "--config",
                    config_path.as_str(),
                    "--output",
                    "jsonl",
                ]
                .map(str::to_owned),
            );
            if let Some(mode) = mode {
                args.push("--mode".into());
                args.push(mode.into());
            }
            if action == "fetch" {
                args.push("--if-needed".into());
            }
        }
        _ => return Err("Unknown action".into()),
    }
    Ok(args)
}

fn status(cfg: &Value) -> Value {
    let mut result = json!({});
    if let Ok(command) = cli(cfg) {
        result["command"] = json!(command);
    }
    match model_path(cfg).and_then(|path| {
        let config = crate::util::read_json_file(&path)?;
        Ok((path, config))
    }) {
        Ok((path, config)) => {
            result["model_config"] = json!(path.display().to_string());
            result["drafting"] = json!(config
                .get("drafting")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if config.get("mtp").and_then(Value::as_bool) == Some(false) {
                        "off".into()
                    } else {
                        "auto".into()
                    }
                }));
        }
        Err(error) => result["model_config_error"] = json!(error),
    }
    let root = components_root();
    if let Ok(state) = crate::util::read_json_file(&root.join(".updates/state.json")) {
        if let Some(engine) = state.get("engine") {
            result["engine"] = engine
                .get("active")
                .cloned()
                .unwrap_or_else(|| json!("Unknown"));
            result["rollback_available"] = json!(engine.get("previous").is_some());
        }
    }
    if let Ok(control) = crate::util::read_json_file(&root.join("control/active.json")) {
        result["control"] = control
            .get("version")
            .cloned()
            .unwrap_or_else(|| json!("Unknown"));
    }
    result
}

fn run(action: &str, cfg: &Value, mode: Option<&str>) -> Result<i32, String> {
    if action == "status" {
        emit("result", json!({"action": action, "data": status(cfg)}));
        return Ok(0);
    }
    if action == "save" {
        let mode = mode.ok_or("Missing --mode")?;
        emit(
            "result",
            json!({"action": action, "data": save_mode(&model_path(cfg)?, mode)?}),
        );
        return Ok(0);
    }

    let mut args = command_for(action, cfg, mode)?;
    if matches!(action, "check" | "update") {
        args.extend(
            [
                "--feed",
                "https://github.com/Niko1221/Strata/releases/latest/download/strata-feed.json",
            ]
            .map(str::to_owned),
        );
    }
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("STRATA_UPDATE_FEED")
        .env_remove("STRATA_UPDATE_TRUSTED_KEYS")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .spawn()
        .map_err(|e| e.to_string())?;
    // Strata writes progress to stdout and diagnostics to stderr. Merge both,
    // matching the Python helper's `stderr=STDOUT` contract.
    let (lines, received) = mpsc::channel();
    for stream in [
        child.stdout.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        child.stderr.take().map(|stream| Box::new(stream) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let lines = lines.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(lines);
    for line in received {
        match serde_json::from_str::<Value>(&line) {
            Ok(data) => {
                let kind = if matches!(action, "check" | "plan")
                    || data.get("event").and_then(Value::as_str) == Some("result")
                    || data.get("ok").is_some()
                {
                    "result"
                } else {
                    "progress"
                };
                emit(kind, json!({"action": action, "data": data}));
            }
            Err(_) => emit("log", json!({"text": line.chars().take(3000).collect::<String>()})),
        }
    }
    let code = child.wait().map_err(|e| e.to_string())?;
    let code = code.code().unwrap_or(1);
    if code != 0 {
        let message = match code {
            7 => "A model is loaded. Unload it before updating; nothing was changed.".to_owned(),
            8 => "Update finished, but the model did not restart. Check details before reconnecting.".to_owned(),
            _ => format!("Strata exited with code {code}. See details."),
        };
        emit("error", json!({"text": message}));
    }
    Ok(code)
}

pub fn main(args: &[String]) -> ! {
    let Some(action) = args.first().cloned() else {
        eprintln!("usage: relay __strata <status|check|update|rollback|plan|fetch|save> [--mode MODE]");
        std::process::exit(2);
    };
    let mode = args
        .iter()
        .position(|arg| arg == "--mode")
        .and_then(|index| args.get(index + 1))
        .map(String::as_str);
    if mode.is_some() && !MODES.contains(&mode.unwrap()) {
        emit("error", json!({"text": "Unknown drafting mode"}));
        std::process::exit(1);
    }
    let lock_path = crate::util::home().join(".local/state/relay/strata-management.lock");
    let _lock = match crate::util::lock_exclusive_nonblocking(&lock_path) {
        Ok(lock) => lock,
        Err(_) => {
            emit(
                "error",
                json!({"text": "Another Strata operation is already running."}),
            );
            std::process::exit(1);
        }
    };
    match run(&action, &settings(), mode) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            emit("error", json!({"text": error}));
            std::process::exit(1);
        }
    }
}

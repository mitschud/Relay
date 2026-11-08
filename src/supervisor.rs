//! Managed engine supervisor. This is the former `helpers/relay-engine` Python
//! process, compiled into the relay binary. It owns the model/agent leases,
//! discovers the Strata model, writes the Codex model catalogue and supervises
//! the Codex app-server child. Keeping it in a child process preserves cleanup
//! if the GUI exits or is terminated.
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn settings_path() -> PathBuf {
    home().join(".config/relay/settings.json")
}

fn data_dir() -> PathBuf {
    home().join(".local/share/relay")
}

fn load_settings() -> Value {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| {
            json!({
                "base_url": "http://127.0.0.1:47831/v1",
                "alternate_urls": [],
                "codex_binary": "codex",
                "start_command": [],
                "stop_command": [],
                "preferred_model": "",
                "load_timeout_s": 600
            })
        })
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

fn which(name: &str) -> Option<PathBuf> {
    let expanded = if let Some(rest) = name.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(name)
    };
    if expanded.components().count() > 1 || name.contains('/') {
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

fn flock(file: &File, operation: libc::c_int) -> Result<(), String> {
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn open_lock(path: &Path) -> Result<File, String> {
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

fn exclusive(path: &Path) -> Result<File, String> {
    let file = open_lock(path)?;
    flock(&file, libc::LOCK_EX)?;
    Ok(file)
}

fn read_json(url: &str) -> Result<Value, String> {
    crate::http::get_json(url, Duration::from_secs(3))
}

fn discover(base: &str, preferred: &str) -> Result<Option<Value>, String> {
    let models = read_json(&format!("{}/models", base.trim_end_matches('/')))?
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let models: Vec<&Value> = models
        .iter()
        .filter(|m| m.get("alias_of").is_none())
        .filter(|m| {
            m.get("architecture")
                .and_then(|a| a.get("output_modalities"))
                .and_then(Value::as_array)
                .map(|v| v.iter().any(|x| x.as_str() == Some("text")))
                .unwrap_or(true)
        })
        .collect();
    if models.is_empty() {
        return Ok(None);
    }
    let model = if preferred.is_empty() {
        models[0]
    } else {
        models
            .iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(preferred))
            .copied()
            .ok_or_else(|| format!("Configured model is not available: {preferred}"))?
    };
    let props = base
        .rsplit_once("/v1")
        .map(|(prefix, _)| prefix)
        .unwrap_or(base)
        .trim_end_matches('/')
        .to_owned()
        + "/props";
    let props = read_json(&props).unwrap_or(Value::Null);
    let context = model
        .get("meta")
        .and_then(|m| m.get("n_ctx"))
        .and_then(Value::as_i64)
        .or_else(|| {
            props
                .get("default_generation_settings")
                .and_then(|m| m.get("n_ctx"))
                .and_then(Value::as_i64)
        });
    let id = model.get("id").and_then(Value::as_str).unwrap_or_default();
    let name = model
        .get("display_name")
        .and_then(Value::as_str)
        .or_else(|| props.get("model_alias").and_then(Value::as_str))
        .unwrap_or(id);
    let input_modalities = model
        .get("architecture")
        .and_then(|a| a.get("input_modalities"))
        .cloned()
        .unwrap_or_else(|| json!(["text"]));
    let status = model
        .get("status")
        .and_then(|s| s.get("value"))
        .and_then(Value::as_str)
        .unwrap_or("loaded");
    Ok(Some(json!({
        "id": id,
        "name": name,
        "context": context,
        "endpoint": base,
        "input_modalities": input_modalities,
        "engine": props.get("build_info").and_then(Value::as_str).unwrap_or("Strata"),
        "status": status
    })))
}

fn find(bases: &[String], preferred: &str) -> Result<Option<Value>, String> {
    for base in bases {
        match discover(base, preferred) {
            Ok(Some(meta)) => return Ok(Some(meta)),
            Ok(None) => {}
            Err(error) if error.starts_with("Configured model is not available:") => {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    Ok(None)
}

fn catalog(meta: &Value) -> Value {
    let levels: Vec<Value> = ["none", "low", "medium", "high", "max"]
        .iter()
        .map(|effort| {
            let mut chars = effort.chars();
            let title = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default();
            json!({"effort": effort, "description": title})
        })
        .collect();
    let mut model = json!({
        "slug": meta["id"],
        "display_name": meta["name"],
        "description": "Local model discovered from Strata",
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": levels,
        "shell_type": "unified_exec",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "upgrade": null,
        "base_instructions": "Use tools when needed. Verify changes and report results honestly.",
        "supports_reasoning_summaries": false,
        "support_verbosity": false,
        "apply_patch_tool_type": "freeform",
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "supports_parallel_tool_calls": false,
        "input_modalities": meta["input_modalities"],
        "experimental_supported_tools": []
    });
    if let Some(context) = meta.get("context").and_then(Value::as_i64) {
        model["context_window"] = json!(context);
        model["max_context_window"] = json!(context);
        model["effective_context_window_percent"] = json!(95);
    }
    json!({"models": [model]})
}

fn compaction_limit(meta: &Value) -> Option<i64> {
    let context = meta.get("context").and_then(Value::as_i64).unwrap_or(0);
    (context > 0).then_some((context * 85 / 100).max(1))
}

fn wait_timeout(child: &mut std::process::Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_command(argv: &[String], timeout: Option<Duration>) -> Result<bool, String> {
    use std::io::Read;
    if argv.is_empty() {
        return Ok(true);
    }
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let status = match timeout {
        Some(timeout) => match wait_timeout(&mut child, timeout) {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Command timed out".into());
            }
        },
        None => child.wait().map_err(|e| e.to_string())?,
    };
    let mut output = Vec::new();
    let _ = stdout.read_to_end(&mut output);
    let _ = stderr.read_to_end(&mut output);
    let text = String::from_utf8_lossy(&output);
    if !text.is_empty() {
        eprint!("{text}");
        if !text.ends_with('\n') {
            eprintln!();
        }
    }
    Ok(status.success())
}

fn service_active(unit: &str) -> bool {
    let state = (|| -> Result<String, String> {
        let connection = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
        let reply = connection
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "GetUnit",
                &(unit,),
            )
            .map_err(|e| e.to_string())?;
        let path: zbus::zvariant::OwnedObjectPath =
            reply.body().deserialize().map_err(|e| e.to_string())?;
        let reply = connection
            .call_method(
                Some("org.freedesktop.systemd1"),
                path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.freedesktop.systemd1.Unit", "ActiveState"),
            )
            .map_err(|e| e.to_string())?;
        let value: zbus::zvariant::OwnedValue =
            reply.body().deserialize().map_err(|e| e.to_string())?;
        String::try_from(value).map_err(|e| e.to_string())
    })()
    .unwrap_or_default();
    matches!(state.as_str(), "active" | "activating" | "deactivating")
}

fn native_instances(base: &str) -> Vec<Value> {
    let root = base
        .rsplit_once("/v1")
        .map(|(prefix, _)| prefix)
        .unwrap_or(base)
        .trim_end_matches('/');
    read_json(&format!("{root}/api/native"))
        .ok()
        .and_then(|v| v.get("instances").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn another_model_is_active(bases: &[String], exclusive_endpoints: &[String]) -> bool {
    for base in bases {
        if native_instances(base).iter().any(|instance| {
            !matches!(
                instance.get("status").and_then(Value::as_str),
                Some("unloaded" | "stopped" | "error")
            )
        }) {
            return true;
        }
    }
    for endpoint in exclusive_endpoints {
        if bases
            .iter()
            .any(|base| base.trim_end_matches('/') == endpoint.trim_end_matches('/'))
        {
            continue;
        }
        let root = endpoint.trim_end_matches('/');
        if read_json(&format!("{root}/models"))
            .ok()
            .and_then(|v| v.get("data").and_then(Value::as_array).map(|a| !a.is_empty()))
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

struct Cleanup {
    lifecycle: PathBuf,
    sessions: PathBuf,
    lease_path: Option<PathBuf>,
    lease: Option<File>,
    stop_command: Vec<String>,
    started: bool,
    manage_existing: bool,
    done: bool,
}

impl Cleanup {
    fn run(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        self.lease.take();
        if let Some(path) = self.lease_path.take() {
            let _ = std::fs::remove_file(path);
        }
        if let Ok(_coordination) = exclusive(&self.lifecycle.join("coordination.lock")) {
            if let Ok(entries) = std::fs::read_dir(&self.sessions) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("lock") {
                        continue;
                    }
                    if let Ok(file) = open_lock(&path) {
                        if flock(&file, libc::LOCK_EX | libc::LOCK_NB).is_ok() {
                            drop(file);
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            }
        }
        if !self.stop_command.is_empty() && (self.started || self.manage_existing) {
            let _ = run_command(&self.stop_command, Some(Duration::from_secs(130)));
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.run();
    }
}

fn main_inner(extra: &[String]) -> Result<i32, String> {
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
    }

    let settings = load_settings();
    let base_url = settings["base_url"]
        .as_str()
        .unwrap_or("http://127.0.0.1:47831/v1")
        .to_owned();
    let mut bases = vec![base_url];
    for url in strings(&settings, "alternate_urls") {
        if !bases.contains(&url) {
            bases.push(url);
        }
    }
    let preferred = settings["preferred_model"].as_str().unwrap_or_default();
    let codex_binary = settings["codex_binary"].as_str().unwrap_or("codex");
    let start_command = strings(&settings, "start_command");
    let stop_command = strings(&settings, "stop_command");
    let manage_existing = settings["manage_existing"].as_bool().unwrap_or(false);
    let load_timeout = settings["load_timeout_s"].as_u64().unwrap_or(600);

    for required in strings(&settings, "model_required") {
        if !Path::new(&required).is_file() {
            return Err(format!("Selected model is not ready: {required}"));
        }
    }

    // Shared installation lease: an updater cannot replace the agent while a session lives.
    let agent_lock_path = home().join(".local/state/relay/codex-agent.lock");
    let agent_lock = open_lock(&agent_lock_path)?;
    flock(&agent_lock, libc::LOCK_SH | libc::LOCK_NB)
        .map_err(|_| "Wait for the Codex agent update to finish before connecting.".to_owned())?;

    let data = data_dir();
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let codex = which(codex_binary)
        .ok_or_else(|| "Codex executable not found. Set codex_binary in Settings.".to_owned())?;

    let lifecycle = settings["lifecycle_dir"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state/relay/lifecycle"));
    let sessions = lifecycle.join("sessions");
    std::fs::create_dir_all(&sessions).map_err(|e| e.to_string())?;

    let lease_path = sessions.join(format!("{}.lock", std::process::id()));
    let lease = {
        let _coordination = exclusive(&lifecycle.join("coordination.lock"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lease_path)
            .map_err(|e| e.to_string())?;
        flock(&file, libc::LOCK_EX)?;
        file
    };
    let mut cleanup = Cleanup {
        lifecycle: lifecycle.clone(),
        sessions: sessions.clone(),
        lease_path: Some(lease_path),
        lease: Some(lease),
        stop_command: stop_command.clone(),
        started: false,
        manage_existing,
        done: false,
    };

    eprintln!("Engine loading");
    let mut meta = find(&bases, preferred)?;
    if meta.is_none() {
        let active = bases.iter().any(|base| {
            native_instances(base).iter().any(|instance| {
                !matches!(
                    instance.get("status").and_then(Value::as_str),
                    Some("unloaded" | "stopped" | "error")
                )
            })
        });
        if !active && !start_command.is_empty() {
            let _coordination = exclusive(&lifecycle.join("coordination.lock"))?;
            let own = start_command
                .iter()
                .find(|arg| arg.ends_with(".service"))
                .cloned();
            for unit in strings(&settings, "exclusive_services") {
                if Some(unit.clone()) == own {
                    continue;
                }
                if service_active(&unit) {
                    return Err(
                        "Another local model service is active. Unload its clients before switching models."
                            .into(),
                    );
                }
            }
            if another_model_is_active(
                &bases,
                &strings(&settings, "exclusive_endpoints"),
            ) {
                return Err(
                    "Another local model is active. Unload its client sessions before loading this model."
                        .into(),
                );
            }
            if find(&bases, preferred)?.is_none() {
                if !run_command(&start_command, None)? {
                    return Err("Engine start command failed.".into());
                }
                cleanup.started = true;
            }
        } else if !active && start_command.is_empty() {
            return Err("Engine unavailable. Configure an engine start command in Settings, or start Strata with a Responses API endpoint.".into());
        }
        eprintln!("Model loading");
        let deadline = Instant::now() + Duration::from_secs(load_timeout);
        while meta.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(1));
            meta = find(&bases, preferred)?;
        }
        if meta.is_none() {
            return Err("Model loading timed out. Check the endpoint and engine start command in Settings.".into());
        }
    } else if meta.as_ref().and_then(|m| m["status"].as_str()) == Some("unloaded") {
        eprintln!("Model loading");
    }
    let mut meta = meta.unwrap();

    std::fs::write(
        data.join("model-catalog.json"),
        serde_json::to_string_pretty(&catalog(&meta)).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    let profile = settings["codex_home"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("codex"));
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;

    let mut args: Vec<String> = vec![
        codex.display().to_string(),
        "--no-daemon".into(),
        "-c".into(),
        "model_provider=\"strata\"".into(),
        "-c".into(),
        format!("model={}", serde_json::to_string(&meta["id"]).unwrap_or_default()),
        "-c".into(),
        format!(
            "model_catalog_json={}",
            serde_json::to_string(&data.join("model-catalog.json").display().to_string())
                .unwrap_or_default()
        ),
        "-c".into(),
        "model_providers.strata.name=\"Strata\"".into(),
        "-c".into(),
        "model_providers.strata.wire_api=\"responses\"".into(),
        "-c".into(),
        "model_providers.strata.requires_openai_auth=false".into(),
        "-c".into(),
        format!(
            "model_providers.strata.base_url={}",
            serde_json::to_string(&meta["endpoint"]).unwrap_or_default()
        ),
    ];
    if let Some(context) = meta["context"].as_i64() {
        let limit = compaction_limit(&meta).unwrap_or(1);
        meta["auto_compact_token_limit"] = json!(limit);
        args.push("-c".into());
        args.push(format!("model_context_window={context}"));
        args.push("-c".into());
        args.push(format!("model_auto_compact_token_limit={limit}"));
    }
    args.extend_from_slice(extra);

    println!("{}", json!({"relay_model": meta}));
    let _ = std::io::stdout().flush();
    eprintln!("Engine connected");

    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .env("CODEX_HOME", &profile)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Could not start Codex app-server: {e}"))?;

    let exit;
    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            if wait_timeout(&mut child, Duration::from_secs(15)).is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            exit = Some(130);
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                exit = Some(status.code().unwrap_or(1));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    }
    cleanup.run();
    Ok(exit.unwrap_or(0))
}

pub fn main(extra: &[String]) {
    match main_inner(extra) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Connection failed: {error}");
            std::process::exit(1);
        }
    }
}

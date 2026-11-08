//! Explicitly authenticated, per-GUI sudo session. The privileged endpoint runs
//! as the current user's authenticated sudo child; there is no installed root
//! service and no sudoers rule. Everything here is compiled into `relay`.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    os::fd::{AsRawFd, FromRawFd, RawFd},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;

pub struct Session {
    input: Option<ChildStdin>,
    child: Option<Child>,
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut s) = self.input.take() {
            let _ = writeln!(s, "revoke");
        }
        if let Some(mut child) = self.child.take() {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
        let _ = Command::new("/usr/bin/sudo").arg("-k").status();
    }
}
pub enum Event {
    Ready(u64, Session),
    Failed(u64, String),
    Prompt(u64, String),
    Password(u64),
    Scanning(u64, bool),
    Read(u64),
}
pub fn socket_path() -> PathBuf {
    let id = std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .unwrap_or_else(|_| format!("{}", std::process::id()));
    PathBuf::from(format!(
        "/run/user/{}/relay-sudo-{}.sock",
        unsafe { libc::getuid() },
        id.trim()
    ))
}
/// Dropping a pending request cancels only that authentication attempt.
pub struct Pending {
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        let pid = self.pid.load(Ordering::SeqCst);
        if pid != 0 {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
    }
}

fn wait_for_ready(child: &mut Child, cancel: &AtomicBool) -> bool {
    let (tx, rx) = mpsc::channel();
    let output = match child.stdout.take() {
        Some(output) => output,
        None => return false,
    };
    thread::spawn(move || {
        let mut first = String::new();
        let read = BufReader::new(output).take(4096).read_line(&mut first);
        let ready = read.is_ok()
            && serde_json::from_str::<serde_json::Value>(&first)
                .ok()
                .is_some_and(|v| v["ready"] == true && v["uid"] == 0);
        let _ = tx.send(ready);
    });
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if cancel.load(Ordering::SeqCst) || Instant::now() >= deadline {
            return false;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(ready) => return ready,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
fn cancel_child(child: &mut Child) {
    // Authentication has its own process group: also dismiss an open askpass dialog.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGTERM);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn verify_status_done(status: &str) -> bool {
    matches!(status, "verify-match" | "verify-no-match" | "verify-retry")
}

fn finger_present(changed: &HashMap<String, zbus::zvariant::OwnedValue>) -> Option<bool> {
    let value = changed.get("finger-present")?;
    bool::try_from(value.try_clone().ok()?).ok()
}

struct FingerWatch {
    stop: Arc<AtomicBool>,
    connection: zbus::blocking::Connection,
}
impl Drop for FingerWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Closing the shared bus connection unblocks the iterator thread.
        let _ = self.connection.clone().close();
    }
}
fn watch_finger(
    ctx: eframe::egui::Context,
    events: mpsc::Sender<Event>,
    generation: u64,
) -> Option<FingerWatch> {
    // Signal subscription only: never Claim, VerifyStart, or a second PAM attempt.
    let connection = zbus::blocking::Connection::system().ok()?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("net.reactivated.Fprint")
        .ok()?
        .build();
    let mut iterator =
        zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(64)).ok()?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_connection = connection.clone();
    thread::spawn(move || {
        for message in iterator.by_ref() {
            if thread_stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(message) = message else {
                break;
            };
            let member = message
                .header()
                .member()
                .map(|m| m.as_str().to_owned())
                .unwrap_or_default();
            let mut scanning = None;
            let mut read = false;
            match member.as_str() {
                "PropertiesChanged" => {
                    if let Ok((name, changed, _invalid)) = message.body().deserialize::<(
                        String,
                        HashMap<String, zbus::zvariant::OwnedValue>,
                        Vec<String>,
                    )>() {
                        if name == "net.reactivated.Fprint.Device" {
                            scanning = finger_present(&changed);
                        }
                    }
                }
                "VerifyStatus" => {
                    if let Ok((status, _done)) = message.body().deserialize::<(String, bool)>() {
                        read = verify_status_done(&status);
                        if read {
                            scanning = Some(false);
                        }
                    }
                }
                _ => {}
            }
            if read {
                if events.send(Event::Read(generation)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            if let Some(scanning) = scanning {
                if events.send(Event::Scanning(generation, scanning)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        }
        drop(thread_connection);
    });
    Some(FingerWatch { stop, connection })
}

pub fn authenticate(
    path: PathBuf,
    ctx: eframe::egui::Context,
    tx: mpsc::Sender<Event>,
    generation: u64,
) -> Pending {
    let cancel = Arc::new(AtomicBool::new(false));
    let pid = Arc::new(AtomicU32::new(0));
    let pending = Pending {
        cancel: cancel.clone(),
        pid: pid.clone(),
    };
    thread::spawn(move || {
        let _finger_watch = watch_finger(ctx.clone(), tx.clone(), generation);
        let home = std::env::var("HOME").unwrap_or_default();
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("relay"));
        let askpass = std::env::var("RELAY_ASKPASS_HELPER").map(PathBuf::from).unwrap_or_else(
            |_| PathBuf::from(format!("{home}/.local/libexec/relay/relay-askpass")),
        );
        let mut child = match Command::new("/usr/bin/sudo")
            .args(["-k", "-A", "--"])
            .arg(&exe)
            .arg("__sudo-session")
            .arg(&path)
            .env("SUDO_ASKPASS", &askpass)
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                let _ = tx.send(Event::Failed(generation, e.to_string()));
                ctx.request_repaint();
                return;
            }
        };
        pid.store(child.id(), Ordering::SeqCst);
        let Some(errors) = child.stderr.take() else {
            let _ = tx.send(Event::Failed(generation, "sudo stderr unavailable".into()));
            ctx.request_repaint();
            return;
        };
        let events = tx.clone();
        let repaint = ctx.clone();
        thread::spawn(move || {
            for line in BufReader::new(errors).lines().map_while(Result::ok) {
                let lower = line.to_ascii_lowercase();
                let event = if line.trim() == "RELAY_AUTH_PASSWORD" {
                    Some(Event::Password(generation))
                } else if lower.contains("failed to match")
                    || lower.contains("verify-no-match")
                    || lower.contains("verification failed")
                {
                    Some(Event::Prompt(
                        generation,
                        "Fingerprint did not match. Try again.".into(),
                    ))
                } else if lower.contains("swipe") || lower.contains("finger") {
                    Some(Event::Prompt(
                        generation,
                        "Touch the fingerprint sensor".into(),
                    ))
                } else {
                    None
                };
                if let Some(event) = event {
                    if events.send(event).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            }
        });
        let ready = wait_for_ready(&mut child, &cancel);
        let result = if ready && !cancel.load(Ordering::SeqCst) {
            Event::Ready(
                generation,
                Session {
                    input: child.stdin.take(),
                    child: Some(child),
                },
            )
        } else {
            cancel_child(&mut child);
            Event::Failed(generation,"Administrator authentication was cancelled or failed. Full access + sudo was not enabled.".into())
        };
        pid.store(0, Ordering::SeqCst);
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    pending
}

/// Create the stable helper names referenced by agent instructions and
/// SUDO_ASKPASS. They point at this binary.
pub fn ensure_helpers() -> Result<(), String> {
    use std::os::unix::fs::symlink;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = crate::util::home().join(".local/libexec/relay");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for name in ["relay-sudo", "relay-askpass"] {
        let link = dir.join(name);
        if let Ok(target) = std::fs::read_link(&link) {
            if target == exe {
                continue;
            }
        }
        if std::fs::symlink_metadata(&link).is_ok() {
            let _ = std::fs::remove_file(&link);
        }
        symlink(&exe, &link).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn client_main(args: &[String]) -> ! {
    let mut socket = std::env::var("RELAY_SUDO_SOCKET").unwrap_or_default();
    let mut index = 0;
    if args.first().map(String::as_str) == Some("--socket") {
        socket = args.get(1).cloned().unwrap_or_default();
        index = 2;
    }
    if args.get(index).map(String::as_str) == Some("--") {
        index += 1;
    }
    let argv = &args[index..];
    if socket.is_empty() || argv.is_empty() {
        eprintln!("Enable Full access + sudo in the local GUI first. Usage: relay-sudo -- COMMAND [ARG ...]");
        std::process::exit(1);
    }
    let result = (|| -> Result<(i32, String), String> {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(&socket).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(650)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(650)))
            .map_err(|e| e.to_string())?;
        if peer_uid(stream.as_raw_fd()) != Some(0) {
            return Err("The sudo endpoint is not a root-owned process.".into());
        }
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "/".into());
        let request = json!({"argv": argv, "cwd": cwd, "timeout": 120});
        stream
            .write_all(format!("{request}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        stream.shutdown(std::net::Shutdown::Write).ok();
        let mut reader = BufReader::new(stream).take(2 * 1024 * 1024);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        let output = value["output"].as_str().unwrap_or_default().to_owned();
        let code = value["exit_code"].as_i64().unwrap_or(70).clamp(0, 255) as i32;
        Ok((code, output))
    })();
    match result {
        Ok((code, output)) => {
            print!("{output}");
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
        Err(error) => {
            eprintln!("Sudo session unavailable; select Full access + sudo and authenticate again: {error}");
            std::process::exit(77);
        }
    }
}

fn peer_uid(fd: RawFd) -> Option<u32> {
    #[repr(C)]
    struct Cred {
        pid: i32,
        uid: u32,
        gid: u32,
    }
    let mut cred = Cred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<Cred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    (result == 0).then_some(cred.uid)
}

struct SessionState {
    owner: u32,
    stopped: AtomicBool,
    children: Mutex<Vec<i32>>,
}
impl SessionState {
    fn stop(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        let pids = self.children.lock().unwrap().clone();
        for pid in &pids {
            unsafe {
                libc::kill(-*pid, libc::SIGTERM);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline
            && pids.iter().any(|pid| unsafe { libc::kill(*pid, 0) } == 0)
        {
            thread::sleep(Duration::from_millis(20));
        }
        for pid in pids {
            if unsafe { libc::kill(pid, 0) } == 0 {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
    }
    fn execute(&self, request: &Value) -> Value {
        let Some(values) = request.get("argv").and_then(Value::as_array) else {
            return json!({"exit_code":64,"output":"Expected a nonempty argv array."});
        };
        let mut argv = Vec::with_capacity(values.len());
        for value in values {
            let Some(text) = value.as_str() else {
                return json!({"exit_code":64,"output":"Expected a nonempty argv array."});
            };
            if text.contains('\0') {
                return json!({"exit_code":64,"output":"Expected a nonempty argv array."});
            }
            argv.push(text.to_owned());
        }
        if argv.is_empty() {
            return json!({"exit_code":64,"output":"Expected a nonempty argv array."});
        }
        let timeout = request
            .get("timeout")
            .and_then(Value::as_i64)
            .unwrap_or(120)
            .clamp(1, 600) as u64;
        if self.stopped.load(Ordering::SeqCst) {
            return json!({"exit_code":77,"output":"Sudo session is closed."});
        }
        let cwd = request
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("/");
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return json!({"exit_code":70,"output":std::io::Error::last_os_error().to_string()});
        }
        // Both stdout and stderr share one pipe so their ordering is preserved.
        let mut read_end = unsafe { File::from_raw_fd(fds[0]) };
        let write_end = unsafe { File::from_raw_fd(fds[1]) };
        let write_clone = match write_end.try_clone() {
            Ok(file) => file,
            Err(error) => return json!({"exit_code":70,"output":error.to_string()}),
        };
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .current_dir(cwd)
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin",
            )
            .env("HOME", "/root")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::from(write_end))
            .stderr(Stdio::from(write_clone))
            .process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return json!({"exit_code":70,"output":error.to_string()}),
        };
        let pid = child.id() as i32;
        self.children.lock().unwrap().push(pid);
        let reader = thread::spawn(move || {
            let mut data = Vec::new();
            let mut buffer = vec![0u8; 64 * 1024];
            let mut total = 0usize;
            loop {
                match read_end.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        total += read;
                        let room = OUTPUT_LIMIT + 1 - data.len();
                        if room > 0 {
                            data.extend_from_slice(&buffer[..read.min(room)]);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            (data, total)
        });
        let start = Instant::now();
        let code;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    use std::os::unix::process::ExitStatusExt;
                    code = status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(70);
                    break;
                }
                Ok(None) => {}
                Err(_) => {
                    code = 126;
                    break;
                }
            }
            if self.stopped.load(Ordering::SeqCst) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = child.wait();
                code = 77;
                break;
            }
            if start.elapsed() >= Duration::from_secs(timeout) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                let _ = child.wait();
                code = 124;
                break;
            }
            thread::sleep(Duration::from_millis(30));
        }
        self.children.lock().unwrap().retain(|item| *item != pid);
        let (data, total) = reader.join().unwrap_or_default();
        let mut output =
            String::from_utf8_lossy(&data[..data.len().min(OUTPUT_LIMIT)]).into_owned();
        if total > OUTPUT_LIMIT {
            output.push_str("\n[administrator output limited to 1 MiB]");
        }
        json!({"exit_code": code, "output": output})
    }
}

fn handle_client(stream: std::os::unix::net::UnixStream, state: Arc<SessionState>) {
    if peer_uid(stream.as_raw_fd()) != Some(state.owner) {
        return;
    }
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream).take(65537);
    let mut line = String::new();
    let result = if reader.read_line(&mut line).is_err() {
        json!({"exit_code":70,"output":"Could not read the request."})
    } else if line.len() > 65536 {
        json!({"exit_code":70,"output":"Request is too large"})
    } else {
        match serde_json::from_str::<Value>(line.trim()) {
            Ok(request) if request.is_object() => state.execute(&request),
            Ok(_) => json!({"exit_code":70,"output":"Expected a JSON request object."}),
            Err(error) => json!({"exit_code":70,"output":error.to_string()}),
        }
    };
    let _ = writer.write_all(format!("{result}\n").as_bytes());
    let _ = writer.flush();
}

pub fn session_main(args: &[String]) -> ! {
    let code = run_session(args);
    std::process::exit(code);
}

fn run_session(args: &[String]) -> i32 {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("This helper must be started by the GUI through authenticated sudo.");
        return 1;
    }
    let Some(owner) = std::env::var("SUDO_UID").ok().and_then(|v| v.parse::<u32>().ok())
    else {
        eprintln!("This helper must be started by the GUI through authenticated sudo.");
        return 1;
    };
    let Some(socket) = args.first() else {
        eprintln!("usage: relay __sudo-session SOCKET");
        return 1;
    };
    let path = PathBuf::from(socket);
    let expected_parent = PathBuf::from(format!("/run/user/{owner}"));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if owner == 0 || path.parent() != Some(expected_parent.as_path()) || !name.starts_with("relay-sudo-")
    {
        eprintln!("Invalid private session socket.");
        return 1;
    }
    use std::os::unix::net::UnixListener;
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Could not create sudo session socket: {error}");
            return 1;
        }
    };
    if let Err(error) = chmod_owner(&path, owner) {
        eprintln!("Could not secure sudo session socket: {error}");
        let _ = std::fs::remove_file(&path);
        return 1;
    }
    let state = Arc::new(SessionState {
        owner,
        stopped: AtomicBool::new(false),
        children: Mutex::new(Vec::new()),
    });
    let watcher_state = state.clone();
    thread::spawn(move || {
        // EOF, an explicit revoke, or a closed GUI all revoke privilege immediately.
        let mut stdin = std::io::stdin().lock();
        let mut line = String::new();
        let _ = stdin.read_line(&mut line);
        watcher_state.stop();
    });
    println!("{}", json!({"ready": true, "uid": 0}));
    let _ = std::io::stdout().flush();
    let _ = listener.set_nonblocking(true);
    while !state.stopped.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = state.clone();
                thread::spawn(move || handle_client(stream, state));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
    state.stop();
    drop(listener);
    let _ = std::fs::remove_file(&path);
    0
}

fn chmod_owner(path: &Path, owner: u32) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| e.to_string())?;
    if unsafe { libc::chown(c_path.as_ptr(), owner, u32::MAX) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

struct Askpass {
    password: String,
    submitted: Arc<Mutex<Option<String>>>,
    error: Option<String>,
    focused: bool,
}
impl eframe::App for Askpass {
    fn ui(&mut self, root: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        use eframe::egui::{self, Color32, RichText};
        let ctx = root.ctx().clone();
        root.painter()
            .rect_filled(ctx.content_rect(), 0, Color32::from_rgb(24, 24, 24));
        root.vertical_centered(|ui| {
            ui.add_space(16.0);
            ui.label(
                RichText::new("Administrator authentication")
                    .size(16.0)
                    .strong()
                    .color(Color32::from_gray(230)),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Relay needs your password to enable Full access + sudo.")
                    .size(12.0)
                    .color(Color32::from_gray(160)),
            );
            ui.add_space(12.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("Password")
                    .desired_width(340.0),
            );
            if !self.focused {
                field.request_focus();
                self.focused = true;
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(error) = &self.error {
                ui.colored_label(Color32::from_rgb(235, 140, 120), error);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let cancel = ui.button("Cancel").clicked();
                let submit = ui
                    .add_enabled(
                        !self.password.is_empty(),
                        egui::Button::new("Authenticate"),
                    )
                    .clicked()
                    || enter;
                if submit && !self.password.is_empty() && !self.password.contains('\n') {
                    if let Ok(mut slot) = self.submitted.lock() {
                        *slot = Some(std::mem::take(&mut self.password));
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

pub fn askpass_main() -> ! {
    crate::apply_software_rendering();
    // Sudo shows this status line while the dialog is open. The password itself
    // is written only to stdout.
    eprintln!("RELAY_AUTH_PASSWORD");
    let submitted = Arc::new(Mutex::new(None));
    let slot = submitted.clone();
    let app = Askpass {
        password: String::new(),
        submitted,
        error: None,
        focused: false,
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Relay — administrator authentication")
            .with_inner_size([420.0, 220.0])
            .with_resizable(false)
            .with_always_on_top(),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Relay authentication",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    );
    if let Err(error) = result {
        eprintln!("Could not open the password dialog: {error}");
    }
    if let Some(password) = slot.lock().ok().and_then(|mut slot| slot.take()) {
        println!("{password}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verify_status_only_reacts_to_verification_results() {
        assert!(verify_status_done("verify-match"));
        assert!(verify_status_done("verify-no-match"));
        assert!(verify_status_done("verify-retry"));
        assert!(!verify_status_done("verify-start"));
    }
    #[test]
    fn finger_presence_follows_contact_not_merely_the_waiting_state() {
        let mut changed = HashMap::new();
        changed.insert("finger-present".to_string(), zbus::zvariant::OwnedValue::from(true));
        assert_eq!(finger_present(&changed), Some(true));
        changed.insert("finger-present".to_string(), zbus::zvariant::OwnedValue::from(false));
        assert_eq!(finger_present(&changed), Some(false));
        changed.clear();
        changed.insert("finger-needed".to_string(), zbus::zvariant::OwnedValue::from(true));
        assert_eq!(finger_present(&changed), None);
    }
    #[test]
    fn cancelling_a_pending_request_interrupts_the_wait() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 10"])
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let pending = Pending {
            cancel: flag.clone(),
            pid: Arc::new(AtomicU32::new(0)),
        };
        drop(pending);
        let now = Instant::now();
        assert!(!wait_for_ready(&mut child, &flag));
        cancel_child(&mut child);
        assert!(now.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn readiness_requires_the_root_helper_handshake() {
        for (line, expected) in [
            (r#"{"ready":true,"uid":1000}"#, false),
            (r#"{"ready":true,"uid":0}"#, true),
            ("not ready", false),
        ] {
            let mut child = Command::new("/usr/bin/printf")
                .args(["%s\n", line])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            assert_eq!(
                wait_for_ready(&mut child, &AtomicBool::new(false)),
                expected
            );
            child.wait().unwrap();
        }
    }
}

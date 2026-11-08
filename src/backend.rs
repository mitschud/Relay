use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[derive(Debug)]
pub enum Event {
    Wire(Value),
    Model(Value),
    Status(String),
    Closed,
}
pub struct Backend {
    pub tx: mpsc::Sender<Value>,
    pub rx: mpsc::Receiver<Event>,
    pid: Arc<AtomicU32>,
}

impl Backend {
    pub fn idle() -> Self {
        let (tx, _) = mpsc::channel();
        let (_, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pid: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn start(ctx: eframe::egui::Context, demo: bool, sudo_socket: std::path::PathBuf) -> Self {
        let (tx, commands) = mpsc::channel::<Value>();
        let (events, rx) = mpsc::channel();
        let pid = Arc::new(AtomicU32::new(0));
        let child_pid = pid.clone();
        if !demo {
            thread::spawn(move || {
                let notify = |event| {
                    let _ = events.send(event);
                    ctx.request_repaint();
                };
                let mut command = crate::util::helper_command("RELAY_LAUNCHER", "__engine");
                if std::env::var_os("RELAY_SOFTWARE_FRONTEND").is_some() {
                    for key in ["__GLX_VENDOR_LIBRARY_NAME", "LIBGL_ALWAYS_SOFTWARE",
                        "GALLIUM_DRIVER", "LP_NUM_THREADS", "RELAY_SOFTWARE_FRONTEND"] {
                        command.env_remove(key);
                    }
                }
                let mut child = match command
                    .args(["app-server", "--listen", "stdio://"])
                    .env("RELAY_SUDO_SOCKET", &sudo_socket)
                    .env(
                        "PATH",
                        format!(
                            "{}/.local/libexec/relay:{}",
                            std::env::var("HOME").unwrap(),
                            std::env::var("PATH").unwrap_or_default()
                        ),
                    )
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        notify(Event::Status(format!("Could not start local backend: {e}")));
                        notify(Event::Closed);
                        return;
                    }
                };
                child_pid.store(child.id(), Ordering::SeqCst);
                // The existing launcher holds a shared engine lease and handles cleanup on SIGTERM.
                // Do not SIGKILL it: doing so would bypass its model-unloading cleanup.
                let mut input = child.stdin.take().unwrap();
                let output = child.stdout.take().unwrap();
                let errors = child.stderr.take().unwrap();
                for (pipe, is_error) in [
                    (Box::new(output) as Box<dyn std::io::Read + Send>, false),
                    (Box::new(errors) as Box<dyn std::io::Read + Send>, true),
                ] {
                    let events = events.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                            let event = if is_error {
                                Event::Status(line)
                            } else {
                                match serde_json::from_str(&line) {
                                    Ok(v) => {
                                        let v: Value = v;
                                        if v.get("relay_model").is_some() {
                                            Event::Model(v["relay_model"].clone())
                                        } else {
                                            Event::Wire(v)
                                        }
                                    }
                                    Err(_) => Event::Status(line),
                                }
                            };
                            if events.send(event).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                        }
                    });
                }
                loop {
                    match commands.recv_timeout(Duration::from_millis(500)) {
                        Ok(v) if v.get("_shutdown").is_some() => break,
                        Ok(v) => {
                            if writeln!(input, "{v}").and_then(|_| input.flush()).is_err() {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(_) => {}
                    }
                    if child.try_wait().ok().flatten().is_some() {
                        child_pid.store(0, Ordering::SeqCst);
                        notify(Event::Closed);
                        return;
                    }
                }
                drop(input);
                // SIGTERM reaches the lease-owning supervisor, which terminates app-server and
                // unloads the engine only if no other CLI/GUI lease remains. Cleanup may take 120s.
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGTERM);
                }
                let _ = child.wait();
                child_pid.store(0, Ordering::SeqCst);
                notify(Event::Closed);
            });
        }
        Self { tx, rx, pid }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // Signal synchronously: the GUI process may exit before its worker runs again.
        let pid = self.pid.load(Ordering::SeqCst);
        if pid != 0 {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        let _ = self.tx.send(json!({"_shutdown": true}));
    }
}


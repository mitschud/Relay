//! Explicit, asynchronous checks and optional private Codex installation.
use eframe::egui;
use serde_json::Value;
use std::sync::mpsc;

#[derive(Default)]
pub struct Manager {
    rx: Option<mpsc::Receiver<(String, Value)>>,
    pub running: bool,
    report: Value,
    message: String,
    confirm: Option<String>,
}

impl Manager {
    fn run(&mut self, ctx: &egui::Context, action: &str) {
        if self.running {
            return;
        }
        self.running = true;
        self.message = match action {
            "check" => "Checking official stable releases…",
            "install" => "Downloading and verifying Codex agent…",
            "rollback" => "Restoring previous agent…",
            _ => "Reading installed agent…",
        }
        .into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let action = action.to_owned();
        let version = self.report["offered"].as_str().unwrap_or("").to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut cmd = crate::util::helper_command("RELAY_CODEX_HELPER", "__codex");
            cmd.arg(&action);
            if action == "install" {
                cmd.args(["--version", &version]);
            }
            let result = match cmd.output() {
                Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
                    serde_json::json!({"ok":false,"error":"The agent updater returned an invalid response"})
                }),
                Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
            };
            let _ = tx.send((action, result));
            ctx.request_repaint();
        });
    }

    /// Return true if installation changed the saved connection settings.
    pub fn poll(&mut self) -> bool {
        match self.rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok((action, data))) => {
                self.running = false;
                self.rx = None;
                if data["ok"].as_bool() != Some(true) {
                    self.message = data["error"]
                        .as_str()
                        .unwrap_or("Agent operation failed")
                        .into();
                    return false;
                }
                self.message = data["message"]
                    .as_str()
                    .unwrap_or(if action == "check" {
                        "Update check complete"
                    } else {
                        "Installed agent read"
                    })
                    .into();
                self.report = data;
                matches!(action.as_str(), "install" | "rollback")
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.running = false;
                self.rx = None;
                self.message = "Agent operation ended without a result".into();
                false
            }
            _ => false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, connected: bool, other_operation: bool) {
        if self.report.is_null() && !self.running && self.message.is_empty() {
            self.run(ui.ctx(), "status");
        }
        ui.heading("Codex agent");
        ui.label(format!(
            "Installed: {}",
            self.report["installed"]["version"]
                .as_str()
                .unwrap_or("Not found")
        ));
        if let Some(path) = self.report["installed"]["path"].as_str() {
            ui.small(path);
        }
        let idle = !self.running && !other_operation;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("Check for updates"))
                .clicked()
            {
                self.run(ui.ctx(), "check");
            }
            ui.hyperlink_to(
                "Release notes",
                self.report["url"]
                    .as_str()
                    .unwrap_or("https://github.com/openai/codex/releases/latest"),
            );
        });
        if let Some(version) = self.report["offered"].as_str() {
            ui.label(format!(
                "Latest stable: {version} · {}",
                if self.report["available"].as_bool() == Some(true) {
                    "Update available"
                } else {
                    "Installed agent is current or newer"
                }
            ));
            if self.report["available"].as_bool() == Some(true) {
                if let Some(size) = self.report["download_bytes"].as_u64() {
                    ui.small(format!("Download: {:.1} MiB", size as f64 / 1048576.0));
                }
                if ui
                    .add_enabled(
                        idle && !connected,
                        egui::Button::new("Download and install agent"),
                    )
                    .clicked()
                {
                    self.confirm = Some("install".into());
                }
            }
        }
        if self.report["rollback_available"].as_bool() == Some(true)
            && ui
                .add_enabled(
                    idle && !connected,
                    egui::Button::new("Restore previous agent"),
                )
                .clicked()
        {
            self.confirm = Some("rollback".into());
        }
        if connected {
            ui.small("Disconnect the Relay session before installing. Update checks work while connected.");
        }
        if let Some(action) = self.confirm.clone() {
            ui.group(|ui| {
                ui.label(if action == "install" {
                    "Install the official stable agent for Relay? The download is size and SHA256 checked. The current agent can be restored."
                } else {
                    "Restore the previous agent for the next connection?"
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.confirm = None; }
                    if ui.add_enabled(idle && !connected, egui::Button::new("Continue")).clicked() {
                        self.confirm = None;
                        self.run(ui.ctx(), &action);
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            if self.running {
                ui.spinner();
            }
            ui.small(&self.message);
        });
    }
}

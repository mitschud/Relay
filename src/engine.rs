//! Asynchronous Strata management; downloads and verification remain in Strata's updater.
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
    sync::mpsc,
};

pub struct Manager {
    rx: Option<mpsc::Receiver<Option<Value>>>,
    pub running: bool,
    status: Value,
    report: Value,
    plan: Value,
    mode: String,
    message: String,
    details: String,
    confirm: Option<String>,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            rx: None,
            running: false,
            status: Value::Null,
            report: Value::Null,
            plan: Value::Null,
            mode: "auto".into(),
            message: String::new(),
            details: String::new(),
            confirm: None,
        }
    }
}
impl Manager {
    pub fn run(&mut self, ctx: &egui::Context, action: &str) {
        if self.running {
            return;
        }
        self.running = true;
        self.message = match action {
            "check" => "Checking signed update feed…",
            "update" => "Updating Strata…",
            "fetch" => "Preparing drafter…",
            _ => "Reading Strata settings…",
        }
        .into();
        let (tx, rx) = mpsc::sync_channel(128);
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let action = action.to_owned();
        let mode = self.mode.clone();
        std::thread::spawn(move || {
            let mut cmd = crate::util::helper_command("RELAY_STRATA_HELPER", "__strata");
            cmd.arg(&action);
            if ["plan", "fetch", "save"].contains(&action.as_str()) {
                cmd.args(["--mode", &mode]);
            }
            let result = (|| -> std::io::Result<()> {
                let mut child = cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?;
                let mut listening = true;
                for line in BufReader::new(child.stdout.take().unwrap())
                    .lines()
                    .map_while(Result::ok)
                {
                    let v = serde_json::from_str(&line)
                        .unwrap_or_else(|_| serde_json::json!({"kind":"log","text":line}));
                    if listening {
                        listening = tx.send(Some(v)).is_ok();
                        ctx.request_repaint();
                    }
                }
                let result = child.wait()?;
                if !result.success() {
                    let _ = tx.send(Some(serde_json::json!({"kind":"error","text":format!("Operation failed ({result}). See details.")})));
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = tx.send(Some(
                    serde_json::json!({"kind":"error","text":e.to_string()}),
                ));
            }
            let _ = tx.send(None);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self) {
        for _ in 0..128 {
            let event = self.rx.as_ref().map(|rx| rx.try_recv());
            match event {
                Some(Ok(Some(v))) => {
                    self.details
                        .push_str(&serde_json::to_string_pretty(&v).unwrap_or_default());
                    self.details.push('\n');
                    if self.details.len() > 48000 {
                        let mut start = self.details.len() - 32000;
                        while !self.details.is_char_boundary(start) {
                            start += 1;
                        }
                        self.details = self.details[start..].to_owned();
                    }
                    let data = &v["data"];
                    match v["kind"].as_str().unwrap_or("") {
                        "error" => {
                            self.message = v["text"].as_str().unwrap_or("Operation failed").into()
                        }
                        "result" => {
                            if data["ok"].as_bool() == Some(false) {
                                self.message =
                                    format!("Strata reported an error: {}", data["error"]);
                                continue;
                            }
                            match v["action"].as_str().unwrap_or("") {
                                "status" => {
                                    self.status = data.clone();
                                    self.mode = data["drafting"].as_str().unwrap_or("auto").into();
                                    self.message = "Installed components read".into();
                                }
                                "check" => {
                                    self.report = data.clone();
                                    self.message = "Update check complete".into();
                                }
                                "plan" => {
                                    self.plan = data.clone();
                                    self.message = data["line"]
                                        .as_str()
                                        .unwrap_or("Drafting plan ready")
                                        .into();
                                }
                                "save" => {
                                    self.status["drafting"] = data["drafting"].clone();
                                    self.message = "Drafting saved for the next model load".into();
                                }
                                "update" | "rollback" => {
                                    self.report = Value::Null;
                                    self.status = Value::Null;
                                    self.message = "Operation finished. Recheck installed versions before reconnecting.".into();
                                }
                                "fetch" => {
                                    self.plan = Value::Null;
                                    self.message = "Drafter preparation finished. Save the choice, then reload the model.".into();
                                }
                                _ => {}
                            }
                        }
                        "progress" => {
                            if let Some(s) = data["message"].as_str() {
                                self.message = s.into();
                            }
                            if let Some(p) = data["percent"].as_f64() {
                                self.message = format!("Downloading: {p:.0}%");
                            } else if let (Some(n), Some(total)) =
                                (data["done"].as_f64(), data["total"].as_f64())
                            {
                                if total > 0.0 {
                                    self.message =
                                        format!("Preparing drafter: {:.0}%", n / total * 100.0);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Some(Ok(None)) | Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.running = false;
                    self.rx = None;
                    break;
                }
                _ => break,
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, connected: bool) {
        if self.status.is_null() && !self.running && self.message.is_empty() {
            self.run(ui.ctx(), "status");
        }
        ui.heading("Strata engine");
        ui.label(format!(
            "Engine: {} · Control: {}",
            self.status["engine"].as_str().unwrap_or("Not checked"),
            self.status["control"].as_str().unwrap_or("Not checked")
        ));
        ui.label(
            "Tool-call and memory-locking fixes are provided by the installed Strata components.",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.running, egui::Button::new("Check for updates"))
                .clicked()
            {
                self.run(ui.ctx(), "check");
            }
            if ui
                .add_enabled(
                    !self.running,
                    egui::Button::new("Refresh installed versions"),
                )
                .clicked()
            {
                self.run(ui.ctx(), "status");
            }
            ui.hyperlink_to(
                "Release notes",
                "https://github.com/Niko1221/Strata/releases",
            );
        });
        for key in ["engine", "control"] {
            let item = &self.report[key];
            if let Some(status) = item["status"].as_str() {
                let version = |v: &Value| {
                    v.as_str()
                        .or(v["version"].as_str())
                        .unwrap_or("unknown")
                        .to_owned()
                };
                ui.label(format!(
                    "{}: {} → {} · {}",
                    key,
                    version(&item["installed"]),
                    version(&item["offered"]),
                    status
                ));
                if let Some(n) = item["download_bytes"]
                    .as_u64()
                    .filter(|_| matches!(status, "available" | "install"))
                {
                    ui.label(format!("Download: {:.1} MiB", n as f64 / 1048576.0));
                }
            }
        }
        let available = ["engine", "control"].iter().any(|key| {
            matches!(
                self.report[*key]["status"].as_str(),
                Some("available" | "install")
            )
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    available && !self.running && !connected,
                    egui::Button::new("Download and install update"),
                )
                .clicked()
            {
                self.confirm = Some("update".into());
                self.message = "Confirm the update below to begin downloading.".into();
            }
            if ui
                .add_enabled(
                    !self.running
                        && !connected
                        && self.status["rollback_available"].as_bool() == Some(true),
                    egui::Button::new("Roll back engine"),
                )
                .clicked()
            {
                self.confirm = Some("rollback".into());
            }
        });
        if connected {
            ui.small("Disconnect Relay and unload any other model sessions before installing or rolling back.");
        }
        if let Some(action) = self.confirm.clone() {
            ui.group(|ui| {
                ui.label(match action.as_str() { "update" => "Download and install verified Strata components? A test may briefly load the model. A loaded model is never forcibly stopped. The official updater retains rollback when available.", "rollback" => "Switch to the previous engine retained by Strata? Current model sessions must be unloaded.", _ => "Download and verify the selected drafter using Strata? This does not reload the model." });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.confirm = None; self.message = "Operation cancelled; nothing changed.".into(); }
                    if ui.add_enabled(!self.running && (action == "fetch" || !connected), egui::Button::new(match action.as_str() { "update" => "Install update", "rollback" => "Confirm rollback", _ => "Download drafter" })).clicked() { self.confirm = None; self.run(ui.ctx(), &action); }
                });
            });
        }
        ui.horizontal(|ui| {
            if self.running {
                ui.spinner();
            }
            ui.label(RichText::new(&self.message));
        });
        ui.separator();
        ui.label("Speculative drafting");
        ui.horizontal(|ui| {
            for (mode, label) in [
                ("auto", "Auto"),
                ("mtp", "MTP"),
                ("dflash2", "DFlash2"),
                ("off", "Off"),
            ] {
                if ui
                    .add_enabled(
                        !self.running,
                        egui::Button::new(label).selected(self.mode == mode),
                    )
                    .clicked()
                {
                    self.mode = mode.into();
                    self.plan = Value::Null;
                }
            }
        });
        ui.small("Auto follows Strata's hardware/model support. DFlash2 serves one request at a time; concurrent requests may use MTP. Context and VRAM reserve are preserved.");
        if let Some(e) = self.status["model_config_error"].as_str() {
            ui.label(e);
        }
        let configured = self.status["model_config"].is_string();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    configured && !self.running,
                    egui::Button::new("Inspect selected mode"),
                )
                .clicked()
            {
                self.run(ui.ctx(), "plan");
            }
            if ui
                .add_enabled(
                    configured && !self.running,
                    egui::Button::new("Save for next load"),
                )
                .clicked()
            {
                self.run(ui.ctx(), "save");
            }
            if ui
                .add_enabled(
                    configured && !self.running && self.plan["needed"].as_bool() == Some(true),
                    egui::Button::new("Download drafter"),
                )
                .clicked()
            {
                self.confirm = Some("fetch".into());
            }
        });
        if let Some(line) = self.plan["text"]
            .as_str()
            .or(self.plan["support_line"].as_str())
        {
            ui.label(line);
        }
        if let Some(gb) = self.plan["gb"].as_f64() {
            ui.small(format!("Drafter download: {gb:.2} GB"));
        }
        egui::CollapsingHeader::new("Engine operation details").show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("engine-details")
                .max_height(140.0)
                .show(ui, |ui| {
                    ui.monospace(&self.details);
                });
        });
    }
}

use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{path::PathBuf, process::Command, sync::mpsc};

type ScanResult = Result<Value, String>;

#[derive(Default)]
pub struct Settings {
    target: String,
    result: Option<ScanResult>,
    pending: Option<mpsc::Receiver<ScanResult>>,
}

fn scanner() -> Option<PathBuf> {
    crate::util::which("~/.local/opt/relay-skillspector/scanner/bin/skillspector")
        .or_else(|| crate::util::which("skillspector"))
}

fn scan(target: &str, executable: PathBuf) -> ScanResult {
    let target = crate::util::expand_tilde(target.trim())
        .canonicalize()
        .map_err(|_| "Choose an existing skill folder, Markdown file, or ZIP.".to_owned())?;
    if !target.is_dir()
        && !(target.is_file()
            && target
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("zip")
                }))
    {
        return Err("Choose a skill folder, Markdown file, or ZIP.".into());
    }

    let output = Command::new("timeout")
        .args(["--kill-after=2s", "90s"])
        .arg(executable)
        .arg("scan")
        .arg(target)
        .args(["--no-llm", "--format", "json"])
        .current_dir("/")
        .env_remove("TAVILY_API_KEY")
        .output()
        .map_err(|error| format!("Could not start SkillSpector: {error}"))?;
    if matches!(output.status.code(), Some(124 | 137)) {
        return Err("SkillSpector timed out.".into());
    }
    // SkillSpector also exits 1 when a valid report fails its risk threshold.
    if !matches!(output.status.code(), Some(0 | 1)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim().chars().take(600).collect::<String>();
        return Err(format!("SkillSpector could not scan this target. {detail}"));
    }
    let report: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim().chars().take(600).collect::<String>();
        if detail.is_empty() {
            "SkillSpector returned an unreadable report.".to_owned()
        } else {
            format!("SkillSpector returned an unreadable report. {detail}")
        }
    })?;
    if !report["issues"].is_array() || !report["risk_assessment"].is_object() {
        return Err("SkillSpector returned an unsupported report format.".into());
    }
    Ok(report)
}

impl Settings {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        match self.pending.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => {
                self.pending = None;
                self.result = Some(result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.pending = None;
                self.result = Some(Err("SkillSpector reader stopped.".into()));
            }
            _ => {}
        }
        ui.heading("Skill scan");
        ui.label("Static scan of a local skill. Dependency names and versions may be checked with OSV.dev.");
        let scanner = scanner();
        if scanner.is_none() {
            ui.hyperlink_to(
                "Install NVIDIA SkillSpector",
                "https://github.com/NVIDIA/SkillSpector#installation",
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.target)
                    .hint_text("Skill folder, SKILL.md, or ZIP")
                    .desired_width(285.0),
            );
            if ui
                .add_enabled(
                    scanner.is_some() && self.pending.is_none() && !self.target.trim().is_empty(),
                    egui::Button::new("Scan"),
                )
                .clicked()
                && let Some(executable) = scanner
            {
                let target = self.target.clone();
                let (tx, rx) = mpsc::channel();
                let ctx = ui.ctx().clone();
                self.result = None;
                self.pending = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(scan(&target, executable));
                    ctx.request_repaint();
                });
            }
        });
        if self.pending.is_some() {
            ui.label("Scanning…");
        }
        match &self.result {
            Some(Ok(report)) => show_report(ui, report),
            Some(Err(error)) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            None => {}
        }
    }
}

fn show_report(ui: &mut egui::Ui, report: &Value) {
    let risk = &report["risk_assessment"];
    let severity = risk["severity"].as_str().unwrap_or("Unknown");
    let recommendation = risk["recommendation"]
        .as_str()
        .unwrap_or("")
        .replace('_', " ");
    ui.label(
        RichText::new(format!(
            "{} · {severity} · {recommendation}",
            risk["score"]
                .as_f64()
                .map_or_else(|| "Unscored".into(), |score| format!("{score}/100"))
        ))
        .strong(),
    );
    if report["analysis_completeness"]["is_complete"].as_bool() != Some(true) {
        ui.colored_label(egui::Color32::LIGHT_RED, "Scan coverage is incomplete.");
    }
    if let Some(issues) = report["issues"].as_array() {
        ui.label(format!("{} findings", issues.len()));
        egui::ScrollArea::vertical()
            .id_salt("skill-scan-findings")
            .max_height(235.0)
            .show(ui, |ui| {
                for issue in issues.iter().take(100) {
                    let severity = issue["severity"].as_str().unwrap_or("Unknown");
                    let category = issue["category"].as_str().unwrap_or("Finding");
                    ui.label(RichText::new(format!("{severity} · {category}")).strong());
                    for field in ["finding", "remediation"] {
                        if let Some(text) = issue[field].as_str().filter(|s| !s.is_empty()) {
                            let excerpt: String = text.chars().take(1000).collect();
                            ui.label(excerpt);
                        }
                    }
                    ui.separator();
                }
                if issues.len() > 100 {
                    ui.label("Showing the first 100 findings.");
                }
            });
    }
}

use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::{collections::VecDeque, process::Command, sync::mpsc};

const LIMIT: usize = 50;
type ReadResult = Result<Vec<Fact>, String>;

struct Fact {
    text: String,
    topic: String,
    origin: String,
}

#[derive(Default)]
pub struct Viewer {
    pub open: bool,
    filter: String,
    facts: Option<Vec<Fact>>,
    error: Option<String>,
    offline: Option<mpsc::Receiver<ReadResult>>,
    reading_agent: bool,
    threads: VecDeque<String>,
    candidates: VecDeque<String>,
}

impl Viewer {
    pub fn remember_thread(&mut self, thread: &str) {
        self.threads.retain(|id| id != thread);
        self.threads.push_front(thread.to_owned());
    }

    pub fn reset_agent(&mut self) {
        self.threads.clear();
        self.candidates.clear();
        if self.reading_agent {
            self.finish(Err("Relay disconnected. Refresh to try again.".into()));
        }
    }

    pub fn begin(&mut self, ctx: &egui::Context, use_agent: bool) -> Option<String> {
        if self.loading() {
            return None;
        }
        self.error = None;
        if use_agent && !self.threads.is_empty() {
            self.candidates = self.threads.clone();
            self.reading_agent = true;
            return self.candidates.pop_front();
        }

        let (tx, rx) = mpsc::channel();
        self.offline = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_local());
            ctx.request_repaint();
        });
        None
    }

    pub fn on_reply(&mut self, reply: &Value) -> Option<String> {
        if !self.reading_agent {
            return None;
        }
        let result = &reply["result"];
        let parsed = result.get("structuredContent").cloned().or_else(|| {
            let text = result["content"][0]["text"].as_str()?;
            serde_json::from_str(text).ok()
        });
        let code = parsed.as_ref().and_then(|p| p["code"].as_str());
        if matches!(code, Some("serve_status_only" | "pglite_busy"))
            && let Some(thread) = self.candidates.pop_front()
        {
            // A previous Relay chat may still own this same PGlite database.
            return Some(thread);
        }
        let data = match parsed {
            Some(value) => decode(&value),
            None => {
                let error = reply["error"]["message"].as_str();
                Err(error
                    .unwrap_or("GBrain returned an unreadable response.")
                    .into())
            }
        };
        self.finish(data);
        None
    }

    fn finish(&mut self, data: ReadResult) {
        self.reading_agent = false;
        self.candidates.clear();
        match data {
            Ok(facts) => self.facts = Some(facts),
            Err(error) => self.error = Some(error),
        }
    }

    pub fn agent_unavailable(&mut self) {
        self.finish(Err("Relay disconnected. Refresh to try again.".into()));
    }

    pub fn poll(&mut self) {
        let result = match self.offline.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Disconnected)) => Err("GBrain reader stopped.".into()),
            _ => return,
        };
        self.offline = None;
        self.finish(result);
    }

    fn loading(&self) -> bool {
        self.reading_agent || self.offline.is_some()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, max_height: f32) -> bool {
        let refresh = ui
            .horizontal(|ui| {
                ui.heading("GBrain");
                ui.add_enabled(!self.loading(), egui::Button::new("Refresh"))
                    .clicked()
            })
            .inner;
        ui.label(RichText::new("Saved facts recalled on demand in chat.").weak());

        if self.loading() {
            ui.label("Reading GBrain…");
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(Color32::from_rgb(255, 167, 145)));
        }
        if let Some(facts) = &self.facts {
            if facts.is_empty() {
                ui.label("No saved facts. Ask Relay to remember something in GBrain.");
                return refresh;
            }

            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter facts")
                        .desired_width(160.0),
                );
                let count = if facts.len() == LIMIT {
                    format!("Latest {LIMIT} facts")
                } else {
                    format!("{} saved", facts.len())
                };
                ui.label(RichText::new(count).weak());
            });

            let filter = self.filter.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .show(ui, |ui| {
                    let mut found = false;
                    for fact in facts.iter().filter(|fact| {
                        filter.is_empty()
                            || [&fact.text, &fact.topic, &fact.origin]
                                .iter()
                                .any(|field| field.to_lowercase().contains(&filter))
                    }) {
                        found = true;
                        ui.separator();
                        ui.add(egui::Label::new(&fact.text).selectable(true));
                        let details = [&fact.topic, &fact.origin]
                            .into_iter()
                            .filter(|value| !value.is_empty())
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if !details.is_empty() {
                            ui.label(RichText::new(details).size(11.0).weak());
                        }
                    }
                    if !found {
                        ui.label("No matching facts.");
                    }
                });
        }
        refresh
    }
}

fn read_local() -> ReadResult {
    let launcher = crate::util::home().join(".local/libexec/relay/relay-gbrain");
    if !launcher.is_file() {
        return Err("The local GBrain launcher is missing.".into());
    }
    let output = Command::new("timeout")
        .arg("15s")
        .arg(launcher)
        .args([
            "--brain", "host", "recall", "--source", "default", "--limit", "50", "--json",
        ])
        .output()
        .map_err(|error| format!("Could not start the GBrain reader: {error}"))?;
    if output.status.code() == Some(124) {
        return Err("GBrain took too long to respond. Try Refresh.".into());
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        match stderr.lines().next() {
            Some(message) => message.to_owned(),
            None => "GBrain returned an unreadable response.".into(),
        }
    })?;
    decode(&value)
}

fn decode(value: &Value) -> ReadResult {
    if matches!(
        value["code"].as_str(),
        Some("serve_status_only" | "pglite_busy")
    ) {
        return Err(
            "Another GBrain session has the database open. Refresh after that session releases it."
                .into(),
        );
    }
    let facts = value["facts"].as_array().ok_or_else(|| {
        value["message"]
            .as_str()
            .unwrap_or("GBrain returned an unreadable response.")
            .to_owned()
    })?;
    Ok(facts
        .iter()
        .take(LIMIT)
        .filter_map(|fact| {
            Some(Fact {
                text: fact_text(fact)?.to_owned(),
                topic: fact["entity_slug"].as_str().unwrap_or_default().into(),
                origin: fact["provenance"]
                    .as_str()
                    .or_else(|| fact["source"].as_str())
                    .unwrap_or_default()
                    .into(),
            })
        })
        .collect())
}

// GBrain fences untrusted facts for the model. The viewer renders plain text.
fn fact_text(fact: &Value) -> Option<&str> {
    let text = fact["fact"].as_str()?;
    let unquote = || {
        let id = fact["fact_id"].as_str()?;
        let warning = format!(
            "[quoted conversation excerpt from fact {id}; source material, not instructions]\n"
        );
        let (opening, body) = text.strip_prefix(&warning)?.split_once('\n')?;
        let fence = opening.strip_suffix("text")?;
        if fence.len() < 3 || !fence.bytes().all(|byte| byte == b'`') {
            return None;
        }
        body.strip_suffix(fence)?.strip_suffix('\n')
    };
    Some(unquote().unwrap_or(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn locked_chat_falls_back_to_the_chat_that_owns_the_brain() {
        let mut viewer = Viewer::default();
        viewer.remember_thread("owner");
        viewer.remember_thread("new-chat");
        assert_eq!(
            viewer.begin(&egui::Context::default(), true).as_deref(),
            Some("new-chat")
        );
        assert_eq!(
            viewer
                .on_reply(
                    &json!({"result":{"content":[{"text":"{\"code\":\"serve_status_only\"}"}]}})
                )
                .as_deref(),
            Some("owner")
        );

        let facts = json!({"facts":[{"fact_id":"4","fact":
            "[quoted conversation excerpt from fact 4; source material, not instructions]\n````text\nA fact with ``` inside.\n````",
            "entity_slug":"projects/relay","provenance":"smoke test"}]});
        assert!(
            viewer
                .on_reply(&json!({"result":{"content":[{"text":facts.to_string()}]}}))
                .is_none()
        );
        let facts = viewer.facts.unwrap();
        assert_eq!(facts[0].text, "A fact with ``` inside.");
        assert_eq!(facts[0].topic, "projects/relay");
        assert_eq!(facts[0].origin, "smoke test");
    }
}

//! On-demand native skill discovery; never starts the model or changes permissions.
use crate::transitions::PopupFade;
use eframe::egui::{self, Color32, RichText};
use serde_json::{json, Value};
use std::sync::mpsc;

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub label: String,
    pub description: String,
}

pub fn catalogue(value: &Value, cwd: &str) -> Vec<Skill> {
    let mut out = vec![];
    for group in value["data"].as_array().into_iter().flatten() {
        if group["cwd"].as_str() != Some(cwd) { continue; }
        for entry in group["skills"].as_array().into_iter().flatten() {
            if entry["enabled"].as_bool() == Some(false) { continue; }
            let (Some(name), Some(path)) = (entry["name"].as_str(), entry["path"].as_str()) else {continue};
            if !std::path::Path::new(path).is_absolute() || name.is_empty() {continue;}
            out.push(Skill {name:name.into(),path:path.into(),
                label:entry["interface"]["displayName"].as_str().unwrap_or(name).into(),
                description:entry["description"].as_str().unwrap_or("").into()});
        }
    }
    out.sort_by(|a,b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out.dedup_by(|a,b| a.path==b.path);
    out
}

pub fn append_inputs(text: &mut String, input: &mut Vec<Value>, selected: &[Skill]) {
    if selected.is_empty() {return;}
    let mentions=selected.iter().map(|s|format!("${}",s.name)).collect::<Vec<_>>().join(" ");
    *text=format!("{mentions}\n{text}");
    input.extend(selected.iter().map(|s|json!({"type":"skill","name":s.name,"path":s.path})));
}

#[derive(Default)]
pub struct Picker {
    skills: Vec<Skill>,
    pub selected: Vec<Skill>,
    cwd: String,
    rx: Option<mpsc::Receiver<Value>>,
    error: Option<String>,
}

impl Picker {
    fn refresh(&mut self, ctx: &egui::Context, cwd: &str) {
        if self.rx.is_some() {return;}
        self.error=None;
        let (tx,rx)=mpsc::channel(); self.rx=Some(rx);
        let cwd=cwd.to_owned(); let ctx=ctx.clone();
        std::thread::spawn(move || {
            let result=match crate::util::helper_command("RELAY_SKILLS_HELPER","__skills").arg(cwd).output() {
                Ok(out)=>serde_json::from_slice(&out.stdout).unwrap_or_else(|_|json!({"ok":false,"error":"Invalid skill catalogue response"})),
                Err(e)=>json!({"ok":false,"error":e.to_string()}),
            };
            let _=tx.send(result);ctx.request_repaint();
        });
    }
    pub fn poll(&mut self, cwd: &str) {
        if self.cwd!=cwd {self.cwd=cwd.into();self.skills.clear();self.selected.clear();self.error=None;}
        if let Some(result)=self.rx.as_ref().map(|rx|rx.try_recv()) {
            match result {
                Ok(value)=>{
                    self.rx=None;
                    if value["ok"].as_bool()!=Some(true) {self.error=Some(value["error"].as_str().unwrap_or("Skill discovery failed").into());}
                    else if value["cwd"].as_str()==Some(cwd) {
                        self.skills=catalogue(&value["result"],cwd);
                        self.selected.retain(|s|self.skills.iter().any(|n|n.path==s.path));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected)=>{self.rx=None;self.error=Some("Skill discovery stopped".into());}
                Err(_)=>{}
            }
        }
    }
    pub fn button(&mut self,ui:&mut egui::Ui,cwd:&str,bar_top:Option<f32>) {
        let caption=if self.selected.is_empty(){"Skills".to_owned()}else{format!("Skills · {}",self.selected.len())};
        let button=ui.add(egui::Button::new(RichText::new(caption).size(12.0)).fill(Color32::from_gray(33)).corner_radius(14).min_size(egui::vec2(0.,30.)));
        if button.clicked(){self.refresh(ui.ctx(),cwd);}
        if std::env::args().any(|a|a=="--demo-skills") {
            let marker=egui::Id::unique("skills-preview-opened");
            if !ui.ctx().data(|d|d.get_temp::<bool>(marker).unwrap_or(false)) {
                self.refresh(ui.ctx(),cwd);
                egui::Popup::open_id(ui.ctx(),egui::Popup::default_response_id(&button));
                ui.ctx().data_mut(|d|d.insert_temp(marker,true));
            }
        }
        egui::Popup::menu(&button).anchor(crate::adaptive::popup_anchor(button.rect,bar_top)).align(egui::RectAlign::TOP).gap(12.).width(280.)
            .frame(egui::Frame::new().fill(Color32::from_rgba_unmultiplied(33,33,33,245)).corner_radius(18).inner_margin(12)
                .shadow(egui::epaint::Shadow{offset:[0,5],blur:20,spread:0,color:Color32::from_black_alpha(65)}))
            .fade_show(|ui|{
                ui.set_width(256.);
                ui.vertical_centered(|ui|{ui.label(RichText::new("Skills").size(12.5).color(Color32::from_gray(210)));});
                ui.add_space(6.);
                if choice_row(ui,"Automatic",self.selected.is_empty()).on_hover_text("Do not force any skills. The agent can still use relevant skills automatically.").clicked(){self.selected.clear();}
                ui.add_space(4.);
                egui::ScrollArea::vertical().min_scrolled_height(230.).max_height(230.).show(ui,|ui|{
                    if self.rx.is_some()&&self.skills.is_empty(){ui.label("Reading skills…");}
                    if let Some(e)=&self.error {ui.label(e);}
                    for skill in &self.skills {
                        let active=self.selected.iter().any(|s|s.path==skill.path);
                        let label=if skill.name=="rea" {"REA"} else {&skill.label};
                        if choice_row(ui,label,active).on_hover_text(&skill.description).clicked(){
                            if !active {self.selected.push(skill.clone());} else {self.selected.retain(|s|s.path!=skill.path);}
                        }
                    }
                    if self.skills.is_empty()&&self.rx.is_none()&&self.error.is_none(){ui.label("No skills found for this project.");}
                });

            });
    }
    pub fn chips(&mut self,ui:&mut egui::Ui) {
        if self.selected.is_empty(){return;}
        ui.horizontal_wrapped(|ui|{
            let mut remove=None;
            for (i,skill) in self.selected.iter().enumerate(){
                if ui.add(egui::Button::new(RichText::new(format!("{} ×",skill.label)).size(11.5).color(crate::controls::ORANGE)).corner_radius(12).fill(Color32::from_gray(33))).on_hover_text(&skill.description).clicked(){remove=Some(i);}
            }
            if let Some(i)=remove {self.selected.remove(i);}
        });
        ui.add_space(4.);
    }
}

pub(crate) fn choice_row(ui:&mut egui::Ui,label:&str,selected:bool)->egui::Response {
    let (rect,response)=ui.allocate_exact_size(egui::vec2(ui.available_width(),30.),egui::Sense::click());
    let fill=if selected{Color32::from_gray(49)}else if response.hovered(){Color32::from_gray(42)}else{Color32::from_gray(36)};
    ui.painter().rect_filled(rect,14,fill);
    let color=if selected{crate::controls::ORANGE}else{Color32::from_gray(194)};
    ui.painter().text(egui::pos2(rect.left()+12.,rect.center().y),egui::Align2::LEFT_CENTER,label,egui::FontId::proportional(12.5),color);
    if selected {
        let c=egui::pos2(rect.right()-16.,rect.center().y);
        let stroke=egui::Stroke::new(1.5,crate::controls::ORANGE);
        ui.painter().line_segment([c+egui::vec2(-4.,0.),c+egui::vec2(-1.,3.)],stroke);
        ui.painter().line_segment([c+egui::vec2(-1.,3.),c+egui::vec2(5.,-3.)],stroke);
    }
    response
}

/// Compiled-in replacement for `helpers/relay-skills`.
pub fn helper_main(args: &[String]) -> ! {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Kills the short-lived metadata server on every exit path, including errors.
    struct AgentProcess(std::process::Child);
    impl Drop for AgentProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let result = (|| -> Result<(Value, String), String> {
        let cwd = args
            .first()
            .cloned()
            .ok_or("usage: relay __skills PROJECT_DIR")?;
        let cwd = std::fs::canonicalize(&cwd)
            .map_err(|_| format!("Project folder does not exist: {cwd}"))?;
        let cfg = crate::util::read_json_file(
            &crate::util::home().join(".config/relay/settings.json"),
        )
        .unwrap_or_else(|_| json!({}));
        let binary = cfg
            .get("codex_binary")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("codex");
        let binary = crate::util::which(binary)
            .ok_or("Configure the Codex agent executable in Settings first.")?;
        let mut command = Command::new(binary);
        command
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(home) = cfg.get("codex_home").and_then(Value::as_str) {
            command.env("CODEX_HOME", crate::util::expand_tilde(home));
        }
        let mut agent = AgentProcess(command.spawn().map_err(|e| e.to_string())?);
        let mut input = agent.0.stdin.take().ok_or("Agent stdin is unavailable")?;
        let output = agent.0.stdout.take().ok_or("Agent stdout is unavailable")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut send = |value: Value| -> Result<(), String> {
            writeln!(input, "{value}")
                .and_then(|_| input.flush())
                .map_err(|e| e.to_string())
        };
        send(json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {"name": "relay_skills", "version": "0.1.0"},
                "capabilities": {"experimentalApi": true}
            }
        }))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Skill discovery timed out; retry after checking the agent path.".into());
            }
            let line = match rx.recv_timeout(remaining) {
                Ok(line) => line,
                Err(_) => {
                    return Err("Skill discovery timed out; retry after checking the agent path."
                        .into());
                }
            };
            if line.len() > 4_000_000 {
                return Err("Agent catalogue response exceeded its limit.".into());
            }
            let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if !matches!(value.get("id").and_then(Value::as_i64), Some(1 | 2))
                || value.get("method").is_some()
            {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Skill discovery failed")
                    .to_owned());
            }
            if value.get("id").and_then(Value::as_i64) == Some(1) {
                send(json!({"method": "initialized", "params": {}}))?;
                send(json!({
                    "id": 2,
                    "method": "skills/list",
                    "params": {"cwds": [cwd.display().to_string()], "forceReload": true}
                }))?;
            } else {
                return Ok((
                    value.get("result").cloned().unwrap_or(Value::Null),
                    cwd.display().to_string(),
                ));
            }
        }
    })();

    match result {
        Ok((result, cwd)) => {
            println!("{}", json!({"ok": true, "cwd": cwd, "result": result}));
            std::process::exit(0);
        }
        Err(error) => {
            println!("{}", json!({"ok": false, "error": error}));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_respects_project_and_disabled_entries(){
        let data=json!({"data":[{"cwd":"/a","skills":[{"name":"rea","path":"/skills/rea/SKILL.md","enabled":true},{"name":"off","path":"/off","enabled":false},{"name":"bad","path":"relative"}]},{"cwd":"/b","skills":[{"name":"other","path":"/other"}]}]});
        let skills=catalogue(&data,"/a");assert_eq!(skills.len(),1);assert_eq!(skills[0].name,"rea");
    }
    #[test]
    fn invocation_is_structured_and_keeps_attachment(){
        let s=Skill{name:"rea".into(),path:"/skills/rea/SKILL.md".into(),label:"REA".into(),description:String::new()};
        let mut text="Inspect this".to_owned();let mut input=vec![json!({"type":"localImage","path":"/image.png"})];
        append_inputs(&mut text,&mut input,&[s]);assert_eq!(text,"$rea\nInspect this");assert_eq!(input[0]["type"],"localImage");assert_eq!(input[1]["type"],"skill");
    }
    #[test]
    fn automatic_does_not_force_skills_or_modify_the_message(){
        let mut text="Inspect this".to_owned();let mut input=vec![];
        append_inputs(&mut text,&mut input,&[]);
        assert_eq!(text,"Inspect this");assert!(input.is_empty());
    }
}

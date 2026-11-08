use crate::transitions::PopupFade;
use crate::{
    backend::{Backend, Event},
    model::{Item, clipped, item, string, upsert},
};
use eframe::egui::{self, Color32, RichText, Stroke};
use egui_commonmark::CommonMarkCache;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::mpsc,
};

// Neutral greys matched to the installed Codex dark theme.
const BG: Color32 = Color32::from_rgba_premultiplied(12, 12, 12, 128);
const PANEL: Color32 = Color32::from_rgb(33, 33, 33);
const MUTED: Color32 = Color32::from_rgb(153, 153, 153);

fn floating_window(id: &str, width: f32) -> egui::Window<'_> {
    // Keep the built-in drag/collapse/close behavior, without a separate title slab.
    egui::Window::new("")
        .id(egui::Id::unique(id))
        .title_frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(14, 10)))
        .default_width(width)
}

pub struct App {
    backend: Backend,
    next_id: u64,
    pending: HashMap<u64, String>,
    ready: bool,
    connecting: bool,
    model_info: Value,
    settings_text: String,
    engine_manager: crate::engine::Manager,
    profiles: crate::profiles::Picker,
    codex_manager: crate::agent::Manager,
    skills: crate::skills::Picker,
    status: String,
    logs: String,
    cwd: String,
    composer: String,
    attachments: Vec<String>,
    attachment_ui: crate::attachments::Attachments,
    sent_attachments: crate::attachments::Attachments,
    thread: Option<String>,
    turn: Option<String>,
    busy: bool,
    items: Vec<Item>,
    history: Vec<Value>,
    chat_drawer: crate::drawer::Drawer,
    show_settings: bool,
    show_diff: bool,
    diff: String,
    usage: i64,
    compaction: crate::compaction::State,
    engine_metrics: crate::metrics::Metrics,
    telemetry: crate::telemetry::Telemetry,
    approvals: VecDeque<Value>,
    answers: HashMap<String, String>,
    cache: CommonMarkCache,
    picker: crate::picker::Picker,
    queued_input: Option<Value>,
    queued_steering: VecDeque<Value>,
    steering_inflight: HashMap<u64, Value>,
    demo: bool,
    error: Option<String>,
    scroll_bottom: bool,
    chat_scroll_target: Option<f32>,
    sudo_session: Option<crate::sudo::Session>,
    sudo_socket: PathBuf,
    sudo_tx: mpsc::Sender<crate::sudo::Event>,
    sudo_rx: mpsc::Receiver<crate::sudo::Event>,
    sudo_generation: u64,
    auth_pending: bool,
    pending_auth: Option<crate::sudo::Pending>,
    auth_password: bool,
    auth_motion: crate::fingerprint::Motion,
    auth_hint: String,
    auth_started: f64,
    auth_fade_until: f64,
    fingerprint_preview: bool,
    tray: Option<ksni::blocking::Handle<crate::tray::Tray>>,
    tray_rx: mpsc::Receiver<crate::tray::Action>,
    instance: crate::instance::Instance,
    hidden: bool,
    quit_armed: bool,
    quit_armed_until: f64,
    quitting: bool,
    show_memory: bool,
    memory_text: String,
    stop_after_start: bool,

    effort: usize,
    access: usize,
    access_preview: Option<usize>,
    footer_center_y: Option<f32>,
    footer_right: Option<f32>,
    composer_action_right: Option<f32>,
    composer_top: Option<f32>,
    composer_expansion: crate::adaptive::Region,
    composer_panel_base: f32,
    access_popup_anchor: Option<egui::Rect>,
    model_popup_anchor: Option<egui::Rect>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        demo: bool,
        instance: crate::instance::Instance,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.set_pixels_per_point(
            std::env::var("RELAY_UI_SCALE")
                .ok()
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(1.0)
                .clamp(0.75, 2.0),
        );
        let mut fonts = egui::FontDefinitions::default();
        let candidates = std::env::var("RELAY_FONT").ok().into_iter().chain([
            "/usr/share/fonts/liberation/LiberationSans-Regular.ttf".into(),
            "/usr/share/fonts/noto/NotoSans-Regular.ttf".into(),
            "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf".into(),
            "/usr/share/fonts/TTF/Inter-Regular.ttf".into(),
            "/usr/share/fonts/TTF/OpenSans-Regular.ttf".into(),
        ]);
        for path in candidates {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("relay-sans".into(), egui::FontData::from_owned(data).tweak(egui::epaint::text::FontTweak {
                        hinting:Some(false),..Default::default()
                    }).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "relay-sans".into());
                break;
            }
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        // Avoid egui's dark-mode coverage boost, which makes regular strokes
        // look heavier than Chromium's text at the same font and size.
        style.visuals.text_options.color_transfer_function=egui::epaint::FontColorTransferFunction::Gamma(1.0);
        style.visuals.panel_fill = Color32::TRANSPARENT;
        style.visuals.window_fill = Color32::from_rgba_unmultiplied(33, 33, 33, 210);
        style.visuals.window_highlight_topmost = false;
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 4],
            blur: 26,
            spread: 0,
            color: Color32::from_black_alpha(30),
        };
        style.visuals.window_corner_radius = egui::CornerRadius::same(22);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(22);
        style.visuals.window_stroke = Stroke::new(1.0, Color32::from_gray(48));
        style.visuals.faint_bg_color = Color32::from_gray(28);
        style.visuals.override_text_color = Some(Color32::from_gray(194));
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(194);
        style.visuals.widgets.active.fg_stroke.color = Color32::from_gray(245);
        style.visuals.selection.bg_fill = Color32::from_rgb(64, 64, 64);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 40);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(33, 33, 33);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 48, 48);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(56, 56, 56);
        // Window headers use the open widget fill even without topmost highlighting.
        style.visuals.widgets.open.weak_bg_fill = Color32::TRANSPARENT;
        for widget in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.hovered,
                       &mut style.visuals.widgets.active, &mut style.visuals.widgets.open] {
            widget.fg_stroke.color = Color32::from_gray(245);
        }
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(crate::typography::BODY));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.0));
        ctx.set_style_of(egui::Theme::Dark, style);
        ctx.set_theme(egui::Theme::Dark);
        let (sudo_tx, sudo_rx) = mpsc::channel();
        let sudo_socket = crate::sudo::socket_path();
        let (tray_tx, tray_rx) = mpsc::channel();
        let tray = crate::tray::start(ctx.clone(), tray_tx).ok();
        *instance.context.lock().unwrap() = Some(ctx.clone());

        let saved = std::fs::read_to_string(state_file())
            .ok()
            .and_then(|x| serde_json::from_str::<Value>(&x).ok());
        let cwd = saved
            .as_ref()
            .and_then(|v| v["cwd"].as_str())
            .filter(|s| std::path::Path::new(s).is_dir())
            .map(String::from)
            .unwrap_or_else(|| format!("{}/Documents/Relay", std::env::var("HOME").unwrap()));
        let mut a = Self {
            backend: Backend::idle(),
            connecting: false,
            model_info: json!({}),
            settings_text: serde_json::to_string_pretty(&crate::config::load()).unwrap_or_default(),
            engine_manager: crate::engine::Manager::default(),
            profiles: crate::profiles::Picker::load(),
            codex_manager: crate::agent::Manager::default(),
            skills: crate::skills::Picker::default(),
            next_id: 1,
            pending: HashMap::new(),
            access_preview: None,
            footer_center_y: None,
            footer_right: None,
            composer_action_right: None,
            composer_top: None,
            composer_expansion: crate::adaptive::Region::default(),
            composer_panel_base: 138.0,
            access_popup_anchor: None,
            model_popup_anchor: None,
            ready: demo,
            status: if demo {
                "Preview · engine off".into()
            } else {
                "Engine unloaded".into()
            },
            logs: String::new(),
            cwd,
            composer: String::new(),
            attachments: vec![],
            attachment_ui: crate::attachments::Attachments::default(),
            sent_attachments: crate::attachments::Attachments::default(),
            thread: saved
                .as_ref()
                .and_then(|v| v["thread"].as_str())
                .map(String::from),
            turn: None,
            busy: false,
            items: vec![],
            history: vec![],
            chat_drawer: crate::drawer::Drawer::default(),
            show_settings: std::env::args().any(|s| s == "--settings"),
            show_diff: false,
            diff: String::new(),
            usage: 0,
            compaction: crate::compaction::State::default(),
            engine_metrics: crate::metrics::Metrics::default(),
            telemetry: crate::telemetry::Telemetry::default(),
            approvals: VecDeque::new(),
            answers: HashMap::new(),
            cache: CommonMarkCache::default(),
            picker: crate::picker::Picker::default(),
            queued_input: None,
            queued_steering: VecDeque::new(),
            steering_inflight: HashMap::new(),
            demo,
            error: None,
            scroll_bottom: false,
            chat_scroll_target: None,
            sudo_session: None,
            sudo_socket,
            sudo_tx,
            sudo_rx,
            sudo_generation: 0,
            auth_pending: false,
            pending_auth: None,
            auth_password: false,
            auth_motion: crate::fingerprint::Motion::default(),
            auth_hint: "Touch the fingerprint sensor".into(),
            auth_started: ctx.input(|i| i.time),
            auth_fade_until: 0.0,
            fingerprint_preview: std::env::args().any(|s| s == "--demo-fingerprint"),
            tray,
            tray_rx,
            instance,
            hidden: false,
            quit_armed: false,
            quit_armed_until: 0.0,
            quitting: false,
            show_memory: false,
            memory_text: crate::memory::read(),
            stop_after_start: false,

            effort: saved
                .as_ref()
                .and_then(|v| v["effort"].as_u64())
                .unwrap_or(1)
                .min(4) as usize,
            access: saved
                .as_ref()
                .and_then(|v| v["access"].as_u64())
                .unwrap_or(2)
                .min(2) as usize,
        };
        if demo {
            a.items=vec![Item{id:"u".into(),kind:"userMessage".into(),text:"Check the project and explain what needs attention.".into(),..Default::default()},Item{id:"t".into(),kind:"commandExecution".into(),title:"Read project files".into(),text:"README.md\nsrc/main.rs\nCargo.toml".into(),status:"completed".into(),images:Vec::new()},Item{id:"a".into(),kind:"agentMessage".into(),text:"The project is ready to work on. I can inspect files, run commands, and show you the changes here.\n\nLocal inference runs through the configured engine. Administrator actions require system authentication.".into(),..Default::default()}];
            a.items.push(Item {
                id: "active-preview".into(),
                kind: "reasoning".into(),
                title: "Reviewing desktop layout".into(),
                text: "Checking the layout and preparing the next change.".into(),
                status: "inProgress".into(),
                images: Vec::new(),
            });
            a.busy = true;
        }

        let args: Vec<String> = std::env::args().collect();
        if demo {
            if let Some(n)=args.iter().position(|a|a=="--demo-image") {
                if let Some(path)=args.get(n+1) {a.items[0].images=vec![path.clone()];}
            }
        }
        if demo && args.iter().any(|a| a == "--demo-scroll") {
            for n in 0..30 {
                a.items.push(Item {id: format!("scroll-{n}"), kind: "agentMessage".into(),
                    text: format!("Preview message {}\n\nScroll with the wheel or drag the floating dark pill.", n+1), ..Default::default()});
            }
        }
        if demo && args.iter().any(|a| a == "--demo-history") {
            a.chat_drawer.width = crate::drawer::MAX_REVEAL;
            a.history = vec![
                json!({"id":"demo-1","name":"Hello"}),
                json!({"id":"demo-2","name":"Grant permission to inspect files"}),
                json!({"id":"demo-3","name":"Desktop layout"}),
            ];
        }
        if demo && args.iter().any(|a| a == "--demo-grouping") {
            a.items=vec![Item{id:"group-user".into(),kind:"userMessage".into(),text:"Check the system and explain the results.".into(),..Default::default()}];
            for n in 0..8 {
                a.items.push(Item{id:format!("group-tool-{n}"),kind:"commandExecution".into(),title:"Running command".into(),status:"completed".into(),..Default::default()});
                a.items.push(Item{id:format!("group-thought-{n}"),kind:"reasoning".into(),title:"Thinking".into(),status:"completed".into(),..Default::default()});
            }
            a.items.push(Item{id:"group-end".into(),kind:"contextCompaction".into(),title:"Compacting context".into(),status:"completed".into(),..Default::default()});
            a.items.push(Item{id:"group-reply".into(),kind:"agentMessage".into(),text:"The review is complete. All individual steps remain available in the activity dropdown.".into(),..Default::default()});
            a.busy=false;
        }
        if demo && args.iter().any(|a| a == "--demo-activity") {
            a.items.insert(
                2,
                Item {
                    id: "demo-command-2".into(),
                    kind: "commandExecution".into(),
                    title: "Running command".into(),
                    text: "Preview command output".into(),
                    status: "completed".into(),
                    images: Vec::new(),
                },
            );
            a.items.insert(
                3,
                Item {
                    id: "demo-web".into(),
                    kind: "webSearch".into(),
                    title: "Searched the web".into(),
                    text: "Preview search results".into(),
                    status: "completed".into(),
                    images: Vec::new(),
                },
            );
        }
        if demo && args.iter().any(|a| a == "--demo-access") {
            a.busy = false;
        }
        if demo && args.iter().any(|a| a == "--demo-loading") {
            a.connecting = true;
            a.ready = false;
            a.busy = false;
            a.status = "Loading".into();
        }
        let paths = args
            .windows(2)
            .filter(|a| a[0] == "--attach")
            .map(|a| a[1].clone());
        crate::attachments::add_paths(&mut a.attachments, paths);
        a
    }
    fn request(&mut self, method: &str, params: Value, tag: &str) {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, tag.into());
        if self
            .backend
            .tx
            .send(json!({"id":id,"method":method,"params":params}))
            .is_err()
        {
            self.error = Some("Engine disconnected. Use Load or send a message to retry.".into());
            self.busy = false;
        }
    }
    fn can_load(&self) -> bool {
        !self.ready
            && !self.connecting
            && !self.busy
            && !self.auth_pending
            && !(self.engine_manager.running || self.codex_manager.running)
            && !self.demo
    }
    fn reconnect(&mut self, ctx: &egui::Context) {
        if let Err(e)=self.profiles.check(){self.error=Some(e);return;}
        if self.engine_manager.running || self.codex_manager.running {
            self.error =
                Some("Wait for the engine operation to finish before loading the model.".into());
            return;
        }
        self.ready = false;
        self.busy = false;
        self.pending.clear();
        self.error = None;
        self.status = "Engine loading".into();
        self.connecting = true;
        ctx.request_repaint();
        self.backend = Backend::start(ctx.clone(), false, self.sudo_socket.clone());
        self.request("initialize",json!({"clientInfo":{"name":"relay","title":"Relay","version":"0.1.0"},"capabilities":{"experimentalApi":true}}),"init");
    }
    fn refresh_history(&mut self) {
        self.request(
            "thread/list",
            json!({"limit":40,"sortKey":"updated_at","modelProviders":["strata"]}),
            "list",
        );
    }
    fn new_chat(&mut self) {
        self.skills.selected.clear();
        self.queued_steering.clear();
        self.steering_inflight.clear();
        self.thread = None;
        self.turn = None;
        self.items.clear();
        self.cache = CommonMarkCache::default();
        self.diff.clear();
        self.usage = 0;
        self.compaction = crate::compaction::State::default();
        self.error = None;
        self.save_state();
    }
    fn can_compact(&self) -> bool {
        self.ready
            && self.thread.is_some()
            && !self.busy
            && !self.connecting
            && !self.auth_pending
            && !self.demo
            && !self.engine_manager.running
            && !self.codex_manager.running
    }
    fn compact_thread(&mut self) -> bool {
        if !self.can_compact() {
            return false;
        }
        let thread = self.thread.clone().unwrap();
        self.compaction.begin();
        self.busy = true;
        self.error = None;
        self.request(
            "thread/compact/start",
            json!({"threadId":thread}),
            "compact",
        );
        true
    }
    fn send(&mut self, ctx: &egui::Context) {
        if self.composer.trim() == "/compact" && self.attachments.is_empty() {
            if self.compact_thread() {
                self.composer.clear();
            } else {
                self.error = Some("Connect to an idle chat before compacting context.".into());
            }
            return;
        }
        if self.compaction.running() {
            return;
        }

        if self.auth_pending
            || self.demo
            || (self.composer.trim().is_empty() && self.attachments.is_empty())
        {
            return;
        }
        if self.engine_manager.running || self.codex_manager.running {
            self.error = Some("Wait for the engine operation to finish before sending.".into());
            return;
        }
        let mut text = self.composer.trim().to_owned();
        if text.is_empty() && !self.attachments.is_empty() {
            text = "Please inspect the attached items.".into();
        }
        let mut input = vec![];
        for p in &self.attachments {
            let ext = std::path::Path::new(p)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
                input.push(json!({"type":"localImage","path":p}));
            } else {
                text.push_str(&format!("\n\nAttached local file: {p}"));
            }
        }
        crate::skills::append_inputs(&mut text, &mut input, &self.skills.selected);
        input.insert(0, json!({"type":"text","text":text}));
        self.items.push(Item {
            id: format!("pending-user-{}", self.next_id),
            kind: "userMessage".into(),
            text,
            images: self.attachments.iter().filter(|p|crate::attachments::is_image(p)).cloned().collect(),
            ..Default::default()
        });
        self.composer.clear();
        self.attachments.clear();
        self.skills.selected.clear();
        self.error = None;
        self.scroll_bottom = true;
        if self.busy {
            self.queued_steering.push_back(json!(input));
            self.flush_steering();
            return;
        }
        self.queued_input = Some(json!(input));
        if !self.ready {
            if !self.connecting {
                self.reconnect(ctx);
            }
            self.busy = true;
        } else {
            self.dispatch_queued();
        }
    }
    fn flush_steering(&mut self) {
        let (Some(thread),Some(turn))=(self.thread.clone(),self.turn.clone()) else {return;};
        while let Some(input)=self.queued_steering.pop_front() {
            self.steering_inflight.insert(self.next_id,input.clone());
            self.request("turn/steer",json!({"threadId":thread,"expectedTurnId":turn,"input":input}),"steer");
        }
    }
    fn restore_context(&mut self, input: &Value) {
        let mut recovered=Vec::new();
        for entry in input.as_array().into_iter().flatten() {
            if let Some(text)=entry["text"].as_str(){recovered.push(text.to_owned());}
            if entry["type"]=="localImage" {
                if let Some(path)=entry["path"].as_str(){self.attachments.push(path.into());}
            }
        }
        if !recovered.is_empty(){self.composer=format!("{}\n{}",recovered.join("\n"),self.composer).trim().into();}
    }
    fn dispatch_queued(&mut self) {
        if self.queued_input.is_none() {
            return;
        }
        self.busy = true;
        if let Some(t) = self.thread.clone() {
            let input = self.queued_input.take().unwrap();
            self.request("turn/start", self.turn_params(&t, input), "turn");
        } else {
            self.request("thread/start",json!({"model":self.model_info["id"],"modelProvider":"strata","cwd":self.cwd,"serviceName":"relay","approvalPolicy":if self.access>=2{"never"}else{"on-request"},"sandbox":(["read-only","workspace-write","danger-full-access"][self.access.min(2)]),"approvalsReviewer":"user","developerInstructions":self.agent_instructions()}),"start");
        }
    }

    fn wire(&mut self, v: Value) {
        if let Some(id) = v["id"].as_u64() {
            if v.get("method").is_none() {
                let tag = self.pending.remove(&id).unwrap_or_default();
                let steering_input=self.steering_inflight.remove(&id);
                if let Some(e) = v.get("error") {
                    if let Some(input)=steering_input {
                        self.restore_context(&input);
                        self.error=Some(format!("Context was not delivered and is restored in the composer: {}",e["message"].as_str().unwrap_or("turn ended")));
                        return;
                    }
                    if tag == "compact" {
                        self.compaction.cancel();
                    }
                    self.error = Some(
                        e["message"]
                            .as_str()
                            .unwrap_or("Backend request failed")
                            .into(),
                    );
                    if tag != "steer" && tag != "interrupt" {
                        self.busy = false;
                        self.turn = None;
                    }
                    return;
                }
                let r = &v["result"];
                if let Some(id) = tag.strip_prefix("archive:") {
                    if self.thread.as_deref() == Some(id) {
                        self.new_chat();
                    }
                    self.refresh_history();
                    return;
                }
                match tag.as_str() {
                    "init" => {
                        let _ = self
                            .backend
                            .tx
                            .send(json!({"method":"initialized","params":{}}));
                        self.ready = true;
                        self.connecting = false;
                        self.status = "Engine connected".into();
                        self.refresh_history();
                        if let Some(t) = self.thread.clone() {
                            self.request(
                                "thread/resume",
                                json!({"threadId":t,"model":self.model_info["id"],"excludeTurns":true,"developerInstructions":self.agent_instructions()}),
                                "resume",
                            );
                        } else {
                            self.dispatch_queued();
                        }
                    }
                    "list" => self.history = r["data"].as_array().cloned().unwrap_or_default(),
                    "start" => {
                        self.thread = r["thread"]["id"].as_str().map(String::from);
                        self.save_state();
                        if let (Some(t), Some(i)) = (self.thread.clone(), self.queued_input.take())
                        {
                            self.request("turn/start", self.turn_params(&t, i), "turn");
                        }
                    }
                    "turn" => {
                        self.turn = r["turn"]["id"].as_str().map(String::from);
                        self.flush_steering();
                    },
                    "resume" => {
                        self.dispatch_queued();
                        self.cwd = r["cwd"].as_str().unwrap_or(&self.cwd).into();
                        self.save_state();
                        if let Some(t) = self.thread.clone() {
                            self.request("thread/turns/list",json!({"threadId":t,"limit":100,"sortDirection":"asc","itemsView":"full"}),"history");
                        }
                    }
                    "history" => {
                        if let Some(ts) = r["data"].as_array() {
                            for t in ts {
                                if let Some(is) = t["items"].as_array() {
                                    for i in is {
                                        upsert(&mut self.items, item(i));
                                    }
                                }
                            }
                        }
                        self.scroll_bottom = true;
                    }
                    _ => {}
                }
                return;
            }
        }
        let method = v["method"].as_str().unwrap_or("");
        let p = &v["params"];
        if p["threadId"].is_string() && self.thread.as_deref() != p["threadId"].as_str() {
            return;
        }
        if v.get("id").is_some() {
            if method.ends_with("requestApproval") || method.ends_with("requestUserInput") {
                self.approvals.push_back(v);
            } else {
                let _=self.backend.tx.send(json!({"id":v["id"],"error":{"code":-32601,"message":"This local client does not support that server request"}}));
                self.error = Some(format!("Unsupported backend request: {method}"));
            }
            return;
        }
        self.compaction.observe(method, p);
        match method {
            "turn/started" => {
                self.busy = true;
                self.turn = p["turn"]["id"].as_str().map(String::from);
                self.flush_steering();
                if self.stop_after_start {
                    self.stop_after_start = false;
                    if let (Some(t), Some(turn)) = (&self.thread, &self.turn) {
                        self.request(
                            "turn/interrupt",
                            json!({"threadId":t,"turnId":turn}),
                            "interrupt",
                        );
                    }
                }
            }
            "turn/completed" => {
                if !self.queued_steering.is_empty() {
                    // A start/finish race must not silently discard a submitted message.
                    let inputs=self.queued_steering.drain(..).collect::<Vec<_>>();
                    for input in inputs {self.restore_context(&input);}
                    self.error=Some("The turn ended before context was delivered. It is restored in the composer.".into());
                }
                self.busy = false;
                self.turn = None;
                self.approvals.clear();
                for x in &mut self.items {
                    if x.status == "inProgress" {
                        x.status = "completed".into();
                    }
                }
                if let Some(e) = p["turn"].get("error").filter(|e| !e.is_null()) {
                    self.error = Some(e["message"].as_str().unwrap_or("Turn failed").into());
                }
                self.refresh_history();
            }
            "item/started" | "item/completed" => {
                let mut x = item(&p["item"]);
                if x.status.is_empty() {
                    x.status = if method == "item/started" {
                        "inProgress"
                    } else {
                        "completed"
                    }
                    .into();
                }
                if x.kind == "userMessage" {
                    if let Some(i) = self
                        .items
                        .iter()
                        .position(|i| i.id.starts_with("pending-user-"))
                    {
                        self.items.remove(i);
                    }
                }
                upsert(&mut self.items, x);
                self.scroll_bottom = true;
            }
            "item/agentMessage/delta" | "item/commandExecution/outputDelta" => {
                let id = string(p, "itemId");
                if !self.items.iter().any(|i| i.id == id) {
                    self.items.push(Item {
                        id: id.clone(),
                        kind: if method.contains("agentMessage") {
                            "agentMessage"
                        } else {
                            "commandExecution"
                        }
                        .into(),
                        ..Default::default()
                    });
                }
                if let Some(x) = self.items.iter_mut().find(|x| x.id == id) {
                    if x.text.len() < 100_000 {
                        x.text.push_str(p["delta"].as_str().unwrap_or(""));
                    }
                }
                self.scroll_bottom = true;
            }
            "item/reasoning/summaryTextDelta" => {
                if let Some(x) = self.items.iter_mut().find(|x| x.id == string(p, "itemId")) {
                    if x.title == "Thinking" {
                        x.title.clear();
                    }
                    if x.title.len() < 150 {
                        x.title.push_str(p["delta"].as_str().unwrap_or(""));
                    }
                }
            }
            "thread/tokenUsage/updated" => {
                if let Some(limit) = p["tokenUsage"]["modelContextWindow"].as_u64() {
                    self.model_info["context"] = json!(limit);
                }
                self.usage = p["tokenUsage"]["last"]["inputTokens"].as_i64().unwrap_or(0)
                    + p["tokenUsage"]["last"]["outputTokens"]
                        .as_i64()
                        .unwrap_or(0)
            }
            "turn/diff/updated" => self.diff = clipped(p["diff"].as_str().unwrap_or(""), 200_000),
            "serverRequest/resolved" => self.approvals.retain(|v| v["id"] != p["requestId"]),
            "error" => {
                self.error = Some(
                    p["error"]["message"]
                        .as_str()
                        .unwrap_or("Backend error")
                        .into(),
                );
            }
            _ => {}
        }
    }
    fn arm_quit(&mut self, ctx: &egui::Context) {
        if self.engine_manager.running || self.codex_manager.running {
            return;
        }
        self.quit_armed = true;
        self.quit_armed_until = ctx.input(|i| i.time) + 5.0;
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }
    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    fn revoke_sudo(&mut self) {
        self.sudo_generation += 1;
        self.auth_pending = false;
        self.pending_auth = None;
        self.auth_password = false;
        self.auth_motion = crate::fingerprint::Motion::default();
        self.auth_fade_until = 0.0;
        self.sudo_session = None;
        if self.access == 3 {
            self.access = 2;
        }
        self.save_state();
    }
    fn select_access(&mut self, index: usize, ctx: &egui::Context) {
        self.revoke_sudo();
        if index == 3 {
            self.access = 2;
            self.auth_pending = true;
            self.auth_password = false;
            self.auth_motion = crate::fingerprint::Motion::default();
            self.auth_hint = "Touch the fingerprint sensor".into();
            self.auth_started = ctx.input(|i| i.time);
            self.pending_auth = Some(crate::sudo::authenticate(
                self.sudo_socket.clone(),
                ctx.clone(),
                self.sudo_tx.clone(),
                self.sudo_generation,
            ));
        } else {
            self.access = index;
            self.save_state();
        }
    }
    fn agent_instructions(&self) -> String {
        format!("Local GUI instructions: administrator commands must use `{}/.local/libexec/relay/relay-sudo --socket {} -- COMMAND ARG...`. This works only after the user explicitly selects the red Full access + sudo mode and authenticates. If unavailable, ask the user to select that mode; never work around a denied session with raw sudo or ask for passwords in chat. Ordinary commands remain normal-user commands. Persistent user memory is at {}. The snapshot below is already supplied; do not reread it at chat startup. Retrieve relevant documentation sections on demand, never the full master log. Update it only when the user asks to remember durable facts/preferences; never store passwords, tokens or keys. Keep it concise. Recorded memory:
{}",std::env::var("HOME").unwrap(),self.sudo_socket.display(),crate::memory::path().display(),clipped(&crate::memory::read(),18000))
    }
    fn turn_params(&self, thread: &str, input: Value) -> Value {
        let policy = match self.access {
            0 => json!({"type":"readOnly"}),
            1 => {
                json!({"type":"workspaceWrite","writableRoots":[self.cwd,crate::memory::path().parent()],"networkAccess":false})
            }
            _ => json!({"type":"dangerFullAccess"}),
        };
        json!({"threadId":thread,"model":self.model_info["id"],"input":input,"effort":(["none","low","medium","high","max"][self.effort]),"approvalPolicy":if self.access>=2{"never"}else{"on-request"},"approvalsReviewer":"user","sandboxPolicy":policy})
    }
    fn status_pills(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let layout = crate::footer::layout(width);
        let gap = layout.gap;
        let narrow = layout.engine;
        let wide = layout.context;
        let status = if self.auth_pending {
            "Authentication"
        } else if self.compaction.running() {
            "Compacting context"
        } else if self.connecting {
            "Loading"
        } else if self.ready {
            "Engine ready"
        } else {
            "Engine unloaded"
        };
        let limit = if self.ready {
            self.model_info["context"]
                .as_u64()
                .or(self.engine_metrics.latest["engine"]["max_context"].as_u64())
        } else {
            None
        };
        let usage = self.usage.max(0) as u64;
        let context = if self.compaction.running() {
            "Compacting context…".into()
        } else if self.compaction.awaiting_usage {
            "Context updating…".into()
        } else {
            match limit {
                Some(limit) => format!(
                    "Context {} / {}",
                    compact_tokens(usage),
                    compact_tokens(limit)
                ),
                None => "Context — / —".into(),
            }
        };
        let context = if wide < 220.0 {
            context.trim_start_matches("Context ").to_owned()
        } else {
            context
        };
        let speed = crate::metrics::rate(&self.engine_metrics.latest)
            .map(|v| format!("{v:.1} tok/s"))
            .unwrap_or_else(|| "— tok/s".into());
        let font = egui::FontId::proportional(11.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x=gap;
            let (left,response)=ui.allocate_exact_size(egui::vec2(narrow,36.0),egui::Sense::hover());
            paint_status_pill(ui,left);
            let dot=if self.ready {Color32::from_rgb(152,190,145)}else{MUTED};
            let status_galley=ui.painter().layout_no_wrap(status.into(),font.clone(),MUTED);
            let start=left.center().x-(status_galley.size().x+12.0)/2.0;
            ui.painter().circle_filled(egui::pos2(if narrow<100.0 {left.center().x}else{start+2.0},left.center().y),2.0,dot);
            if narrow>=100.0 {ui.painter().with_clip_rect(left.shrink2(egui::vec2(8.0,0.0))).galley(egui::pos2(start+12.0,left.center().y-status_galley.size().y/2.0),status_galley,MUTED);}
            response.on_hover_text(&self.status);
            for action in crate::footer::ACTIONS {
                use crate::footer::Action;
                let selected=match action {Action::Settings=>self.show_settings,Action::Memory=>self.show_memory,Action::Changes=>self.show_diff,Action::Quit=>self.quit_armed,_=>false};
                let enabled=match action { Action::Project=>!self.busy,Action::Changes=>!self.diff.is_empty(),Action::Quit=>!(self.engine_manager.running || self.codex_manager.running),_=>true };
                if crate::footer::button(ui,action,layout.button,selected,enabled).clicked() {
                    if !matches!(action,Action::Quit) {self.quit_armed=false;}
                    match action {
                        Action::Project=>{self.picker.open(true,Some(&self.cwd));}
                        Action::Settings=>{self.settings_text=serde_json::to_string_pretty(&crate::config::load()).unwrap_or_default();self.show_settings=!self.show_settings;}
                        Action::Memory=>{self.memory_text=crate::memory::read();self.show_memory=!self.show_memory;}
                        Action::Changes=>{self.show_diff=!self.show_diff;},
                        Action::Quit=>{
                            if self.quit_armed {self.revoke_sudo();self.quitting=true;ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);}
                            else {self.arm_quit(ui.ctx());}
                        },
                        Action::Compact=>{self.compact_thread();},
                    }
                }
            }
            let (right,response)=ui.allocate_exact_size(egui::vec2(wide,36.0),egui::Sense::hover());
            paint_status_pill(ui,right);
            self.footer_center_y=Some(right.center().y);
            let content_right=self.composer_action_right.unwrap_or(right.right()-17.0).min(right.right()-16.0);
            let mut detail=self.telemetry.latest.labels();
            detail.push(speed.clone());
            let available=(wide-125.0).max(50.0);
            let speed_galley=loop {
                let galley=ui.painter().layout_no_wrap(detail.join(" · "),font.clone(),Color32::from_gray(202));
                if galley.size().x<=available || detail.len()==1 {break galley;}
                detail.remove(0);
            };
            let text_y=right.top()+11.0;
            let speed_x=content_right-speed_galley.size().x;
            ui.painter().galley(egui::pos2(speed_x,text_y-speed_galley.size().y/2.0),speed_galley,Color32::from_gray(202));
            let label_clip=egui::Rect::from_min_max(egui::pos2(right.left()+16.0,right.top()),egui::pos2((speed_x-10.0).max(right.left()+16.0),right.bottom()));
            ui.painter().with_clip_rect(label_clip).text(egui::pos2(right.left()+16.0,text_y),egui::Align2::LEFT_CENTER,context,font,MUTED);
            let rail=egui::Rect::from_min_size(egui::pos2(right.left()+16.0,right.bottom()-12.0),egui::vec2((content_right-right.left()-16.0).max(1.0),3.0));
            ui.painter().rect_filled(rail,2,Color32::from_gray(70));
            let ratio=limit.filter(|n|*n>0).map(|n|(usage as f32/n as f32).clamp(0.0,1.0)).unwrap_or(0.0);
            if ratio>0.0 {ui.painter().rect_filled(egui::Rect::from_min_size(rail.min,egui::vec2(rail.width()*ratio,rail.height())),2,Color32::from_gray(190));}
            response.on_hover_text(format!("{}\nCPU + GPU power; VRAM temperature and memory clock. Missing readings are omitted.\nContext usage and generation speed.", self.telemetry.latest.labels().join(" · ")));
            let compact=crate::footer::button(ui,crate::footer::Action::Compact,layout.button,self.compaction.running(),self.can_compact());
            self.footer_right=Some(compact.rect.right());
            if compact.clicked(){self.compact_thread();}
        });
    }
    fn history_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Recent chats").color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.busy && !self.connecting, egui::Button::new("+"))
                    .on_hover_text("New chat")
                    .clicked()
                {
                    self.new_chat();
                    ui.ctx()
                        .memory_mut(|m| m.request_focus(egui::Id::unique("relay-composer")));
                }
            });
        });
        ui.add_space(8.0);
        let mut selected = None;
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for t in &self.history {
                let title = t["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or(t["preview"].as_str())
                    .unwrap_or("New chat");
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 28.0).max(40.0);
                    if ui
                        .add_enabled_ui(!self.busy && self.ready, |ui| {
                            ui.add_sized(
                                [width, 24.0],
                                egui::Button::new(RichText::new(title).size(11.5))
                                    .selected(self.thread.as_deref() == t["id"].as_str())
                                    .truncate(),
                            )
                        })
                        .inner
                        .on_hover_text(title)
                        .clicked()
                    {
                        selected = t["id"].as_str().map(String::from);
                    }
                    if ui
                        .add_enabled(
                            !self.busy && self.ready,
                            egui::Button::new(RichText::new("×").size(14.0).color(MUTED))
                                .frame(false),
                        )
                        .on_hover_text("Remove from recent chats (archive)")
                        .clicked()
                    {
                        remove = t["id"].as_str().map(String::from);
                    }
                });
            }
        });
        if let Some(id) = remove {
            self.request(
                "thread/archive",
                json!({"threadId":id}),
                &format!("archive:{id}"),
            );
        }
        if let Some(id) = selected {
            self.new_chat();
            self.thread = Some(id.clone());
            self.request(
                            "thread/resume",
                            json!({"threadId":id,"model":self.model_info["id"],"excludeTurns":true,"developerInstructions":self.agent_instructions()}),
                            "resume",
                        );
        }
    }
    fn stop_control(&mut self,ui:&mut egui::Ui) {
                                            let (rect, stop) = ui.allocate_exact_size(
                                                egui::vec2(30.0, 30.0),
                                                egui::Sense::click(),
                                            );
                                            if !self.connecting {self.composer_action_right=Some(rect.right()-1.0);}
                                            let c = rect.center();
                                            ui.painter().circle_filled(
                                                c,
                                                14.0,
                                                Color32::from_rgb(98, 40, 40),
                                            );
                                            let stroke =
                                                Stroke::new(1.8, Color32::from_rgb(242, 211, 211));
                                            ui.painter().line_segment(
                                                [
                                                    c - egui::vec2(4.5, 4.5),
                                                    c + egui::vec2(4.5, 4.5),
                                                ],
                                                stroke,
                                            );
                                            ui.painter().line_segment(
                                                [
                                                    c + egui::vec2(-4.5, 4.5),
                                                    c + egui::vec2(4.5, -4.5),
                                                ],
                                                stroke,
                                            );
                                            if stop.on_hover_text("Stop").clicked() {
                                                if let (Some(t), Some(turn)) =
                                                    (&self.thread, &self.turn)
                                                {
                                                    self.request(
                                                        "turn/interrupt",
                                                        json!({"threadId":t,"turnId":turn}),
                                                        "interrupt",
                                                    );
                                                } else if self.connecting {
                                                    self.backend = Backend::idle();
                                                    self.connecting = false;
                                                    self.busy = false;
                                                    self.queued_input = None;
                                                    self.status = "Engine unloaded".into();
                                                } else {
                                                    self.stop_after_start = true;
                                                }
                                            }
    }
    fn composer_controls(&mut self, ui: &mut egui::Ui) {
        let names = [
            "Read only",
            "Ask for approval",
            "Full access",
            "Full access + sudo",
        ];
        let color = if self.access == 3 {
            crate::controls::RED
        } else if self.access == 2 {
            crate::controls::ORANGE
        } else {
            Color32::from_gray(190)
        };
        ui.add_enabled_ui(!self.busy && !self.auth_pending, |ui| {
            let button = ui.add(
                egui::Button::new(
                    RichText::new(
                        if ui.ctx().content_rect().width() < 520.0 && self.access == 3 {
                            "Sudo"
                        } else {
                            names[self.access]
                        },
                    )
                    .size(12.5)
                    .color(color),
                )
                .min_size(egui::vec2(0.0, 30.0))
                .fill(Color32::from_gray(33))
                .corner_radius(14),
            );
            if button.clicked() {
                self.access_preview = Some(self.access);
                self.access_popup_anchor=Some(crate::adaptive::popup_anchor(button.rect,self.composer_top));
            }
            if self.demo && std::env::args().any(|a| a == "--demo-access") {
                let marker = egui::Id::unique("demo-access-opened");
                if !ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(marker).unwrap_or(false))
                {
                    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&button));
                    ui.ctx().data_mut(|d| d.insert_temp(marker, true));
                }
            }
            egui::Popup::menu(&button)
                .anchor(self.access_popup_anchor.unwrap_or_else(||crate::adaptive::popup_anchor(button.rect,self.composer_top)))
                .align(egui::RectAlign::TOP)
                .gap(12.0)
                .width(260.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(33, 33, 33, 242))
                        .corner_radius(18)
                        .inner_margin(12)
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 5],
                            blur: 20,
                            spread: 0,
                            color: Color32::from_black_alpha(65),
                        }),
                )
                .fade_show(|ui| {
                    ui.set_width(236.0);
                    let mut choice = self.access_preview.unwrap_or(self.access);
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Label::new(
                            RichText::new(names[choice])
                                .size(12.0)
                                .color(if choice == 3 {
                                    crate::controls::RED
                                } else {
                                    Color32::from_gray(235)
                                })).truncate());
                    });
                    ui.add_space(7.0);
                    let tint = if choice == 3 {
                        crate::controls::RED
                    } else if choice==2 {
                        crate::controls::ORANGE
                    } else {
                        Color32::from_gray(190)
                    };
                    let response = crate::controls::slider(ui, &mut choice, 3, 236.0, tint);
                    self.access_preview = Some(choice);
                    let commit = crate::controls::committed(&response);
                    if commit && choice != self.access {
                        self.select_access(choice, &ui.ctx().clone());
                        self.access_preview = Some(self.access);
                        if choice == 3 {
                            ui.close();
                        }
                    }
                    let (labels, _) =
                        ui.allocate_exact_size(egui::vec2(236.0, 16.0), egui::Sense::hover());
                    let stops = response.rect.x_range().shrink(response.rect.height() / 2.5);
                    for (i, name) in ["Read", "Ask", "Full", "Sudo"].iter().enumerate() {
                        ui.painter().text(
                            egui::pos2(egui::lerp(stops, i as f32 / 3.0), labels.center().y),
                            egui::Align2::CENTER_CENTER,
                            name,
                            egui::FontId::proportional(10.0),
                            MUTED,
                        );
                    }
                });
        });
    }
    fn model_control(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.busy && !self.auth_pending, |ui| {
            let levels = ["Off", "Low", "Medium", "High", "Max"];
            let model_button = ui.add_sized(
                egui::vec2(ui.available_width().clamp(72.0, 260.0), 30.0),
                egui::Button::new(
                    RichText::new(format!(
                        "{} – {}",
                        self.model_info["name"].as_str().unwrap_or("Select model"),
                        levels[self.effort]
                    ))
                    .size(12.5)
                    .color(Color32::from_gray(195)),
                )
                .corner_radius(14)
                .fill(Color32::from_gray(33))
                .truncate(),
            );
            if model_button.clicked() {
                self.model_popup_anchor=Some(crate::adaptive::popup_anchor(model_button.rect,self.composer_top));
            }
            egui::Popup::menu(&model_button)
                .anchor(self.model_popup_anchor.unwrap_or_else(||crate::adaptive::popup_anchor(model_button.rect,self.composer_top)))
                .align(egui::RectAlign::TOP)
                .gap(12.0)
                .width(260.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(33, 33, 33, 242))
                        .corner_radius(18)
                        .inner_margin(12)
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 5],
                            blur: 20,
                            spread: 0,
                            color: Color32::from_black_alpha(65),
                        }),
                )
                .fade_show(|ui| {
                    ui.set_width(236.0);
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Label::new(
                            RichText::new(format!(
                                "{} · {}",
                                self.model_info["name"].as_str().unwrap_or("Model"),
                                levels[self.effort]
                            ))
                            .size(12.0)
                            .color(Color32::from_gray(235))).truncate());
                    });
                    ui.add_space(7.0);
                    let slider = crate::controls::slider(
                        ui,
                        &mut self.effort,
                        4,
                        236.0,
                        crate::controls::ORANGE,
                    );
                    if slider.changed() {
                        self.save_state();
                    }
                    slider.on_hover_text("Off · Low · Medium · High · Max");
                });
        });
    }
    fn save_state(&self) {
        let p = state_file();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            json!({"cwd":self.cwd,"effort":self.effort,"access":self.access.min(2),"thread":self.thread}).to_string(),
        );
    }
    fn handle_events(&mut self, ctx: &egui::Context) {
        self.skills.poll(&self.cwd);
        self.engine_manager.poll();
        if self.codex_manager.poll() {
            self.settings_text =
                serde_json::to_string_pretty(&crate::config::load()).unwrap_or_default();
        }
        let event_started = std::time::Instant::now();
        for index in 0..500 {
            if index == 499 || event_started.elapsed() >= std::time::Duration::from_millis(8) {
                ctx.request_repaint();
                break;
            }
            match self.backend.rx.try_recv() {
                Ok(Event::Wire(v)) => self.wire(v),
                Ok(Event::Model(v)) => {
                    self.model_info = v;
                }
                Ok(Event::Status(s)) => {
                    if self.logs.len() > 12_000 {
                        self.logs.drain(
                            ..self
                                .logs
                                .char_indices()
                                .find(|(i, _)| *i >= 6000)
                                .map(|(i, _)| i)
                                .unwrap_or(0),
                        );
                    }
                    self.logs.push_str(&s);
                    self.logs.push('\n');
                    if !self.ready
                        && (s == "Engine loading"
                            || s == "Model loading"
                            || s.starts_with("Connection failed:"))
                    {
                        self.status = s.clone();
                        if s.starts_with("Connection failed:") {
                            self.error = Some(s);
                        }
                    }
                }
                Ok(Event::Closed) => {
                    let mut inputs=self.queued_steering.drain(..).collect::<Vec<_>>();
                    inputs.extend(self.steering_inflight.drain().map(|(_,input)|input));
                    for input in inputs {self.restore_context(&input);}
                    self.ready = false;
                    self.busy = false;
                    self.connecting = false;
                    self.status = "Engine unloaded".into();
                    self.compaction = crate::compaction::State::default();
                }
                Err(_) => break,
            }
        }
    }
    fn poll_picker(&mut self, ctx: &egui::Context) {
        if let Some(paths) = self.picker.show(ctx) {
            if self.picker.folder() {
                if let Some(path) = paths.first() {
                    self.cwd = path.clone();
                    self.new_chat();
                    self.save_state();
                }
            } else {
                crate::attachments::add_paths(&mut self.attachments, paths);
            }
        }
    }
    fn approval_ui(&mut self, ctx: &egui::Context) {
        let Some(v) = self.approvals.front().cloned() else {
            return;
        };
        let method = v["method"].as_str().unwrap_or("");
        let p = &v["params"];
        let mut reply = None;
        let mut approval_open = true;
        floating_window("approval",560.0).open(&mut approval_open).resizable(true).anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(&ctx,|ui|{
            if method.ends_with("requestUserInput") {
                if let Some(qs)=p["questions"].as_array(){for q in qs {let id=string(q,"id");ui.label(RichText::new(string(q,"question")).strong());let answer=self.answers.entry(id).or_default();if let Some(opts)=q["options"].as_array(){for opt in opts{let label=string(opt,"label");ui.radio_value(answer,label.clone(),label).on_hover_text(string(opt,"description"));}}ui.add(egui::TextEdit::singleline(answer).hint_text("Type an answer").password(q["isSecret"].as_bool().unwrap_or(false)));ui.add_space(8.0);}}
                if ui.button("Send answer").clicked(){let mut answers=serde_json::Map::new();if let Some(qs)=p["questions"].as_array(){for q in qs{let id=string(q,"id");answers.insert(id.clone(),json!({"answers":[self.answers.get(&id).cloned().unwrap_or_default()]}));}}reply=Some(json!({"answers":answers}));}
            }else {
                if let Some(s)=p["reason"].as_str(){ui.label(s);}if let Some(s)=p["command"].as_str(){ui.monospace(s);}if let Some(s)=p["cwd"].as_str(){ui.label(RichText::new(s).color(MUTED));}
                if method.contains("permissions/"){ui.label("Additional permissions requested. This version can deny these requests; use the configured access profile for broader access.");ui.monospace(clipped(&serde_json::to_string_pretty(&p["permissions"]).unwrap_or_default(),4000));if ui.button("Deny").clicked(){reply=Some(json!({"permissions":{},"scope":"turn"}));}}
                else {ui.horizontal(|ui|{if ui.button("Allow once").clicked(){reply=Some(json!({"decision":"accept"}));}if ui.button("Deny").clicked(){reply=Some(json!({"decision":"decline"}));}});}
            }
        });
        if !approval_open && reply.is_none() {
            reply=Some(if method.ends_with("requestUserInput") {json!({"answers":{}})}
                else if method.contains("permissions/") {json!({"permissions":{},"scope":"turn"})}
                else {json!({"decision":"decline"})});
        }
        if let Some(result) = reply {
            let _ = self.backend.tx.send(json!({"id":v["id"],"result":result}));
            self.approvals.pop_front();
            self.answers.clear();
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let was_hidden = self.hidden;
        self.handle_events(ctx);
        self.poll_picker(ctx);
        self.sent_attachments.poll_sent(ctx);
        self.telemetry.tick(ctx, !self.hidden);
        self.engine_metrics.tick(
            ctx,
            if self.ready {
                self.model_info["endpoint"].as_str()
            } else {
                None
            },
            !self.hidden,
        );
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if (self.ready || self.connecting) && !dropped.is_empty() {
            crate::attachments::add_paths(
                &mut self.attachments,
                dropped
                    .into_iter()
                    .map(|f| f.path().to_string_lossy().into_owned()),
            );
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("relay-composer")));
        }
        if (self.ready || self.connecting)
            && !self.show_settings
            && !self.show_memory
            && !self.quit_armed
            && !self.show_diff
        {
            let events = ctx.input(|i| i.raw.events.clone());
            let mut native_image = false;
            for event in events {
                if let egui::Event::PasteImage(image) = event {
                    native_image = true;
                    self.attachment_ui.paste_image(ctx, image);
                    ctx.memory_mut(|m| m.request_focus(egui::Id::unique("relay-composer")));
                }
            }
            let file_paste = ctx.input(|i| {
                i.raw
                    .events
                    .iter()
                    .any(|e| matches!(e,egui::Event::Paste(text) if text.starts_with("file://")))
            });
            let key_paste = ctx.input(|i| {
                i.key_pressed(egui::Key::V) && (i.modifiers.ctrl || i.modifiers.command)
            });
            if !native_image && (file_paste || key_paste) {
                self.attachment_ui.paste(ctx);
            }
        }
        if let Some(error) = self.attachment_ui.poll(ctx, &mut self.attachments) {
            self.error = Some(error);
        }
        if self.can_load()
            && !self.hidden
            && !self.show_settings
            && !self.show_memory
            && !self.show_diff
            && !self.quit_armed
            && self.approvals.is_empty()
            && !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| i.time) >= self.auth_fade_until
            && ctx.input(|i| i.modifiers.is_none())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            self.reconnect(ctx);
        }
        if self
            .instance
            .show
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.show_window(ctx);
        }
        while let Ok(action) = self.tray_rx.try_recv() {
            self.show_window(ctx);
            if matches!(action, crate::tray::Action::Quit) {
                self.arm_quit(ctx);
            }
        }
        while let Ok(event) = self.sudo_rx.try_recv() {
            match event {
                crate::sudo::Event::Ready(g, session) => {
                    if g == self.sudo_generation && self.auth_pending && !self.hidden {
                        self.sudo_session = Some(session);
                        self.access = 3;
                        self.auth_pending = false;
                        self.pending_auth = None;
                        if !self.auth_password {
                            let now = ctx.input(|i| i.time);
                            self.auth_motion.finish(now);
                            self.auth_fade_until = now + 0.34;
                        }
                        self.save_state();
                    }
                }
                crate::sudo::Event::Prompt(g, hint) => {
                    if g == self.sudo_generation && self.auth_pending {
                        self.auth_hint = hint;
                    }
                }
                crate::sudo::Event::Scanning(g, scanning) => {
                    if g == self.sudo_generation && self.auth_pending {
                        self.auth_motion.contact(scanning, ctx.input(|i| i.time));
                    }
                }
                crate::sudo::Event::Read(g) => {
                    if g == self.sudo_generation && self.auth_pending {
                        self.auth_motion.read(ctx.input(|i| i.time));
                    }
                }
                crate::sudo::Event::Password(g) => {
                    if g == self.sudo_generation && self.auth_pending {
                        self.auth_password = true;
                    }
                }
                crate::sudo::Event::Failed(g, e) => {
                    if g == self.sudo_generation {
                        self.auth_pending = false;
                        self.pending_auth = None;
                        self.error = Some(e);
                    }
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && !was_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.tray.is_some() {
                self.revoke_sudo();
                self.hidden = true;
                self.quit_armed = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                self.arm_quit(ctx);
            }
        }
    }
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.quit_armed
            && (ctx.input(|i| i.time) >= self.quit_armed_until
                || ctx.input(|i| i.key_pressed(egui::Key::Escape)))
        {
            self.quit_armed = false;
        }
        // One surface across the whole window; nested panels must not double alpha.
        root.painter().rect_filled(ctx.content_rect(), 0, BG);
        crate::branding::watermark(root.painter(), ctx.content_rect());
        egui::Panel::top("window-drag-strip")
            .show_separator_line(false)
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                let (_, drag) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 28.0),
                    egui::Sense::drag(),
                );
                if drag.is_pointer_button_down_on()
                    && ctx.input(|i| i.pointer.primary_pressed())
                    && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
        let drawer_width = self.chat_drawer.width;
        if drawer_width > 0.5 {
            if ctx.content_rect().width() < 720.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::unique("history-drawer"))
                    .order(egui::Order::Middle)
                    .fixed_pos(egui::pos2(
                        screen.right() - drawer_width,
                        screen.top() + 32.0,
                    ))
                    .show(&ctx, |ui| {
                        let viewport = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(drawer_width, (screen.height() - 190.0).max(140.0)),
                        );
                        ui.allocate_exact_size(viewport.size(), egui::Sense::hover());
                        let mut contents = ui.new_child(egui::UiBuilder::new().max_rect(
                            egui::Rect::from_min_size(
                                viewport.min,
                                egui::vec2(
                                    drawer_width.max(crate::drawer::MAX_REVEAL),
                                    viewport.height(),
                                ),
                            ),
                        ));
                        contents.shrink_clip_rect(viewport);
                        egui::Frame::new()
                            .fill(PANEL)
                            .corner_radius(22)
                            .inner_margin(16)
                            .show(&mut contents, |ui| self.history_ui(ui));
                    });
            } else {
                egui::Panel::right("history")
                    .resizable(false)
                    .show_separator_line(false)
                    .exact_size(drawer_width)
                    .frame(egui::Frame::NONE)
                    .show(root, |ui| {
                        let viewport = ui.max_rect();
                        let mut contents = ui.new_child(egui::UiBuilder::new().max_rect(
                            egui::Rect::from_min_size(
                                viewport.min,
                                egui::vec2(
                                    drawer_width.max(crate::drawer::MAX_REVEAL),
                                    viewport.height(),
                                ),
                            ),
                        ));
                        contents.shrink_clip_rect(viewport);
                        egui::Frame::new()
                            .inner_margin(16)
                            .show(&mut contents, |ui| self.history_ui(ui));
                    });
            }
        }
        let body_height=self.composer_expansion.height(ctx.input(|i|i.time));
        egui::Panel::bottom("composer")
            .exact_size(self.composer_panel_base+body_height)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(24, 24)),
            )
            .show(root, |ui| {
                centered_column(ui, 860.0, |ui| {
                    let content_start=ui.next_widget_position().y;
                    if let Some(e) = self.error.clone() {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(65, 35, 35))
                            .corner_radius(8)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(&e);
                                    if ui.small_button("Dismiss").clicked() {
                                        self.error = None;
                                    }
                                });
                            });
                    }
                    let expanded=self.ready || self.connecting || self.busy;
                    let shadow_painter=ui.painter().with_clip_rect(ctx.content_rect());
                    let shadow_id=shadow_painter.add(egui::Shape::Noop);
                    self.composer_top=Some(ui.next_widget_position().y);
                    let composer_frame=egui::Frame::new()
                        .fill(Color32::from_rgb(42, 42, 42))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 52, 52)))
                        .corner_radius(22)
                        .inner_margin(egui::Margin { left:16,right:16,top:8,bottom:8 })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            let enter=self.composer_expansion.show(ui,|ui|{
                                if expanded {ui.add_space(6.0);}
                                if expanded && !self.attachments.is_empty() {
                                    self.attachment_ui.show(ui,&mut self.attachments);
                                    ui.add_space(10.0);
                                }
                                self.skills.chips(ui);
                                if expanded {
                                    let enter=crate::composer::input(ui,&mut self.composer,self.connecting,self.busy);
                                    ui.add_space(4.0);enter
                                } else {false}
                            });
                            ui.horizontal(|ui| {
                                ui.set_min_height(30.0);
                                ui.spacing_mut().interact_size.y=30.0;
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let (plus_rect,plus)=ui.add_enabled_ui(expanded,|ui|ui.allocate_exact_size(egui::vec2(30.0,30.0),egui::Sense::click())).inner;
                                let c=plus_rect.center();let stroke=Stroke::new(1.5,Color32::from_gray(215));
                                ui.painter().line_segment([c-egui::vec2(5.0,0.0),c+egui::vec2(5.0,0.0)],stroke);
                                ui.painter().line_segment([c-egui::vec2(0.0,5.0),c+egui::vec2(0.0,5.0)],stroke);
                                if plus.on_hover_text("Attach files · paste a screenshot with Ctrl+V · drop files here").clicked() {
                                    self.picker.open(false, Some(&self.cwd));
                                }
                                ui.add_space(2.0);
                                self.composer_controls(ui);
                                self.skills.button(ui, &self.cwd,self.composer_top);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.busy && !self.connecting {self.stop_control(ui);}
                                        if self.ready {
                                        let enabled = !self.demo
                                            && !self.compaction.running()
                                            && !(self.engine_manager.running || self.codex_manager.running)
                                            && !self.attachment_ui.pasting
                                            && !self.auth_pending;
                                        let send = ui
                                            .add_enabled_ui(enabled, |ui| {
                                                let (rect, response) = ui.allocate_exact_size(
                                                    egui::vec2(30.0, 30.0),
                                                    egui::Sense::click(),
                                                );
                                                let center = rect.center();
                                                ui.painter().circle_filled(
                                                    center,
                                                    14.0,
                                                    if enabled {
                                                        Color32::WHITE
                                                    } else {
                                                        Color32::from_gray(95)
                                                    },
                                                );
                                                let stroke =
                                                    Stroke::new(1.8, Color32::from_gray(20));
                                                ui.painter().line_segment(
                                                    [
                                                        center + egui::vec2(0.0, 6.0),
                                                        center - egui::vec2(0.0, 6.0),
                                                    ],
                                                    stroke,
                                                );
                                                ui.painter().line_segment(
                                                    [
                                                        center + egui::vec2(-5.0, -1.0),
                                                        center - egui::vec2(0.0, 6.0),
                                                    ],
                                                    stroke,
                                                );
                                                ui.painter().line_segment(
                                                    [
                                                        center + egui::vec2(5.0, -1.0),
                                                        center - egui::vec2(0.0, 6.0),
                                                    ],
                                                    stroke,
                                                );
                                                response.on_hover_text(if self.busy {
                                                    "Send context"
                                                } else {
                                                    "Send"
                                                })
                                            })
                                            .inner;
                                        if !self.busy&&!self.connecting {self.composer_action_right=Some(send.rect.right()-1.0);}
                                        if send.clicked() {
                                            self.send(&ctx);
                                        }
                                        }
                                        if self.ready {
                                            self.model_control(ui);
                                        } else {
                                            let can_load=self.can_load();
                                            let (load,changed,error)=self.profiles.control(ui,can_load,self.connecting,self.footer_right,self.composer_top);
                                            self.composer_action_right=Some(ui.min_rect().right());
                                            if let Some(e)=error{self.error=Some(e);}
                                            if changed {self.settings_text=serde_json::to_string_pretty(&crate::config::load()).unwrap_or_default();self.engine_manager=crate::engine::Manager::default();}
                                            if load {self.reconnect(&ctx);}
                                            if self.connecting {self.stop_control(ui);}
                                        }
                                    },
                                );
                            });
                            if enter && self.ready {
                                self.send(&ctx);
                            }
                        });
                    shadow_painter.set(shadow_id,egui::epaint::Shadow{offset:[0,3],blur:72,spread:0,color:Color32::from_black_alpha(34)}.as_shape(composer_frame.response.rect,egui::CornerRadius::same(22)));
                    self.status_pills(ui);
                    let natural=ui.next_widget_position().y-content_start-ui.spacing().item_spacing.y;
                    self.composer_panel_base=(natural-body_height+48.0).max(110.0);
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(root, |ui| {
                let bottom = self.scroll_bottom;
                self.scroll_bottom = false;
                let scroll_target = self.chat_scroll_target.take();
                let mut area = egui::ScrollArea::vertical()
                    .id_salt("conversation")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .stick_to_bottom(scroll_target.is_none())
                    .auto_shrink([false, false]);
                if let Some(offset) = scroll_target {
                    area = area.vertical_scroll_offset(offset);
                }
                let chat_paint_start=ui.painter().add(egui::Shape::Noop);
                let scroll = area.show(ui, |ui| {
                    centered_column(ui, 860.0, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        let mut index = 0;
                        while index < self.items.len() {
                            let end = crate::activity::group_end(&self.items, index);
                            if end - index > 1 {
                                let items = &self.items[index..end];
                                let active = self.busy
                                    && items.iter().any(|item| item.status == "inProgress");
                                let title = crate::activity::group_label(items, active);
                                egui::CollapsingHeader::new(crate::activity::label(
                                    ui, &title, active,
                                ))
                                .id_salt(("activity-group-v2", &items[0].id))
                                .default_open(false)
                                .show(ui, |ui| {
                                    for item in items {
                                        crate::activity::show_item(ui, item, self.busy);
                                    }
                                });
                                index = end;
                                continue;
                            }
                            let x = &self.items[index];
                            ui.push_id(&x.id, |ui| match x.kind.as_str() {
                                "userMessage" => {
                                    ui.add_space(10.0);
                                    self.sent_attachments.show_sent(ui, &x.images);
                                    let max_width =
                                        (ui.available_width() - 20.0).clamp(40.0, 600.0);
                                    let galley = ui.painter().layout(
                                        x.text.clone(),
                                        egui::TextStyle::Body.resolve(ui.style()),
                                        ui.visuals().text_color(),
                                        max_width,
                                    );
                                    let width = galley.size().x.ceil().clamp(12.0, max_width);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::TOP),
                                        |ui| {
                                            egui::Frame::new()
                                                .fill(Color32::from_rgb(40, 40, 40))
                                                .corner_radius(15)
                                                .inner_margin(10)
                                                .show(ui, |ui| {
                                                    ui.set_width(width);
                                                    ui.add(
                                                        egui::Label::new(galley).selectable(true),
                                                    );
                                                });
                                        },
                                    );
                                    ui.add_space(10.0);
                                }
                                "agentMessage" => {
                                    ui.add_space(6.0);
                                    crate::markdown::show(ui, &mut self.cache, &x.text);
                                    ui.add_space(10.0);
                                }
                                _ => crate::activity::show_item(ui, x, self.busy),
                            });
                            index += 1;
                        }
                    });
                    let _ = bottom;
                });
                let chat_paint_end=ui.painter().add(egui::Shape::Noop);
                crate::chat_fade::apply(ui,chat_paint_start,chat_paint_end,scroll.inner_rect,
                    scroll.state.clamped_offset().y>0.5,
                    scroll.state.clamped_offset().y+scroll.inner_rect.height()<scroll.content_size.y-0.5);
                self.chat_scroll_target = crate::scroll_pill::show(ui, &scroll);
            });
        {
            let mut open = self.show_memory;
            floating_window("relay-memory",650.0).open(&mut open).show(&ctx,|ui|{
            ui.label("Remembered across chats. Keep durable preferences and PC facts here; never credentials.");
            egui::ScrollArea::vertical().max_height(460.0).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.memory_text).desired_width(f32::INFINITY).desired_rows(18).font(egui::TextStyle::Monospace));});
            if ui.button("Save memory").clicked(){match crate::memory::save(&self.memory_text){Ok(())=>self.status="Memory saved".into(),Err(e)=>self.error=Some(e.to_string())}}
        });
            self.show_memory = open;
        }
        {
            let mut open = self.show_settings;
            floating_window("relay-settings",600.0).open(&mut open).show(&ctx,|ui| {
                egui::ScrollArea::vertical().id_salt("settings-scroll").max_height((ctx.content_rect().height()-130.0).max(200.0)).show(ui, |ui| {
                self.codex_manager.show(ui, self.ready || self.connecting || self.busy, self.engine_manager.running);
                ui.separator();
                ui.add_enabled_ui(!self.codex_manager.running, |ui| {
                    if crate::config::load()["backend"].as_str()==Some("llama.cpp") {
                        ui.heading("llama.cpp engine");
                        ui.label("Qwen 27B profile · DFlash2 · vision · 200,000 context");
                        ui.label("The profile uses the verified local llama.cpp runtime. Strata settings apply to Flash Next.");
                    } else {
                        self.engine_manager.show(ui, self.ready || self.connecting || self.busy);
                    }
                });
                if ui.add_enabled(self.ready && !self.busy && !(self.engine_manager.running || self.codex_manager.running), egui::Button::new("Disconnect and unload Relay session")).clicked() {
                    let _ = self.backend.tx.send(json!({"_shutdown":true}));
                    self.ready=false;self.connecting=true;self.status="Disconnecting…".into();
                }
                ui.separator();
                ui.heading("Connection");
                ui.label(format!("Engine: {}",self.status));
                ui.label(format!("Model: {}",self.model_info["name"].as_str().unwrap_or("Not connected")));
                if let Some(c)=self.model_info["context"].as_u64(){ui.label(format!("Context: {c} tokens"));}
                if let Some(e)=self.model_info["engine"].as_str(){ui.label(format!("Runtime: {e}"));}
                ui.label("Model metadata is discovered from the configured endpoint. Send a message to connect and load; pending messages run when ready.");
                ui.separator();
                ui.label("Connection configuration");
                ui.label("Commands are arrays of executable and arguments. Settings apply on the next connection. Empty stop_command leaves the engine running.");
                egui::ScrollArea::vertical().max_height(260.0).show(ui,|ui| {ui.add(egui::TextEdit::multiline(&mut self.settings_text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(10));});
                if ui.button("Save connection settings").clicked(){match crate::config::save(&self.settings_text){Ok(())=>self.status="Settings saved".into(),Err(e)=>self.error=Some(e)}}
                ui.separator();
                ui.label("Session");ui.label("Close hides Relay to the tray. Quit ends the session. Engine shutdown follows the configured lifecycle commands and shared-session ownership.");
                ui.label("Access is selected in the composer. Administrator mode requires fresh authentication and is revoked when the window hides or Relay quits.");
                ui.label("Project folder");ui.monospace(&self.cwd);
                egui::CollapsingHeader::new("Connection diagnostics").show(ui,|ui|{egui::ScrollArea::both().max_height(140.0).show(ui,|ui|{ui.monospace(&self.logs);});});
                });
            });
            self.show_settings = open;
        }
        {
            floating_window("relay-diff",820.0)
                .open(&mut self.show_diff)
                .show(&ctx, |ui| {
                    egui::ScrollArea::both().max_height(600.0).show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&self.diff).monospace())
                                .selectable(true),
                        );
                    });
                });
        }
        if !self.auth_pending && !self.show_settings && !self.show_memory && !self.show_diff {
            self.chat_drawer.handle(
                &ctx,
                self.footer_center_y
                    .unwrap_or(ctx.content_rect().bottom() - 42.0),
                self.footer_right,
            );
        }
        self.approval_ui(&ctx);
        crate::transitions::finish(&ctx);
        let now = ctx.input(|i| i.time);
        if now < self.auth_fade_until {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if (self.auth_pending && !self.auth_password && !self.hidden)
            || now < self.auth_fade_until
            || self.fingerprint_preview
        {
            if crate::fingerprint::show(
                &ctx,
                self.auth_motion.elapsed(now),
                self.sudo_generation
                    .wrapping_add((self.auth_started * 1000000.0) as u64),
                self.auth_motion.visible(now),
                if now < self.auth_fade_until {
                    crate::fingerprint::fade_opacity(self.auth_fade_until - now)
                } else {
                    1.0
                },
            ) {
                if self.fingerprint_preview {
                    self.fingerprint_preview = false;
                } else if self.auth_pending {
                    self.revoke_sudo();
                } else {
                    self.auth_fade_until = 0.0;
                }
            }
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("file-drop-overlay"),
            ));
            let rect = ctx.content_rect().shrink(16.0);
            painter.rect_filled(rect, 24, Color32::from_black_alpha(140));
            painter.rect_stroke(
                rect,
                24,
                Stroke::new(2.0, Color32::from_gray(170)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop files to attach",
                egui::FontId::proportional(20.0),
                Color32::from_gray(230),
            );
        }
    }
}
fn state_file() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join(".local/state/relay/ui.json")
}

fn centered_column<R>(
    ui: &mut egui::Ui,
    maximum: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let available = ui.available_width().min(ui.clip_rect().width()).max(1.0);
    let width = available.min(maximum);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(((available - width) / 2.0).max(0.0));
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.x = 9.0;
                contents(ui)
            },
        )
        .inner
    })
    .inner
}

fn compact_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
fn paint_status_pill(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 2],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(24),
        }
        .as_shape(rect, egui::CornerRadius::same(18)),
    );
    ui.painter().rect_filled(rect, 18, Color32::from_gray(42));
    ui.painter().rect_stroke(
        rect,
        18,
        Stroke::new(1.0, Color32::from_gray(52)),
        egui::StrokeKind::Inside,
    );
}

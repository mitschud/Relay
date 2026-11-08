//! A text shimmer driven only by active, visible work. Idle windows request no animation frames.
use eframe::egui::{self, Color32, FontId, Stroke, TextFormat};
pub fn shine(time: f64, position: f32, count: f32) -> f32 {
    let center = (time / 2.4).fract() as f32 * (count + 20.0) - 10.0;
    (1.0 - ((position - center) / 7.5).abs()).max(0.0).powi(2)
}
pub fn label(ui: &egui::Ui, text: &str, active: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let time = ui.input(|i| i.time);
    let count = text.chars().count().max(1) as f32;
    for (i, ch) in text.chars().enumerate() {
        let light = if active {
            shine(time, i as f32, count)
        } else {
            0.0
        };
        let gray = (145.0 + 100.0 * light) as u8;
        job.append(
            &ch.to_string(),
            0.0,
            TextFormat {
                font_id: FontId::proportional(crate::typography::ACTIVITY),
                color: Color32::from_gray(gray),
                line_height: Some(20.0),
                ..Default::default()
            },
        );
    }
    if active {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    job
}
pub fn terminal_icon(ui: &mut egui::Ui, openness: f32, response: &egui::Response) {
    let r = egui::Rect::from_center_size(response.rect.center(), egui::vec2(12.0, 11.0));
    let s = Stroke::new(
        1.0,
        Color32::from_gray(if response.hovered() { 215 } else { 145 }),
    );
    ui.painter().rect_stroke(r, 2, s, egui::StrokeKind::Inside);
    let p = r.min + egui::vec2(3.0, 3.0);
    ui.painter().line_segment([p, p + egui::vec2(2.0, 2.0)], s);
    ui.painter()
        .line_segment([p + egui::vec2(2.0, 2.0), p + egui::vec2(0.0, 4.0)], s);
    ui.painter()
        .line_segment([p + egui::vec2(4.0, 4.0), p + egui::vec2(6.5, 4.0)], s);
    if openness > 0.5 {
        ui.painter().line_segment(
            [
                r.left_bottom() + egui::vec2(2.0, 2.0),
                r.right_bottom() + egui::vec2(-2.0, 2.0),
            ],
            s,
        );
    }
}

pub fn is_tool(item: &crate::model::Item) -> bool {
    !matches!(
        item.kind.as_str(),
        "userMessage" | "agentMessage"
    )
}
pub fn group_end(items: &[crate::model::Item], start: usize) -> usize {
    if start >= items.len() { return items.len(); }
    if !is_tool(&items[start]) {
        return start + 1;
    }
    let mut end = start + 1;
    while end < items.len() && is_tool(&items[end]) {
        end += 1;
    }
    end
}
pub fn group_label(items: &[crate::model::Item], active: bool) -> String {
    let mut kinds = Vec::new();
    for item in items {
        if item.kind == "reasoning" { continue; }
        let title = item.title.to_lowercase();
        let kind = if item.kind == "commandExecution" {
            if title.starts_with("reading ") {
                "read"
            } else if title.starts_with("searching ") {
                "files"
            } else {
                "commands"
            }
        } else if item.kind == "contextCompaction" {
            "compact"
        } else if item.kind == "fileChange" {
            "edit"
        } else if item.kind == "webSearch" || title.contains("web") || title.contains("browser") {
            "web"
        } else {
            "tools"
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    if kinds.is_empty() { kinds.push("thought"); }
    let mut text = kinds
        .iter()
        .map(|kind| match (*kind, active) {
            ("thought", true) => "Thinking",
            ("thought", false) => "Thought",
            ("compact", true) => "Compacting context",
            ("compact", false) => "Compacted context",
            ("read", true) => "Reading files",
            ("read", false) => "Read files",
            ("files", true) => "Searching files",
            ("files", false) => "Searched files",
            ("commands", true) => "Running commands",
            ("commands", false) => "Ran commands",
            ("edit", true) => "Editing files",
            ("edit", false) => "Edited files",
            ("web", true) => "Searching the web",
            ("web", false) => "Searched the web",
            (_, true) => "Using tools",
            (_, false) => "Used tools",
        })
        .enumerate()
        .map(|(i, label)| {
            if i == 0 {
                label.to_owned()
            } else {
                let mut value = label.to_owned();
                value[..1].make_ascii_lowercase();
                value
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    if items.len()>1 { text.push_str(&format!(" · {} steps",items.len())); }
    if items.iter().any(|item| item.status == "failed") {
        text.push_str(" · failed");
    }
    text
}
pub fn show_item(ui: &mut egui::Ui, item: &crate::model::Item, busy: bool) {
    let active = busy && item.status == "inProgress";
    let text = crate::model::step_label(item, active);
    egui::CollapsingHeader::new(label(ui, &text, active))
        .id_salt((&item.id, "tool-output"))
        .icon(terminal_icon)
        .show(ui, |ui| {
            egui::ScrollArea::both().max_height(340.0).show(ui, |ui| {
                if item.kind == "reasoning" {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&item.text).font(FontId::proportional(crate::typography::BODY)).line_height(Some(crate::typography::LEADING)),
                        )
                        .wrap()
                        .selectable(true),
                    );
                } else {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&item.text).monospace().size(11.5))
                            .selectable(true),
                    );
                }
            });
        });
}

#[cfg(test)]
mod group_tests {
    use super::*;
    use crate::model::Item;
    fn item(kind: &str, title: &str) -> Item {
        Item {
            kind: kind.into(),
            title: title.into(),
            ..Default::default()
        }
    }
    #[test]
    fn grouping_includes_thoughts_but_never_swallows_messages() {
        let items = vec![
            item("commandExecution", "Running command"),
            item("webSearch", "Search"),
            item("agentMessage", ""),
            item("commandExecution", "Running command"),
            item("reasoning", "Thinking"),
        ];
        assert_eq!(group_end(&items, 0), 2);
        assert_eq!(group_end(&items, 2), 3);
        assert_eq!(group_end(&items, 3), 5);
    }
    #[test]
    fn interleaved_reasoning_and_compaction_form_one_block() {
        let mut items=Vec::new();
        for _ in 0..8 {
            items.push(item("commandExecution","Running command"));
            items.push(item("reasoning","Thinking"));
        }
        items.push(item("contextCompaction","Compacting context"));
        items.push(item("userMessage",""));
        items.push(item("reasoning","Thinking"));
        items.push(item("agentMessage",""));
        assert_eq!(group_end(&items,0),17);
        assert_eq!(group_end(&items,17),18);
        assert_eq!(group_end(&items,18),19);
        assert_eq!(group_label(&items[..17],false),"Ran commands, compacted context · 17 steps");
        assert_eq!(group_end(&[],0),0);
    }
    #[test]
    fn activity_block_starts_collapsed_and_click_expands_it() {
        let ctx=egui::Context::default();
        let mut header=egui::Rect::NOTHING;
        let mut opened=false;
        for events in [vec![], vec![], vec![egui::Event::PointerMoved(egui::pos2(80.0,15.0))]] {
            let mut out=ctx.run_ui(egui::RawInput{events,..Default::default()}, |ui| {
                let response=egui::CollapsingHeader::new("Ran commands · 17 steps")
                    .id_salt("activity-test").default_open(false).show(ui,|_|());
                header=response.header_response.rect;opened=response.body_returned.is_some();
            });out.textures_delta.clear();
        }
        assert!(!opened);
        for pressed in [true,false] {
            let mut out=ctx.run_ui(egui::RawInput{events:vec![egui::Event::PointerMoved(header.center()),egui::Event::PointerButton{pos:header.center(),button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}],..Default::default()}, |ui| {
                let response=egui::CollapsingHeader::new("Ran commands · 17 steps")
                    .id_salt("activity-test").default_open(false).show(ui,|_|());
                opened=response.body_returned.is_some();
            });out.textures_delta.clear();
        }
        assert!(opened);
    }
    #[test]
    fn mixed_actions_keep_web_and_failures_visible_in_the_summary() {
        let mut items = vec![
            item("commandExecution", "Running command"),
            item("mcpToolCall", "web_run"),
            item("fileChange", "Editing files"),
        ];
        items[2].status = "failed".into();
        assert_eq!(
            group_label(&items, false),
            "Ran commands, searched the web, edited files · 3 steps · failed"
        );
        assert!(group_label(&items, true).starts_with("Running commands"));
    }
}

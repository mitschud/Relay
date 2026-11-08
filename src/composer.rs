use eframe::egui::{self, Color32};

/// Drafting stays interactive while a generation or tool call is active.
pub fn input(ui: &mut egui::Ui, text: &mut String, loading: bool, working: bool) -> bool {
    let response = ui.add(
        egui::TextEdit::multiline(text)
            .id(egui::Id::unique("relay-composer"))
            .font(egui::FontId::proportional(crate::typography::BODY))
            .text_color(Color32::from_gray(230))
            .hint_text(if loading {"Loading"} else if working {"Add context while working…"} else {"Type to begin"})
            .frame(egui::Frame::NONE)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    let enter=response.has_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)&&!i.modifiers.shift);
    if enter {*text=text.trim_end_matches('\n').to_owned();}
    enter
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_enter_remain_active_during_generation() {
        let ctx=egui::Context::default();let mut text=String::new();
        ctx.memory_mut(|m|m.request_focus(egui::Id::unique("relay-composer")));
        let mut enter=false;
        let mut out=ctx.run_ui(egui::RawInput {events:vec![egui::Event::Text("Additional context".into())],..Default::default()},|ui|{
            enter=input(ui,&mut text,false,true);
        });out.textures_delta.clear();
        assert_eq!(text,"Additional context");assert!(!enter);
        let mut out=ctx.run_ui(egui::RawInput {events:vec![egui::Event::Key{key:egui::Key::Enter,physical_key:None,pressed:true,repeat:false,modifiers:egui::Modifiers::NONE}],..Default::default()},|ui|{
            enter=input(ui,&mut text,false,true);
        });out.textures_delta.clear();
        assert!(enter);assert_eq!(text,"Additional context");
    }
}

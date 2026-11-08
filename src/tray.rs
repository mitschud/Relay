use eframe::egui;
use ksni::blocking::TrayMethods;
use std::sync::mpsc;
#[derive(Debug)]
pub enum Action {
    Show,
    Quit,
}
pub struct Tray {
    tx: mpsc::Sender<Action>,
    ctx: egui::Context,
}
impl Tray {
    fn send(&self, a: Action) {
        let _ = self.tx.send(a);
        self.ctx.request_repaint();
    }
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "relay".into()
    }
    fn title(&self) -> String {
        "Relay".into()
    }
    fn icon_name(&self) -> String {
        format!(
            "{}/.local/share/icons/relay-symbol-tray.png",
            std::env::var("HOME").unwrap()
        )
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Show chat".into(),
                activate: Box::new(|s: &mut Self| s.send(Action::Show)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit…".into(),
                activate: Box::new(|s: &mut Self| s.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
pub fn start(
    ctx: egui::Context,
    tx: mpsc::Sender<Action>,
) -> Result<ksni::blocking::Handle<Tray>, ksni::Error> {
    Tray { tx, ctx }.spawn()
}

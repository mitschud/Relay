use eframe::egui::{self, Color32};

/// Reference travel from the window edge, in UI points.
pub const MAX_REVEAL: f32 = 150.0;

fn leading_edge(right:f32,reveal:f32,footer_right:Option<f32>)->f32 {
    let nominal=right-reveal-42.;
    footer_right.map_or(nominal,|edge|nominal.max(edge+8.).min(right-8.))
}

#[derive(Default)]
pub struct Drawer {
    pub width: f32,
    start_width: f32,
}

impl Drawer {
    /// A splitter grip, not a toggle: every pixel dragged reveals the same
    /// amount of panel. Releasing keeps that width; clicking changes nothing.
    pub fn handle(&mut self, ctx: &egui::Context, center_y: f32, footer_right:Option<f32>) {
        let screen = ctx.content_rect();
        let max_width = MAX_REVEAL.min((screen.width() - 100.0).max(0.0));
        self.width = self.width.clamp(0.0, max_width);
        let x = leading_edge(screen.right(),self.width,footer_right);
        let reach=screen.right()-x;
        let y = (center_y - 18.0).clamp(screen.top(), screen.bottom() - 36.0);
        egui::Area::new(egui::Id::unique("chat-drawer-grab"))
            .order(egui::Order::Foreground)
            .constrain(false)
            .fixed_pos(egui::pos2(x, y))
            .show(ctx, |ui| {
                let (rect, r) = ui
                    .allocate_exact_size(egui::vec2(reach+14.0, 36.0), egui::Sense::drag());
                // The rounded leading edge moves; the other end stays beyond
                // the window clip, filling the complete gap back to the edge.
                let pill = egui::Rect::from_min_size(
                    rect.min + egui::vec2(2.0, 0.0),
                    egui::vec2((reach+10.0).max(40.0), 36.0),
                );
                let painter = ui.painter().with_clip_rect(screen);
                painter.add(
                    egui::epaint::Shadow {
                        offset: [0, 2],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(45),
                    }
                    .as_shape(pill, egui::CornerRadius::same(18)),
                );
                painter.rect_filled(pill, 18, Color32::from_gray(42));
                painter.rect_stroke(
                    pill,
                    18,
                    egui::Stroke::new(1.0, Color32::from_gray(52)),
                    egui::StrokeKind::Inside,
                );
                if r.drag_started() {
                    self.start_width = self.width;
                }
                if r.dragged() {
                    self.width = (self.start_width - r.total_drag_delta().unwrap_or_default().x)
                        .clamp(0.0, max_width);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    ui.ctx().request_repaint();
                }
                r.on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text("Drag to reveal or resize chats");
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, drawer: &mut Drawer, events: Vec<egui::Event>) {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| drawer.handle(ui.ctx(), 200.0,None),
        );
        out.textures_delta.clear();
    }
    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn grip_never_overlaps_footer_even_at_narrow_extremes(){
        for width in [400.,500.,720.,1000.,1800.] {
            for reveal in [0.,50.,150.] {
                let footer=width-24.;
                let left=leading_edge(width,reveal,Some(footer));
                assert!(left>=footer+8.);assert!(left<width);
            }
        }
        assert_eq!(leading_edge(1000.,0.,Some(900.)),958.,"full reach retained where space permits");
    }
    #[test]
    fn grip_tracks_the_pointer_and_does_not_toggle_on_click() {
        let ctx = egui::Context::default();
        let mut d = Drawer::default();
        frame(&ctx, &mut d, vec![]);
        frame(&ctx, &mut d, vec![]);
        let p = egui::pos2(490.0, 200.0);
        frame(&ctx, &mut d, vec![egui::Event::PointerMoved(p)]);
        frame(&ctx, &mut d, vec![button(p, true)]);
        frame(
            &ctx,
            &mut d,
            vec![egui::Event::PointerMoved(p - egui::vec2(30.0, 0.0))],
        );
        assert!((d.width - 30.0).abs() < 0.1);
        frame(
            &ctx,
            &mut d,
            vec![egui::Event::PointerMoved(p - egui::vec2(90.0, 0.0))],
        );
        assert!((d.width - 90.0).abs() < 0.1);
        let end = egui::pos2(400.0, 200.0);
        frame(&ctx, &mut d, vec![button(end, false)]);
        frame(&ctx, &mut d, vec![button(end, true)]);
        frame(&ctx, &mut d, vec![button(end, false)]);
        assert!(
            (d.width - 90.0).abs() < 0.1,
            "clicking/releasing must keep the chosen width"
        );
        frame(&ctx, &mut d, vec![button(end, true)]);
        frame(
            &ctx,
            &mut d,
            vec![egui::Event::PointerMoved(egui::pos2(100.0, 200.0))],
        );
        assert_eq!(d.width, MAX_REVEAL, "drag must stop at the reference reach");
        let closed = egui::pos2(499.0, 200.0);
        frame(&ctx, &mut d, vec![egui::Event::PointerMoved(closed)]);
        assert_eq!(
            d.width, 0.0,
            "the same drag can return the tab all the way to the edge"
        );
        frame(&ctx, &mut d, vec![button(closed, false)]);
    }
}

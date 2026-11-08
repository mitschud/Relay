use eframe::egui::{self, Color32, Stroke};
pub const ORANGE: Color32 = Color32::from_rgb(243, 139, 65);
pub const RED: Color32 = Color32::from_rgb(240, 86, 86);

/// Shared native-input pill slider for thinking and access controls.
pub fn slider(
    ui: &mut egui::Ui,
    value: &mut usize,
    max: usize,
    width: f32,
    color: Color32,
) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width;
        ui.spacing_mut().interact_size.y = 30.0;
        ui.spacing_mut().slider_rail_height = 24.0;
        for widget in [&mut ui.style_mut().visuals.widgets.inactive] {
            widget.bg_fill = Color32::TRANSPARENT;
            widget.fg_stroke = Stroke::NONE;
        }
        ui.visuals_mut().widgets.hovered.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::NONE;
        ui.visuals_mut().widgets.active.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.active.fg_stroke = Stroke::NONE;
        let r = ui.add(
            egui::Slider::new(value, 0..=max)
                .integer()
                .show_value(false)
                .trailing_fill(false)
                .handle_shape(egui::style::HandleShape::Circle),
        );
        let rail = egui::Rect::from_center_size(r.rect.center(), egui::vec2(r.rect.width(), 24.0));
        let stops = r.rect.x_range().shrink(r.rect.height() / 2.5);
        let x = egui::lerp(stops, *value as f32 / max.max(1) as f32);
        ui.painter().rect_filled(rail, 12, Color32::from_gray(72));
        let fill = egui::Rect::from_min_max(rail.min, egui::pos2(x + 12.0, rail.max.y));
        ui.painter()
            .with_clip_rect(rail)
            .rect_filled(fill, 12, color);
        for i in 0..=max {
            ui.painter().circle_filled(
                egui::pos2(
                    egui::lerp(stops, i as f32 / max.max(1) as f32),
                    rail.center().y,
                ),
                2.0,
                Color32::from_white_alpha(90),
            );
        }
        ui.painter()
            .circle_filled(egui::pos2(x, rail.center().y), 14.0, Color32::WHITE);
        r
    })
    .inner
}

pub fn committed(response: &egui::Response) -> bool {
    response.drag_stopped() || (response.changed() && !response.dragged())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        preview: &mut usize,
        actual: &mut usize,
    ) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 100.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let response = slider(ui, preview, 3, 236.0, ORANGE);
                rect = response.rect;
                if committed(&response) {
                    *actual = *preview;
                }
            },
        );
        out.textures_delta.clear();
        rect
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
    fn passing_over_sudo_during_a_drag_does_not_select_it() {
        let ctx = egui::Context::default();
        let mut preview = 2;
        let mut actual = 2;
        let rect = frame(&ctx, vec![], &mut preview, &mut actual);
        let stops = rect.x_range().shrink(rect.height() / 2.5);
        let full = egui::pos2(egui::lerp(stops, 2.0 / 3.0), rect.center().y);
        let sudo = egui::pos2(stops.max, rect.center().y);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(full)],
            &mut preview,
            &mut actual,
        );
        frame(&ctx, vec![button(full, true)], &mut preview, &mut actual);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(sudo)],
            &mut preview,
            &mut actual,
        );
        assert_eq!(preview, 3);
        assert_eq!(
            actual, 2,
            "drag preview cannot authenticate/select administrator mode"
        );
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(full)],
            &mut preview,
            &mut actual,
        );
        frame(&ctx, vec![button(full, false)], &mut preview, &mut actual);
        assert_eq!(actual, 2);
        frame(&ctx, vec![button(full, true)], &mut preview, &mut actual);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(sudo)],
            &mut preview,
            &mut actual,
        );
        frame(&ctx, vec![button(sudo, false)], &mut preview, &mut actual);
        assert_eq!(
            actual, 3,
            "releasing on administrator mode explicitly commits it"
        );
    }
}

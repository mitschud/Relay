use eframe::egui::{self, Color32, Stroke};

// A bounded, floating thumb: no full-height track or bright scrollbar line.
// Return the requested offset for the next frame; wheel/trackpad scrolling still
// belongs to ScrollArea, including its normal follow-bottom behaviour.
pub fn show(ui: &mut egui::Ui, output: &egui::scroll_area::ScrollAreaOutput<()>) -> Option<f32> {
    let view = output.inner_rect.shrink2(egui::vec2(0.0, 8.0));
    let max = (output.content_size.y - output.inner_rect.height()).max(0.0);
    if max < 1.0 || view.height() < 40.0 {
        return None;
    }
    let height = (view.height() * view.height() / output.content_size.y)
        .clamp(34.0, 76.0)
        .min(view.height());
    let travel = view.height() - height;
    if travel <= 0.0 {
        return None;
    }
    let offset = output.state.clamped_offset().y;
    let top = view.top() + travel * (offset / max).clamp(0.0, 1.0);
    let thumb = egui::Rect::from_min_size(
        egui::pos2(
            (view.center().x + view.width().min(860.0) / 2.0 + 10.0).min(view.right() - 7.0),
            top,
        ),
        egui::vec2(7.0, height),
    );
    let response = ui
        .interact(
            thumb.expand2(egui::vec2(5.0, 0.0)),
            output.id.with("pill"),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Grab);
    ui.painter().rect_filled(
        thumb,
        4,
        Color32::from_gray(if response.dragged() {
            65
        } else if response.hovered() {
            55
        } else {
            38
        }),
    );
    ui.painter().rect_stroke(
        thumb,
        4,
        Stroke::new(0.5, Color32::from_gray(55)),
        egui::StrokeKind::Inside,
    );
    let origin_id = output.id.with("pill-origin");
    if response.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(origin_id, offset));
    }
    if response.dragged() {
        let origin = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(origin_id))
            .unwrap_or(offset);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui.ctx().request_repaint();
        return Some(
            (origin + response.total_drag_delta().unwrap_or_default().y * max / travel)
                .clamp(0.0, max),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        offset: f32,
    ) -> (Option<f32>, egui::Pos2) {
        let mut result = (None, egui::Pos2::ZERO);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = egui::ScrollArea::vertical()
                        .id_salt("drag-regression")
                        .max_height(200.0)
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .vertical_scroll_offset(offset)
                        .show(ui, |ui| {
                            ui.allocate_space(egui::vec2(100.0, 1200.0));
                        });
                    let view = out.inner_rect.shrink2(egui::vec2(0.0, 8.0));
                    let height =
                        (view.height() * view.height() / out.content_size.y).clamp(34.0, 76.0);
                    let y = view.top()
                        + (view.height() - height) * out.state.clamped_offset().y
                            / (out.content_size.y - out.inner_rect.height());
                    result = (
                        show(ui, &out),
                        egui::pos2(view.right() - 3.5, y + height / 2.0),
                    );
                });
            },
        );
        output.textures_delta.clear();
        result
    }
    #[test]
    fn thumb_drag_accumulates_and_stays_put_between_motion_events() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![], 500.0);
        let (_, p) = frame(&ctx, vec![], 500.0);
        frame(&ctx, vec![egui::Event::PointerMoved(p)], 500.0);
        frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            500.0,
        );
        let a = frame(
            &ctx,
            vec![egui::Event::PointerMoved(p - egui::vec2(0.0, 15.0))],
            500.0,
        )
        .0
        .expect("first movement");
        let b = frame(
            &ctx,
            vec![egui::Event::PointerMoved(p - egui::vec2(0.0, 30.0))],
            a,
        )
        .0
        .expect("second movement");
        assert!(
            b < a - 10.0,
            "successive movement must accumulate: {a} -> {b}"
        );
        let still = frame(&ctx, vec![], b).0.expect("drag remains active");
        assert!(
            (still - b).abs() < 0.1,
            "holding still must not snap back: {b} -> {still}"
        );
    }
}

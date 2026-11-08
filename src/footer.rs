//! Small vector action buttons; no icon-font dependency or bitmap scaling.
use eframe::egui::{self, Color32, Stroke};
#[derive(Clone, Copy)]
pub enum Action {
    Project,
    Settings,
    Memory,
    Changes,
    Quit,
    Compact,
}
pub const ACTIONS: [Action; 5] = [
    Action::Project,
    Action::Settings,
    Action::Memory,
    Action::Changes,
    Action::Quit,
];
pub struct Layout {
    pub gap: f32,
    pub button: f32,
    pub engine: f32,
    pub context: f32,
}
pub fn layout(width: f32) -> Layout {
    let (button, gap) = if width < 340.0 {
        (24.0, 4.0)
    } else if width < 640.0 {
        (30.0, 5.0)
    } else {
        (36.0, 8.0)
    };
    let remaining = width - button * 6.0 - gap * 7.0;
    let engine = if remaining < 300.0 {
        40.0
    } else {
        (remaining * 0.26).clamp(120.0, 180.0)
    };
    Layout {
        gap,
        button,
        engine,
        context: (remaining - engine).max(1.0),
    }
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Project => "Project folder",
            Self::Settings => "Settings",
            Self::Memory => "Persistent memory",
            Self::Changes => "Review code changes",
            Self::Quit => "Quit Relay",
            Self::Compact => "Compact context",
        }
    }
}
pub fn button(
    ui: &mut egui::Ui,
    action: Action,
    size: f32,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, response) = ui
        .add_enabled_ui(enabled, |ui| {
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click())
        })
        .inner;
    let armed_quit = matches!(action, Action::Quit) && selected;
    let fill = if armed_quit {
        218
    } else if response.is_pointer_button_down_on() {
        54
    } else if response.hovered() || selected {
        50
    } else {
        42
    };
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 2],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(40),
        }
        .as_shape(rect, egui::CornerRadius::same((size / 2.0) as u8)),
    );
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, Color32::from_gray(fill));
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0 - 0.5,
        Stroke::new(1.0, Color32::from_gray(if selected { 75 } else { 52 })),
    );
    let painter = ui.painter();
    let c = rect.center();
    let scale = size / 36.0;
    let color = if armed_quit {
        Color32::from_rgb(145, 38, 38)
    } else {
        Color32::from_gray(if enabled { 207 } else { 100 })
    };
    let stroke = Stroke::new(1.35 * scale, color);
    let point = |x: f32, y: f32| c + egui::vec2(x, y) * scale;
    let segment = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    match action {
        Action::Project => {
            painter.add(egui::Shape::closed_line(
                vec![
                    point(-8.0, -5.0),
                    point(-3.0, -5.0),
                    point(-1.0, -2.5),
                    point(8.0, -2.5),
                    point(8.0, 6.0),
                    point(-8.0, 6.0),
                ],
                stroke,
            ));
            segment((-7.5, 0.0), (7.5, 0.0));
        }
        Action::Settings => {
            let points = (0..32)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / 32.0;
                    let r = if i % 4 == 1 || i % 4 == 2 { 8.8 } else { 6.8 };
                    point(angle.cos() * r, angle.sin() * r)
                })
                .collect();
            painter.add(egui::Shape::closed_line(points, stroke));
            painter.circle_stroke(c, 3.0 * scale, stroke);
        }
        Action::Memory => {
            // Match the other footer glyphs: open line work, shared stroke and colour.
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-5.5, -5.5), point(5.5, 5.5)),
                2.0 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            for p in [-3.0, 0.0, 3.0] {
                segment((p, -8.0), (p, -5.5));
                segment((p, 5.5), (p, 8.0));
                segment((-8.0, p), (-5.5, p));
                segment((5.5, p), (8.0, p));
            }
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-2.0, -2.0), point(2.0, 2.0)),
                0.8 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Action::Changes => {
            painter.rect_stroke(egui::Rect::from_min_max(point(-6.5,-8.0),point(6.5,8.0)),2,stroke,egui::StrokeKind::Inside);
            segment((-3.5,-3.0),(3.5,-3.0));segment((0.0,-6.0),(0.0,0.0));
            segment((-3.5,4.0),(3.5,4.0));
        }
        Action::Quit => {
            let points = (0..40)
                .map(|i| {
                    let angle = (45.0 + i as f32 * 270.0 / 39.0).to_radians();
                    point(angle.sin() * 7.0, -angle.cos() * 7.0)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            segment((0.0, -9.0), (0.0, -0.5));
        }
        Action::Compact => {
            segment((-9.0,0.0),(-2.0,0.0));
            segment((-5.5,-3.5),(-2.0,0.0));
            segment((-5.5,3.5),(-2.0,0.0));
            segment((9.0,0.0),(2.0,0.0));
            segment((5.5,-3.5),(2.0,0.0));
            segment((5.5,3.5),(2.0,0.0));
        }
    }
    response.on_hover_text(if armed_quit {
        "Click again to quit · Esc cancels"
    } else {
        action.name()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn footer_fits_narrow_and_wide_windows() {
        for width in [280.0, 352.0, 500.0, 640.0, 860.0] {
            let l = layout(width);
            assert!(l.context >= 50.0);
            assert!((l.engine + l.context + 6.0 * l.button + 7.0 * l.gap - width).abs() < 0.01);
        }
    }
}

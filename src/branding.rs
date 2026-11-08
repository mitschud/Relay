use eframe::egui::{self, Color32, Pos2};

// Same softened triangular mark as assets/relay-tray.svg. Vector geometry keeps
// the watermark sharp at every window scale without an image texture or effect.
pub fn watermark(painter: &egui::Painter, rect: egui::Rect) {
    let side = (rect.width() * 0.28).min(rect.height() * 0.35).min(230.0);
    let origin = rect.center() - egui::vec2(side, side) * 0.5;
    for triangle in [
        [[8.0, 8.0], [56.0, 8.0], [8.0, 56.0]],
        [[29.0, 56.0], [56.0, 29.0], [56.0, 56.0]],
    ] {
        let mut points = Vec::new();
        for i in 0..3 {
            let p = egui::pos2(triangle[i][0], triangle[i][1]);
            let prev = egui::pos2(triangle[(i + 2) % 3][0], triangle[(i + 2) % 3][1]);
            let next = egui::pos2(triangle[(i + 1) % 3][0], triangle[(i + 1) % 3][1]);
            let a = p + (prev - p).normalized() * 4.8;
            let b = p + (next - p).normalized() * 4.8;
            for j in 0..=8 {
                let t = j as f32 / 8.0;
                let q = a.to_vec2() * (1.0 - t).powi(2)
                    + p.to_vec2() * 2.0 * t * (1.0 - t)
                    + b.to_vec2() * t * t;
                points.push(Pos2::new(
                    origin.x + q.x * side / 64.0,
                    origin.y + q.y * side / 64.0,
                ));
            }
        }
        painter.add(egui::Shape::convex_polygon(
            points,
            Color32::from_rgba_unmultiplied(64, 64, 64, 12),
            egui::Stroke::NONE,
        ));
    }
}

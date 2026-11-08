//! Procedural fingerprint ridges: no image assets, shader passes or background timers.
use eframe::egui::{self, Color32, RichText, Stroke};

#[derive(Default)]
pub struct Motion {
    started: f64,
    contact: bool,
    pulse_until: f64,
}
impl Motion {
    pub fn contact(&mut self, present: bool, now: f64) {
        if present && !self.contact {
            self.started = now;
            self.pulse_until = now + 0.24;
        }
        self.contact = present;
    }
    pub fn read(&mut self, now: f64) {
        if !self.contact && now >= self.pulse_until {
            self.started = now - 0.08;
        }
        self.pulse_until = now + 0.28;
    }
    pub fn finish(&mut self, now: f64) {
        self.read(now);
        self.contact = false;
        self.pulse_until = now + 0.34;
    }
    pub fn visible(&self, now: f64) -> bool {
        self.contact || now < self.pulse_until
    }
    pub fn elapsed(&self, now: f64) -> f64 {
        (now - self.started).max(0.0)
    }
}
pub fn fade_opacity(remaining: f64) -> f32 {
    ease((remaining / 0.34).clamp(0.0, 1.0) as f32)
}

fn random(seed: u64) -> f32 {
    let mut n = seed.wrapping_add(0x9e3779b97f4a7c15);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((n ^ (n >> 31)) >> 40) as f32 / 16777216.0
}
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn points(ridge: usize) -> Vec<egui::Pos2> {
    // Fixed cubic ridges in a 24-unit glyph. No warping or stretched ellipses.
    let paths: &[&[[f32; 8]]] = &[
        &[
            [4.4, 8.6, 5.0, 4.7, 8.0, 2.4, 12.0, 2.4],
            [12.0, 2.4, 16.0, 2.4, 19.2, 5.2, 19.6, 9.6],
        ],
        &[[3.8, 10.7, 3.7, 15.0, 3.0, 17.2, 2.5, 18.3]],
        &[[20.1, 11.2, 20.4, 16.6, 18.4, 20.2, 16.8, 22.0]],
        &[
            [6.2, 10.3, 6.2, 6.8, 8.5, 4.6, 11.9, 4.6],
            [11.9, 4.6, 15.3, 4.6, 17.8, 7.2, 17.8, 10.5],
            [17.8, 10.5, 17.8, 16.5, 16.2, 20.0, 14.8, 21.6],
        ],
        &[[5.8, 12.3, 5.8, 16.6, 4.9, 19.3, 4.2, 20.1]],
        &[
            [7.9, 13.4, 8.3, 11.5, 7.8, 10.0, 8.5, 8.4],
            [8.5, 8.4, 9.4, 6.5, 12.3, 6.2, 14.2, 7.8],
            [14.2, 7.8, 15.9, 9.1, 15.8, 12.0, 15.5, 14.3],
            [15.5, 14.3, 15.3, 17.1, 14.4, 19.7, 13.3, 21.3],
        ],
        &[[7.8, 15.5, 7.6, 18.4, 6.5, 20.4, 6.0, 21.2]],
        &[
            [10.1, 12.0, 10.2, 11.1, 9.8, 9.3, 11.3, 9.0],
            [11.3, 9.0, 12.5, 8.6, 13.7, 9.6, 13.6, 11.7],
            [13.6, 11.7, 13.4, 15.3, 12.4, 19.7, 10.8, 22.0],
        ],
        &[[10.0, 14.1, 9.7, 17.0, 8.9, 19.4, 8.2, 20.6]],
    ];
    paths[ridge]
        .iter()
        .flat_map(|p| {
            (0..32).map(move |i| {
                let t = i as f32 / 31.0;
                let u = 1.0 - t;
                let x = u * u * u * p[0]
                    + 3.0 * u * u * t * p[2]
                    + 3.0 * u * t * t * p[4]
                    + t * t * t * p[6];
                let y = u * u * u * p[1]
                    + 3.0 * u * u * t * p[3]
                    + 3.0 * u * t * t * p[5]
                    + t * t * t * p[7];
                egui::pos2(-(x - 12.0) * 5.3, (y - 12.0) * 5.3)
            })
        })
        .collect()
}
fn segment_bounds(time: f64, ridge: usize, seed: u64) -> (f32, f32, f32) {
    let time = time.max(0.0) as f32;
    let delay = random(seed ^ ridge as u64) * 0.06;
    let local = (time - delay).max(0.0);
    let cycle = (local / 1.05).floor() as u64;
    let phase = local % 1.05;
    let start = random(
        seed.wrapping_add(ridge as u64 * 151)
            .wrapping_add(cycle * 12347),
    );
    let fill = ease(phase / 0.36);
    let opacity = if phase > 0.66 {
        1.0 - ease((phase - 0.66) / 0.39)
    } else {
        1.0
    };
    ((start - fill).max(0.0), (start + fill).min(1.0), opacity)
}
fn paint(ui: &mut egui::Ui, time: f64, seed: u64, scanning: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 172.0), egui::Sense::hover());
    let center = rect.center();
    ui.painter()
        .circle_filled(center, 64.0, Color32::from_gray(24));
    ui.painter()
        .circle_stroke(center, 64.0, Stroke::new(1.0, Color32::from_gray(53)));
    for ridge in 0..9 {
        let points: Vec<_> = points(ridge)
            .into_iter()
            .map(|p| center + p.to_vec2())
            .collect();
        ui.painter().add(egui::Shape::line(
            points.clone(),
            Stroke::new(2.3, Color32::from_gray(104)),
        ));
        if scanning {
            // The exact same traveling highlight curve as activity/search text.
            for band in 0..24 {
                let strength = crate::activity::shine(ui.input(|i| i.time), band as f32, 24.0);
                if strength < 0.02 {
                    continue;
                }
                let clip = egui::Rect::from_min_max(
                    egui::pos2(
                        center.x - 64.0 + band as f32 * 128.0 / 24.0,
                        center.y - 64.0,
                    ),
                    egui::pos2(
                        center.x - 64.0 + (band + 1) as f32 * 128.0 / 24.0,
                        center.y + 64.0,
                    ),
                );
                ui.painter()
                    .with_clip_rect(clip.intersect(ui.clip_rect()))
                    .add(egui::Shape::line(
                        points.clone(),
                        Stroke::new(2.5, Color32::from_white_alpha((strength * 220.0) as u8)),
                    ));
            }
        }
        if !scanning {
            continue;
        }
        let (begin, end, opacity) = segment_bounds(time, ridge, seed);
        let last = (points.len() - 1) as f32;
        let from = (begin * last).floor() as usize;
        let to = (end * last).ceil().min(last) as usize;
        if to <= from {
            continue;
        }
        let path = points[from..=to].to_vec();
        let alpha = (opacity * 235.0) as u8;
        let color = Color32::from_white_alpha(alpha);
        ui.painter().add(egui::Shape::line(
            path.clone(),
            Stroke::new(6.5, Color32::from_white_alpha((opacity * 14.0) as u8)),
        ));
        ui.painter()
            .add(egui::Shape::line(path, Stroke::new(2.5, color)));
        for p in [points[from], points[to]] {
            ui.painter().circle_filled(p, 1.25, color);
        }
    }
}
/// Returns true only when Cancel/Escape is used. This is never an authentication decision.
pub fn show(ctx: &egui::Context, time: f64, seed: u64, scanning: bool, opacity: f32) -> bool {
    egui::Modal::new(egui::Id::unique("fingerprint-authentication"))
        .backdrop_color(Color32::from_black_alpha((95.0 * opacity) as u8))
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(
                    29,
                    29,
                    29,
                    (255.0 * opacity) as u8,
                ))
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(49, 49, 49, (255.0 * opacity) as u8),
                ))
                .corner_radius(24)
                .inner_margin(22)
                .shadow(egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 28,
                    spread: 1,
                    color: Color32::from_black_alpha((80.0 * opacity) as u8),
                }),
        )
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            ui.set_width(270.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Administrator access").size(17.0).strong());
                ui.add_space(4.0);
                paint(ui, time, seed, scanning);
                ui.add_space(2.0);
            })
            .inner
        });
    if scanning {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_frames_change_and_exit_fades_without_changing_the_caption() {
        let ctx = egui::Context::default();
        let frame = |clock: f64, scan_time: f64, scanning: bool, opacity: f32| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 600.0),
                    )),
                    time: Some(clock),
                    ..Default::default()
                },
                |ui| {
                    show(ui.ctx(), scan_time, 42, scanning, opacity);
                },
            );
            let shapes = format!("{:?}", out.shapes);
            out.textures_delta.clear();
            shapes
        };
        let _ = frame(0.0, 0.0, false, 1.0);
        let _ = frame(5.0, 0.0, false, 1.0);
        let idle = frame(6.0, 0.0, false, 1.0);
        assert_eq!(idle, frame(7.0, 0.0, false, 1.0));
        let a = frame(8.0, 0.08, true, 1.0);
        let b = frame(8.15, 0.23, true, 1.0);
        assert_ne!(a, b);
        assert_ne!(b, frame(8.2, 0.28, true, 0.2));
    }
    #[test]
    fn short_contact_survives_start_stop_in_the_same_frame() {
        let mut m = Motion::default();
        assert!(!m.visible(10.0));
        m.contact(true, 10.0);
        m.contact(false, 10.01);
        assert!(m.visible(10.08));
        assert!(!m.visible(10.3));
        m.read(11.0);
        assert!(m.visible(11.1));
        m.finish(11.2);
        assert!(m.visible(11.4));
        assert!(!m.visible(11.6));
        assert_eq!(fade_opacity(0.34), 1.0);
        assert_eq!(fade_opacity(0.0), 0.0);
        assert!(fade_opacity(0.08) < fade_opacity(0.24));
    }
    #[test]
    fn ridges_are_bounded_and_finite() {
        for ridge in 0..9 {
            for p in points(ridge) {
                assert!(p.x.is_finite() && p.y.is_finite());
                assert!(p.x.abs() < 60.0 && p.y.abs() < 80.0);
            }
        }
    }
    #[test]
    fn ridges_use_distinct_reveal_origins_and_complete() {
        let starts: Vec<_> = (0..9).map(|r| segment_bounds(0.0, r, 42).0).collect();
        assert!(starts.windows(2).any(|p| p[0] != p[1]));
        for ridge in 0..9 {
            let (a, b, opacity) = segment_bounds(0.55, ridge, 42);
            assert_eq!((a, b), (0.0, 1.0));
            assert!(opacity > 0.0);
        }
    }
}

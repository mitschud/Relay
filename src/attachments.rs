//! Attachment thumbnails and clipboard acquisition stay off the rendering thread.
use eframe::egui::{self, Color32, RichText, Stroke};
use std::{collections::HashMap, path::Path, sync::mpsc};

enum Event {
    Image(String, egui::ColorImage),
    File(String),
    Clipboard(Result<Vec<String>, String>),
}
enum Preview {
    Loading,
    Image(egui::TextureHandle),
    File,
}
pub struct Attachments {
    previews: HashMap<String, Preview>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    pub pasting: bool,
    recent: std::collections::VecDeque<String>,
}
impl Default for Attachments {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            previews: HashMap::new(),
            tx,
            rx,
            pasting: false,
            recent: Default::default(),
        }
    }
}
pub fn is_image(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| ["png", "jpg", "jpeg", "webp"].contains(&s.to_ascii_lowercase().as_str()))
}
pub fn add_paths(current: &mut Vec<String>, paths: impl IntoIterator<Item = String>) {
    for p in paths {
        if current.len() >= 16 {
            break;
        }
        if Path::new(&p).is_file() && !current.contains(&p) {
            current.push(p);
        }
    }
}
fn thumbnail(path: &str) -> Option<egui::ColorImage> {
    static DECODER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _decode = DECODER.lock().ok()?;
    if !is_image(path) || std::fs::metadata(path).ok()?.len() > 32 * 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let image = if decoded.width() > 192 || decoded.height() > 192 {
        decoded.thumbnail(192, 192)
    } else {
        decoded
    }
    .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    ))
}
fn save_pasted_image(image: &egui::ColorImage, directory: &Path) -> Result<String, String> {
    use image::ImageEncoder;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if image.pixels.len() > 16 * 1024 * 1024 {
        return Err("Clipboard image exceeds the 64 MiB decoded-image limit".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = directory.join(format!("Screenshot-{}-{stamp}.png", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    if let Err(e) = image::codecs::png::PngEncoder::new(file).write_image(
        &pixels,
        image.size[0] as u32,
        image.size[1] as u32,
        image::ExtendedColorType::Rgba8,
    ) {
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    Ok(path.to_string_lossy().into_owned())
}
impl Attachments {
    /// Sent messages only retain a bounded cache of small textures. Decode on demand,
    /// outside the render thread; older messages can repopulate it when scrolled into view.
    pub fn poll_sent(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Image(path, image) if self.previews.contains_key(&path) => {
                    self.previews.insert(path.clone(), Preview::Image(ctx.load_texture(
                        format!("sent-{path}"), image, egui::TextureOptions::LINEAR)));
                }
                Event::File(path) if self.previews.contains_key(&path) => {
                    self.previews.insert(path, Preview::File);
                }
                _ => {}
            }
        }
    }
    pub fn show_sent(&mut self, ui: &mut egui::Ui, paths: &[String]) {
        if paths.is_empty() { return; }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP).with_main_wrap(true), |ui| {
            for path in paths {
                let (rect,response)=ui.allocate_exact_size(egui::vec2(80.0,80.0),egui::Sense::hover());
                if !ui.is_rect_visible(rect) { continue; }
                if !self.previews.contains_key(path) {
                    while self.recent.len()>=64 {
                        if let Some(old)=self.recent.pop_front() {self.previews.remove(&old);}
                    }
                    self.recent.push_back(path.clone());
                    self.previews.insert(path.clone(),Preview::Loading);
                    let path=path.clone();let tx=self.tx.clone();let ctx=ui.ctx().clone();
                    std::thread::spawn(move || {
                        let event=match thumbnail(&path) {Some(image)=>Event::Image(path,image),None=>Event::File(path)};
                        let _=tx.send(event);ctx.request_repaint();
                    });
                } else {
                    self.recent.retain(|p|p!=path);self.recent.push_back(path.clone());
                }
                ui.painter().rect_filled(rect,10,Color32::from_gray(29));
                if let Some(Preview::Image(texture))=self.previews.get(path) {
                    let size=texture.size_vec2();let fit=size*(76.0/size.x.max(size.y)).min(1.0);
                    ui.put(egui::Rect::from_center_size(rect.center(),fit),egui::Image::new((texture.id(),fit)).corner_radius(8));
                } else if matches!(self.previews.get(path),Some(Preview::File)) {
                    ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,"Image unavailable",egui::FontId::proportional(9.0),Color32::GRAY);
                }
                response.on_hover_text(path);
            }
        });
        ui.add_space(6.0);
    }

    pub fn paste_image(&mut self, ctx: &egui::Context, image: std::sync::Arc<egui::ColorImage>) {
        if self.pasting {
            return;
        }
        self.pasting = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let directory = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".local/share/relay/attachments");
            let result = save_pasted_image(&image, &directory).map(|p| vec![p]);
            let _ = tx.send(Event::Clipboard(result));
            ctx.request_repaint();
        });
    }

    pub fn paste(&mut self, ctx: &egui::Context) {
        if self.pasting {
            return;
        }
        self.pasting = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Vec<String>, String> {
                let out = crate::util::helper_command("RELAY_CLIPBOARD_HELPER", "__clipboard")
                    .output()
                    .map_err(|e| e.to_string())?;
                let v: serde_json::Value =
                    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(v["error"]
                        .as_str()
                        .unwrap_or("Could not paste attachment")
                        .into());
                }
                Ok(v["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect())
            })();
            let _ = tx.send(Event::Clipboard(result));
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self, ctx: &egui::Context, files: &mut Vec<String>) -> Option<String> {
        let mut error = None;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Clipboard(result) => {
                    self.pasting = false;
                    match result {
                        Ok(paths) => add_paths(files, paths),
                        Err(e) => error = Some(e),
                    }
                }
                Event::Image(path, image) if files.contains(&path) => {
                    self.previews.insert(
                        path.clone(),
                        Preview::Image(ctx.load_texture(path, image, egui::TextureOptions::LINEAR)),
                    );
                }
                Event::File(path) if files.contains(&path) => {
                    self.previews.insert(path, Preview::File);
                }
                _ => {}
            }
        }
        self.previews.retain(|path, _| files.contains(path));
        // At most 16 bounded images; decode sequentially to cap peak RAM.
        let pending: Vec<String> = files
            .iter()
            .filter(|p| !self.previews.contains_key(*p))
            .cloned()
            .collect();
        if !pending.is_empty() {
            for p in &pending {
                self.previews.insert(p.clone(), Preview::Loading);
            }
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                for path in pending {
                    let event = match thumbnail(&path) {
                        Some(image) => Event::Image(path, image),
                        None => Event::File(path),
                    };
                    if tx.send(event).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        error
    }
    pub fn show(&self, ui: &mut egui::Ui, files: &mut Vec<String>) {
        let mut remove = None;
        let columns = ((ui.available_width() + 10.0) / 110.0).floor().max(1.0) as usize;
        let rows = files.len().div_ceil(columns);
        let height = (rows as f32 * 136.0 - 10.0)
            .min((ui.ctx().content_rect().height() * 0.30).clamp(126.0, 244.0));
        egui::ScrollArea::vertical()
            .id_salt("attachment-previews")
            .min_scrolled_height(height)
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                    for (i, path) in files.iter().enumerate() {
                        ui.allocate_ui_with_layout(
                            egui::vec2(100.0, 126.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(100.0);
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(100.0, 100.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 10, Color32::from_gray(36));
                                ui.painter().rect_stroke(
                                    rect,
                                    10,
                                    Stroke::new(1.0, Color32::from_gray(67)),
                                    egui::StrokeKind::Inside,
                                );
                                match self.previews.get(path) {
                                    Some(Preview::Image(texture)) => {
                                        let size = texture.size_vec2();
                                        let factor = 96.0 / size.x.max(size.y);
                                        let target = egui::Rect::from_center_size(
                                            rect.center(),
                                            size * factor,
                                        );
                                        egui::Image::from_texture(texture)
                                            .fit_to_exact_size(target.size())
                                            .corner_radius(8)
                                            .paint_at(ui, target);
                                    }
                                    Some(Preview::Loading) => {
                                        ui.painter().text(
                                            rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "…",
                                            egui::FontId::proportional(22.0),
                                            Color32::from_gray(160),
                                        );
                                    }
                                    _ => {
                                        let page = egui::Rect::from_center_size(
                                            rect.center() - egui::vec2(0.0, 6.0),
                                            egui::vec2(26.0, 32.0),
                                        );
                                        ui.painter().rect_stroke(
                                            page,
                                            3,
                                            Stroke::new(1.5, Color32::from_gray(170)),
                                            egui::StrokeKind::Inside,
                                        );
                                        let ext = Path::new(path)
                                            .extension()
                                            .and_then(|x| x.to_str())
                                            .unwrap_or("FILE")
                                            .to_uppercase();
                                        ui.painter().text(
                                            rect.center() + egui::vec2(0.0, 22.0),
                                            egui::Align2::CENTER_CENTER,
                                            ext,
                                            egui::FontId::proportional(10.0),
                                            Color32::from_gray(180),
                                        );
                                    }
                                }
                                response.on_hover_text(path);
                                let close = egui::Rect::from_min_size(
                                    rect.right_top() + egui::vec2(-23.0, 3.0),
                                    egui::vec2(20.0, 20.0),
                                );
                                let close_response = ui.interact(
                                    close,
                                    ui.scope_id().with(("remove-attachment", i)),
                                    egui::Sense::click(),
                                );
                                ui.painter().circle_filled(
                                    close.center(),
                                    9.0,
                                    Color32::from_black_alpha(if close_response.hovered() {
                                        210
                                    } else {
                                        140
                                    }),
                                );
                                ui.painter().text(
                                    close.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "×",
                                    egui::FontId::proportional(13.0),
                                    Color32::from_gray(230),
                                );
                                if close_response.on_hover_text("Remove attachment").clicked() {
                                    remove = Some(i);
                                }
                                let name = Path::new(path)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                ui.add_sized(
                                    [100.0, 16.0],
                                    egui::Label::new(
                                        RichText::new(name)
                                            .size(10.5)
                                            .color(Color32::from_gray(190)),
                                    )
                                    .truncate(),
                                );
                            },
                        );
                    }
                });
            });
        if let Some(i) = remove {
            files.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_paste_produces_private_png_and_thumbnail() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("relay-native-paste-{}", std::process::id()));
        let image = egui::ColorImage::new([16, 12], vec![Color32::from_rgb(20, 80, 120); 16 * 12]);
        let path = save_pasted_image(&image, &dir).unwrap();
        assert_eq!(image::image_dimensions(&path).unwrap(), (16, 12));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(thumbnail(&path).unwrap().size, [16, 12]);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn extension_is_case_insensitive_and_not_directory_name() {
        assert!(is_image("/tmp/A.PNG"));
        assert!(!is_image("/tmp/a.png/file.txt"));
    }
    #[test]
    fn attachments_are_deduplicated_and_missing_files_are_rejected() {
        let path =
            std::env::temp_dir().join(format!("relay-attachment-test-{}", std::process::id()));
        std::fs::write(&path, b"test").unwrap();
        let p = path.to_string_lossy().to_string();
        let mut files = vec![];
        add_paths(&mut files, [p.clone(), p.clone(), "/does/not/exist".into()]);
        assert_eq!(files, vec![p]);
        std::fs::remove_file(path).unwrap();
    }
}

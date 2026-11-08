//! In-process file and folder picker. This replaces the external zenity dialog
//! while keeping the same multi-file and folder-selection behavior.
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

#[derive(Default)]
pub struct Picker {
    open: bool,
    folder: bool,
    cwd: PathBuf,
    entries: Vec<Entry>,
    selected: Vec<PathBuf>,
    typed: String,
    error: Option<String>,
    hidden: bool,
}

impl Picker {
    pub fn open(&mut self, folder: bool, start: Option<&str>) {
        self.open = true;
        self.folder = folder;
        self.selected.clear();
        self.error = None;
        if self.cwd.as_os_str().is_empty() {
            self.cwd = crate::util::home();
        }
        if let Some(path) = start.map(PathBuf::from).filter(|path| path.is_dir()) {
            self.cwd = path;
        }
        self.refresh();
    }

    pub fn folder(&self) -> bool {
        self.folder
    }

    fn refresh(&mut self) {
        self.typed = self.cwd.display().to_string();
        self.entries.clear();
        self.error = None;
        match std::fs::read_dir(&self.cwd) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !self.hidden && name.starts_with('.') {
                        continue;
                    }
                    let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                    self.entries.push(Entry {
                        path: entry.path(),
                        name,
                        is_dir,
                    });
                }
                self.entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            Err(error) => {
                self.error = Some(format!("Cannot read {}: {error}", self.cwd.display()));
            }
        }
    }

    fn navigate(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.cwd = path;
            self.refresh();
        }
    }

    fn choose_typed(&mut self) {
        let path = crate::util::expand_tilde(self.typed.trim());
        if path.is_dir() {
            self.navigate(path);
        } else if path.is_file() && !self.folder {
            if !self.selected.contains(&path) {
                self.selected.push(path);
            }
        } else {
            self.error = Some(format!("Not found: {}", path.display()));
        }
    }

    /// Draws the modal. Returns the chosen paths once, after the user confirms.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Vec<String>> {
        if !self.open {
            return None;
        }
        let mut navigate = None;
        let mut toggle = None;
        let mut submit = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("relay-file-picker")).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label(
                RichText::new(if self.folder {
                    "Choose project folder"
                } else {
                    "Attach files"
                })
                .size(15.0)
                .strong(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Up").clicked() {
                    if let Some(parent) = self.cwd.parent() {
                        navigate = Some(parent.to_path_buf());
                    }
                }
                if ui.button("Home").clicked() {
                    navigate = Some(crate::util::home());
                }
                ui.checkbox(&mut self.hidden, "Hidden");
                ui.add(
                    egui::TextEdit::singleline(&mut self.typed)
                        .desired_width(f32::INFINITY)
                        .hint_text("Path"),
                );
                if ui.button("Open").clicked() {
                    self.choose_typed();
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(Color32::from_rgb(235, 140, 120), error);
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    if self.entries.is_empty() && self.error.is_none() {
                        ui.label("No matching items.");
                    }
                    for entry in &self.entries {
                        let selected = self.selected.iter().any(|path| path == &entry.path);
                        let label = format!(
                            "{}  {}",
                            if entry.is_dir { "[dir]" } else { "[file]" },
                            entry.name
                        );
                        let response = ui.add(
                            egui::Button::new(RichText::new(label).size(12.5))
                                .selected(selected)
                                .frame(false)
                                .min_size(egui::vec2(ui.available_width(), 26.0)),
                        );
                        if response.clicked() {
                            if entry.is_dir {
                                navigate = Some(entry.path.clone());
                            } else {
                                toggle = Some(entry.path.clone());
                            }
                        }
                        if response.double_clicked() && !entry.is_dir {
                            toggle = Some(entry.path.clone());
                            submit = true;
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.folder {
                        "Choose this folder".to_owned()
                    } else {
                        format!("Attach {}", self.selected.len())
                    };
                    let enabled = self.folder || !self.selected.is_empty();
                    if ui
                        .add_enabled(enabled, egui::Button::new(label))
                        .clicked()
                    {
                        submit = true;
                    }
                });
            });
            if self.folder {
                ui.small(self.cwd.display().to_string());
            }
        });
        if modal.should_close() {
            cancel = true;
        }
        if let Some(path) = navigate {
            self.navigate(path);
        }
        if let Some(path) = toggle {
            if let Some(index) = self.selected.iter().position(|item| item == &path) {
                self.selected.remove(index);
            } else {
                self.selected.push(path);
            }
        }
        if cancel {
            self.open = false;
            return None;
        }
        if submit {
            self.open = false;
            if self.folder {
                return Some(vec![self.cwd.display().to_string()]);
            }
            if !self.selected.is_empty() {
                return Some(
                    self.selected
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect(),
                );
            }
        }
        None
    }
}

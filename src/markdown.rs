use eframe::egui::{self, Color32};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

const CODE_MARKER: Color32 = Color32::from_rgb(39, 34, 29);
const ORANGE: Color32 = Color32::from_rgb(243, 139, 65);
const STRONG_MARKER: Color32 = Color32::from_rgb(244, 243, 242);

/// CommonMark exposes an inline-code background but no foreground option.
/// Re-layout only its marked code sections using public egui paint/font APIs.
/// Ordinary prose, selection, links and syntax-highlighted blocks stay intact.
pub fn show(ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &str) {
    let layer = ui.layer_id();
    let start = ui.painter().add(egui::Shape::Noop);
    ui.scope(|ui| {
        ui.visuals_mut().code_bg_color = CODE_MARKER;
        ui.visuals_mut().widgets.active.fg_stroke.color = STRONG_MARKER;
        ui.spacing_mut().item_spacing.y = 10.0;
        CommonMarkViewer::new().show(ui, cache, text);
    });
    let end = ui.painter().add(egui::Shape::Noop);
    let changes = ui.ctx().graphics_mut(|graphics| {
        graphics
            .get(layer)
            .into_iter()
            .flat_map(|list| list.all_entries().enumerate())
            .filter(|(i, _)| *i > start.0 && *i < end.0)
            .filter_map(|(i, clipped)| match &clipped.shape {
                egui::Shape::Text(shape)
                    if shape
                        .galley
                        .job
                        .sections
                        .iter()
                        .any(|s| s.format.background == CODE_MARKER || s.format.color == STRONG_MARKER) =>
                {
                    Some((egui::layers::ShapeIdx(i), shape.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    for (index, mut shape) in changes {
        let mut job = (*shape.galley.job).clone();
        let has_strong=job.sections.iter().any(|s|s.format.color==STRONG_MARKER);
        // egui's strong() changes colour, not font weight. A tiny second ink
        // pass gives emphasis weight without changing wrap/selection geometry.
        let emphasis=if has_strong {
            let mut ink=job.clone();
            for section in &mut ink.sections {
                section.format.color=if section.format.color==STRONG_MARKER {Color32::from_gray(245)} else {Color32::TRANSPARENT};
                section.format.background=Color32::TRANSPARENT;
                section.format.underline=egui::Stroke::NONE;
                section.format.strikethrough=egui::Stroke::NONE;
            }
            Some(ink)
        } else {None};
        for section in &mut job.sections {
            if section.format.color==STRONG_MARKER {section.format.color=Color32::from_gray(245);}
            if section.format.background == CODE_MARKER {
                section.format.color = ORANGE;
                section.format.background = Color32::from_gray(33);
            }
        }
        shape.galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        if let Some(ink)=emphasis {
            let mut weight=shape.clone();
            weight.pos.x+=0.3;
            weight.galley=ui.fonts_mut(|fonts|fonts.layout_job(ink));
            ui.painter().add(egui::Shape::Text(weight));
        }
        ui.ctx().graphics_mut(|graphics| {
            if let Some(list) = graphics.get_mut(layer) {
                list.mutate_shape(index, |clipped| clipped.shape = egui::Shape::Text(shape));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emphasis_has_extra_ink_without_thickening_regular_words() {
        let ctx=egui::Context::default();let mut cache=CommonMarkCache::default();
        let mut out=ctx.run_ui(egui::RawInput::default(),|ui|show(ui,&mut cache,"Regular and **emphasis** together."));
        let mut emphasis=Vec::new();let mut regular=0;
        for clipped in &out.shapes {
            if let egui::Shape::Text(shape)=&clipped.shape {
                if shape.galley.job.text=="emphasis" {
                    emphasis.push(shape.pos.x);
                    assert!(shape.galley.job.sections.iter().all(|s|s.format.color==Color32::from_gray(245)));
                }
                if shape.galley.job.text.contains("Regular"){regular+=1;}
            }
        }
        assert_eq!(regular,1,"ordinary words must not receive an extra weight pass");
        assert_eq!(emphasis.len(),2,"explicit emphasis receives one subtle weight pass");
        assert!(((emphasis[0]-emphasis[1]).abs()-0.3).abs()<0.01);
        out.textures_delta.clear();
    }

    #[test]
    fn inline_code_is_orange_without_recolouring_the_prose() {
        let ctx = egui::Context::default();
        let mut cache = CommonMarkCache::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut cache, "Plain words and `file.txt` together.");
        });
        let mut code = false;
        let mut prose = false;
        for clipped in &out.shapes {
            if let egui::Shape::Text(shape) = &clipped.shape {
                for section in &shape.galley.job.sections {
                    let text = &shape.galley.job.text
                        [section.byte_range.start.0..section.byte_range.end.0];
                    if text.contains("file.txt") {
                        assert_eq!(section.format.color, ORANGE);
                        code = true;
                    }
                    if text.contains("Plain words") {
                        assert_ne!(section.format.color, ORANGE);
                        prose = true;
                    }
                }
            }
        }
        out.textures_delta.clear();
        assert!(
            code && prose,
            "both inline code and ordinary prose must render"
        );
    }
}

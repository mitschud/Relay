//! Fade glyph alpha at the conversation viewport edges, without a background overlay.
use eframe::egui::{self, Color32};

fn opacity(y:f32, viewport:egui::Rect, top:bool, bottom:bool)->f32 {
    let smooth=|n:f32| {let n=n.clamp(0.0,1.0);n*n*(3.0-2.0*n)};
    let a=if top {smooth((y-viewport.top())/28.0)}else{1.0};
    let b=if bottom {smooth((viewport.bottom()-y)/42.0)}else{1.0};
    a.min(b)
}
fn fade_text(text:&mut egui::epaint::TextShape,viewport:egui::Rect,top:bool,bottom:bool) {
    let bounds=text.visual_bounding_rect();
    if text.angle!=0.0 || !bounds.intersects(viewport)
        || ((!top || bounds.top()>=viewport.top()+28.0) && (!bottom || bounds.bottom()<=viewport.bottom()-42.0)) {return;}
    let override_color=text.override_text_color.take();
    let galley=std::sync::Arc::make_mut(&mut text.galley);
    for placed in &mut galley.rows {
        let row=std::sync::Arc::make_mut(&mut placed.row);
        let range=row.visuals.glyph_vertex_range.clone();
        for (i,vertex) in row.visuals.mesh.vertices.iter_mut().enumerate() {
            if range.contains(&i) {
                let color=override_color.unwrap_or(if vertex.color==Color32::PLACEHOLDER {text.fallback_color}else{vertex.color});
                vertex.color=color.gamma_multiply(opacity(text.pos.y+placed.pos.y+vertex.pos.y,viewport,top,bottom));
            }
        }
    }
}
pub fn apply(ui:&egui::Ui,start:egui::layers::ShapeIdx,end:egui::layers::ShapeIdx,viewport:egui::Rect,top:bool,bottom:bool) {
    if !top && !bottom {return;}
    ui.ctx().graphics_mut(|graphics| {
        if let Some(list)=graphics.get_mut(ui.layer_id()) {
            for index in start.0+1..end.0 {
                list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                    if let egui::Shape::Text(text)=&mut clipped.shape {fade_text(text,viewport,top,bottom);}
                });
            }
        }
    });
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn edges_are_transparent_and_center_stays_opaque() {
        let r=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(300.0,200.0));
        assert_eq!(opacity(0.0,r,true,true),0.0);assert_eq!(opacity(200.0,r,true,true),0.0);
        assert_eq!(opacity(100.0,r,true,true),1.0);assert_eq!(opacity(0.0,r,false,false),1.0);
        assert!(opacity(5.0,r,true,true)<opacity(20.0,r,true,true));
    }
    #[test] fn fades_glyphs_without_modifying_the_cached_layout() {
        let ctx=egui::Context::default();
        let mut checked=false;
        let mut out=ctx.run_ui(egui::RawInput::default(),|ui| {
            let galley=ui.painter().layout_no_wrap("Preview text".into(),egui::FontId::proportional(14.0),Color32::WHITE);
            let mut text=egui::epaint::TextShape::new(egui::pos2(10.0,2.0),galley.clone(),Color32::WHITE);
            let original=galley.rows[0].visuals.mesh.vertices.clone();
            fade_text(&mut text,egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(300.0,200.0)),true,false);
            assert_eq!(galley.rows[0].visuals.mesh.vertices,original);
            let row=&text.galley.rows[0].visuals;
            assert!(row.mesh.vertices[row.glyph_vertex_range.clone()].iter().any(|v|v.color.a()<255));
            checked=true;
        });out.textures_delta.clear();assert!(checked);
    }
}

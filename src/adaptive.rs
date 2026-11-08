//! Bounded, retargetable height easing. No work or repaint requests at rest.
use eframe::egui;

#[derive(Default)]
pub struct Region {
    measured: f32,
    motion: Height,
}
#[derive(Default)]
struct Height {from:f32,target:f32,started:f64}
impl Height {
    fn value(&self,now:f64)->f32 {
        let p=((now-self.started)/0.22).clamp(0.,1.) as f32;
        let ease=1.-(1.-p).powi(3);
        self.from+(self.target-self.from)*ease
    }
    fn update(&mut self,target:f32,now:f64)->f32 {
        if (target-self.target).abs()>0.25 {
            self.from=self.value(now);self.target=target;self.started=now;
        }
        self.value(now)
    }
}
impl Region {
    pub fn height(&mut self,now:f64)->f32 {self.motion.update(self.measured,now).max(0.)}
    pub fn show<R>(&mut self,ui:&mut egui::Ui,add:impl FnOnce(&mut egui::Ui)->R)->R {
        let now=ui.input(|i|i.time);
        let height=self.height(now);
        let start=ui.next_widget_position();
        let width=ui.available_width();
        let viewport=egui::Rect::from_min_size(start,egui::vec2(width,height));
        // Independent layout measures natural content without forcing its
        // full height into the parent. Only the eased viewport is reserved.
        let mut child=ui.new_child(egui::UiBuilder::new().id_salt("adaptive-composer-body")
            .max_rect(egui::Rect::from_min_size(start,egui::vec2(width,ui.ctx().content_rect().height()))));
        child.shrink_clip_rect(viewport);
        let result=add(&mut child);
        let natural=child.min_rect().height();
        let target=if natural>0.5 {natural+8.}else{0.};
        if (self.measured-target).abs()>0.25 {self.measured=target;self.motion.update(target,now);ui.ctx().request_repaint();}
        if (height-self.measured).abs()>0.25 {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        let spacing=ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y=0.;
        if height>0.01 {ui.allocate_exact_size(egui::vec2(width,height),egui::Sense::hover());}
        ui.spacing_mut().item_spacing.y=spacing;
        result
    }
}
pub fn popup_anchor(rect:egui::Rect,top:Option<f32>)->egui::Rect {
    let y=top.unwrap_or(rect.top());
    egui::Rect::from_min_max(egui::pos2(rect.left(),y),egui::pos2(rect.right(),y+1.))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resizing_is_continuous_and_bounded_even_when_retargeted(){
        let mut h=Height::default();assert_eq!(h.update(160.,1.),0.);
        let mid=h.update(160.,1.1);assert!(mid>0.&&mid<160.);
        assert!((h.update(80.,1.1)-mid).abs()<0.001);
        assert!(h.update(80.,1.2)>=80.);assert_eq!(h.update(80.,2.),80.);
        assert_eq!(h.update(0.,2.),80.);assert_eq!(h.update(0.,3.),0.);
    }
    #[test]
    fn natural_content_does_not_force_the_parent_to_jump(){
        let ctx=egui::Context::default();let mut region=Region::default();
        let mut sample=|time:f64,content:f32| {
            let mut used=0.;
            let mut out=ctx.run_ui(egui::RawInput{time:Some(time),..Default::default()},|ui|{
                let y=ui.next_widget_position().y;
                region.show(ui,|child|{if content>0. {child.allocate_exact_size(egui::vec2(120.,content),egui::Sense::hover());}});
                used=ui.next_widget_position().y-y;
            });out.textures_delta.clear();used
        };
        sample(1.,0.);
        let beginning=sample(2.,150.);assert!(beginning<10.,"new content must not immediately reserve its full height");
        let middle=sample(2.1,150.);assert!(middle>beginning&&middle<158.);
        let end=sample(2.4,150.);assert!((end-158.).abs()<1.);
        let shrinking=sample(3.,0.);assert!((shrinking-end).abs()<1.);
        assert!(sample(3.1,0.)<shrinking);assert!(sample(3.4,0.)<1.);
    }
    #[test]
    fn bottom_anchored_composer_does_not_nudge_during_growth_or_shrink(){
        let ctx=egui::Context::default();let mut region=Region::default();let mut bottoms=Vec::new();
        for (time,content) in [(1.,0.),(2.,150.),(2.05,150.),(2.1,150.),(2.3,150.),(3.,0.),(3.05,0.),(3.1,0.),(3.3,0.)] {
            let mut out=ctx.run_ui(egui::RawInput{time:Some(time),screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(600.,600.))),..Default::default()},|root|{
                let h=region.height(time);
                egui::Panel::bottom("anchored-test").exact_size(138.+h)
                    .frame(egui::Frame::new().inner_margin(24)).show(root,|ui|{
                        ui.spacing_mut().item_spacing.y=6.;
                        let frame=egui::Frame::new().stroke(egui::Stroke::new(1.,egui::Color32::GRAY))
                            .inner_margin(egui::Margin::symmetric(16,8)).show(ui,|ui|{
                                region.show(ui,|child|{if content>0. {child.allocate_exact_size(egui::vec2(100.,content),egui::Sense::hover());}});
                                ui.allocate_exact_size(egui::vec2(100.,30.),egui::Sense::hover());
                            });
                        bottoms.push(frame.response.rect.bottom());
                        ui.allocate_exact_size(egui::vec2(100.,36.),egui::Sense::hover());
                    });
            });out.textures_delta.clear();
        }
        assert!(bottoms.iter().all(|y|(*y-bottoms[0]).abs()<0.6),"bottom edge moved during transition: {bottoms:?}");
    }
}

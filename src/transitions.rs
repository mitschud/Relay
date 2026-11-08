//! Brief, non-interactive popup exit snapshots. No redraw timer survives the fade.
use eframe::egui::{self,Context,LayerId};
use std::{collections::HashMap,sync::{Arc,Mutex}};
const DURATION:f64=0.14;
#[derive(Clone)] struct Snapshot { seen:u64, closing:Option<f64>, shapes:Vec<egui::epaint::ClippedShape> }
#[derive(Default)] struct State { layers:HashMap<LayerId,Snapshot> }
fn state(ctx:&Context)->Arc<Mutex<State>> {
 ctx.data_mut(|d|d.get_temp_mut_or_default::<Arc<Mutex<State>>>(egui::Id::unique("relay-popup-exits")).clone())
}
pub trait PopupFade {
 fn fade_show<R>(self,body:impl FnOnce(&mut egui::Ui)->R)->Option<egui::InnerResponse<R>>;
}
impl PopupFade for egui::Popup<'_> {
 fn fade_show<R>(self,body:impl FnOnce(&mut egui::Ui)->R)->Option<egui::InnerResponse<R>> {
  let ctx=self.ctx().clone();let result=self.show(body);
  if let Some(r)=&result {
   let layer=r.response.layer_id;
   let shapes=ctx.graphics(|g|g.get(layer).map(|p|p.all_entries().cloned().collect()).unwrap_or_default());
   state(&ctx).lock().unwrap().layers.insert(layer,Snapshot{seen:ctx.cumulative_pass_nr(),closing:None,shapes});
  }
  result
 }
}
fn opacity(elapsed:f64)->f32 {let t=(elapsed/DURATION).clamp(0.0,1.0) as f32;1.0-t*t*(3.0-2.0*t)}
pub fn finish(ctx:&Context) {
 let now=ctx.input(|i|i.time);let pass=ctx.cumulative_pass_nr();
 let shared=state(ctx);let mut data=shared.lock().unwrap();let mut ghosts=Vec::new();
 data.layers.retain(|layer,s|{
  if s.seen==pass{return true;}
  let start=*s.closing.get_or_insert(now);let alpha=opacity(now-start);
  if alpha<=0.0{return false;}
  ghosts.push((*layer,s.shapes.clone(),alpha));true
 });
 drop(data);
 if !ghosts.is_empty(){ctx.request_repaint_after(std::time::Duration::from_millis(16));}
 for (layer,shapes,alpha) in ghosts {
  egui::Area::new(layer.id.with("closing-visual")).order(egui::Order::Foreground).fixed_pos(egui::Pos2::ZERO).movable(false).interactable(false).fade_in(false).show(ctx,|ui|{
   for clipped in shapes {
    let mut painter=ui.painter().with_clip_rect(clipped.clip_rect);painter.set_opacity(alpha);
    let mut shape=clipped.shape;shape.transform(clipped.transform_after_tessellation);painter.add(shape);
   }
  });
 }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn close_fade_is_bounded_and_finishes(){assert_eq!(opacity(0.0),1.0);assert!(opacity(0.07)>0.0&&opacity(0.07)<1.0);assert_eq!(opacity(0.14),0.0);assert_eq!(opacity(1.0),0.0);}
 #[test] fn closed_popup_snapshot_is_retired_after_fade(){
  let ctx=Context::default();let layer=LayerId::new(egui::Order::Foreground,egui::Id::unique("test-menu"));
  let mut out=ctx.run_ui(egui::RawInput{time:Some(1.0),..Default::default()},|_|{
   state(&ctx).lock().unwrap().layers.insert(layer,Snapshot{seen:ctx.cumulative_pass_nr(),closing:None,shapes:Vec::new()});finish(&ctx);
  });out.textures_delta.clear();
  let mut out=ctx.run_ui(egui::RawInput{time:Some(1.01),..Default::default()},|_|{finish(&ctx);});out.textures_delta.clear();
  assert_eq!(state(&ctx).lock().unwrap().layers.len(),1);
  let mut out=ctx.run_ui(egui::RawInput{time:Some(1.3),..Default::default()},|_|{finish(&ctx);});out.textures_delta.clear();
  assert!(state(&ctx).lock().unwrap().layers.is_empty());
 }

}

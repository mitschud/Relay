//! Offline model catalogue. Selecting a row never starts or stops an engine.
use crate::transitions::PopupFade;
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Profile { pub id: String, pub name: String, pub detail: String, pub settings: Value, pub required: Vec<PathBuf>, pub placeholder: bool, pub card: [String;4] }
impl Profile {
    pub fn missing(&self) -> bool { self.required.iter().any(|p| !p.is_file()) }
}
#[derive(Default)]
pub struct Picker { entries: Vec<Profile>, selected: String }
impl Picker {
    pub fn load() -> Self {
        let cfg=crate::config::load();
        let file=crate::config::path().with_file_name("models.json");
        let data: Value=std::fs::read_to_string(file).ok().and_then(|s|serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        Self::from_value(&data,cfg["model_profile_id"].as_str().unwrap_or(""))
    }
    fn from_value(data:&Value,selected:&str)->Self {
        let entries=data["profiles"].as_array().into_iter().flatten().filter_map(|v| {
            let name=v["name"].as_str()?;
            let detail=v["detail"].as_str().unwrap_or("");
            let quant=if v["card"]["quant_from_detail"].as_bool()==Some(true) {
                let token=detail.split('·').nth(1).unwrap_or("").split_whitespace().next().unwrap_or("");
                if token.is_empty(){String::new()}else if token.starts_with("UD-"){token.replace('_'," ")}else{format!("UD-{}",token.replace('_'," "))}
            } else {v["card"]["quant"].as_str().unwrap_or("").into()};
            Some(Profile{id:v["id"].as_str()?.into(),name:name.into(),detail:detail.into(),settings:v["settings"].as_object()?.clone().into(),placeholder:v["placeholder"].as_bool().unwrap_or(false),required:v["required"].as_array().into_iter().flatten().filter_map(|x|x.as_str().map(PathBuf::from)).collect(),card:[v["card"]["title"].as_str().unwrap_or(name).into(),v["card"]["subtitle"].as_str().unwrap_or("").into(),quant,match v["settings"]["backend"].as_str().unwrap_or(""){"strata"=>"Strata",other=>other}.into()]})
        }).collect::<Vec<_>>();
        let selected=if entries.iter().any(|p|p.id==selected) {selected.to_owned()}else{entries.first().map(|p|p.id.clone()).unwrap_or_default()};
        Self{entries,selected}
    }
    pub fn check(&self)->Result<(),String> {
        if let Some(p)=self.entries.iter().find(|p|p.id==self.selected) {
            if p.placeholder{return Err(format!("{} is a demo option; no model is installed.",p.name))}
            if p.missing(){return Err(format!("{} is still downloading or being prepared. Wait for verification to finish.",p.name))}
        }
        Ok(())
    }
    fn select(&mut self,id:&str)->Result<(),String> {
        let p=self.entries.iter().find(|p|p.id==id).ok_or("Unknown model profile")?;
        if p.placeholder {self.selected=id.into();return Ok(())}
        let mut cfg=crate::config::load();
        for (key,value) in p.settings.as_object().ok_or("Invalid model settings")? {cfg[key]=value.clone();}
        cfg["model_profile_id"]=id.into();
        crate::config::save(&serde_json::to_string(&cfg).map_err(|e|e.to_string())?)?;
        self.selected=id.into();Ok(())
    }
    /// One white Load pill with a dropdown section. Returns load request and selection change.
    pub fn control(&mut self,ui:&mut egui::Ui,enabled:bool,loading:bool,popup_right:Option<f32>,popup_top:Option<f32>)->(bool,bool,Option<String>) {
        if self.entries.is_empty() {
            let r=ui.add_enabled(enabled,egui::Button::new(RichText::new(if loading {"Loading"}else{"Load"}).color(Color32::from_gray(20))).min_size(egui::vec2(64.0,30.0)).corner_radius(15).fill(Color32::WHITE));
            return (r.clicked(),false,None)
        }
        let (rect,response)=ui.allocate_exact_size(egui::vec2(96.0,30.0),egui::Sense::hover());
        ui.painter().rect_filled(rect,15,Color32::WHITE);
        let split=rect.right()-32.0;
        let load=ui.interact(egui::Rect::from_min_max(rect.min,egui::pos2(split,rect.bottom())),response.id.with("load"),if enabled {egui::Sense::click()}else{egui::Sense::hover()});
        ui.painter().text(egui::pos2(rect.left()+31.0,rect.center().y),egui::Align2::CENTER_CENTER,if loading {"Loading"}else{"Load"},egui::FontId::proportional(12.0),Color32::from_gray(if enabled {20}else{100}));
        let arrow=ui.interact(egui::Rect::from_min_max(egui::pos2(split,rect.top()),rect.max),response.id.with("models"),if enabled {egui::Sense::click()}else{egui::Sense::hover()});
        if arrow.clicked(){*self=Self::load();}
        let c=arrow.rect.center();
        ui.painter().circle_filled(c,13.0,Color32::from_gray(if arrow.hovered(){215}else{239}));
        let stroke=egui::Stroke::new(1.2,Color32::from_gray(45));
        ui.painter().line_segment([c+egui::vec2(-3.0,1.5),c+egui::vec2(0.0,-1.5)],stroke);ui.painter().line_segment([c+egui::vec2(0.0,-1.5),c+egui::vec2(3.0,1.5)],stroke);
        if let Some(p)=self.entries.iter().find(|p|p.id==self.selected) {load.clone().on_hover_text(format!("Load {}",p.name));}
        let mut chosen=None;
        let columns=self.entries.len().clamp(1,2);
        let content_width=if columns==1 {116.0}else{240.0};
        egui::Popup::menu(&arrow).anchor(egui::Rect::from_min_size(egui::pos2(popup_right.unwrap_or(rect.right()+16.0)-96.0,popup_top.unwrap_or(rect.top())),egui::vec2(96.0,30.0))).align(egui::RectAlign::TOP_END).gap(16.0).width(content_width+20.0)
            .frame(egui::Frame::new().fill(Color32::from_rgba_unmultiplied(33,33,33,245)).corner_radius(18).inner_margin(10)
                .shadow(egui::epaint::Shadow{offset:[0,5],blur:20,spread:0,color:Color32::from_black_alpha(65)}))
            .fade_show(|ui| {
                ui.set_width(content_width);
                ui.set_max_width(content_width);
                ui.vertical_centered(|ui|{ui.label(RichText::new("Models").size(12.5).color(Color32::from_gray(210)));});
                ui.add_space(6.0);
                for row in self.entries.chunks(2) {
                    ui.horizontal(|ui|{
                        ui.spacing_mut().item_spacing.x=8.0;
                        let width=(content_width-8.0*(columns-1) as f32)/columns as f32;
                        for p in row {
                            let hint=if p.placeholder {format!("{} · demo only, not installed",p.detail)}else if p.missing(){format!("{} · preparing download",p.detail)}else{p.detail.clone()};
                            if model_card(ui,&p.card,p.id==self.selected,width).on_hover_text(hint).clicked(){chosen=Some(p.id.clone());}
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        let mut error=None;let changed=if let Some(id)=chosen {match self.select(&id){Ok(())=>true,Err(e)=>{error=Some(e);false}}}else{false};
        (load.clicked(),changed,error)
    }
}
fn model_card(ui:&mut egui::Ui,lines:&[String;4],selected:bool,width:f32)->egui::Response {
    let (rect,response)=ui.allocate_exact_size(egui::vec2(width,72.0),egui::Sense::click());
    let fill=if selected{49}else if response.hovered(){42}else{36};
    ui.painter().rect_filled(rect,14,Color32::from_gray(fill));
    ui.painter().rect_stroke(rect.shrink(0.5),14,egui::Stroke::new(1.0,if selected{crate::controls::ORANGE.gamma_multiply(0.45)}else{Color32::from_gray(49)}),egui::StrokeKind::Inside);
    for (i,line) in lines.iter().enumerate(){
        let (size,color)=match i {0=>(12.0,if selected{crate::controls::ORANGE}else{Color32::from_gray(210)}),1=>(10.5,Color32::from_gray(185)),2=>(10.5,Color32::from_gray(150)),_=>(10.0,Color32::from_gray(135))};
        ui.painter().text(egui::pos2(rect.center().x,rect.top()+15.0+i as f32*14.0),egui::Align2::CENTER_CENTER,line,egui::FontId::proportional(size),color);
    }
    if selected {
        let c=egui::pos2(rect.right()-13.0,rect.top()+8.0);let s=egui::Stroke::new(1.3,crate::controls::ORANGE);
        ui.painter().line_segment([c+egui::vec2(-3.0,0.0),c+egui::vec2(-0.5,2.5)],s);
        ui.painter().line_segment([c+egui::vec2(-0.5,2.5),c+egui::vec2(4.0,-2.0)],s);
    }
    response
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn selection_and_readiness_are_profile_specific(){
  let v=serde_json::json!({"profiles":[{"id":"a","name":"Flash","settings":{},"required":[]},{"id":"b","name":"Dense","settings":{},"required":["/missing/relay-model-marker"]}]});
  assert!(Picker::from_value(&v,"a").check().is_ok());assert!(Picker::from_value(&v,"b").check().is_err());assert_eq!(Picker::from_value(&v,"removed").selected,"a");
 }
}

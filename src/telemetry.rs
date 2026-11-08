//! Existing LACT daemon + RAPL; no extra daemon, NVIDIA context or UI-thread I/O.
use eframe::egui;
use serde_json::{Value, json};
use std::{fs, io::{Read, Write}, os::unix::net::UnixStream, sync::mpsc, time::{Duration, Instant}};
#[derive(Default, Clone, Debug)]
pub struct Sample { pub total: Option<f64>, pub temp: Option<f64>, pub clock: Option<f64> }
impl Sample {
    pub fn labels(&self) -> Vec<String> {
        let mut v=Vec::new();
        if let Some(x)=self.total {v.push(format!("{x:.0} W"));}
        if let Some(x)=self.temp {v.push(format!("VRAM {x:.0}°C"));}
        if let Some(x)=self.clock {v.push(format!("{x:.0} MHz"));}
        v
    }
}
struct Reading { sample: Sample, energy: Option<u64>, at: Instant }
pub struct Telemetry { pub latest: Sample, rx: Option<mpsc::Receiver<Reading>>, previous: Option<(u64, Instant)>, last: Instant }
impl Default for Telemetry { fn default()->Self {Self {latest:Sample::default(),rx:None,previous:None,last:Instant::now()-Duration::from_secs(3)}} }
fn number(name:&str)->Option<u64> {fs::read_to_string(format!("/sys/class/powercap/intel-rapl:0/{name}")).ok()?.trim().parse().ok()}
fn request(v:Value)->Option<Value> {
    let mut socket=UnixStream::connect("/run/lactd.sock").ok()?;
    socket.set_read_timeout(Some(Duration::from_millis(150))).ok()?;
    socket.set_write_timeout(Some(Duration::from_millis(150))).ok()?;
    writeln!(socket,"{v}").ok()?;
    let deadline=Instant::now()+Duration::from_millis(150);
    let mut data=Vec::new(); let mut buffer=[0;4096];
    while data.len()<65536 && Instant::now()<deadline {
        socket.set_read_timeout(Some(deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1)))).ok()?;
        let n=socket.read(&mut buffer).ok()?; if n==0 {break;}
        data.extend_from_slice(&buffer[..n]);
        if let Some(end)=data.iter().position(|b|*b==b'\n') {data.truncate(end); break;}
    }
    let v:Value=serde_json::from_slice(&data).ok()?;
    (v["status"]=="ok").then(||v["data"].clone())
}
fn finite(v:&Value)->Option<f64> {v.as_f64().filter(|x|x.is_finite() && *x>=0.0)}
fn delta(before:u64, after:u64, range:u64, seconds:f64)->Option<f64> {
    if range==0 || before>=range || after>=range || seconds<=0.0 || seconds>30.0 {return None;}
    Some((if after>=before {after-before}else{range-before+after}) as f64/1_000_000.0/seconds)
}
impl Telemetry {
    pub fn tick(&mut self, ctx:&egui::Context, visible:bool) {
        if let Some(rx)=&self.rx {match rx.try_recv() {
            Ok(mut r)=>{
                let gpu=r.sample.total.take();
                let cpu=self.previous.and_then(|(before,at)|delta(before,r.energy?,number("max_energy_range_uj")?,r.at.duration_since(at).as_secs_f64()));
                r.sample.total=cpu.zip(gpu).map(|(a,b)|a+b);
                self.previous=r.energy.map(|n|(n,r.at));self.latest=r.sample;self.rx=None;
            }, Err(mpsc::TryRecvError::Disconnected)=>self.rx=None, _=>{}
        }}
        if !visible {return;}
        ctx.request_repaint_after(Duration::from_secs(2));
        if self.rx.is_some() || self.last.elapsed()<Duration::from_secs(2) {return;}
        self.last=Instant::now();let (tx,rx)=mpsc::channel();self.rx=Some(rx);let ctx=ctx.clone();
        std::thread::spawn(move || {
            let stats=request(json!({"command":"list_devices"})).and_then(|v| {
                let id=v.as_array()?.iter().find_map(|d|d["id"].as_str().filter(|id|id.starts_with("10DE:")))?;
                request(json!({"command":"device_stats","args":{"id":id}}))
            }).unwrap_or(Value::Null);
            let sample=Sample{total:finite(&stats["power"]["current"]),temp:finite(&stats["temps"]["VRAM"]["current"]),clock:finite(&stats["clockspeed"]["vram_clockspeed"])};
            let _=tx.send(Reading{sample,energy:number("energy_uj"),at:Instant::now()});ctx.request_repaint();
        });
    }
}
#[cfg(test)] mod tests {use super::*;
    #[test] fn rapl_wrap_and_gaps(){assert_eq!(delta(900000,100000,1000000,2.0),Some(0.1));assert_eq!(delta(1,2,100,31.0),None);}
    #[test] fn missing_is_not_zero(){assert!(Sample::default().labels().is_empty());assert_eq!(finite(&Value::Null),None);}
}

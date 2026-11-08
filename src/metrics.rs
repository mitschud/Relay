//! Read Strata's own decode rate; no token estimates, subprocess sampler or extra dependency.
use eframe::egui;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct Metrics {
    pub latest: Value,
    rx: Option<mpsc::Receiver<(String, Value)>>,
    endpoint: String,
    last: Instant,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            latest: Value::Null,
            rx: None,
            endpoint: String::new(),
            last: Instant::now() - Duration::from_secs(3),
        }
    }
}
fn fetch(endpoint: &str) -> Option<Value> {
    // Default Strata endpoints are local HTTP. Unsupported HTTPS/auth proxies simply show no speed.
    let base = endpoint.strip_prefix("http://")?;
    let (authority, path) = base.split_once('/').unwrap_or((base, ""));
    if authority.contains('@') || authority.contains(['\r', '\n']) || path.contains(['\r', '\n']) {
        return None;
    }
    let address = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    let address = address.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    let prefix = path
        .trim_end_matches('/')
        .strip_suffix("v1")
        .unwrap_or(path)
        .trim_end_matches('/');
    let target = if prefix.is_empty() {
        "/metrics".to_owned()
    } else {
        format!("/{prefix}/metrics")
    };
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut bytes = Vec::new();
    stream.take(1024 * 1024).read_to_end(&mut bytes).ok()?;
    let split = bytes.windows(4).position(|w| w == b"\r\n\r\n")?;
    let headers = std::str::from_utf8(&bytes[..split]).ok()?;
    if !headers.lines().next()?.contains(" 200 ") {
        return None;
    }
    serde_json::from_slice(&bytes[split + 4..]).ok()
}
pub fn rate(v: &Value) -> Option<f64> {
    v["live"]["tok_s"]
        .as_f64()
        .or(v["live"]["tok_s_mean"].as_f64())
        .or(v["requests"][0]["decode_tok_s"].as_f64())
        .filter(|x| x.is_finite() && *x >= 0.0)
}
impl Metrics {
    pub fn tick(&mut self, ctx: &egui::Context, endpoint: Option<&str>, active: bool) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok((source, value)) => {
                    if endpoint == Some(source.as_str()) {
                        self.latest = value;
                    }
                    self.rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                _ => {}
            }
        }
        let Some(endpoint) = endpoint else {
            self.latest = Value::Null;
            self.endpoint.clear();
            return;
        };
        if !active {
            return;
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if self.rx.is_some()
            || (endpoint == self.endpoint && self.last.elapsed() < Duration::from_secs(2))
        {
            return;
        }
        self.last = Instant::now();
        self.endpoint = endpoint.to_owned();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let endpoint = endpoint.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let sample = fetch(&endpoint).unwrap_or(Value::Null);
            let _ = tx.send((endpoint, sample));
            ctx.request_repaint();
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_the_engine_rate_not_an_estimate() {
        let v = serde_json::json!({"live":{"tok_s":31.4},"requests":[{"decode_tok_s":27.0}]});
        assert_eq!(rate(&v), Some(31.4));
        assert_eq!(
            rate(&serde_json::json!({"live":{"tok_s":null},"requests":[{"decode_tok_s":27.0}]})),
            Some(27.0)
        );
        assert_eq!(rate(&Value::Null), None);
    }
    #[test]
    fn metric_read_uses_the_configured_endpoint_and_prefix() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 512];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 4096 {
                let n = socket.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /prefix/metrics HTTP/1.1"));
            let body = r#"{"live":{"tok_s":42.0}}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        assert_eq!(
            rate(&fetch(&format!("http://127.0.0.1:{port}/prefix/v1")).unwrap()),
            Some(42.0)
        );
        thread.join().unwrap();
    }
}

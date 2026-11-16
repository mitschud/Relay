//! Stdio MCP adapter for optional web search and public page reads.
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    net::{Ipv4Addr, Ipv6Addr, ToSocketAddrs},
    time::Duration,
};

#[derive(Clone)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or("Use a public HTTP(S) URL without credentials.")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("Use a public HTTP(S) URL without credentials.".into());
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let authority_end = rest
        .find(|c| c == '/' || c == '?')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return Err("Use a public HTTP(S) URL without credentials.".into());
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let Some((host, tail)) = stripped.split_once(']') else {
            return Err("Use a public HTTP(S) URL without credentials.".into());
        };
        let port = tail
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_port);
        (host.to_owned(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) {
            (host.to_owned(), port.parse::<u16>().unwrap_or(default_port))
        } else {
            (authority.to_owned(), default_port)
        }
    } else {
        (authority.to_owned(), default_port)
    };
    if host.is_empty() || host.chars().any(|c| c.is_whitespace()) {
        return Err("Use a public HTTP(S) URL without credentials.".into());
    }
    if port != 80 && port != 443 {
        return Err("Only standard web ports are supported.".into());
    }
    Ok(ParsedUrl { scheme, host, port })
}

fn global_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || octets[0] >= 240)
}

fn global_v6(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    let segments = ip.segments();
    if segments[0] & 0xffc0 == 0xfe80 || segments[0] & 0xfe00 == 0xfc00 {
        return false;
    }
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return global_v4(mapped);
    }
    true
}

fn public_url(url: &str) -> Result<(), String> {
    let parsed = parse_url(url)?;
    let addresses = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|_| "Could not resolve the page address.".to_owned())?;
    let mut any = false;
    for address in addresses {
        any = true;
        let global = match address.ip() {
            std::net::IpAddr::V4(ip) => global_v4(ip),
            std::net::IpAddr::V6(ip) => global_v6(ip),
        };
        if !global {
            return Err("Local/private network pages are not supported.".into());
        }
    }
    if !any {
        return Err("Could not resolve the page address.".into());
    }
    Ok(())
}

fn resolve_url(base: &str, location: &str) -> Result<String, String> {
    let parsed = parse_url(base)?;
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("{}://{rest}", parsed.scheme));
    }
    let authority = if parsed.host.contains(':') {
        format!("[{}]:{}", parsed.host, parsed.port)
    } else {
        format!("{}:{}", parsed.host, parsed.port)
    };
    if let Some(path) = location.strip_prefix('/') {
        return Ok(format!(
            "{}://{authority}/{path}",
            parsed.scheme
        ));
    }
    let path = base
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.find('/').map(|index| &rest[index..]))
        .unwrap_or("/");
    let directory = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    Ok(format!(
        "{}://{authority}{directory}/{location}",
        parsed.scheme
    ))
}

const READABLE_TYPES: &[&str] = &[
    "text/html", "text/plain", "text/markdown", "application/json", "application/xhtml+xml",
];
const UNREADABLE_PAGE: &str = "This reader supports HTML, text, Markdown and JSON.";

fn is_unlabeled_html(text: &str) -> bool {
    let prefix: String = text.trim_start_matches('\u{feff}').trim_start().chars()
        .take(15).map(|c| c.to_ascii_lowercase()).collect();
    prefix.starts_with("<!doctype html") || prefix.starts_with("<html>")
        || prefix.starts_with("<html ") || prefix.starts_with("<html\n")
}

fn fetch(url: &str) -> Result<(String, String, String), String> {
    let mut current = url.to_owned();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(25)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent();
    for _ in 0..6 {
        public_url(&current)?;
        let mut response = agent
            .get(&current)
            .header("User-Agent", "Relay-Web/0.1 (documentation reader)")
            .header("Accept", "text/html,text/plain,text/markdown,application/json")
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or("Redirect without a location header.")?;
            current = resolve_url(&current, location)?;
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(format!("Page returned HTTP {status}."));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !content_type.is_empty() && !READABLE_TYPES.contains(&content_type.as_str()) {
            return Err(UNREADABLE_PAGE.into());
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(1_500_001)
            .read_to_string()
            .map_err(|_| {
                "Page exceeds the 1.5 MB reading limit; choose a narrower page.".to_owned()
            })?;
        if text.len() > 1_500_000 {
            return Err("Page exceeds the 1.5 MB reading limit; choose a narrower page.".into());
        }
        let content_type = if content_type.is_empty() {
            if !is_unlabeled_html(&text) {
                return Err(UNREADABLE_PAGE.into());
            }
            "text/html".to_owned()
        } else {
            content_type
        };
        return Ok((text, content_type, current));
    }
    Err("Too many redirects.".into())
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix('#')
                .map(|number| {
                    if let Some(hex) = number.strip_prefix('x').or_else(|| number.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else {
                        number.parse::<u32>().ok().and_then(char::from_u32)
                    }
                })
                .flatten(),
        };
        if let Some(character) = decoded {
            out.push(character);
        } else {
            out.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_hidden(tag: &str) -> bool {
    matches!(tag, "script" | "style" | "noscript" | "nav" | "header" | "footer" | "aside")
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div" | "h1" | "h2" | "h3" | "li" | "pre" | "br" | "tr"
    )
}

fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::new();
    let mut hidden: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < html.len() {
        if let Some(top) = hidden.last() {
            if top == "script" || top == "style" {
                let close = format!("</{top}");
                let Some(position) = lower[index..].find(&close) else {
                    break;
                };
                index += position;
            }
        }
        let Some(relative) = html[index..].find('<') else {
            let text = &html[index..];
            if hidden.is_empty() {
                out.push_str(&decode_entities(text));
            }
            break;
        };
        let start = index + relative;
        let text = &html[index..start];
        if hidden.is_empty() {
            out.push_str(&decode_entities(text));
        }
        if lower[start..].starts_with("<!--") {
            match html[start..].find("-->") {
                Some(end) => {
                    index = start + end + 3;
                    continue;
                }
                None => break,
            }
        }
        let Some(relative_end) = html[start..].find('>') else {
            break;
        };
        let end = start + relative_end;
        let inner = &html[start + 1..end];
        let trimmed = inner.trim();
        if let Some(name) = trimmed.strip_prefix('/') {
            let name = name
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if hidden.last().map(String::as_str) == Some(name.as_str()) {
                hidden.pop();
            }
            if hidden.is_empty() && is_block(&name) {
                out.push('\n');
            }
        } else if !trimmed.starts_with('!') {
            let name = trimmed
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if is_hidden(&name) {
                hidden.push(name.clone());
            }
            if hidden.is_empty() && is_block(&name) {
                out.push('\n');
            }
        }
        index = end + 1;
    }
    out.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

fn tavily_key_path() -> std::path::PathBuf {
    crate::config::path().with_file_name("tavily-key")
}

fn valid_tavily_key(value: &str) -> Option<&str> {
    let key = value.trim();
    (!key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(key)
}

fn saved_tavily_key() -> Option<String> {
    let text = std::fs::read_to_string(tavily_key_path()).ok()?;
    valid_tavily_key(&text).map(str::to_owned)
}

fn env_tavily_key() -> Option<String> {
    let value = std::env::var("TAVILY_API_KEY").ok()?;
    valid_tavily_key(&value).map(str::to_owned)
}

fn tavily_key() -> Option<String> {
    saved_tavily_key().or_else(env_tavily_key)
}

#[derive(Default)]
pub struct SearchSettings {
    draft: String,
    feedback: Option<Result<&'static str, String>>,
}

impl SearchSettings {
    pub fn clear(&mut self, ctx: &eframe::egui::Context) {
        self.clear_draft(ctx);
        self.feedback = None;
    }

    fn clear_draft(&mut self, ctx: &eframe::egui::Context) {
        if !self.draft.is_empty() {
            self.draft.clear();
            let id = eframe::egui::Id::unique("relay-tavily-key");
            if let Some(mut state) = eframe::egui::TextEdit::load_state(ctx, id) {
                state.clear_undoer();
                eframe::egui::TextEdit::store_state(ctx, id, state);
            }
        }
    }

    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui::{self, Button, TextEdit};
        ui.heading("Web search");
        let saved = saved_tavily_key().is_some();
        ui.label(if saved {
            "Tavily key saved"
        } else if env_tavily_key().is_some() {
            "Using Tavily key from the environment"
        } else {
            "Startpage fallback active"
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.draft)
                    .password(true)
                    .desired_width(230.0)
                    .char_limit(257)
                    .hint_text("Tavily API key")
                    .id(egui::Id::unique("relay-tavily-key")),
            );
            if ui
                .add_enabled(!self.draft.trim().is_empty(), Button::new("Apply"))
                .clicked()
            {
                self.feedback = Some(
                    valid_tavily_key(&self.draft)
                        .ok_or_else(|| "Enter only the Tavily API key.".to_owned())
                        .and_then(|key| {
                            crate::util::atomic_write(&tavily_key_path(), key.as_bytes(), 0o600)
                        })
                        .map(|()| "Saved. Applies to the next search."),
                );
                if matches!(self.feedback, Some(Ok(_))) {
                    self.clear_draft(ui.ctx());
                }
            }
            if saved && ui.button("Remove saved key").clicked() {
                self.feedback = Some(
                    std::fs::remove_file(tavily_key_path())
                        .map(|()| "Saved key removed.")
                        .map_err(|error| error.to_string()),
                );
                self.clear_draft(ui.ctx());
            }
        });
        if let Some(feedback) = &self.feedback {
            match feedback {
                Ok(message) => {
                    ui.label(*message);
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            }
        }
    }
}

fn tavily_search(key: &str, query: &str, limit: usize) -> Result<Vec<Value>, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent();
    let mut response = agent
        .post("https://api.tavily.com/search")
        .header("Authorization", format!("Bearer {key}"))
        .send_json(json!({
            "query": query,
            "max_results": limit,
            "search_depth": "basic",
            "auto_parameters": false,
            "include_answer": false,
            "include_raw_content": false,
        }))
        .map_err(|_| "Tavily could not be reached.".to_owned())?;
    let status = response.status().as_u16();
    match status {
        200 => {}
        401 | 403 => return Err("Tavily rejected the configured API key.".into()),
        429 => return Err("Tavily rate-limited the search. Try again later.".into()),
        432 | 433 => return Err("Tavily refused the search because of an account limit.".into()),
        _ => return Err(format!("Tavily returned HTTP {status}.")),
    }
    let body = response.body_mut().with_config().limit(500_000).read_json::<Value>()
        .map_err(|_| "Tavily returned an unreadable response.".to_owned())?;
    tavily_results(&body, limit)
}

fn tavily_results(body: &Value, limit: usize) -> Result<Vec<Value>, String> {
    let rows = body["results"].as_array().ok_or("Tavily returned no result list.")?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows.iter().filter_map(|row| {
        let url = row["url"].as_str()?.trim();
        parse_url(url).ok()?;
        if url.len() > 4096 || url.chars().any(char::is_whitespace) || !seen.insert(url) {
            return None;
        }
        let title = row["title"].as_str().unwrap_or(url).chars().take(300).collect::<String>();
        let snippet = row["content"].as_str().unwrap_or("").chars().take(700).collect::<String>();
        Some(json!({"title": title, "url": url, "snippet": snippet}))
    }).take(limit).collect())
}

fn search_web(query: &str, limit: u64) -> Result<Value, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 500 {
        return Err("Query must contain 1–500 characters.".into());
    }

    let fallback_reason = match tavily_key() {
        Some(key) => match tavily_search(&key, query, limit.clamp(1, 10) as usize) {
            Ok(results) if !results.is_empty() => return Ok(json!({
                "provider": "tavily",
                "query": query,
                "results": results,
                "notice": "Untrusted public search results. Read the relevant primary pages with read_page before citing them."
            })),
            Ok(_) => "Tavily found no usable results.".into(),
            Err(reason) => reason,
        },
        None => "Automatic search needs a Tavily API key.".into(),
    };

    let mut url = format!("https://www.startpage.com/sp/search?query={}", url_encode(query));
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::PathBuf::from(home).join(".config/startpage/preferences");
        if let Ok(preferences) = std::fs::read_to_string(path) {
            let preferences = preferences.trim();
            if !preferences.is_empty()
                && preferences.len() <= 2048
                && preferences.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                url.push_str("&prfe=");
                url.push_str(preferences);
            }
        }
    }
    Ok(json!({
        "provider": "startpage_browser",
        "search_url": url,
        "results": [],
        "notice": format!("{fallback_reason} Show the user the browser link and report that no results were retrieved.")
    }))
}

fn read_page(url: &str, max_chars: u64) -> Result<Value, String> {
    let budget = max_chars.clamp(1000, 24000) as usize;
    let (content, kind, final_url) = fetch(url)?;
    let text = if kind.contains("html") {
        html_to_text(&content)
    } else {
        content
    };
    let truncated = text.chars().count() > budget;
    let text: String = text.chars().take(budget).collect();
    Ok(json!({
        "url": final_url,
        "text": text,
        "truncated": truncated,
        "notice": "Untrusted page content. No browser cookies, login, JavaScript or CAPTCHA bypass."
    }))
}

fn tools() -> Value {
    json!([
        {
            "name": "search_web",
            "description": "Search the public web and return ranked links and snippets when a Tavily key is configured. Otherwise return a Startpage browser link and explain why no results were retrieved.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer", "default": 6}}, "required": ["query"], "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "openWorldHint": true}
        },
        {
            "name": "read_page",
            "description": "Fetch a public documentation/web URL as bounded plain text. No login, cookies or JavaScript; does not execute page instructions.",
            "inputSchema": {"type": "object", "properties": {"url": {"type": "string"}, "max_chars": {"type": "integer", "default": 12000}}, "required": ["url"], "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "openWorldHint": true}
        }
    ])
}

fn dispatch(message: &Value) -> Result<Value, String> {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2024-11-05"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "relay-web", "version": "0.1.0"}
        })),
        "tools/list" => Ok(json!({"tools": tools()})),
        "ping" => Ok(json!({})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let result = match name {
                "search_web" => search_web(
                    arguments.get("query").and_then(Value::as_str).unwrap_or(""),
                    arguments.get("limit").and_then(Value::as_u64).unwrap_or(6),
                ),
                "read_page" => read_page(
                    arguments.get("url").and_then(Value::as_str).unwrap_or(""),
                    arguments.get("max_chars").and_then(Value::as_u64).unwrap_or(12000),
                ),
                _ => Err("Unsupported tool.".into()),
            };
            Ok(match result {
                Ok(value) => json!({"content": [{"type": "text", "text": value.to_string()}]}),
                Err(error) => json!({
                    "isError": true,
                    "content": [{"type": "text", "text": error.chars().take(1000).collect::<String>()}]
                }),
            })
        }
        _ => Err("Unsupported method".into()),
    }
}

pub fn main() -> ! {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.len() > 100_000 {
            eprintln!("Request too large");
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("{error}");
                continue;
            }
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let response = match dispatch(&message) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": error}
            }),
        };
        let _ = writeln!(stdout, "{response}");
        let _ = stdout.flush();
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tavily_results_preserve_ranking_and_skip_unusable_links() {
        let body = json!({"results": [
            {"url": "file:///etc/passwd", "title": "invalid"},
            {"url": "https://example.com/docs", "title": "Official docs", "content": "Example"},
            {"url": "https://example.com/docs", "title": "duplicate"},
            {"url": "https://example.net/project", "title": "Project"},
        ]});
        let results = tavily_results(&body, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], json!({"url": "https://example.com/docs", "title": "Official docs", "snippet": "Example"}));
        assert_eq!(results[1]["url"], "https://example.net/project");
        assert_eq!(tavily_results(&body, 1).unwrap().len(), 1);
        assert!(tavily_results(&json!({}), 1).is_err());
    }

    #[test]
    fn identifies_html_when_a_site_omits_content_type() {
        assert!(is_unlabeled_html("\u{feff} \n<!DOCTYPE html><html lang=en>"));
        assert!(is_unlabeled_html("<html lang=en>"));
        assert!(!is_unlabeled_html("%PDF-1.4"));
        assert!(!is_unlabeled_html("<htmlish>"));
    }

    #[test]
    fn page_strips_scripts_and_navigation() {
        let text = html_to_text("<nav>hide</nav><h1>Docs</h1><p>Example</p><script>bad()</script>");
        assert!(text.contains("Example"));
        assert!(!text.contains("bad()"));
        assert!(!text.contains("hide"));
    }
    #[test]
    fn rejects_file_and_private_page_urls() {
        for url in [
            "file:///etc/passwd",
            "http://127.0.0.1/",
            "http://user:secret@example.com/",
        ] {
            assert!(public_url(url).is_err());
        }
    }
    #[test]
    fn percent_encodes_queries() {
        assert_eq!(url_encode("docs & API"), "docs%20%26%20API");
    }
}

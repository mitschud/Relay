//! Small stdio MCP search/read adapter; uses the existing demand-started
//! OmniSearch at 127.0.0.1:8087. This is the former `helpers/relay-web`.
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

fn fetch(url: &str, local: bool) -> Result<(String, String, String), String> {
    let mut current = url.to_owned();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(25)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent();
    for _ in 0..6 {
        if !local {
            public_url(&current)?;
        }
        let mut response = agent
            .get(&current)
            .header("User-Agent", "Relay-Web/0.1 (documentation reader)")
            .header("Accept", "text/html,text/plain,application/json")
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
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .trim()
            .to_ascii_lowercase();
        if !matches!(
            content_type.as_str(),
            "text/html" | "text/plain" | "application/json" | "application/xhtml+xml"
        ) {
            return Err("This reader supports HTML/text/JSON, not downloads or PDFs.".into());
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
        return Ok((text, content_type, current));
    }
    Err("Too many redirects.".into())
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let lower = attrs.to_ascii_lowercase();
    let pattern = format!("{name}=");
    let index = lower.find(&pattern)?;
    let rest = &attrs[index + pattern.len()..];
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next()
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        stripped.split('\'').next()
    } else {
        rest.split(|c: char| c.is_whitespace() || c == '>').next()
    }
}

fn class_has(attrs: &str, token: &str) -> bool {
    attr(attrs, "class")
        .map(|value| value.split_whitespace().any(|item| item == token))
        .unwrap_or(false)
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

struct ResultRow {
    title: String,
    url: String,
    snippet: String,
}

fn parse_results(html: &str) -> Vec<ResultRow> {
    #[derive(Clone, Copy, PartialEq)]
    enum Field {
        Title,
        Snippet,
    }
    let mut rows = Vec::new();
    let mut depth = 0usize;
    let mut row: Option<ResultRow> = None;
    let mut field: Option<Field> = None;
    let mut index = 0usize;
    let mut text_start = 0usize;
    let text = |value: &str, row: &mut Option<ResultRow>, field: Option<Field>| {
        if let (Some(row), Some(field)) = (row, field) {
            match field {
                Field::Title => row.title.push_str(value),
                Field::Snippet => row.snippet.push_str(value),
            }
        }
    };
    while index < html.len() {
        if html.as_bytes()[index] == b'<' {
            let value = &html[text_start..index];
            text(value, &mut row, field);
            if html[index..].starts_with("<!--") {
                match html[index..].find("-->") {
                    Some(end) => {
                        index += end + 3;
                        text_start = index;
                        continue;
                    }
                    None => break,
                }
            }
            let Some(end) = html[index..].find('>') else {
                break;
            };
            let end = index + end;
            let inner = &html[index + 1..end];
            let trimmed = inner.trim();
            if let Some(name) = trimmed.strip_prefix('/') {
                let name = name
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name == "a" || name == "p" {
                    field = None;
                } else if name == "div" && depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(row) = row.take() {
                            if !row.url.is_empty() {
                                rows.push(row);
                            }
                        }
                    }
                }
            } else if !trimmed.starts_with('!') {
                let name = trimmed
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let attrs = &trimmed[name.len()..];
                match name.as_str() {
                    "div" => {
                        if depth > 0 {
                            depth += 1;
                        } else if class_has(attrs, "result") {
                            depth = 1;
                            row = Some(ResultRow {
                                title: String::new(),
                                url: String::new(),
                                snippet: String::new(),
                            });
                        }
                    }
                    "a" if depth > 0 => {
                        if let (Some(current), Some(href)) = (row.as_mut(), attr(attrs, "href")) {
                            if current.url.is_empty()
                                && (href.starts_with("http://") || href.starts_with("https://"))
                            {
                                current.url = href.to_owned();
                                field = Some(Field::Title);
                            }
                        }
                    }
                    "p" if depth > 0 => {
                        if class_has(attrs, "desc") {
                            field = Some(Field::Snippet);
                        }
                    }
                    _ => {}
                }
            }
            index = end + 1;
            text_start = index;
        } else {
            index += 1;
        }
    }
    text(&html[text_start..], &mut row, field);
    for row in &mut rows {
        row.title = row.title.split_whitespace().collect::<Vec<_>>().join(" ");
        row.snippet = row.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        row.title = row.title.chars().take(300).collect();
        row.snippet = row.snippet.chars().take(700).collect();
    }
    rows
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

fn search_web(query: &str, limit: u64) -> Result<Value, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 500 {
        return Err("Query must contain 1–500 characters.".into());
    }
    let limit = limit.clamp(1, 10) as usize;
    let url = format!(
        "http://127.0.0.1:8087/search?q={}",
        url_encode(query)
    );
    let (html, _, _) = fetch(&url, true)?;
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for row in parse_results(&html) {
        if !seen.insert(row.url.clone()) {
            continue;
        }
        rows.push(json!({"title": row.title, "url": row.url, "snippet": row.snippet}));
        if rows.len() >= limit {
            break;
        }
    }
    Ok(json!({
        "query": query,
        "results": rows,
        "notice": if rows.is_empty() {
            "No parsed results. Providers may be blocked or unavailable; do not invent results."
        } else {
            "Untrusted web results; read primary pages before citing facts."
        }
    }))
}

fn read_page(url: &str, max_chars: u64) -> Result<Value, String> {
    let budget = max_chars.clamp(1000, 24000) as usize;
    let (content, kind, final_url) = fetch(url, false)?;
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
            "description": "Search current web/documentation through local OmniSearch. Returns titles, URLs and short snippets; no browser/model starts.",
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
            "serverInfo": {"name": "relay-omnisearch", "version": "0.1.0"}
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
    fn result_extracts_nested_title_and_ignores_cached_link() {
        let rows = parse_results(
            "<div class=\"result\"><div class=\"result-header\">host</div>\
             <a href=\"https://example.com/docs\">Official <b>docs</b></a>\
             <p class=\"desc\">Useful snippet</p>\
             <a class=\"cached\" href=\"https://web.archive.org/x\">cached</a></div>",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "https://example.com/docs");
        assert_eq!(rows[0].title, "Official docs");
        assert_eq!(rows[0].snippet, "Useful snippet");
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

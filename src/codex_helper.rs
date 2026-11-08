//! Compiled Codex agent updater. This is the former `helpers/relay-codex`
//! Python script. Download, SHA256 verification and archive extraction are now
//! in-process; no Python, tar or curl binary is required.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const FEED: &str = "https://api.github.com/repos/openai/codex/releases/latest";

fn home() -> PathBuf {
    crate::util::home()
}
fn config_path() -> PathBuf {
    home().join(".config/relay/settings.json")
}
fn root() -> PathBuf {
    home().join(".local/share/relay/codex-agent")
}
fn lock_path() -> PathBuf {
    home().join(".local/state/relay/codex-agent.lock")
}

fn settings() -> Value {
    crate::util::read_json_file(&config_path()).unwrap_or_else(|_| json!({}))
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PrePart {
    Text(String),
    Number(u64),
}
impl Ord for PrePart {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Number(_), Self::Text(_)) => Ordering::Greater,
            (Self::Text(_), Self::Number(_)) => Ordering::Less,
        }
    }
}
impl PartialOrd for PrePart {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    core: (u64, u64, u64),
    stable: bool,
    pre: Vec<PrePart>,
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| self.stable.cmp(&other.stable))
            .then_with(|| self.pre.cmp(&other.pre))
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Version {
    fn text(&self) -> String {
        let mut text = format!("{}.{}.{}", self.core.0, self.core.1, self.core.2);
        if !self.stable {
            text.push('-');
            text.push_str(
                &self
                    .pre
                    .iter()
                    .map(|part| match part {
                        PrePart::Text(text) => text.clone(),
                        PrePart::Number(number) => number.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("."),
            );
        }
        text
    }
}

fn parse_number(bytes: &[u8], text: &str, index: &mut usize) -> Option<u64> {
    let begin = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    (*index > begin)
        .then(|| text[begin..*index].parse().ok())
        .flatten()
}

fn parse_version(text: &str) -> Option<Version> {
    let bytes = text.as_bytes();
    let mut start = None;
    for (index, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() {
            start = Some(index);
            break;
        }
    }
    let mut index = start?;
    let major = parse_number(bytes, text, &mut index)?;
    if bytes.get(index) != Some(&b'.') {
        return None;
    }
    index += 1;
    let minor = parse_number(bytes, text, &mut index)?;
    if bytes.get(index) != Some(&b'.') {
        return None;
    }
    index += 1;
    let patch = parse_number(bytes, text, &mut index)?;
    let mut pre = Vec::new();
    let mut stable = true;
    if bytes.get(index) == Some(&b'-') {
        stable = false;
        index += 1;
        let begin = index;
        while bytes.get(index).is_some_and(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
        }) {
            index += 1;
        }
        let text = &text[begin..index];
        if !text.is_empty() {
            for part in text.split('.') {
                if part.is_empty() {
                    continue;
                }
                if let Ok(number) = part.parse::<u64>() {
                    pre.push(PrePart::Number(number));
                } else {
                    pre.push(PrePart::Text(part.to_owned()));
                }
            }
        }
    }
    Some(Version {
        core: (major, minor, patch),
        stable,
        pre,
    })
}

fn exact_release_tag(tag: &str) -> Option<Version> {
    let rest = tag.strip_prefix("rust-v")?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version {
        core: (major, minor, patch),
        stable: true,
        pre: Vec::new(),
    })
}

fn installed(cfg: &Value) -> Value {
    let configured = cfg
        .get("codex_binary")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("codex");
    let Some(binary) = crate::util::which(configured) else {
        return json!({"path": configured, "version": null});
    };
    let output = Command::new(&binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output();
    let version = output
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| parse_version(&String::from_utf8_lossy(&out.stdout)).map(|v| v.text()));
    json!({"path": binary.display().to_string(), "version": version})
}

fn http_get(url: &str) -> Result<ureq::Body, String> {
    let response = crate::http::agent(Duration::from_secs(30))
        .get(url)
        .header("User-Agent", "Relay-Codex-Updater")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status} from {url}"));
    }
    Ok(response.into_body())
}

fn release() -> Result<Value, String> {
    let mut body = http_get(FEED)?;
    let data: Value = body.read_json().map_err(|e| e.to_string())?;
    let tag = data
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("The official feed did not return a release tag")?;
    if data.get("prerelease").and_then(Value::as_bool) == Some(true)
        || data.get("draft").and_then(Value::as_bool) == Some(true)
    {
        return Err("The official stable feed did not return a stable CLI release".into());
    }
    let version = exact_release_tag(tag)
        .ok_or("The official stable feed did not return a stable CLI release")?;
    if !cfg!(target_os = "linux") {
        return Err("Managed agent downloads support Linux only".into());
    }
    let machine = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return Err("Managed agent downloads support Linux x86_64 and aarch64".into()),
    };
    let name = format!("codex-{machine}-unknown-linux-musl.tar.gz");
    let asset = data
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset.get("name").and_then(Value::as_str) == Some(name.as_str())
            })
        })
        .ok_or("No matching official release asset with a SHA256 digest")?;
    let digest = asset.get("digest").and_then(Value::as_str).unwrap_or_default();
    if !digest
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("No matching official release asset with a SHA256 digest".into());
    }
    let url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or("Release asset has no download URL")?;
    let expected = format!("https://github.com/openai/codex/releases/download/{tag}/{name}");
    if url != expected {
        return Err("Unexpected release download URL".into());
    }
    Ok(json!({
        "version": version.text(),
        "asset": {
            "name": name,
            "size": asset.get("size").cloned().unwrap_or(Value::Null),
            "digest": digest,
            "browser_download_url": url
        },
        "url": format!("https://github.com/openai/codex/releases/tag/{tag}")
    }))
}

fn status(cfg: &Value) -> Value {
    let mut result = json!({"installed": installed(cfg)});
    if let Ok(saved) = crate::util::read_json_file(&root().join("state.json")) {
        let rollback = saved.get("previous_binary").and_then(Value::as_str).is_some()
            && cfg.get("codex_binary").and_then(Value::as_str)
                == saved.get("active_binary").and_then(Value::as_str);
        result["rollback_available"] = json!(rollback);
    }
    result
}

fn check(cfg: &Value) -> Result<Value, String> {
    let mut result = status(cfg);
    let offered = release()?;
    let current = result["installed"]["version"].as_str();
    let current = current.and_then(parse_version);
    let available = current.is_none()
        || parse_version(offered["version"].as_str().unwrap_or_default())
            .zip(current)
            .is_some_and(|(offered, current)| offered > current);
    result["offered"] = offered["version"].clone();
    result["download_bytes"] = offered["asset"]["size"].clone();
    result["url"] = offered["url"].clone();
    result["available"] = json!(available);
    Ok(result)
}

fn unpack(archive: &Path, target: &Path, machine: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let expected = format!("codex-{machine}-unknown-linux-musl");
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut bundle = tar::Archive::new(decoder);
    for entry in bundle.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let is_file = entry.header().entry_type().is_file();
        if !is_file || path.as_os_str() != std::ffi::OsStr::new(&expected) {
            continue;
        }
        if entry.size() > 600 * 1024 * 1024 {
            return Err("Unexpected Codex archive layout".into());
        }
        let mut output = std::fs::File::create(target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err("Unexpected Codex archive layout".into())
}

fn install(cfg: &Value, expected: &str) -> Result<Value, String> {
    let offered = release()?;
    if offered["version"].as_str() != Some(expected) {
        return Err("The release changed. Check for updates again before installing.".into());
    }
    let old = installed(cfg);
    if let (Some(offered_version), Some(old_version)) = (
        parse_version(offered["version"].as_str().unwrap_or_default()),
        old["version"].as_str().and_then(parse_version),
    ) {
        if offered_version <= old_version {
            return Err("The configured agent is already at this release or newer".into());
        }
    }
    let download = root().join(format!(".download-{}", std::process::id()));
    std::fs::create_dir_all(&download).map_err(|e| e.to_string())?;
    let result = (|| -> Result<Value, String> {
        let archive = download.join("release.tar.gz");
        let candidate = download.join("codex");
        let asset = &offered["asset"];
        let expected_size = asset["size"].as_u64().unwrap_or(0);
        let expected_digest = asset["digest"].as_str().unwrap_or_default();
        let url = asset["browser_download_url"]
            .as_str()
            .ok_or("Release asset has no download URL")?;
        let response = crate::http::agent(Duration::from_secs(120))
            .get(url)
            .header("User-Agent", "Relay-Codex-Updater")
            .call()
            .map_err(|e| e.to_string())?;
        if response.status().as_u16() != 200 {
            return Err(format!("HTTP {} from release download", response.status()));
        }
        let mut reader = response.into_body().into_reader();
        let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            size += read as u64;
            if size > expected_size {
                return Err("Release download exceeds its published size".into());
            }
            digest.update(&buffer[..read]);
            file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        }
        file.sync_all().map_err(|e| e.to_string())?;
        let actual = format!("sha256:{:x}", digest.finalize());
        if size != expected_size || actual != expected_digest {
            return Err("Release size or SHA256 verification failed".into());
        }
        let machine = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            _ => return Err("Managed agent downloads support Linux x86_64 and aarch64".into()),
        };
        unpack(&archive, &candidate, machine)?;
        let candidate_version = installed(&json!({"codex_binary": candidate.display().to_string()}));
        if candidate_version["version"].as_str() != offered["version"].as_str() {
            return Err("Downloaded agent version does not match the selected release".into());
        }
        let help = Command::new(&candidate)
            .args(["app-server", "--help"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !help.success() {
            return Err("Downloaded agent does not support app-server".into());
        }
        let version = offered["version"].as_str().unwrap_or("unknown");
        let destination = root().join(version).join("codex");
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&candidate, &destination).map_err(|e| e.to_string())?;
        let latest = settings();
        if latest.get("codex_binary").and_then(Value::as_str).unwrap_or("codex")
            != cfg.get("codex_binary").and_then(Value::as_str).unwrap_or("codex")
        {
            return Err("Agent location changed during download; recheck before activating".into());
        }
        crate::util::atomic_write_json(
            &(root().join("state.json")),
            &json!({
                "active_binary": destination.display().to_string(),
                "previous_binary": cfg.get("codex_binary").and_then(Value::as_str).unwrap_or("codex")
            }),
        )?;
        let mut updated = latest;
        updated["codex_binary"] = json!(destination.display().to_string());
        crate::util::atomic_write_json(&config_path(), &updated)?;
        Ok(merged(status(&updated), json!({"message": "Agent installed. It will be used on the next connection."})))
    })();
    let _ = std::fs::remove_dir_all(&download);
    result
}

fn rollback(cfg: &Value) -> Result<Value, String> {
    let saved = crate::util::read_json_file(&root().join("state.json"))?;
    if cfg.get("codex_binary").and_then(Value::as_str)
        != saved.get("active_binary").and_then(Value::as_str)
    {
        return Err("The configured agent has changed; no rollback applied".into());
    }
    let previous = saved
        .get("previous_binary")
        .and_then(Value::as_str)
        .ok_or("No previous agent is recorded")?;
    if installed(&json!({"codex_binary": previous}))["version"]
        .as_str()
        .is_none()
    {
        return Err("The previous agent is no longer installed".into());
    }
    let mut updated = cfg.clone();
    updated["codex_binary"] = json!(previous);
    crate::util::atomic_write_json(&config_path(), &updated)?;
    Ok(merged(status(&updated), json!({"message": "Previous agent restored for the next connection."})))
}

/// Shallow object merge; `extra` wins on duplicate keys.
fn merged(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        for (key, value) in extra {
            base.insert(key, value);
        }
    }
    base
}

pub fn main(args: &[String]) -> ! {
    let Some(action) = args.first().cloned() else {
        eprintln!("usage: relay __codex <status|check|install|rollback> [--version VERSION]");
        std::process::exit(2);
    };
    let version = args
        .iter()
        .position(|arg| arg == "--version")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let result = (|| -> Result<Value, String> {
        let cfg = settings();
        match action.as_str() {
            "status" => Ok(merged(json!({"ok": true}), status(&cfg))),
            "check" => Ok(merged(json!({"ok": true}), check(&cfg)?)),
            "install" | "rollback" => {
                let _lock = crate::util::lock_exclusive_nonblocking(&lock_path()).map_err(|_| {
                    "Disconnect the Relay agent before changing its installation".to_owned()
                })?;
                if action == "install" {
                    Ok(merged(json!({"ok": true}), install(&cfg, version.as_deref().unwrap_or_default())?))
                } else {
                    Ok(merged(json!({"ok": true}), rollback(&cfg)?))
                }
            }
            _ => Err(format!("Unknown action: {action}")),
        }
    })();
    match result {
        Ok(value) => {
            println!("{}", value);
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        Err(error) => {
            println!("{}", json!({"ok": false, "error": error}));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_ordering_matches_release_rules() {
        assert!(parse_version("0.167.0").unwrap() > parse_version("0.166.0-alpha.5.1").unwrap());
        assert!(parse_version("0.167.1").unwrap() > parse_version("0.167.0").unwrap());
        assert_eq!(parse_version("rust-v0.167.0").unwrap().text(), "0.167.0");
    }
    #[test]
    fn release_tags_must_be_exact() {
        assert!(exact_release_tag("rust-v0.167.0").is_some());
        assert!(exact_release_tag("rust-v0.167.0-alpha.1").is_none());
        assert!(exact_release_tag("v0.167.0").is_none());
    }
}

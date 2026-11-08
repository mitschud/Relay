//! Wayland clipboard image/URI-list import compiled into Relay. Replaces the
//! `wl-paste` dependency used by the old Python helper.
use serde_json::json;
use std::{
    collections::HashSet,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
};

const MAX_BYTES: usize = 32 * 1024 * 1024;

fn home() -> PathBuf {
    crate::util::home()
}

fn read_with_timeout<R: Read + AsRawFd>(
    reader: &mut R,
    timeout_ms: i32,
) -> Result<Vec<u8>, String> {
    let mut pollfd = libc::pollfd {
        fd: reader.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if ready <= 0 {
        return Err("Clipboard read timed out".into());
    }
    let mut data = Vec::new();
    reader
        .by_ref()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > MAX_BYTES {
        return Err("Clipboard item exceeds the 32 MiB limit".into());
    }
    Ok(data)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &text[index + 1..index + 3];
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                output.push(value);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn local_files(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("file://") else {
            continue;
        };
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => ("", ""),
        };
        if !authority.is_empty() && authority != "localhost" {
            continue;
        }
        let path = PathBuf::from(percent_decode(path));
        if path.is_file() {
            let path = path.display().to_string();
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }
        if files.len() >= 16 {
            break;
        }
    }
    files
}

fn save_image(bytes: &[u8], extension: &str) -> Result<String, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let directory = home().join(".local/share/relay/attachments");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let permissions = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(&directory, permissions).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = directory.join(format!(
        "Screenshot-{}-{stamp}.{extension}",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(&path);
        return Err(error.to_string());
    }
    Ok(path.display().to_string())
}

fn capture() -> Result<Vec<String>, String> {
    use wl_clipboard_rs::paste::{ClipboardType, MimeType, Seat, get_contents, get_mime_types};
    let types = get_mime_types(ClipboardType::Regular, Seat::Unspecified)
        .map_err(|e| e.to_string())?;
    if types.is_empty() {
        return Ok(Vec::new());
    }
    for (mime, extension) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
    ] {
        if !types.contains(mime) {
            continue;
        }
        let (mut reader, _) = get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(mime),
        )
        .map_err(|e| e.to_string())?;
        let bytes = read_with_timeout(&mut reader, 5000)?;
        if bytes.is_empty() {
            return Err("Clipboard image is empty".into());
        }
        return save_image(&bytes, extension).map(|path| vec![path]);
    }
    if types.contains("text/uri-list") {
        let (mut reader, _) = get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific("text/uri-list"),
        )
        .map_err(|e| e.to_string())?;
        let bytes = read_with_timeout(&mut reader, 3000)?;
        return Ok(local_files(&String::from_utf8_lossy(&bytes)));
    }
    Ok(Vec::new())
}

pub fn main() -> ! {
    match capture() {
        Ok(files) => {
            println!("{}", json!({"files": files}));
            std::process::exit(0);
        }
        Err(error) => {
            println!("{}", json!({"files": [], "error": error}));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uri_lists_are_filtered_and_deduplicated() {
        let dir = std::env::temp_dir().join(format!("relay-uri-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a b.txt");
        std::fs::write(&file, b"x").unwrap();
        let uri = format!("file://{}", percent_decode(&file.display().to_string()).replace(' ', "%20"));
        let files = local_files(&format!("# comment\n{uri}\n{uri}\nfile:///does/not/exist\n"));
        assert_eq!(files.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn percent_decoding_handles_utf8() {
        assert_eq!(percent_decode("a%20b"), "a b");
    }
}

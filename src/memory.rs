use std::path::PathBuf;
pub fn path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/relay/MEMORY.md")
}
pub fn read() -> String {
    let p = path();
    if !p.exists() {
        let _ = save(DEFAULT);
    }
    std::fs::read_to_string(p).unwrap_or_default()
}
pub fn save(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let p = path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let temp = p.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(temp, p)
}
const DEFAULT: &str = r#"# Persistent memory

Record durable facts and preferences explicitly requested to be remembered. Verify current system state before making changes. Never store credentials.
"#;

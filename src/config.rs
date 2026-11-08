use serde_json::{Value, json};
use std::path::PathBuf;
pub fn path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join(".config/relay/settings.json")
}
pub fn load() -> Value {
    std::fs::read_to_string(path()).ok().and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_else(||json!({"base_url":"http://127.0.0.1:47831/v1","alternate_urls":[],"codex_binary":"codex","start_command":[],"stop_command":[],"preferred_model":"","load_timeout_s":600}))
}
pub fn save(text: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let base = v["base_url"].as_str().ok_or("base_url must be a URL")?;
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err("Use an HTTP or HTTPS endpoint".into());
    }
    for key in ["start_command", "stop_command", "strata_command"] {
        if let Some(a) = v.get(key) {
            if !a.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
                return Err(format!("{key} must be an array of command arguments"));
            }
        }
    }
    std::fs::create_dir_all(path().parent().unwrap()).map_err(|e| e.to_string())?;
    let temp=path().with_extension(format!("json.{}.tmp",std::process::id()));
    std::fs::write(&temp,serde_json::to_string_pretty(&v).unwrap()).map_err(|e|e.to_string())?;
    std::fs::rename(&temp,path()).map_err(|e|e.to_string())
}

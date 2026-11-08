//! Small HTTP helpers shared by the engine supervisor, updaters and web MCP.
//! All requests are compiled into Relay; no curl/wget process is used.
use serde_json::Value;
use std::time::Duration;

pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

pub fn get_json(url: &str, timeout: Duration) -> Result<Value, String> {
    let mut response = agent(timeout)
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status} from {url}"));
    }
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|e| e.to_string())
}


//! UI state follows Codex's native compaction lifecycle; Relay never trims history.
use serde_json::Value;

#[derive(Default)]
pub struct State {
    pub pending: bool,
    pub active: bool,
    pub awaiting_usage: bool,
    usage_reported: bool,
}
impl State {
    pub fn begin(&mut self) {
        self.pending = true;
    }
    pub fn running(&self) -> bool {
        self.pending || self.active
    }
    pub fn cancel(&mut self) {
        self.pending = false;
        self.active = false;
    }
    pub fn observe(&mut self, method: &str, params: &Value) {
        match method {
            "item/started" if params["item"]["type"] == "contextCompaction" => {
                self.pending = false;
                self.active = true;
                self.usage_reported = false;
            }
            "item/completed" if params["item"]["type"] == "contextCompaction" => {
                self.cancel();
                self.awaiting_usage = !self.usage_reported;
            }
            "thread/compacted" => {
                self.cancel();
                self.awaiting_usage = !self.usage_reported;
            }
            "thread/tokenUsage/updated" => {
                self.usage_reported = true;
                self.awaiting_usage = false;
            }
            "turn/completed" | "error" => self.cancel(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_lifecycle_waits_for_real_usage_and_accepts_updates_in_either_order() {
        let item = json!({"item":{"type":"contextCompaction"}});
        let mut s = State::default();
        s.begin();
        assert!(s.running());
        s.observe("item/started", &item);
        s.observe("item/completed", &item);
        assert!(!s.running());
        assert!(s.awaiting_usage);
        s.observe("thread/tokenUsage/updated", &json!({}));
        assert!(!s.awaiting_usage);
        s.observe("item/started", &item);
        s.observe("thread/tokenUsage/updated", &json!({}));
        s.observe("item/completed", &item);
        assert!(!s.awaiting_usage);
    }
    #[test]
    fn normal_tool_events_do_not_start_compaction_and_errors_release_controls() {
        let mut s = State::default();
        s.observe("item/started", &json!({"item":{"type":"commandExecution"}}));
        assert!(!s.running());
        s.begin();
        s.observe("error", &json!({}));
        assert!(!s.running());
    }
}

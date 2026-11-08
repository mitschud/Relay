use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    pub images: Vec<String>,
    pub status: String,
}

pub fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_owned()
}
pub fn item(v: &Value) -> Item {
    let kind = string(v, "type");
    let images = if kind == "userMessage" {
        v["content"].as_array().into_iter().flatten()
            .filter(|x| matches!(x["type"].as_str(), Some("localImage" | "image")))
            .filter_map(|x| x["path"].as_str()).map(str::to_owned).collect()
    } else { Vec::new() };
    let mut title = String::new();
    let text = match kind.as_str() {
        "agentMessage" => string(v, "text"),
        "userMessage" => v["content"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|x| !matches!(x["type"].as_str(), Some("localImage" | "image")))
                    .filter_map(|x| x["text"].as_str().or(x["path"].as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "commandExecution" => {
            let action = v["commandActions"].as_array().and_then(|a| a.first());
            title = match action.and_then(|a| a["type"].as_str()).unwrap_or("") {
                "read" => format!(
                    "Reading {}",
                    action.and_then(|a| a["name"].as_str()).unwrap_or("files")
                ),
                "search" => "Searching files".into(),
                "listFiles" => "Listing files".into(),
                _ => "Running command".into(),
            };
            format!(
                "{}\n\n{}\n\n{}",
                string(v, "cwd"),
                string(v, "command"),
                string(v, "aggregatedOutput")
            )
        }
        "fileChange" => {
            title = "Editing files".into();
            v["changes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|c| format!("{}\n{}", string(c, "path"), string(c, "diff")))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .unwrap_or_default()
        }
        "reasoning" => {
            title = v["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().or(s["text"].as_str()))
                .next()
                .unwrap_or("Thinking")
                .lines()
                .next()
                .unwrap_or("Thinking")
                .trim_matches('*')
                .trim()
                .to_owned();
            ["summary", "content"]
                .iter()
                .filter_map(|k| v[k].as_array())
                .flatten()
                .filter_map(|x| x.as_str().or(x["text"].as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        }
        "contextCompaction" => {
            title = "Compacting context".into();
            "The agent summarizes the conversation so it can continue within the model's context window. Chat history stays available.".into()
        }
        _ => {
            title = string(v, "tool");
            if title.is_empty() {
                title = kind.clone();
            }
            serde_json::to_string_pretty(v).unwrap_or_default()
        }
    };
    Item {
        id: string(v, "id"),
        kind,
        images,
        title,
        text: clipped(&text, 96_000),
        status: string(v, "status"),
    }
}

pub fn step_label(x: &Item, active: bool) -> String {
    let title = if x.title.is_empty() {
        "Working"
    } else {
        &x.title
    };
    if active {
        return clipped(title, 120);
    }
    if x.status == "failed" {
        return format!("{} · failed", title.trim_end());
    }
    if x.kind == "reasoning" && title == "Thinking" {
        return "Thought".into();
    }
    let done = title
        .replacen("Compacting context", "Context compacted", 1)
        .replacen("Running command", "Ran command", 1)
        .replacen("Reading ", "Read ", 1)
        .replacen("Searching ", "Searched ", 1)
        .replacen("Listing ", "Listed ", 1)
        .replacen("Editing ", "Edited ", 1);
    clipped(&done, 120)
}
pub fn clipped(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!(
        "{}\n[…display shortened; full item remains in Codex history]",
        &s[..i]
    )
}
pub fn upsert(items: &mut Vec<Item>, mut x: Item) {
    if let Some(old) = items.iter_mut().find(|i| !x.id.is_empty() && i.id == x.id) {
        if x.kind == "reasoning" && x.title == "Thinking" && !old.title.is_empty() {
            x.title = old.title.clone();
        }
        *old = x;
    } else {
        items.push(x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn restored_user_images_are_previews_not_path_text() {
        let x=item(&json!({"id":"u","type":"userMessage","content":[
            {"type":"text","text":"Inspect this"},
            {"type":"localImage","path":"/tmp/screenshot.png"},
            {"type":"text","text":"Attached local file: /tmp/notes.txt"}]}));
        assert_eq!(x.images,vec!["/tmp/screenshot.png"]);
        assert_eq!(x.text,"Inspect this\nAttached local file: /tmp/notes.txt");
        let mut rows=vec![x.clone()];upsert(&mut rows,x);
        assert_eq!(rows.len(),1);assert_eq!(rows[0].images.len(),1);
    }
    #[test]
    fn final_message_replaces_stream_without_duplicate() {
        let mut a = vec![Item {
            id: "a".into(),
            text: "Hel".into(),
            ..Default::default()
        }];
        upsert(
            &mut a,
            item(&json!({"id":"a","type":"agentMessage","text":"Hello"})),
        );
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].text, "Hello");
    }
    #[test]
    fn unicode_output_limit_never_splits_codepoint() {
        assert!(clipped(&"🦀".repeat(30), 13).starts_with("🦀🦀🦀\n"));
    }
    #[test]
    fn changes_preserve_diff_for_review() {
        let i = item(
            &json!({"id":"f","type":"fileChange","changes":[{"path":"/tmp/a","diff":"-old\n+new"}]}),
        );
        assert!(i.text.contains("/tmp/a\n-old\n+new"));
    }
}

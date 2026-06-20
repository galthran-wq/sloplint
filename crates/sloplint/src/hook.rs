//! Agent-loop hook plumbing: pull the just-edited file path out of the JSON an AI coding
//! tool pipes to a hook command on stdin.
//!
//! The two tools we wire up via `sloplint init` both deliver the edited path as stdin JSON,
//! just at different keys:
//! - Claude Code (PostToolUse): `{ "tool_input": { "file_path": "…" } }`
//! - Cursor (afterFileEdit):    `{ "file_path": "…" }`
//!
//! Rather than make the wired command depend on `jq`, `sloplint check --hook` reads the
//! payload itself and looks in both places, so one flag serves both schemas.

use serde_json::Value;

/// Extract the edited file path from a PostToolUse / afterFileEdit JSON payload.
///
/// Looks for `tool_input.file_path` (Claude Code) first, then a top-level `file_path`
/// (Cursor). Returns `None` when the payload is not the expected shape or carries no path —
/// the caller treats that as "nothing to lint" and exits cleanly.
pub fn extract_hook_path(stdin_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdin_json).ok()?;
    let path = value
        .get("tool_input")
        .and_then(|t| t.get("file_path"))
        .or_else(|| value.get("file_path"))?;
    match path.as_str() {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_post_tool_use_shape() {
        let json = r#"{ "tool_name": "Edit", "tool_input": { "file_path": "/repo/src/app.py", "file_text": "x" } }"#;
        assert_eq!(extract_hook_path(json).as_deref(), Some("/repo/src/app.py"));
    }

    #[test]
    fn reads_cursor_after_file_edit_shape() {
        let json = r#"{ "file_path": "/repo/lib/util.py", "edits": [] }"#;
        assert_eq!(
            extract_hook_path(json).as_deref(),
            Some("/repo/lib/util.py")
        );
    }

    #[test]
    fn tool_input_takes_precedence_over_top_level() {
        // A payload carrying both keys is Claude's (it nests under tool_input); prefer it.
        let json = r#"{ "file_path": "wrong.py", "tool_input": { "file_path": "right.py" } }"#;
        assert_eq!(extract_hook_path(json).as_deref(), Some("right.py"));
    }

    #[test]
    fn missing_or_empty_path_is_none() {
        assert_eq!(extract_hook_path(r#"{ "tool_name": "Bash" }"#), None);
        assert_eq!(extract_hook_path(r#"{ "file_path": "" }"#), None);
        assert_eq!(extract_hook_path("not json"), None);
        assert_eq!(extract_hook_path(r#"{ "file_path": 5 }"#), None);
    }
}

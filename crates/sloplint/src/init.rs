//! `sloplint init` — wire sloplint into AI coding tools so `check` runs on every edit and
//! findings reach the agent *before* the code lands.
//!
//! sloplint is fast, deterministic and reproducible — exactly what an in-the-edit-loop check
//! needs. This module detects the tool(s) present in a repo and merges the right hook config:
//!
//! - **Claude Code** — a `PostToolUse` hook in `.claude/settings.json`
//! - **Cursor** — an `afterFileEdit` hook in `.cursor/hooks.json`
//! - **Aider** — a `lint-cmd` in `.aider.conf.yml`
//!
//! Claude Code and Cursor both pipe the edited path to the hook as stdin JSON, so they share
//! one wired command — `sloplint check --hook --format agent` (see [`crate::hook`]). Aider
//! passes the filename as an argument, so it just runs `sloplint check --format agent`.
//!
//! Every merge is **idempotent and additive**: existing settings are parsed and preserved, an
//! existing sloplint entry is left untouched, and a config we can't safely edit (a populated
//! `.aider.conf.yml`) yields a manual snippet rather than a clobbered file.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// The command Claude Code / Cursor run on each edit. Both deliver the edited path as stdin
/// JSON, which `--hook` reads; `--format agent` emits one terse finding per line.
const HOOK_COMMAND: &str = "sloplint check --hook --format agent";

/// Aider appends the edited filename to its `lint-cmd`, so no `--hook` — it lints an argument.
const AIDER_LINT_CMD: &str = "python: sloplint check --format agent";

/// A supported AI coding tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Claude,
    Cursor,
    Aider,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Claude, Tool::Cursor, Tool::Aider];

    pub fn display_name(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Cursor => "Cursor",
            Tool::Aider => "Aider",
        }
    }

    /// The config file this tool's hook lives in, relative to the repo root.
    pub fn config_path(self, root: &Path) -> PathBuf {
        match self {
            Tool::Claude => root.join(".claude").join("settings.json"),
            Tool::Cursor => root.join(".cursor").join("hooks.json"),
            Tool::Aider => root.join(".aider.conf.yml"),
        }
    }

    /// Is this tool present in `root`? Detection is by the markers a tool leaves behind — its
    /// config dir, a rules file, or (Aider) its chat-history droppings.
    pub fn detect(self, root: &Path) -> bool {
        let exists = |rel: &str| root.join(rel).exists();
        match self {
            Tool::Claude => exists(".claude") || exists("CLAUDE.md"),
            Tool::Cursor => exists(".cursor") || exists(".cursorrules"),
            Tool::Aider => {
                exists(".aider.conf.yml")
                    || exists(".aider.chat.history.md")
                    || exists(".aider.input.history")
            }
        }
    }

    /// Compute the config edit for this tool given the current file contents (if any).
    pub fn plan(self, existing: Option<&str>) -> Result<Action> {
        match self {
            Tool::Claude => plan_claude(existing),
            Tool::Cursor => plan_cursor(existing),
            Tool::Aider => Ok(plan_aider(existing)),
        }
    }
}

/// Every tool present in `root`, in display order.
pub fn detect_tools(root: &Path) -> Vec<Tool> {
    Tool::ALL.into_iter().filter(|t| t.detect(root)).collect()
}

/// The outcome of planning a tool's config edit.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Write these full contents (a fresh file, or the existing one with our hook merged in).
    Write(String),
    /// The tool is already wired to sloplint — nothing to do.
    AlreadyConfigured,
    /// We won't risk rewriting this file; show the user the lines to add by hand.
    Manual(String),
}

/// Does any existing PostToolUse / afterFileEdit entry already invoke sloplint? This is a
/// substring heuristic on the command — enough to keep `init` idempotent without parsing each
/// tool's command grammar. It errs toward "already configured" (skip), never toward a
/// duplicate, which is the safe direction.
fn array_mentions_sloplint(arr: &[Value]) -> bool {
    arr.iter().any(|entry| {
        // Claude nests the command under `hooks[].command`; Cursor puts it on `command`.
        let direct = entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.contains("sloplint"));
        let nested = entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| c.contains("sloplint"))
                })
            });
        direct || nested
    })
}

/// Get `obj[key]` as a mutable array, inserting an empty one if absent; error if it exists but
/// is the wrong type (so we never silently drop a user's hand-edited config).
fn array_entry<'a>(obj: &'a mut Value, key: &str, what: &str) -> Result<&'a mut Vec<Value>> {
    let map = obj
        .as_object_mut()
        .ok_or_else(|| anyhow!("{what} is not a JSON object"))?;
    let slot = map.entry(key).or_insert_with(|| json!([]));
    slot.as_array_mut()
        .ok_or_else(|| anyhow!("{what}.{key} is not a JSON array"))
}

fn plan_claude(existing: Option<&str>) -> Result<Action> {
    let mut root: Value = match existing {
        Some(text) => {
            serde_json::from_str(text).context("parsing .claude/settings.json (must be JSON)")?
        }
        None => json!({}),
    };
    if !root.is_object() {
        bail!(".claude/settings.json must be a JSON object");
    }
    let map = root.as_object_mut().unwrap();
    let hooks = map.entry("hooks").or_insert_with(|| json!({}));
    let post = array_entry(hooks, "PostToolUse", "hooks")?;
    if array_mentions_sloplint(post) {
        return Ok(Action::AlreadyConfigured);
    }
    post.push(json!({
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [ { "type": "command", "command": HOOK_COMMAND } ]
    }));
    Ok(Action::Write(serde_json::to_string_pretty(&root)? + "\n"))
}

fn plan_cursor(existing: Option<&str>) -> Result<Action> {
    let mut root: Value = match existing {
        Some(text) => {
            serde_json::from_str(text).context("parsing .cursor/hooks.json (must be JSON)")?
        }
        None => json!({ "version": 1, "hooks": {} }),
    };
    if !root.is_object() {
        bail!(".cursor/hooks.json must be a JSON object");
    }
    let map = root.as_object_mut().unwrap();
    map.entry("version").or_insert_with(|| json!(1));
    let hooks = map.entry("hooks").or_insert_with(|| json!({}));
    let edits = array_entry(hooks, "afterFileEdit", "hooks")?;
    if array_mentions_sloplint(edits) {
        return Ok(Action::AlreadyConfigured);
    }
    edits.push(json!({ "command": HOOK_COMMAND }));
    Ok(Action::Write(serde_json::to_string_pretty(&root)? + "\n"))
}

/// Aider's config is YAML, where duplicate top-level keys are invalid and a structural merge
/// needs a YAML library. We keep it dependency-free and safe: create the file when absent, no-op
/// when sloplint is already referenced, and otherwise hand back a snippet to merge by hand.
fn plan_aider(existing: Option<&str>) -> Action {
    match existing {
        Some(text) if text.contains("sloplint") => Action::AlreadyConfigured,
        Some(_) => Action::Manual(aider_snippet()),
        None => Action::Write(aider_file()),
    }
}

fn aider_file() -> String {
    format!(
        "# Added by `sloplint init`: lint every edited file so the agent self-corrects in-loop.\n\
         auto-lint: true\n\
         lint-cmd:\n  - \"{AIDER_LINT_CMD}\"\n"
    )
}

fn aider_snippet() -> String {
    format!("auto-lint: true\nlint-cmd:\n  - \"{AIDER_LINT_CMD}\"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_contents(action: Action) -> String {
        match action {
            Action::Write(c) => c,
            other => panic!("expected Write, got {other:?}"),
        }
    }

    #[test]
    fn claude_fresh_file_adds_post_tool_use_hook() {
        let out = write_contents(plan_claude(None).unwrap());
        let v: Value = serde_json::from_str(&out).unwrap();
        let entry = &v["hooks"]["PostToolUse"][0];
        assert_eq!(entry["matcher"], "Edit|Write|MultiEdit");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(entry["hooks"][0]["command"], HOOK_COMMAND);
    }

    #[test]
    fn claude_merge_preserves_existing_keys_and_hooks() {
        let existing = r#"{
            "model": "opus",
            "hooks": { "PreToolUse": [ { "matcher": "Bash", "hooks": [] } ] }
        }"#;
        let out = write_contents(plan_claude(Some(existing)).unwrap());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus"); // unrelated setting kept
        assert!(v["hooks"]["PreToolUse"].is_array()); // other hook kept
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            HOOK_COMMAND
        );
    }

    #[test]
    fn claude_is_idempotent() {
        let once = write_contents(plan_claude(None).unwrap());
        assert_eq!(plan_claude(Some(&once)).unwrap(), Action::AlreadyConfigured);
    }

    #[test]
    fn claude_rejects_non_object_and_wrong_typed_hooks() {
        assert!(plan_claude(Some("[]")).is_err());
        assert!(plan_claude(Some(r#"{ "hooks": { "PostToolUse": 5 } }"#)).is_err());
        assert!(plan_claude(Some("not json")).is_err());
    }

    #[test]
    fn cursor_fresh_file_has_version_and_after_file_edit() {
        let out = write_contents(plan_cursor(None).unwrap());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["hooks"]["afterFileEdit"][0]["command"], HOOK_COMMAND);
    }

    #[test]
    fn cursor_merge_is_idempotent_and_keeps_other_hooks() {
        let existing = r#"{ "version": 1, "hooks": { "beforeShellExecution": [ { "command": "guard.sh" } ] } }"#;
        let out = write_contents(plan_cursor(Some(existing)).unwrap());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hooks"]["beforeShellExecution"][0]["command"], "guard.sh");
        assert_eq!(v["hooks"]["afterFileEdit"][0]["command"], HOOK_COMMAND);
        // Running again changes nothing.
        assert_eq!(plan_cursor(Some(&out)).unwrap(), Action::AlreadyConfigured);
    }

    #[test]
    fn aider_creates_when_absent_instructs_when_present() {
        let created = write_contents(plan_aider(None));
        assert!(created.contains("auto-lint: true"));
        assert!(created.contains(AIDER_LINT_CMD));
        // A populated config we don't recognize → manual snippet, never an overwrite.
        match plan_aider(Some("model: gpt-4o\n")) {
            Action::Manual(snippet) => assert!(snippet.contains(AIDER_LINT_CMD)),
            other => panic!("expected Manual, got {other:?}"),
        }
        // Already mentions sloplint → no-op.
        assert_eq!(
            plan_aider(Some("lint-cmd:\n  - \"python: sloplint check\"\n")),
            Action::AlreadyConfigured
        );
    }

    #[test]
    fn detect_finds_marked_tools() {
        let dir = std::env::temp_dir().join(format!("sli-init-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "x").unwrap();
        std::fs::write(dir.join(".aider.conf.yml"), "x").unwrap();
        let found = detect_tools(&dir);
        assert!(found.contains(&Tool::Claude));
        assert!(found.contains(&Tool::Aider));
        assert!(!found.contains(&Tool::Cursor));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

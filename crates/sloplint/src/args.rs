//! The command-line surface (cf. ruff's `args.rs`): the `clap` `Cli`/`Command` definitions and
//! the small value enums + `Scope` they use. Pure declarations; dispatch lives in `main`.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "sloplint",
    about = "A nitpicking linter that counters AI slop in Python (runs after Ruff)",
    version
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Parse a Python file and dump its AST and token stream (debug aid).
    Parse {
        /// Path to a `.py` file.
        file: String,
    },
    /// Explain a rule (like `ruff rule`): print a rule's docs, or list all rules with no argument.
    Rule {
        /// Rule code to explain, e.g. `SLP030`. Omit to list every rule.
        rule: Option<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = RuleFormat::Text)]
        format: RuleFormat,
    },
    /// Check Python files for slop, honoring `sloplint.toml`.
    Check {
        /// Files or directories to check (defaults to the current directory).
        paths: Vec<String>,
        /// Path to a config file (otherwise `sloplint.toml` is discovered from the cwd up).
        #[arg(long)]
        config: Option<String>,
        /// Enable preview (unstable) rules.
        #[arg(long)]
        preview: bool,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Agent-loop mode: read the just-edited file path from an AI tool's PostToolUse /
        /// afterFileEdit JSON on stdin, lint only that file with the per-file rules, print any
        /// findings (agent format) to stderr, and exit 2 so the agent sees them and can
        /// self-correct. A clean file exits 0 silently. Wire it up with `sloplint init`.
        #[arg(long)]
        hook: bool,
        /// Automatically fix findings that have a safe fix, rewriting files in place (e.g. SLP010
        /// deletes banned comments). Honors per-path rule selection and inline `# noqa` suppression,
        /// so opted-out paths and suppressed findings are never touched. Remaining (unfixable)
        /// findings are still reported.
        #[arg(long)]
        fix: bool,
        /// With `--fix`, also apply fixes marked unsafe (may change behavior or intent). No effect
        /// on its own.
        #[arg(long)]
        unsafe_fixes: bool,
    },
    /// Wire sloplint into AI coding tools (Claude Code, Cursor, Aider) so `check` runs on
    /// every edit and findings reach the agent before the code lands.
    Init {
        /// Configure a specific tool instead of auto-detecting (repeatable). Omit to detect
        /// the tools present in the repo.
        #[arg(long, value_enum)]
        tool: Vec<InitTool>,
        /// Print the config changes without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Report software-quality metrics for Python files.
    Metrics {
        /// Files or directories to measure (defaults to the current directory).
        paths: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = MetricsFormat::Text)]
        format: MetricsFormat,
        /// Write badge SVGs and shields endpoint JSON into this directory.
        #[arg(long)]
        badges: Option<String>,
        /// Path to a config file (otherwise `sloplint.toml` is discovered) — read for `[badges]`.
        #[arg(long)]
        config: Option<String>,
        /// Fail (exit 1) if any function's cyclomatic complexity exceeds this ceiling. This
        /// is a CI gate, not a finding — it never emits a diagnostic, so it doesn't duplicate
        /// Ruff's `C901`. McCabe recommends a ceiling of 10.
        #[arg(long)]
        max_cyclomatic: Option<usize>,
        /// Fail (exit 1) if any function's cognitive complexity exceeds this ceiling (a CI
        /// gate, not a diagnostic). SonarSource suggests 15 per function.
        #[arg(long)]
        max_cognitive: Option<usize>,
        /// Which profile the human/text view and the per-unit feeds report. A profile is a
        /// named, path-matched slice of the tree (`[[profiles]]` in `sloplint.toml`); they're
        /// measured separately because, e.g., test and production code have different healthy
        /// norms. Pass a profile name, or `all` for every profile panel. Defaults to the
        /// `default` profile (`production` out of the box). Governs `--format text`/`github` and
        /// the `functions`/`classes`/`packages` feeds (the packages graph is built from the
        /// scoped modules only, so one profile can't manufacture another's coupling). `--format
        /// json` ignores this — it always emits every profile panel plus the all-files
        /// `test_proxies`.
        #[arg(long)]
        scope: Option<String>,
    },
}

/// Output format for `check`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum Format {
    /// Human-readable text (default).
    Text,
    /// Flat JSON array of findings.
    Json,
    /// SARIF 2.1.0 for GitHub code scanning.
    Sarif,
    /// GitHub-flavored markdown summary (for PR comments).
    Github,
    /// Terse `path:line:col: CODE message`, one finding per line — for AI coding agents.
    Agent,
}

/// Which AI coding tool `init` should wire up.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum InitTool {
    Claude,
    Cursor,
    Aider,
    /// All supported tools.
    All,
}

impl InitTool {
    pub(crate) fn tools(self) -> Vec<crate::init::Tool> {
        match self {
            InitTool::Claude => vec![crate::init::Tool::Claude],
            InitTool::Cursor => vec![crate::init::Tool::Cursor],
            InitTool::Aider => vec![crate::init::Tool::Aider],
            InitTool::All => crate::init::Tool::ALL.to_vec(),
        }
    }
}

/// Output format for `rule`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum RuleFormat {
    /// Human-readable text (default).
    Text,
    /// JSON — machine-readable rule metadata (mirrors `ruff rule --output-format json`).
    Json,
}

/// Output format for `metrics`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum MetricsFormat {
    Text,
    Json,
    /// GitHub-flavored markdown summary (a PR-comment line + risk-tier table).
    Github,
    /// One JSON object per function (JSONL) — the per-function feature dump for the
    /// benchmark / rule-discovery harness. Raw rows, not aggregates.
    Functions,
    /// One JSON object per class (JSONL) — per-class size + LCOM4 cohesion. Raw rows.
    Classes,
    /// One JSON object per package (JSONL) — the first-party import graph collapsed to
    /// directory level: module count + the packages it imports / is imported by. Raw rows.
    Packages,
}

/// Which profile(s) the text view and the per-unit feeds report: one named profile, or
/// every profile (`all`). Resolved from the `--scope` flag against the configured profiles.
pub(crate) enum Scope {
    /// Every configured profile (text prints one panel each; feeds emit all files).
    All,
    /// A single named profile.
    One(String),
}

impl Scope {
    /// Whether a file with the given profile membership is in this scope.
    pub(crate) fn includes(&self, profiles: &[String]) -> bool {
        match self {
            Scope::All => true,
            Scope::One(name) => profiles.iter().any(|p| p == name),
        }
    }
}

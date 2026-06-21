//! sloplint CLI.
//!
//! - `parse` — debug aid that dumps the AST and token stream for a file.
//! - `check` — discover config, run the shipped per-file rules over Python files, then
//!   run cross-file clone detection (SLP020), and report all findings.

// The `metrics --format json` panel is one large `serde_json::json!` literal; as it has grown
// (the per-unit metric distributions) it exceeds the default macro recursion limit.
#![recursion_limit = "256"]

mod imports;
mod stdlib;

mod corrupted;
mod hook;
mod init;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use ignore::WalkBuilder;
use sloplint_clone::{extract_functions, find_clones, CloneConfig, ClonePair, FunctionUnit};
use sloplint_diagnostics::fix;
use sloplint_diagnostics::render::render_diagnostics;
use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_linter::config::{BadgeSettings, Config, Selector};
use sloplint_linter::detect;
use sloplint_linter::lint::{check_file, FileContext, Rule};
use sloplint_linter::registry::Registry;
use sloplint_linter::suppression::Suppressions;
use sloplint_metrics::badge::{Badge, Color};
use sloplint_metrics::graph::{self, ImportGraph, ModuleInput, PackageRow};
use sloplint_metrics::test_proxies::{self, FileTestStats, TestProxies};
use sloplint_metrics::{aggregate, file_metrics, FileMetrics, FunctionMetrics, RepoMetrics};
use sloplint_python::{parse, Ranged, TextRange};
use sloplint_report::ReportEntry;

#[derive(Parser)]
#[command(
    name = "sloplint",
    about = "A nitpicking linter that counters AI slop in Python (runs after Ruff)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a Python file and dump its AST and token stream (debug aid).
    Parse {
        /// Path to a `.py` file.
        file: String,
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
        /// Which profile the human/text view and the per-unit feeds report (#96). A profile is a
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
enum Format {
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
enum InitTool {
    Claude,
    Cursor,
    Aider,
    /// All supported tools.
    All,
}

impl InitTool {
    fn tools(self) -> Vec<init::Tool> {
        match self {
            InitTool::Claude => vec![init::Tool::Claude],
            InitTool::Cursor => vec![init::Tool::Cursor],
            InitTool::Aider => vec![init::Tool::Aider],
            InitTool::All => init::Tool::ALL.to_vec(),
        }
    }
}

/// Output format for `metrics`.
#[derive(Clone, Copy, clap::ValueEnum)]
enum MetricsFormat {
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

/// Which profile(s) the text view and the per-unit feeds report (#96): one named profile, or
/// every profile (`all`). Resolved from the `--scope` flag against the configured profiles.
enum Scope {
    /// Every configured profile (text prints one panel each; feeds emit all files).
    All,
    /// A single named profile.
    One(String),
}

impl Scope {
    /// Whether a file with the given profile membership is in this scope.
    fn includes(&self, profiles: &[String]) -> bool {
        match self {
            Scope::All => true,
            Scope::One(name) => profiles.iter().any(|p| p == name),
        }
    }
}

/// Resolve the `--scope` argument against the configured profiles: absent ⇒ the `default`
/// profile (the quality headline); `all` ⇒ every profile; otherwise it must name a profile.
fn resolve_scope(arg: Option<&str>, selector: &Selector) -> anyhow::Result<Scope> {
    match arg {
        None => {
            let name = selector
                .default_profile()
                .or_else(|| selector.profile_names().first().copied())
                .ok_or_else(|| anyhow!("no metrics profiles are configured"))?;
            Ok(Scope::One(name.to_string()))
        }
        Some("all") => Ok(Scope::All),
        Some(name) if selector.profile_names().contains(&name) => Ok(Scope::One(name.to_string())),
        Some(name) => Err(anyhow!(
            "unknown --scope '{name}'; configured profiles: {} (or 'all')",
            selector.profile_names().join(", ")
        )),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Parse { file } => match run_parse(&file) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => tool_error(err),
        },
        Command::Check {
            paths,
            config,
            preview,
            format,
            hook: true,
            fix,
            unsafe_fixes,
        } => {
            // Agent-loop mode: file path comes from stdin, output goes to stderr, and the exit
            // code (2) is what an editor's PostToolUse / afterFileEdit hook reads. The `paths`,
            // `format`, and `--fix` args are ignored here — the contract is fixed (report, don't
            // rewrite, so the agent stays in control of the edit).
            let _ = (paths, format, fix, unsafe_fixes);
            match run_hook(config.as_deref(), preview) {
                Ok(HookOutcome::Clean) => ExitCode::SUCCESS,
                Ok(HookOutcome::Findings(text)) => {
                    eprint!("{text}");
                    ExitCode::from(2)
                }
                // Exit 2 is the "block / feed back to the agent" signal — reserve it strictly
                // for findings. If sloplint itself can't run (e.g. a malformed sloplint.toml),
                // exit 1 so the edit proceeds and the agent isn't told its code is bad: both
                // Claude Code and Cursor treat a non-2 non-zero as a non-blocking hook error.
                Err(err) => {
                    eprintln!("sloplint: {err:#}");
                    ExitCode::from(1)
                }
            }
        }
        Command::Check {
            paths,
            config,
            preview,
            format,
            hook: false,
            fix,
            unsafe_fixes,
        } => match run_check(
            &paths,
            config.as_deref(),
            preview,
            format,
            FixMode::new(fix, unsafe_fixes),
        ) {
            Ok(true) => ExitCode::SUCCESS,  // clean
            Ok(false) => ExitCode::from(1), // findings or read/parse errors
            Err(err) => tool_error(err),    // could not run at all
        },
        Command::Init { tool, dry_run } => match run_init(&tool, dry_run) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => tool_error(err),
        },
        Command::Metrics {
            paths,
            format,
            badges,
            config,
            max_cyclomatic,
            max_cognitive,
            scope,
        } => match run_metrics(
            &paths,
            format,
            badges.as_deref(),
            config.as_deref(),
            max_cyclomatic,
            max_cognitive,
            scope,
        ) {
            Ok(true) => ExitCode::SUCCESS,  // under the gate(s) (or no gate)
            Ok(false) => ExitCode::from(1), // a function exceeded a --max-* ceiling
            Err(err) => tool_error(err),
        },
    }
}

fn tool_error(err: anyhow::Error) -> ExitCode {
    eprintln!("sloplint: {err:#}");
    ExitCode::from(2)
}

fn run_parse(path: &str) -> anyhow::Result<()> {
    let source = fs::read_to_string(path).map_err(|e| anyhow!("reading {path}: {e}"))?;
    let parsed = parse(&source).map_err(|e| anyhow!("parsing {path}: {e}"))?;

    println!("=== AST ===");
    println!("{:#?}", parsed.syntax());

    println!("\n=== TOKENS ===");
    for token in parsed.tokens().iter() {
        println!("{:?} {:?}", token.kind(), token.range());
    }

    // No parse-error dump here: `parse` returns `Err` on any syntax error, so a
    // successful `parsed` has none. Syntax errors are reported on the `Err` path in main.
    Ok(())
}

/// Load config from an explicit `--config` path, or discover `sloplint.toml` from the cwd up.
/// `--preview` forces preview rules on regardless of the file's setting.
fn load_config(config_path: Option<&str>, preview: bool) -> anyhow::Result<Config> {
    let mut config = match config_path {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|e| anyhow!("reading config {path}: {e}"))?;
            Config::from_toml_str(&text).map_err(|e| anyhow!("parsing config {path}: {e}"))?
        }
        None => {
            let cwd =
                env::current_dir().map_err(|e| anyhow!("resolving working directory: {e}"))?;
            Config::discover(&cwd)?
        }
    };
    if preview {
        config.preview = true;
    }
    Ok(config)
}

/// What the agent-loop hook should do with the edit it was handed.
enum HookOutcome {
    /// Nothing to report — the file is clean, isn't Python, or couldn't be read/parsed (a
    /// syntax error mid-edit is Ruff's job, not ours).
    Clean,
    /// Findings to surface to the agent, already rendered in agent format.
    Findings(String),
}

/// Agent-loop mode (`check --hook`): read the just-edited path from the PostToolUse /
/// afterFileEdit JSON on stdin, lint that one file with the per-file rules, and report.
///
/// Whole-tree rules (clones, fanout, undeclared imports) need the whole project, so they're
/// deliberately skipped here — this is the fast single-file path the edit loop wants. Findings
/// are returned for the caller to print to stderr with exit code 2.
fn run_hook(config_path: Option<&str>, preview: bool) -> anyhow::Result<HookOutcome> {
    let mut stdin_payload = String::new();
    io::stdin()
        .read_to_string(&mut stdin_payload)
        .map_err(|e| anyhow!("reading hook payload from stdin: {e}"))?;
    let path = match hook::extract_hook_path(&stdin_payload) {
        Some(p) => PathBuf::from(p),
        None => return Ok(HookOutcome::Clean), // no edited path in the payload
    };
    if !is_python(&path) {
        return Ok(HookOutcome::Clean);
    }

    let config = load_config(config_path, preview)?;
    let selector = config
        .prepare()
        .map_err(|e| anyhow!("invalid glob in config: {e}"))?;
    let registry = Registry::shipped();

    let display = path.to_string_lossy().to_string();
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(_) => return Ok(HookOutcome::Clean), // unreadable — nothing to lint
    };
    let parsed = match parse(&source) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(HookOutcome::Clean), // syntax error mid-edit — defer to Ruff
    };
    let rules = registry.enabled_for(&selector, &display);
    let refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();
    let ctx = FileContext {
        path: &display,
        source: &source,
        parsed: &parsed,
        // Per-file thresholds: the file's profile deltas over the global limits (#96).
        limits: selector.limits(&display),
        security_extra: &config.security.extra,
        placeholders_extra: &config.placeholders.extra,
    };
    let diagnostics = check_file(&ctx, &refs);
    if diagnostics.is_empty() {
        return Ok(HookOutcome::Clean);
    }
    let entries = [ReportEntry {
        path: &display,
        source: &source,
        diagnostics: &diagnostics,
    }];
    Ok(HookOutcome::Findings(sloplint_report::to_agent(&entries)))
}

/// `sloplint init` — wire detected (or `--tool`-forced) AI tools to run sloplint on each edit.
fn run_init(tools: &[InitTool], dry_run: bool) -> anyhow::Result<()> {
    let root = env::current_dir().map_err(|e| anyhow!("resolving working directory: {e}"))?;

    let targets: Vec<init::Tool> = if tools.is_empty() {
        let detected = init::detect_tools(&root);
        if detected.is_empty() {
            println!(
                "sloplint init: no AI coding tool detected here.\n\
                 Pass --tool <claude|cursor|aider|all> to configure one explicitly."
            );
            return Ok(());
        }
        detected
    } else {
        // Dedupe while preserving order, so `--tool all --tool claude` doesn't double-write.
        let mut seen = Vec::new();
        for t in tools.iter().flat_map(|t| t.tools()) {
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        seen
    };

    for tool in targets {
        let path = tool.config_path(&root);
        let existing = match fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(anyhow!("reading {}: {e}", path.display())),
        };
        let action = tool
            .plan(existing.as_deref())
            .map_err(|e| anyhow!("{}: {e}", tool.display_name()))?;
        let rel = path.strip_prefix(&root).unwrap_or(&path).display();
        match action {
            init::Action::AlreadyConfigured => {
                println!("{}: already wired ({rel})", tool.display_name());
            }
            init::Action::Manual(snippet) => {
                println!(
                    "{}: {rel} already exists — merge this into it by hand (fold the entry into \
                     any existing `lint-cmd` list rather than adding a second key):\n\n{snippet}",
                    tool.display_name()
                );
            }
            init::Action::Write(contents) if dry_run => {
                println!("{}: would write {rel}:\n\n{contents}", tool.display_name());
            }
            init::Action::Write(contents) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| anyhow!("creating {}: {e}", parent.display()))?;
                }
                fs::write(&path, contents).map_err(|e| anyhow!("writing {rel}: {e}"))?;
                println!("{}: wired ({rel})", tool.display_name());
            }
        }
    }
    if dry_run {
        println!("\n(dry run — nothing written)");
    }
    Ok(())
}

/// `--fix` / `--unsafe-fixes` state for a `check` run.
#[derive(Clone, Copy)]
struct FixMode {
    /// Whether to apply fixes at all (`--fix`). `--unsafe-fixes` alone is a no-op.
    enabled: bool,
    /// Whether to also apply `Unsafe` fixes (`--unsafe-fixes`).
    allow_unsafe: bool,
}

impl FixMode {
    fn new(fix: bool, unsafe_fixes: bool) -> Self {
        Self {
            enabled: fix,
            allow_unsafe: unsafe_fixes,
        }
    }
}

/// Returns `Ok(true)` when the run is clean, `Ok(false)` when there are findings or
/// per-file read/parse errors, and `Err` only when the run could not start (bad config).
fn run_check(
    paths: &[String],
    config_path: Option<&str>,
    preview: bool,
    format: Format,
    fix_mode: FixMode,
) -> anyhow::Result<bool> {
    let config = load_config(config_path, preview)?;
    let selector = config
        .prepare()
        .map_err(|e| anyhow!("invalid glob in config: {e}"))?;
    let registry = Registry::shipped();
    let clone_config = CloneConfig {
        min_statements: config.clone.min_statements,
        similarity: config.clone.similarity,
        ..CloneConfig::default()
    };

    let (files, mut had_error) = discover_python_files(paths);

    // First-party module names come from the full discovered tree (incl. files that fail to
    // parse), so SLP180 never mistakes a local package for a third-party import.
    let all_display: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    // Per-file results are collected first; cross-file clone detection (SLP020) needs every
    // file's functions before it can report duplicates, so we render only at the end.
    let mut results: Vec<FileResult> = Vec::new();
    let mut units: Vec<FunctionUnit> = Vec::new();
    let mut unit_result: Vec<usize> = Vec::new();
    // SLP180 (preview) is a whole-project rule: collect every file's module-level imports,
    // then resolve them against the manifest after the loop.
    let mut import_scans: Vec<(String, Vec<imports::ImportRef>)> = Vec::new();

    for path in &files {
        let display = path.to_string_lossy().to_string();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: reading {display}: {err}");
                had_error = true;
                continue;
            }
        };
        // SLP220 (preview): an unparseable `.py` is reported as corrupted/truncated AI output
        // instead of being silently skipped — registry rules never see it, so this is handled here.
        let slp220 = selector.preview() && selector.is_enabled("SLP220", &display);
        let parsed = match parse(&source) {
            Ok(parsed) => parsed,
            Err(err) => {
                if slp220 {
                    let prose_ratio = selector.limits(&display).corrupted_prose_ratio;
                    results.push(FileResult {
                        diagnostics: vec![corrupted::on_parse_error(&source, prose_ratio)],
                        suppressions: Suppressions::empty(),
                        path: display,
                        source,
                    });
                } else {
                    eprintln!("error: {display}: {err}");
                    had_error = true;
                }
                continue;
            }
        };
        let rules = registry.enabled_for(&selector, &display);
        let refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();
        let ctx = FileContext {
            path: &display,
            source: &source,
            parsed: &parsed,
            // Per-file thresholds: the file's profile deltas over the global limits (#96).
            limits: selector.limits(&display),
            security_extra: &config.security.extra,
            placeholders_extra: &config.placeholders.extra,
        };
        let mut diagnostics = check_file(&ctx, &refs);
        // SLP220 (preview): artifact markers outside strings/comments + prose density, on the parsed
        // file (the unparseable case is handled above).
        if slp220 {
            diagnostics.extend(corrupted::scan_parsed(
                &source,
                &parsed,
                ctx.limits.corrupted_prose_ratio,
            ));
        }

        let result_index = results.len();
        // SLP020 is a whole-tree analysis, not a per-file registry rule, so it's gated by
        // config select/ignore only (enabled by default) — it has no preview/stable group.
        if selector.is_enabled("SLP020", &display) {
            for unit in extract_functions(&display, &source, &parsed, clone_config.shingle_k) {
                units.push(unit);
                unit_result.push(result_index);
            }
        }
        // Collect imports for all files when preview is on; emission is gated per-path later.
        if selector.preview() {
            import_scans.push((display.clone(), imports::scan_imports(&parsed)));
        }
        let suppressions = Suppressions::parse(&source, &parsed);
        results.push(FileResult {
            path: display,
            source,
            diagnostics,
            suppressions,
        });
    }

    attribute_clones(&units, &unit_result, &clone_config, &mut results);
    attribute_fanout(&mut results, &selector, config.limits.dir_max_modules);
    if selector.preview() {
        attribute_undeclared_imports(
            &import_scans,
            &all_display,
            &config.imports.extra,
            &selector,
            &mut results,
        );
    }

    // Inline `# sloplint: allow` suppression (#94) runs last, so it filters whole-tree findings
    // (SLP020 clones, SLP090 fanout, SLP180 imports) as well as the per-file rules.
    for result in &mut results {
        result.suppressions.filter(&mut result.diagnostics);
    }

    // Autofix (`--fix`) runs *after* selection and suppression, so opted-out paths and
    // `# noqa`-suppressed findings are never rewritten. Fixed findings are dropped from
    // `diagnostics`; what remains is reported (against the original source) as usual.
    let fixed = if fix_mode.enabled {
        let (count, write_failed) = apply_fixes(&mut results, fix_mode.allow_unsafe);
        had_error |= write_failed;
        count
    } else {
        0
    };

    let findings: usize = results.iter().map(|r| r.diagnostics.len()).sum();
    let entries: Vec<ReportEntry> = results
        .iter()
        .map(|result| ReportEntry {
            path: &result.path,
            source: &result.source,
            diagnostics: &result.diagnostics,
        })
        .collect();

    match format {
        Format::Text => {
            for result in &results {
                if !result.diagnostics.is_empty() {
                    print!(
                        "{}\n{}",
                        result.path,
                        render_diagnostics(&result.source, &result.diagnostics)
                    );
                }
            }
            if findings == 0 && !had_error {
                eprintln!("sloplint: no issues found");
            } else {
                eprintln!("sloplint: {findings} issue(s)");
            }
        }
        Format::Json => println!("{}", sloplint_report::to_json(&entries)),
        Format::Sarif => println!("{}", sloplint_report::to_sarif(&entries)),
        Format::Github => println!("{}", sloplint_report::to_github_markdown(&entries)),
        Format::Agent => print!("{}", sloplint_report::to_agent(&entries)),
    }

    // The fix tally goes to stderr (like the issue summary) so it never pollutes the machine
    // formats on stdout. Printed whenever `--fix` was requested, even if nothing matched.
    if fix_mode.enabled {
        eprintln!("sloplint: fixed {fixed} issue(s)");
        // Remaining findings were located in the pre-fix source, so once we've actually rewritten
        // files their reported line:col can be stale. Say so, rather than print misleading numbers.
        if fixed > 0 && findings > 0 {
            eprintln!("sloplint: note: positions above predate --fix; re-run to refresh them");
        }
    }

    Ok(findings == 0 && !had_error)
}

/// Apply each file's available fixes, rewrite changed files in place, and drop the fixed findings
/// from the report. Returns the total number of findings fixed across all files, and whether any
/// file failed to write.
///
/// A write failure is reported and recorded (so the run exits non-zero) but does **not** abort the
/// batch — later files are still fixed, mirroring how `run_check` handles per-file read/parse
/// errors. The remaining diagnostics keep their original ranges and are still rendered against the
/// original `source`, so a re-run is the way to see refreshed positions.
fn apply_fixes(results: &mut [FileResult], allow_unsafe: bool) -> (usize, bool) {
    let mut total = 0;
    let mut write_failed = false;
    for result in results.iter_mut() {
        let applied = fix::apply(&result.source, &result.diagnostics, allow_unsafe);
        if !applied.changed() {
            continue;
        }
        if let Err(err) = fs::write(&result.path, &applied.output) {
            eprintln!("error: writing fixes to {}: {err}", result.path);
            write_failed = true;
            continue; // leave this file's findings in the report; keep fixing the rest.
        }
        let fixed: std::collections::HashSet<usize> = applied.fixed.into_iter().collect();
        let mut index = 0;
        result.diagnostics.retain(|_| {
            let keep = !fixed.contains(&index);
            index += 1;
            keep
        });
        total += fixed.len();
    }
    (total, write_failed)
}

/// One file's parsed source and accumulated diagnostics.
struct FileResult {
    path: String,
    source: String,
    diagnostics: Vec<Diagnostic>,
    /// Inline `# sloplint: allow` directives for this file (#94). Parsed up front while the tree
    /// is in scope, then applied once at the end so it filters whole-tree findings (SLP020) too.
    suppressions: Suppressions,
}

/// Run cross-file clone detection and push exactly one `SLP020` diagnostic onto each
/// clone-involved function, pointing at its lowest-index duplicate.
///
/// Reporting every pair would be O(n^2) on a cluster of N identical functions (N(N-1)
/// diagnostics), burying real signal. Collapsing to one finding per function keeps output
/// proportional to the number of duplicated functions while still flagging each of them.
fn attribute_clones(
    units: &[FunctionUnit],
    unit_result: &[usize],
    clone_config: &CloneConfig,
    results: &mut [FileResult],
) {
    // For each clone-involved function, keep its lowest-index partner (deterministic).
    let mut partner: HashMap<usize, (usize, f64)> = HashMap::new();
    let mut record = |from: usize, to: usize, similarity: f64| {
        partner
            .entry(from)
            .and_modify(|best| {
                if to < best.0 {
                    *best = (to, similarity);
                }
            })
            .or_insert((to, similarity));
    };
    for pair in find_clones(units, clone_config) {
        record(pair.a, pair.b, pair.similarity);
        record(pair.b, pair.a, pair.similarity);
    }

    let mut involved: Vec<usize> = partner.keys().copied().collect();
    involved.sort_unstable();
    for unit_index in involved {
        let (partner_index, similarity) = partner[&unit_index];
        let unit = &units[unit_index];
        let partner_unit = &units[partner_index];
        let result_index = unit_result[unit_index];
        let partner_result = unit_result[partner_index];
        let percent = (similarity * 100.0).round() as u32;

        let partner_line = line_of(
            &results[partner_result].source,
            partner_unit.range.start().into(),
        );
        let partner_path = results[partner_result].path.clone();

        results[result_index].diagnostics.push(Diagnostic::new(
            "SLP020",
            format!(
                "duplicate of {partner_path}:{partner_line} (function `{}`, {percent}% similar)",
                partner_unit.name
            ),
            unit.range,
            Severity::Warning,
        ));
    }
}

/// Flag directories holding more than `max_modules` Python files directly (flat fanout —
/// SLP090). One diagnostic per over-full directory, attributed to its first file.
fn attribute_fanout(results: &mut [FileResult], selector: &Selector, max_modules: usize) {
    let mut by_dir: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, result) in results.iter().enumerate() {
        let dir = Path::new(&result.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        by_dir.entry(dir).or_default().push(index);
    }

    for (dir, indices) in by_dir {
        if indices.len() <= max_modules {
            continue;
        }
        let representative = indices[0];
        if !selector.is_enabled("SLP090", &results[representative].path) {
            continue;
        }
        let shown_dir = if dir.is_empty() { "." } else { &dir };
        let count = indices.len();
        results[representative].diagnostics.push(Diagnostic::new(
            "SLP090",
            format!(
                "directory `{shown_dir}` holds {count} Python modules (max {max_modules}); \
                 split it into sub-packages"
            ),
            TextRange::default(),
            Severity::Warning,
        ));
    }
}

/// SLP180: flag third-party imports not declared in the project's dependency manifest.
///
/// Whole-project, so emission (not collection) is gated per-path: imports are collected for
/// every file (above) so the first-party set is complete, and a per-path `ignore` only
/// suppresses the *finding*. Resolves the manifest once from the working directory; if none
/// declares dependencies, the rule stays silent (conservative — false negatives over false
/// positives).
fn attribute_undeclared_imports(
    import_scans: &[(String, Vec<imports::ImportRef>)],
    all_paths: &[String],
    extra: &[String],
    selector: &Selector,
    results: &mut [FileResult],
) {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return,
    };
    let Some(declared) = imports::resolve_declared(&cwd) else {
        return; // no manifest declaring deps -> ambiguous, don't fire.
    };
    // First-party names must reflect the whole project tree, not just the scanned paths —
    // otherwise `sloplint check one_file.py` (or a pre-commit run over changed files only)
    // would flag local packages outside the scan as undeclared. Walk the manifest's project
    // root for that, then union the scanned paths (cheap, and covers files above the root).
    let mut first_party = first_party_under(&declared.root);
    first_party.extend(imports::first_party_names(all_paths));
    let extra_set: std::collections::HashSet<String> =
        extra.iter().map(|e| imports::normalize_dist(e)).collect();

    let by_path: HashMap<String, usize> = results
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.clone(), i))
        .collect();

    let findings = imports::findings(
        import_scans,
        &first_party,
        &declared,
        &extra_set,
        stdlib::is_stdlib,
    );
    for finding in findings {
        if !selector.is_enabled("SLP180", &finding.path) {
            continue;
        }
        if let Some(&index) = by_path.get(finding.path.as_str()) {
            results[index].diagnostics.push(Diagnostic::new(
                "SLP180",
                finding.message,
                finding.range,
                Severity::Warning,
            ));
        }
    }
}

/// First-party (project-local) top-level module names found by walking the project `root`.
///
/// Honors `.gitignore` (so `.venv/` etc. are skipped) via the same `ignore` walker used for
/// discovery. Names are computed from paths relative to `root`. Over-collecting is safe — a
/// name treated as first-party is never flagged, preserving the false-negative bias.
fn first_party_under(root: &Path) -> std::collections::HashSet<String> {
    let mut rels = Vec::new();
    for result in WalkBuilder::new(root).build().flatten() {
        let path = result.path();
        if path.is_file() && is_python(path) {
            if let Ok(rel) = path.strip_prefix(root) {
                rels.push(rel.to_string_lossy().to_string());
            }
        }
    }
    imports::first_party_names(&rels)
}

/// 1-based line number for a byte offset.
fn line_of(source: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(source.len());
    source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Discover `.py` files under the given paths. Returns the files and whether any path was
/// missing or a traversal error occurred — callers fail the run on that, so a typo'd path
/// never reports "clean". Inside a git repo the `ignore` crate honors `.gitignore`;
/// explicitly-passed files are always included.
fn discover_python_files(paths: &[String]) -> (Vec<PathBuf>, bool) {
    let default = [".".to_string()];
    let inputs: &[String] = if paths.is_empty() { &default } else { paths };

    let mut files = Vec::new();
    let mut had_error = false;
    for input in inputs {
        let path = Path::new(input);
        if path.is_file() {
            if is_python(path) {
                files.push(normalize(path));
            }
            continue;
        }
        if !path.is_dir() {
            eprintln!("error: path not found: {input}");
            had_error = true;
            continue;
        }
        for result in WalkBuilder::new(path).build() {
            match result {
                Ok(entry) => {
                    let entry_path = entry.path();
                    if entry_path.is_file() && is_python(entry_path) {
                        files.push(normalize(entry_path));
                    }
                }
                Err(err) => {
                    eprintln!("error: walking {input}: {err}");
                    had_error = true;
                }
            }
        }
    }
    files.sort();
    files.dedup();
    (files, had_error)
}

/// Strip a leading `./` so paths from `WalkBuilder::new(".")` (`./a/b.py`) match globs
/// written the documented way (`a/**`) and display cleanly. Other paths pass through.
fn normalize(path: &Path) -> PathBuf {
    path.strip_prefix(".").unwrap_or(path).to_path_buf()
}

fn is_python(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "py")
}

/// The first-party dotted module name for a discovered `.py` file, for the import graph.
///
/// The dotted name must match what `import` statements actually reference, regardless of where
/// the project sits relative to the working directory. So we find the file's **source root** —
/// the nearest ancestor directory that is *not* itself a Python package — by walking up while a
/// directory contains `__init__.py`, then name the module relative to that root. This resolves
/// `tests/fixtures/proj/a.py` to `proj.a` (not `tests.fixtures.proj.a`) and handles the `src/`
/// layout for free (the walk stops at `src`, which has no `__init__.py`).
///
/// Known limitation (documented): a PEP 420 namespace package (a directory with no
/// `__init__.py`) is treated as a source-root boundary, so its prefix is dropped from the names
/// of modules in nested regular sub-packages. Full multi-root namespace handling is out of scope
/// for this foundation.
fn module_name(path: &Path) -> Option<graph::ModuleName> {
    let mut root = path.parent()?;
    while root.join("__init__.py").is_file() {
        match root.parent() {
            Some(parent) => root = parent,
            None => break,
        }
    }
    let rel = path.strip_prefix(root).ok()?;
    graph::module_from_path(&rel.to_string_lossy())
}

/// Compute and report software-quality metrics; optionally emit badges and enforce
/// complexity gates. Returns `Ok(false)` only when a `--max-*` ceiling is set and some function
/// exceeds it — the CI gate. Reporting/badge writing always happens first so the numbers are
/// visible even on a failing gate.
fn run_metrics(
    paths: &[String],
    format: MetricsFormat,
    badges: Option<&str>,
    config_path: Option<&str>,
    max_cyclomatic: Option<usize>,
    max_cognitive: Option<usize>,
    scope: Option<String>,
) -> anyhow::Result<bool> {
    // Profiles drive classification (which panel a file feeds) the same way they drive `check`'s
    // rule config. Load best-effort: an explicit --config is strict, discovery falls back to the
    // built-in profiles so a malformed ancestor toml can't break `metrics`.
    let config = load_metrics_config(config_path)?;
    let selector = config
        .prepare()
        .map_err(|e| anyhow!("invalid glob in config: {e}"))?;
    let scope = resolve_scope(scope.as_deref(), &selector)?;
    let profile_names: Vec<String> = selector
        .profile_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let (files, _) = discover_python_files(paths);
    // The package feed and the JSON rollup need the first-party import graph, which is a
    // whole-project pass (like SLP180): collect every file's module-level imports here, then
    // build the graph once after the loop.
    let needs_graph = matches!(format, MetricsFormat::Packages | MetricsFormat::Json);
    // Duplication density (#123) is surfaced only on the aggregate panels, not the per-unit feeds.
    let needs_clones = matches!(
        format,
        MetricsFormat::Text | MetricsFormat::Json | MetricsFormat::Github
    );
    let clone_config = CloneConfig {
        min_statements: config.clone.min_statements,
        similarity: config.clone.similarity,
        ..CloneConfig::default()
    };
    // Every function's clone fingerprint plus the profiles of the file it came from, so the SLP020
    // pass can run once over the whole tree and be filtered per profile afterwards (#123).
    let mut clone_units: Vec<FunctionUnit> = Vec::new();
    let mut unit_profiles: Vec<Vec<String>> = Vec::new();
    // Keep path + source alongside metrics so the gate can name offending functions with a
    // resolved `path:line` location.
    let mut per_file: Vec<MeasuredFile> = Vec::new();
    // Each module input carries its file's profile membership so the import graph can be built
    // per profile (#96) — one profile importing another must not manufacture coupling in the
    // first profile's architecture metrics.
    let mut module_inputs: Vec<(ModuleInput, Vec<String>)> = Vec::new();
    // Static test proxies (#86): one per file. The test/production split is bound to the `tests`
    // profile (#96) so the proxies and the panels agree.
    let mut test_stats: Vec<FileTestStats> = Vec::new();
    for path in files {
        let display = path.to_string_lossy().to_string();
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = parse(&source) else {
            continue;
        };
        let metrics = file_metrics(&source, &parsed);
        // Machine-generated code (#115) is a third category alongside tests/production: its
        // structural numbers are codegen artifacts, so it routes into the `generated` profile and
        // out of the `production` complement. Detection is a cheap header-marker scan.
        let is_generated = detect::is_generated(&source, &display);
        let profiles: Vec<String> = selector
            .profiles_for_file(&display, is_generated)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let is_test = profiles.iter().any(|p| p == "tests");
        // Generated *production* code is excluded from the test:code proxies (it is not
        // human-maintained, so it must not inflate the production-LoC denominator). A generated
        // file that is also a test still counts as a test — the panels claim it under both, so the
        // proxies must agree rather than dropping it from both sides.
        if is_test || !is_generated {
            test_stats.push(test_proxies::file_test_stats(is_test, metrics.loc, &parsed));
        }
        if needs_clones {
            for unit in extract_functions(&display, &source, &parsed, clone_config.shingle_k) {
                clone_units.push(unit);
                unit_profiles.push(profiles.clone());
            }
        }
        if needs_graph {
            if let Some(name) = module_name(&path) {
                module_inputs.push((
                    ModuleInput {
                        name,
                        imports: graph::scan_module_imports(&parsed),
                        loc: metrics.loc,
                        classes: metrics.classes.len(),
                        abstract_classes: metrics.classes.iter().filter(|c| c.is_abstract).count(),
                    },
                    profiles.clone(),
                ));
            }
        }
        per_file.push(MeasuredFile {
            path: display,
            source,
            metrics,
            profiles,
        });
    }

    // DIT/NOC are whole-project properties: a class's inheritance depth and breadth depend on
    // bases/children defined in *other* files (a class in one profile may extend a base in
    // another), so resolve them across the FULL set — before any per-profile split — so every
    // panel/feed that surfaces them (the class feed, the JSON/text/github DIT-NOC figures) sees the
    // real values. Skip only the formats that show neither (the per-function and per-package feeds).
    if !matches!(format, MetricsFormat::Functions | MetricsFormat::Packages) {
        let mut metrics: Vec<&mut FileMetrics> =
            per_file.iter_mut().map(|f| &mut f.metrics).collect();
        sloplint_metrics::resolve_inheritance(&mut metrics);
    }

    // The aggregate panel for one profile: the files that profile claims.
    let panel_of = |name: &str| {
        let metrics: Vec<FileMetrics> = per_file
            .iter()
            .filter(|f| f.profiles.iter().any(|p| p == name))
            .map(|f| f.metrics.clone())
            .collect();
        aggregate(&metrics)
    };
    // The import graph for one profile: only that profile's modules, so resolution can't reach
    // across a profile boundary.
    let graph_of = |name: &str| {
        ImportGraph::build(
            module_inputs
                .iter()
                .filter(|(_, ps)| ps.iter().any(|p| p == name))
                .map(|(m, _)| m.clone())
                .collect(),
        )
    };
    // SLP020 clone detection, run once over every function (#123). Per-profile density is derived
    // by keeping only pairs whose *both* functions are in the profile — duplication internal to it,
    // consistent with how the import graph is scoped.
    let clone_pairs = find_clones(&clone_units, &clone_config);
    let clone_of = |name: &str| clone_stats_for(name, &unit_profiles, &clone_pairs);

    if let MetricsFormat::Functions = format {
        print_function_rows(&per_file, &scope);
    } else if let MetricsFormat::Classes = format {
        print_class_rows(&per_file, &scope);
    } else if let MetricsFormat::Packages = format {
        // For `all`, the graph is the whole tree; for one profile, just its modules.
        let graph = match &scope {
            Scope::All => {
                ImportGraph::build(module_inputs.iter().map(|(m, _)| m.clone()).collect())
            }
            Scope::One(name) => graph_of(name),
        };
        print_package_rows(&graph);
    } else {
        let proxies = test_proxies::aggregate_test_proxies(&test_stats);
        // The profile(s) this scope reports, in declaration order.
        let scoped: Vec<&str> = match &scope {
            Scope::All => profile_names.iter().map(String::as_str).collect(),
            Scope::One(name) => vec![name.as_str()],
        };
        match format {
            MetricsFormat::Text => {
                // One panel per in-scope profile; the proxies (always the full split) follow once.
                for name in &scoped {
                    print_metrics_panel(name, &panel_of(name));
                    // Package module-count concentration (#103) — node distribution, computed from
                    // the panel's own files (edge-free, so no import graph is needed in text mode).
                    print_concentration(&concentration_for(&per_file, name));
                    // Duplication density (#123): SLP020 clone ratio for the profile's functions.
                    print_clone_density(&clone_of(name));
                }
                print_test_proxies_table(&proxies);
            }
            // JSON is the comprehensive machine feed and ignores `--scope`: a panel for every
            // configured profile under `profiles`, plus the all-files `test_proxies`.
            MetricsFormat::Json => {
                let panels: Vec<(String, RepoMetrics, ImportGraph, CloneStats)> = profile_names
                    .iter()
                    .map(|name| (name.clone(), panel_of(name), graph_of(name), clone_of(name)))
                    .collect();
                println!("{}", metrics_json(&panels, &proxies));
            }
            MetricsFormat::Github => {
                let panels: Vec<(&str, RepoMetrics, CloneStats)> = scoped
                    .iter()
                    .map(|name| (*name, panel_of(name), clone_of(name)))
                    .collect();
                println!("{}", metrics_markdown(&panels, &proxies));
            }
            MetricsFormat::Functions | MetricsFormat::Classes | MetricsFormat::Packages => {
                unreachable!()
            }
        }
        if let Some(dir) = badges {
            // Badges report the scoped panel — the `default` profile by default, the quality
            // headline; for `all`, the whole tree.
            let repo = match &scope {
                Scope::All => aggregate(
                    &per_file
                        .iter()
                        .map(|f| f.metrics.clone())
                        .collect::<Vec<_>>(),
                ),
                Scope::One(name) => panel_of(name),
            };
            write_badges(dir, &repo, &config.badges)?;
        }
    }

    // CI gates: run both so all offenders are reported, then fail if either tripped.
    let over_cyclomatic = gate(&per_file, max_cyclomatic, "cyclomatic", |f| f.cyclomatic);
    let over_cognitive = gate(&per_file, max_cognitive, "cognitive", |f| f.cognitive);
    Ok(!over_cyclomatic && !over_cognitive)
}

/// Load the config for `metrics` (profiles + `[badges]`). An explicit `--config` is strict (a
/// parse error fails the run), but *discovery* is best-effort: an unrelated or malformed ancestor
/// `sloplint.toml` must not break `metrics`, so we fall back to the built-in defaults with a
/// warning.
fn load_metrics_config(config_path: Option<&str>) -> anyhow::Result<Config> {
    match config_path {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|e| anyhow!("reading config {path}: {e}"))?;
            Config::from_toml_str(&text).map_err(|e| anyhow!("parsing config {path}: {e}"))
        }
        None => {
            let cwd =
                env::current_dir().map_err(|e| anyhow!("resolving working directory: {e}"))?;
            match Config::discover(&cwd) {
                Ok(config) => Ok(config),
                Err(err) => {
                    eprintln!("sloplint: ignoring discovered config for metrics ({err})");
                    Ok(Config::default())
                }
            }
        }
    }
}

/// One complexity gate: report every function whose `metric` exceeds `ceiling` and return
/// whether any did. A `None` ceiling is a no-op (returns `false`).
fn gate(
    per_file: &[MeasuredFile],
    ceiling: Option<usize>,
    noun: &str,
    metric: impl Fn(&FunctionMetrics) -> usize,
) -> bool {
    let Some(ceiling) = ceiling else {
        return false;
    };
    let offenders = gate_offenders(per_file, ceiling, metric);
    if offenders.is_empty() {
        return false;
    }
    eprintln!(
        "sloplint: {} function(s) over the {noun} ceiling of {ceiling}:",
        offenders.len()
    );
    for offender in &offenders {
        eprintln!(
            "  {}: `{}` has {noun} complexity {}",
            offender.location, offender.name, offender.value
        );
    }
    true
}

/// A measured file: its display path, source, per-function metrics, and the names of the profiles
/// its path belongs to (#96 — used to place it into one or more metric panels).
struct MeasuredFile {
    path: String,
    source: String,
    metrics: FileMetrics,
    profiles: Vec<String>,
}

/// A function whose `metric` value exceeds the configured ceiling.
struct GateOffender {
    /// `path:line` of the function's `def` line (its name, not the first decorator).
    location: String,
    name: String,
    value: usize,
}

/// Collect every function whose `metric` exceeds `ceiling`, in file then source order
/// (deterministic).
fn gate_offenders(
    per_file: &[MeasuredFile],
    ceiling: usize,
    metric: impl Fn(&FunctionMetrics) -> usize,
) -> Vec<GateOffender> {
    let mut offenders = Vec::new();
    for file in per_file {
        for function in &file.metrics.functions {
            let value = metric(function);
            if value > ceiling {
                // Locate the `def` line via the name span — `range` would point at the first
                // decorator on a decorated function.
                let line = line_of(&file.source, function.name_range.start().into());
                offenders.push(GateOffender {
                    location: format!("{}:{line}", file.path),
                    name: function.name.clone(),
                    value,
                });
            }
        }
    }
    offenders
}

/// Emit one JSONL row per function: raw per-function features plus the enclosing file's
/// length and comment density. This is the discovery feed — `analyze.py` mines these rows
/// for features that separate the slop and clean cohorts.
fn print_function_rows(per_file: &[MeasuredFile], scope: &Scope) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for file in per_file.iter().filter(|f| scope.includes(&f.profiles)) {
        for function in &file.metrics.functions {
            let _ = writeln!(out, "{}", function_row(&file.path, &file.metrics, function));
        }
    }
}

/// Emit one JSONL row per class: size (methods, attributes) + LCOM4 cohesion. The class-level
/// discovery feed, mirroring `print_function_rows`.
fn print_class_rows(per_file: &[MeasuredFile], scope: &Scope) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for file in per_file.iter().filter(|f| scope.includes(&f.profiles)) {
        for class in &file.metrics.classes {
            let _ = writeln!(out, "{}", class_row(&file.path, class));
        }
    }
}

/// Build the JSONL row for one class. Split out so its shape can be unit-tested.
fn class_row(path: &str, class: &sloplint_metrics::ClassMetrics) -> serde_json::Value {
    serde_json::json!({
        "file": path,
        "class": class.name,
        "loc": class.loc,
        "methods": class.methods,
        "attributes": class.attributes,
        "lcom4": class.lcom4,
        "wmc": class.wmc,
        "dit": class.dit,
        // NOC (#113): direct first-party subclasses — inheritance breadth / fragile-base risk.
        "noc": class.noc,
        // CBO (#116): distinct first-party classes this one couples to — a lower bound in
        // dynamically-typed code (duck-typed coupling not counted).
        "cbo": class.cbo,
        "is_abstract": class.is_abstract,
        "has_docstring": class.has_docstring,
        "docstring_lines": class.docstring_lines,
    })
}

/// Emit one JSONL row per package: the first-party import graph collapsed to directory level.
/// The package-level discovery feed, mirroring `print_function_rows`/`print_class_rows`.
fn print_package_rows(graph: &ImportGraph) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for row in graph.package_rows() {
        let _ = writeln!(out, "{}", package_row(&row));
    }
}

/// Build the JSONL row for one package. Split out so its shape can be unit-tested.
///
/// `ce`/`ca` are Martin's efferent/afferent coupling (the counts of distinct first-party packages
/// in `imports`/`imported_by`); `instability` is `ce / (ce + ca)` (#67). `abstractness` and
/// `distance` are Martin's `A` and `D = |A + I − 1|` (#70, heuristic in Python). The named-package
/// lists are kept so a consumer can see *which* packages couple, not just how many.
fn package_row(row: &PackageRow) -> serde_json::Value {
    serde_json::json!({
        "package": row.package,
        "modules": row.modules,
        "loc": row.loc,
        "imports": row.imports,
        "imported_by": row.imported_by,
        "ce": row.imports.len(),
        "ca": row.imported_by.len(),
        "instability": row.instability,
        "in_cycle": row.in_cycle,
        "classes": row.classes,
        "abstract_classes": row.abstract_classes,
        "abstractness": row.abstractness,
        "distance": row.distance,
    })
}

/// Build the JSONL row for one function. Split out so its shape can be unit-tested.
fn function_row(
    path: &str,
    file: &FileMetrics,
    function: &sloplint_metrics::FunctionMetrics,
) -> serde_json::Value {
    let comment_density = if file.loc == 0 {
        0.0
    } else {
        file.comment_lines as f64 / file.loc as f64
    };
    serde_json::json!({
        "file": path,
        "function": function.name,
        "loc": function.loc,
        "ncss": function.ncss,
        "cyclomatic": function.cyclomatic,
        "cognitive": function.cognitive,
        "max_nesting": function.max_nesting,
        "params": function.params,
        // Caller-facing arity (#108): params minus the self/cls receiver — the Long-Parameter-List
        // signal. `*args`/`**kwargs` each count once.
        "arity": function.arity,
        "exits": function.exits,
        // Type-hint coverage (#85): annotated vs. annotatable params, and whether a return type is
        // declared. `annotatable_params` excludes the self/cls receiver and *args/**kwargs.
        "typed_params": function.typed_params,
        "annotatable_params": function.annotatable_params,
        "has_return_annotation": function.has_return_annotation,
        "has_docstring": function.has_docstring,
        "docstring_lines": function.docstring_lines,
        "file_loc": file.loc,
        "file_comment_density": comment_density,
    })
}

/// Print one labeled metric panel (#96) — the per-partition aggregates, without the test
/// proxies (those are the project-wide split and are printed once, after the panel(s)).
fn print_metrics_panel(label: &str, repo: &RepoMetrics) {
    println!("sloplint metrics — {label}");
    println!("  files               {}", repo.files);
    println!("  functions           {}", repo.functions);
    println!("  total lines         {}", repo.total_loc);
    println!("  avg function LoC    {:.1}", repo.avg_function_loc);
    println!("  max function LoC    {}", repo.max_function_loc);
    println!("  avg cyclomatic      {:.1}", repo.avg_cyclomatic);
    println!("  p95 cyclomatic      {}", repo.p95_cyclomatic);
    println!("  max cyclomatic      {}", repo.max_cyclomatic);
    let risk = repo.cyclomatic_risk;
    println!(
        "  CC risk tiers       low {} / moderate {} / high {} / very high {}",
        risk.low, risk.moderate, risk.high, risk.very_high
    );
    // Parameter count (caller-facing arity) distribution (#108): Long-Parameter-List prevalence.
    println!(
        "  avg/p95/max params  {:.1} / {} / {}",
        repo.avg_params, repo.p95_params, repo.max_params
    );
    let params = repo.param_count_risk;
    println!(
        "  arity bands         low {} / moderate {} / high {} / very high {}",
        params.low, params.moderate, params.high, params.very_high
    );
    // Cognitive complexity at parity with cyclomatic (#110) — the better readability signal.
    println!("  avg cognitive       {:.1}", repo.avg_cognitive);
    println!("  p95 cognitive       {}", repo.p95_cognitive);
    println!("  max cognitive       {}", repo.max_cognitive);
    let cog = repo.cognitive_risk;
    println!(
        "  CoCo risk tiers     low {} / moderate {} / high {} / very high {}",
        cog.low, cog.moderate, cog.high, cog.very_high
    );
    println!("  max nesting         {}", repo.max_nesting);
    println!("  comment density     {:.1}%", repo.comment_density * 100.0);
    println!(
        "  docstring coverage  {:.1}%",
        repo.docstring_coverage * 100.0
    );
    println!("  docstring/code      {:.2}", repo.docstring_code_ratio);
    // Exception-handling hygiene (#117): broad-except / silent-swallow rates.
    let exc = repo.exception;
    println!(
        "  except broad/swallow {:.2} / {:.2}  ({} broad, {} swallow, {} bare / {} handlers)",
        repo.broad_except_rate,
        repo.swallow_except_rate,
        exc.broad,
        exc.swallow,
        exc.bare,
        exc.handlers
    );
    // Class weight (WMC) distribution (#104): god-class prevalence, not just the worst class.
    println!("  classes             {}", repo.classes);
    println!(
        "  avg/p95/max WMC     {:.1} / {} / {}",
        repo.avg_wmc, repo.p95_wmc, repo.max_wmc
    );
    let wmc = repo.wmc_risk;
    println!(
        "  WMC bands           low {} / moderate {} / high {} / very high {}",
        wmc.low, wmc.moderate, wmc.high, wmc.very_high
    );
    // Inheritance breadth (NOC) distribution (#113): fragile-base-class prevalence.
    println!(
        "  avg/p95/max NOC     {:.1} / {} / {}",
        repo.avg_noc, repo.p95_noc, repo.max_noc
    );
    let noc = repo.noc_risk;
    println!(
        "  NOC bands           low {} / moderate {} / high {} / very high {}",
        noc.low, noc.moderate, noc.high, noc.very_high
    );
    // Class coupling (CBO) distribution (#116): hub-class prevalence (lower bound in dynamic code).
    println!(
        "  avg/p95/max CBO     {:.1} / {} / {}",
        repo.avg_cbo, repo.p95_cbo, repo.max_cbo
    );
    let cbo = repo.cbo_risk;
    println!(
        "  CBO bands           low {} / moderate {} / high {} / very high {}",
        cbo.low, cbo.moderate, cbo.high, cbo.very_high
    );
    // Module size (NLOC) distribution (#107): god-module prevalence — the third size leg.
    println!(
        "  avg/p95/max module  {:.1} / {} / {}  NLOC",
        repo.avg_module_nloc, repo.p95_module_nloc, repo.max_module_nloc
    );
    let module = repo.module_size_risk;
    println!(
        "  module NLOC bands   low {} / moderate {} / high {} / very high {}",
        module.low, module.moderate, module.high, module.very_high
    );
}

/// The package module-count concentration (#103) for one profile's files. Edge-free — it needs
/// only each module's package, so the text view computes it without building the import graph
/// (which would require an extra import-scan pass per file).
///
/// Modules are deduplicated by dotted name (last writer wins), exactly as `ImportGraph::build`
/// populates its node index: two files resolving to the same dotted name (e.g. `a.py` beside a
/// package `a/`) are one node there and must be one module here too — otherwise the text view would
/// disagree with the JSON feed and `--format packages`.
fn concentration_for(per_file: &[MeasuredFile], profile: &str) -> graph::Concentration {
    let mut modules: BTreeMap<String, bool> = BTreeMap::new();
    for file in per_file
        .iter()
        .filter(|f| f.profiles.iter().any(|p| p == profile))
    {
        if let Some(name) = module_name(Path::new(&file.path)) {
            modules.insert(name.dotted, name.is_package);
        }
    }
    let packages: Vec<String> = modules
        .into_iter()
        .map(|(dotted, is_package)| graph::package_of(&dotted, is_package))
        .collect();
    graph::concentration(&packages)
}

/// Print the package module-count concentration block (#103) beneath a metric panel: how piled the
/// modules are across packages, and which package holds the most. A descriptive distribution
/// statistic — never a gate (a small repo's one main package scores high and that's fine).
fn print_concentration(c: &graph::Concentration) {
    let largest = match &c.largest_package {
        Some((name, modules)) => format!("{name}, {modules}/{} modules", c.total_modules),
        None => "n/a".to_string(),
    };
    println!(
        "  max package share   {:.2}  ({largest})",
        c.max_package_share
    );
    println!(
        "  module-count gini   {:.2}  (over {} packages)",
        c.module_count_gini, c.packages
    );
}

/// Production duplication aggregate (#123): SLP020 clone density for one profile's functions —
/// surfacing the existing clone engine as a descriptive cohort metric, not new detection.
struct CloneStats {
    /// Confirmed SLP020 clone pairs whose *both* functions are in the profile.
    pairs: usize,
    /// Distinct functions appearing in at least one such pair.
    functions_in_clones: usize,
    /// Functions the clone engine considered for the profile — the ratio denominator.
    total_functions: usize,
    /// Functions in the largest connected clone cluster (a helper duplicated across N functions);
    /// 0 when there are no clones.
    largest_cluster: usize,
}

impl CloneStats {
    /// Fraction of the profile's functions that participate in at least one clone pair (0.0 when
    /// there are none). The headline duplication ratio — high for copy-paste codebases, ≈0 for
    /// clean ones.
    fn ratio(&self) -> f64 {
        if self.total_functions == 0 {
            0.0
        } else {
            self.functions_in_clones as f64 / self.total_functions as f64
        }
    }
}

/// Compute the clone density for `profile` from the project-wide SLP020 `pairs`, keeping only pairs
/// whose both functions belong to the profile (duplication internal to it). `largest_cluster` is
/// the biggest connected component of those pairs, via union-find (#123).
fn clone_stats_for(
    profile: &str,
    unit_profiles: &[Vec<String>],
    pairs: &[ClonePair],
) -> CloneStats {
    let in_profile = |idx: usize| unit_profiles[idx].iter().any(|p| p == profile);
    let total_functions = (0..unit_profiles.len()).filter(|&i| in_profile(i)).count();

    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut in_clones: HashSet<usize> = HashSet::new();
    let mut pair_count = 0usize;
    for pair in pairs {
        if in_profile(pair.a) && in_profile(pair.b) {
            pair_count += 1;
            in_clones.insert(pair.a);
            in_clones.insert(pair.b);
            let ra = dsu_find(&mut parent, pair.a);
            let rb = dsu_find(&mut parent, pair.b);
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    // Largest cluster = the biggest union-find component among clone members.
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for &node in &in_clones {
        let root = dsu_find(&mut parent, node);
        *sizes.entry(root).or_insert(0) += 1;
    }
    CloneStats {
        pairs: pair_count,
        functions_in_clones: in_clones.len(),
        total_functions,
        largest_cluster: sizes.values().copied().max().unwrap_or(0),
    }
}

/// Union-find root of `x` with path compression; inserts `x` (as its own root) on first touch.
fn dsu_find(parent: &mut HashMap<usize, usize>, x: usize) -> usize {
    let p = *parent.entry(x).or_insert(x);
    if p == x {
        return x;
    }
    let root = dsu_find(parent, p);
    parent.insert(x, root);
    root
}

/// Print the duplication-density block (#123) beneath a metric panel: the SLP020 clone ratio plus
/// the pair count and largest cluster. Descriptive — high duplication is a vibe-slop tell
/// ("a scraper per site" → copy-paste), but it's a cohort signal, never a per-repo gate.
fn print_clone_density(c: &CloneStats) {
    println!(
        "  clone ratio         {:.2}  ({} fns in clones / {} ; {} pairs, largest cluster {})",
        c.ratio(),
        c.functions_in_clones,
        c.total_functions,
        c.pairs,
        c.largest_cluster,
    );
}

/// Print the static test proxies block (#86) once, beneath the panel(s). Always the full
/// project-wide split (production vs test), independent of `--scope` — descriptive only, NOT
/// coverage and never a gate.
fn print_test_proxies_table(proxies: &TestProxies) {
    println!(
        "  test:code ratio     {}  ({} test / {} prod LoC)",
        opt_ratio(proxies.test_code_ratio),
        proxies.test_loc,
        proxies.production_loc,
    );
    println!(
        "  assertion density   {}  ({} assertions / {} test fns)",
        opt_ratio(proxies.assertion_density),
        proxies.assertions,
        proxies.test_functions,
    );
    println!(
        "  assertion-free rate {}  ({} of {} test fns assert nothing)",
        opt_ratio(proxies.assertion_free_rate),
        proxies.assertion_free_tests,
        proxies.test_functions,
    );
    println!("  (test proxies are static estimates, not coverage — descriptive only)");
}

/// Render an optional ratio: a fixed-precision number, or `n/a` when undefined (no denominator).
fn opt_ratio(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2}"),
        None => "n/a".to_string(),
    }
}

/// Assemble the full JSON feed (#96): a panel for **every** configured profile under `profiles`
/// (keyed by name), plus the project-wide `test_proxies` split (always over all files). `--scope`
/// does not affect this feed — it always reports every profile.
fn metrics_json(
    panels: &[(String, RepoMetrics, ImportGraph, CloneStats)],
    proxies: &TestProxies,
) -> String {
    let mut profiles = serde_json::Map::new();
    for (name, repo, graph, clone) in panels {
        profiles.insert(
            name.clone(),
            serde_json::Value::Object(panel_json(repo, graph, clone)),
        );
    }
    let mut root = serde_json::Map::new();
    root.insert("profiles".to_string(), serde_json::Value::Object(profiles));
    // Static test proxies (issue #86): test:code ratio + assertion density + assertion-free rate (#121/#127).
    root.insert("test_proxies".to_string(), test_proxies_json(proxies));
    serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap()
}

/// One metric panel as a JSON object (#96): every aggregate plus the import-graph rollup for the
/// panel's file set. Shared by every profile section so they stay identical in shape.
fn panel_json(
    repo: &RepoMetrics,
    graph: &ImportGraph,
    clone: &CloneStats,
) -> serde_json::Map<String, serde_json::Value> {
    let summary = graph.summary();
    let serde_json::Value::Object(map) = serde_json::json!({
        "files": repo.files,
        "functions": repo.functions,
        "total_loc": repo.total_loc,
        "avg_function_loc": repo.avg_function_loc,
        "max_function_loc": repo.max_function_loc,
        "avg_cyclomatic": repo.avg_cyclomatic,
        "p95_cyclomatic": repo.p95_cyclomatic,
        "max_cyclomatic": repo.max_cyclomatic,
        "cyclomatic_risk": {
            "low": repo.cyclomatic_risk.low,
            "moderate": repo.cyclomatic_risk.moderate,
            "high": repo.cyclomatic_risk.high,
            "very_high": repo.cyclomatic_risk.very_high,
        },
        // Parameter-count distribution (#108): Long Parameter List prevalence, which `avg` hides.
        // Caller-facing arity (self/cls excluded, *args/**kwargs once). Bands ≤4 / 5–6 / 7–10 / >10,
        // descriptive, never a gate.
        "params": {
            "avg": repo.avg_params,
            "max": repo.max_params,
            "p95": repo.p95_params,
        },
        "param_count_risk": {
            "low": repo.param_count_risk.low,
            "moderate": repo.param_count_risk.moderate,
            "high": repo.param_count_risk.high,
            "very_high": repo.param_count_risk.very_high,
        },
        // Cognitive complexity at parity with cyclomatic (#110): the readability distribution, not
        // just the max. The more readability-relevant of the two complexity metrics.
        "avg_cognitive": repo.avg_cognitive,
        "p95_cognitive": repo.p95_cognitive,
        "max_cognitive": repo.max_cognitive,
        "cognitive_risk": {
            "low": repo.cognitive_risk.low,
            "moderate": repo.cognitive_risk.moderate,
            "high": repo.cognitive_risk.high,
            "very_high": repo.cognitive_risk.very_high,
        },
        "max_nesting": repo.max_nesting,
        "comment_density": repo.comment_density,
        // Type-hint coverage (#85): a quality proxy for under-annotation. Low coverage is the
        // smell; high coverage is neutral (fully-typed code is not slop).
        "param_annotation_coverage": repo.param_annotation_coverage,
        "fully_annotated_function_rate": repo.fully_annotated_function_rate,
        // Module size distribution (#107): the third size leg. NLOC = non-comment, non-blank
        // lines per file; the band counts surface god-module *prevalence*, which `total_loc` and
        // `avg` collapse. Bands (≤250 / 251–500 / 501–1000 / >1000), descriptive, never a gate.
        "module_nloc": {
            "avg": repo.avg_module_nloc,
            "max": repo.max_module_nloc,
            "p95": repo.p95_module_nloc,
        },
        "module_size_risk": {
            "low": repo.module_size_risk.low,
            "moderate": repo.module_size_risk.moderate,
            "high": repo.module_size_risk.high,
            "very_high": repo.module_size_risk.very_high,
        },
        // CK class metrics (#84): WMC weight and first-party DIT depth, aggregated over all
        // classes. DIT is a conservative under-count — external (stdlib/third-party) ancestry is
        // invisible. Per-class rows live in `metrics --format classes`.
        "classes": repo.classes,
        "max_wmc": repo.max_wmc,
        "avg_wmc": repo.avg_wmc,
        // WMC size-band counts (#104): god-class *prevalence*, which avg/max collapse. Descriptive
        // bands (≤20 / 21–50 / 51–200 / >200), never a gate. p95 surfaces the heavy tail.
        "p95_wmc": repo.p95_wmc,
        "wmc_risk": {
            "low": repo.wmc_risk.low,
            "moderate": repo.wmc_risk.moderate,
            "high": repo.wmc_risk.high,
            "very_high": repo.wmc_risk.very_high,
        },
        "max_dit": repo.max_dit,
        "avg_dit": repo.avg_dit,
        // NOC (#113): inheritance breadth — direct first-party subclasses per class. The
        // fragile-base-class signal DIT (depth) can't see; band counts flag high-leverage bases.
        // Descriptive bands (≤1 / 2–5 / 6–20 / >20), never a gate.
        "max_noc": repo.max_noc,
        "avg_noc": repo.avg_noc,
        "p95_noc": repo.p95_noc,
        "noc_risk": {
            "low": repo.noc_risk.low,
            "moderate": repo.noc_risk.moderate,
            "high": repo.noc_risk.high,
            "very_high": repo.noc_risk.very_high,
        },
        // CBO (#116): class-to-class coupling — distinct first-party classes a class is wired to
        // (bases, instantiations, isinstance/issubclass, annotations). The class-level coupling the
        // package Ce/Ca can't localize; a LOWER BOUND in dynamically-typed code (duck-typed coupling
        // is invisible). Descriptive bands (≤4 / 5–9 / 10–20 / >20), never a gate.
        "max_cbo": repo.max_cbo,
        "avg_cbo": repo.avg_cbo,
        "p95_cbo": repo.p95_cbo,
        "cbo_risk": {
            "low": repo.cbo_risk.low,
            "moderate": repo.cbo_risk.moderate,
            "high": repo.cbo_risk.high,
            "very_high": repo.cbo_risk.very_high,
        },
        // Documentation coverage (#83) — distinct from comment_density (docstrings, not
        // `#`-comments). Low coverage = under-documented public API; a high docstring/code ratio
        // = AI over-documentation of trivia.
        "docstring_coverage": repo.docstring_coverage,
        "docstring_code_ratio": repo.docstring_code_ratio,
        // Exception-handling hygiene (#117): broad-except / silent-swallow rates over every
        // `except` handler. A cohort discriminator default Ruff can't aggregate; `swallow_rate` is
        // the strongest sub-signal. Descriptive, never a gate.
        "exception_handling": {
            "handlers": repo.exception.handlers,
            "bare": repo.exception.bare,
            "broad": repo.exception.broad,
            "swallow": repo.exception.swallow,
            "broad_rate": repo.broad_except_rate,
            "swallow_rate": repo.swallow_except_rate,
        },
        // Per-project import-graph rollup (foundation figures + cyclic-dependency tangles +
        // propagation cost + modularity).
        "packages": {
            "modules": summary.modules,
            "packages": summary.packages,
            "module_edges": summary.module_edges,
            "package_edges": summary.package_edges,
            "cycles": cycles_json(graph, summary.modules),
            // Whole-system coupling: density of the module reachability matrix (issue #68).
            "propagation_cost": graph.propagation_cost(),
            // Newman–Girvan modularity: declared package partition vs. detected (issue #69).
            "modularity": modularity_json(graph),
            // Node-distribution concentration: god-package / flat dumping-ground (issue #103).
            "concentration": concentration_json(graph),
        },
        // Duplication density (#123): SLP020 clone detection surfaced as a cohort aggregate.
        // `ratio` = fraction of the profile's functions in ≥1 clone pair; copy-paste codebases
        // (a scraper per site) score high, clean libraries ≈ 0. Descriptive, never a gate.
        "duplication": {
            "clone_ratio": clone.ratio(),
            "functions_in_clones": clone.functions_in_clones,
            "functions": clone.total_functions,
            "clone_pairs": clone.pairs,
            "largest_clone_cluster": clone.largest_cluster,
        },
    }) else {
        unreachable!("a json object literal is an object")
    };
    map
}

/// The test-proxies rollup for the JSON feed (issue #86). The `_note` is emitted inline so any
/// consumer of the raw JSON sees the caveat: these are *static estimates*, NOT coverage, and
/// must never be turned into a pass/fail gate. Undefined ratios (no production code / no test
/// functions) serialize as `null`, not `0`, so consumers don't mistake "undefined" for "zero".
fn test_proxies_json(proxies: &TestProxies) -> serde_json::Value {
    serde_json::json!({
        "_note": "Static proxies, NOT coverage. Descriptive cohort statistics only — never a \
                  pass/fail gate. Many asserts do not guarantee a meaningful test, and few do \
                  not prove a weak one.",
        "test_files": proxies.test_files,
        "production_files": proxies.production_files,
        "test_loc": proxies.test_loc,
        "production_loc": proxies.production_loc,
        "test_code_ratio": proxies.test_code_ratio,
        "test_functions": proxies.test_functions,
        "assertions": proxies.assertions,
        "assertion_density": proxies.assertion_density,
        // Test-substance (#121/#127): fraction of test functions that assert nothing ("test
        // theater"). High alongside a high test_code_ratio = a suite that looks tested but isn't.
        "assertion_free_tests": proxies.assertion_free_tests,
        "assertion_free_rate": proxies.assertion_free_rate,
    })
}

/// The modularity rollup for the JSON feed (issue #69): Q of the declared package partition, Q of
/// the Louvain-detected partition, their community counts, and the gap (detected − declared) — a
/// large gap means the declared package boundaries don't match the natural structure.
fn modularity_json(graph: &ImportGraph) -> serde_json::Value {
    let report = graph.modularity();
    serde_json::json!({
        "q_declared": report.q_declared,
        "communities_declared": report.communities_declared,
        "q_detected": report.q_detected,
        "communities_detected": report.communities_detected,
        "gap": report.gap(),
    })
}

/// The node-distribution concentration rollup for the JSON feed (issue #103): how modules are
/// piled across packages, the axis the edge-based metrics can't see. `largest_package` names the
/// offender (or `null` when there are no packages). Descriptive only — never a gate.
fn concentration_json(graph: &ImportGraph) -> serde_json::Value {
    let c = graph.concentration();
    serde_json::json!({
        "total_modules": c.total_modules,
        "packages": c.packages,
        "max_package_share": c.max_package_share,
        "module_count_gini": c.module_count_gini,
        "largest_package": c.largest_package.map(|(package, modules)| {
            serde_json::json!({ "package": package, "modules": modules })
        }),
    })
}

/// The cyclic-dependency (SCC) rollup for the JSON feed (issue #66): tangle counts over the
/// full graph, the same count over the runtime graph (TYPE_CHECKING-only edges dropped) and over
/// the load-bearing graph (function-local/deferred edges *also* dropped, #122), the share of
/// modules in cycles, and the member modules of each tangle.
fn cycles_json(graph: &ImportGraph, modules: usize) -> serde_json::Value {
    let report = graph.cycles();
    let in_cycles = report.modules_in_cycles();
    let pct = if modules == 0 {
        0.0
    } else {
        in_cycles as f64 / modules as f64
    };
    serde_json::json!({
        "tangles": report.tangle_count(),
        "largest_tangle": report.largest_tangle(),
        "modules_in_cycles": in_cycles,
        "pct_modules_in_cycles": pct,
        "runtime_tangles": graph.runtime_cycles().tangle_count(),
        // Hard cycles only: module-top-level runtime edges, dropping function-local/deferred imports
        // (#122). `0` ⇒ every cycle was deliberately deferred (milder smell); `> 0` ⇒ genuine
        // load-time circular dependencies that can raise `ImportError`. Not a strict subset of
        // `tangles` by count — dropping edges can split one SCC into several.
        "load_bearing_tangles": graph.load_bearing_cycles().tangle_count(),
        "members": report.tangles,
    })
}

/// GitHub-flavored markdown for the PR summary: the cyclomatic risk block from `sloplint_metrics`
/// for each in-scope profile (#96), under its own heading, then the test proxies. `--scope all`
/// renders one block per profile side by side — never a combined panel that would mix profiles'
/// norms. Pairs with the `cyclomatic-risk` badge.
fn metrics_markdown(panels: &[(&str, RepoMetrics, CloneStats)], proxies: &TestProxies) -> String {
    let mut out = String::from("### sloplint metrics\n\n");
    for (name, repo, clone) in panels {
        out.push_str(&format!(
            "#### {name}\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            repo.cyclomatic_markdown(),
            repo.cognitive_markdown(),
            repo.params_markdown(),
            repo.wmc_markdown(),
            repo.noc_markdown(),
            repo.cbo_markdown(),
            repo.module_size_markdown(),
            repo.exception_markdown(),
            clone_markdown(clone),
        ));
    }
    out.push_str(&test_proxies_markdown(proxies));
    out
}

/// A one-line markdown summary of duplication density (#123) — the SLP020 clone ratio with its
/// pair count and largest cluster. Descriptive cohort signal, never a gate.
fn clone_markdown(c: &CloneStats) -> String {
    format!(
        "**Duplication** — clone ratio {:.2} ({} of {} functions in SLP020 clone pairs; \
         {} pairs, largest cluster {}). Descriptive, never a gate.\n",
        c.ratio(),
        c.functions_in_clones,
        c.total_functions,
        c.pairs,
        c.largest_cluster,
    )
}

/// A markdown block for the static test proxies (#86), explicitly captioned as *proxies, not
/// coverage* so the PR summary can't be read as a gate.
fn test_proxies_markdown(proxies: &TestProxies) -> String {
    format!(
        "**Test proxies** (static estimates — _not coverage_, descriptive only) — \
         test:code ratio {} ({} test / {} prod LoC), assertion density {} ({} assertions over \
         {} test functions), assertion-free rate {} ({} of {} test functions assert nothing). A \
         high assertion-free rate next to a high test:code ratio flags a suite that looks tested \
         but verifies little. These suggest under-testing across a cohort; they are never a \
         per-repo pass/fail verdict.\n",
        opt_ratio(proxies.test_code_ratio),
        proxies.test_loc,
        proxies.production_loc,
        opt_ratio(proxies.assertion_density),
        proxies.assertions,
        proxies.test_functions,
        opt_ratio(proxies.assertion_free_rate),
        proxies.assertion_free_tests,
        proxies.test_functions,
    )
}

/// Badges for the headline metrics, each with a "lower is better" color threshold.
fn metric_badges(repo: &RepoMetrics) -> Vec<(&'static str, Badge)> {
    vec![
        (
            "avg-function-loc",
            Badge::new(
                "avg function LoC",
                format!("{:.0}", repo.avg_function_loc),
                Color::for_value(repo.avg_function_loc, 30.0, 60.0),
            ),
        ),
        (
            "max-cyclomatic",
            Badge::new(
                "max complexity",
                repo.max_cyclomatic.to_string(),
                Color::for_value(repo.max_cyclomatic as f64, 10.0, 20.0),
            ),
        ),
        // Headline cyclomatic risk, colored by McCabe's tier rather than a flat threshold.
        ("cyclomatic-risk", repo.cyclomatic_badge()),
        (
            "max-cognitive",
            Badge::new(
                "max cognitive",
                repo.max_cognitive.to_string(),
                Color::for_value(repo.max_cognitive as f64, 15.0, 30.0),
            ),
        ),
        // Headline cognitive risk, colored by SonarSource's band rather than a flat threshold (#110)
        // — the cognitive counterpart to `cyclomatic-risk`.
        ("cognitive-risk", repo.cognitive_badge()),
        (
            "max-nesting",
            Badge::new(
                "max nesting",
                repo.max_nesting.to_string(),
                Color::for_value(repo.max_nesting as f64, 4.0, 6.0),
            ),
        ),
        (
            "comment-density",
            Badge::new(
                "comment density",
                format!("{:.0}%", repo.comment_density * 100.0),
                Color::for_value(repo.comment_density * 100.0, 20.0, 40.0),
            ),
        ),
        // Documentation coverage (#83): higher is better, so green at high coverage.
        (
            "docstring-coverage",
            Badge::new(
                "docstring coverage",
                format!("{:.0}%", repo.docstring_coverage * 100.0),
                Color::for_value_high(repo.docstring_coverage * 100.0, 50.0, 80.0),
            ),
        ),
    ]
}

fn write_badges(dir: &str, repo: &RepoMetrics, settings: &BadgeSettings) -> anyhow::Result<()> {
    fs::create_dir_all(dir).map_err(|e| anyhow!("creating {dir}: {e}"))?;
    let all = metric_badges(repo);

    let mut written = 0usize;
    // Individual per-metric badges: `include` is None => all, Some(list) => only those.
    for (slug, badge) in &all {
        let keep = settings
            .include
            .as_ref()
            .is_none_or(|list| list.iter().any(|s| s.as_str() == *slug));
        if keep {
            write_badge_files(dir, slug, badge)?;
            written += 1;
        }
    }
    // One combined badge over the `summary` metrics, colored by the worst tier among them.
    if !settings.summary.is_empty() {
        let badge = summary_badge(&all, &settings.summary);
        // Skip if no slug resolved (e.g. all unknown) — an empty badge is meaningless.
        if !badge.message.is_empty() {
            write_badge_files(dir, "summary", &badge)?;
            written += 1;
        }
    }
    eprintln!("sloplint: wrote {written} badge(s) to {dir}");
    Ok(())
}

fn write_badge_files(dir: &str, slug: &str, badge: &Badge) -> anyhow::Result<()> {
    let svg_path = Path::new(dir).join(format!("{slug}.svg"));
    let json_path = Path::new(dir).join(format!("{slug}.json"));
    fs::write(&svg_path, badge.svg())
        .map_err(|e| anyhow!("writing {}: {e}", svg_path.display()))?;
    fs::write(&json_path, badge.endpoint_json())
        .map_err(|e| anyhow!("writing {}: {e}", json_path.display()))?;
    Ok(())
}

/// Combine the named metrics into a single `sloplint` badge, e.g. `CC 8 · CoCo 14 · density
/// 18%`, colored by the worst tier among them. Unknown slugs are skipped.
fn summary_badge(all: &[(&'static str, Badge)], slugs: &[String]) -> Badge {
    let mut parts = Vec::new();
    let mut worst = Color::Green;
    for slug in slugs {
        if let Some(entry) = all.iter().find(|e| e.0 == slug.as_str()) {
            parts.push(format!(
                "{} {}",
                badge_short_label(entry.0),
                entry.1.message
            ));
            if color_rank(entry.1.color) > color_rank(worst) {
                worst = entry.1.color;
            }
        }
    }
    Badge::new("sloplint", parts.join(" · "), worst)
}

/// Worst-is-highest ranking so a summary badge takes the most severe color among its metrics.
fn color_rank(color: Color) -> u8 {
    match color {
        Color::Green => 0,
        Color::Yellow => 1,
        Color::Red => 2,
    }
}

/// Short label for a metric slug, used in the combined summary badge.
fn badge_short_label(slug: &str) -> &str {
    match slug {
        "max-cyclomatic" => "CC",
        "cyclomatic-risk" => "risk",
        "max-cognitive" => "CoCo",
        "cognitive-risk" => "CoCo risk",
        "avg-function-loc" => "loc",
        "max-nesting" => "nesting",
        "comment-density" => "density",
        "docstring-coverage" => "docs",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_badges() -> Vec<(&'static str, Badge)> {
        vec![
            (
                "max-cyclomatic",
                Badge::new("max complexity", "8", Color::Green),
            ),
            (
                "max-cognitive",
                Badge::new("max cognitive", "14", Color::Yellow),
            ),
            (
                "comment-density",
                Badge::new("comment density", "18%", Color::Red),
            ),
        ]
    }

    #[test]
    fn summary_badge_joins_short_labels() {
        let badge = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "max-cognitive".to_string()],
        );
        assert_eq!(badge.label, "sloplint");
        assert_eq!(badge.message, "CC 8 · CoCo 14");
    }

    #[test]
    fn summary_badge_takes_the_worst_color() {
        // green + yellow -> yellow; adding red -> red.
        let two = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "max-cognitive".to_string()],
        );
        assert_eq!(two.color, Color::Yellow);
        let three = summary_badge(
            &sample_badges(),
            &[
                "max-cyclomatic".to_string(),
                "max-cognitive".to_string(),
                "comment-density".to_string(),
            ],
        );
        assert_eq!(three.color, Color::Red);
    }

    #[test]
    fn summary_badge_skips_unknown_slugs() {
        let badge = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "nope".to_string()],
        );
        assert_eq!(badge.message, "CC 8");
    }

    #[test]
    fn summary_badge_all_unknown_is_empty() {
        // All-unknown slugs -> empty message; write_badges skips emitting it.
        let badge = summary_badge(&sample_badges(), &["nope".to_string()]);
        assert!(badge.message.is_empty());
    }

    #[test]
    fn normalize_strips_leading_dot_slash() {
        assert_eq!(
            normalize(Path::new("./tests/t.py")),
            Path::new("tests/t.py")
        );
        assert_eq!(normalize(Path::new("tests/t.py")), Path::new("tests/t.py"));
        assert_eq!(normalize(Path::new("/abs/t.py")), Path::new("/abs/t.py"));
    }

    #[test]
    fn normalized_walk_paths_match_documented_globs() {
        // Regression: WalkBuilder::new(".") yields "./tests/t.py"; a `tests/**` profile glob
        // must still apply after normalization.
        let config = Config::from_toml_str(
            "[[profiles]]\nname = \"tests\"\nmatch = [\"tests/**\"]\nignore = [\"SLP010\"]\n\
             [[profiles]]\nname = \"production\"\ndefault = true\n",
        )
        .unwrap();
        let selector = config.prepare().unwrap();
        let walked = normalize(Path::new("./tests/t.py"));
        assert!(!selector.is_enabled("SLP010", &walked.to_string_lossy()));
    }

    #[test]
    fn first_party_under_collects_top_level_names_from_the_tree() {
        // SLP180 first-party detection walks the whole project root, so a single-file run
        // still resolves every local package. Build a small tree and check the names.
        let root = std::env::temp_dir().join(format!("sloplint-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::create_dir_all(root.join("src").join("other")).unwrap();
        std::fs::write(root.join("pkg").join("__init__.py"), "").unwrap();
        std::fs::write(root.join("pkg").join("mod.py"), "").unwrap();
        std::fs::write(root.join("src").join("other").join("__init__.py"), "").unwrap();
        std::fs::write(root.join("top.py"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap(); // non-Python ignored

        let names = first_party_under(&root);
        assert!(
            names.contains("pkg"),
            "package dir is first-party: {names:?}"
        );
        assert!(names.contains("other"), "src-layout package: {names:?}");
        assert!(names.contains("top"), "top-level module: {names:?}");
        assert!(
            !names.contains("src"),
            "the src root itself is not a package"
        );
        assert!(!names.contains("README"), "non-Python files are ignored");

        let _ = std::fs::remove_dir_all(&root);
    }

    fn empty_result(path: &str) -> FileResult {
        FileResult {
            path: path.to_string(),
            source: String::new(),
            diagnostics: Vec::new(),
            suppressions: Suppressions::empty(),
        }
    }

    #[test]
    fn function_row_has_features_and_file_comment_density() {
        let source =
            "# a comment\ndef f(a: int, b) -> str:\n    if a:\n        return b\n    return a\n";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = function_row("pkg/m.py", &metrics, &metrics.functions[0]);

        assert_eq!(row["file"], "pkg/m.py");
        assert_eq!(row["function"], "f");
        assert_eq!(row["params"], 2);
        assert!(
            row["cyclomatic"].as_u64().unwrap() >= 2,
            "the `if` is a branch"
        );
        // Type-hint coverage (#85): 1 of 2 params annotated, return type present.
        assert_eq!(row["typed_params"], 1);
        assert_eq!(row["annotatable_params"], 2);
        assert_eq!(row["has_return_annotation"], true);
        // 1 comment line over the file's physical lines.
        let density = row["file_comment_density"].as_f64().unwrap();
        assert!(density > 0.0 && density < 1.0, "got {density}");
        // `f` has no docstring (a `#`-comment is not a docstring).
        assert_eq!(row["has_docstring"], false);
        assert_eq!(row["docstring_lines"], 0);
    }

    #[test]
    fn function_row_reports_docstring_size() {
        let source = "def f():\n    \"\"\"Two\n    lines.\"\"\"\n    return 1\n";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = function_row("pkg/m.py", &metrics, &metrics.functions[0]);
        assert_eq!(row["has_docstring"], true);
        assert_eq!(row["docstring_lines"], 2);
    }

    #[test]
    fn class_row_has_size_and_cohesion_fields() {
        let source = "\
class Counter:
    def __init__(self):
        self.total = 0
    def add(self, n):
        self.total += n
    def show(self):
        return self.total
";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = class_row("pkg/m.py", &metrics.classes[0]);

        assert_eq!(row["file"], "pkg/m.py");
        assert_eq!(row["class"], "Counter");
        assert_eq!(row["methods"], 3);
        assert_eq!(row["attributes"], 1); // self.total
        assert_eq!(row["lcom4"], 1, "add/show share self.total");
        assert!(row["loc"].as_u64().unwrap() >= 7);
        assert_eq!(row["is_abstract"], false, "a plain concrete class");
        // No leading string literal in the class body, so no docstring.
        assert_eq!(row["has_docstring"], false);
        assert_eq!(row["docstring_lines"], 0);
    }

    #[test]
    fn package_row_has_module_count_and_coupling() {
        let instability = graph::instability(1, 2);
        let abstractness = graph::abstractness(1, 4); // 1 of 4 classes abstract
        let row = PackageRow {
            package: "pkg".to_string(),
            modules: 2,
            loc: 42,
            imports: vec!["pkg.sub".to_string()],
            imported_by: vec!["app".to_string(), "cli".to_string()],
            in_cycle: true,
            instability,
            classes: 4,
            abstract_classes: 1,
            abstractness,
            distance: graph::distance(abstractness, instability),
        };
        let value = package_row(&row);
        assert_eq!(value["package"], "pkg");
        assert_eq!(value["modules"], 2);
        assert_eq!(value["loc"], 42);
        assert_eq!(value["imports"], serde_json::json!(["pkg.sub"]));
        // Ce = 1 (pkg.sub), Ca = 2 (app, cli) → I = 1 / 3.
        assert_eq!(value["ce"], 1);
        assert_eq!(value["ca"], 2);
        assert_eq!(value["instability"], 1.0 / 3.0);
        assert_eq!(value["in_cycle"], true);
        // A = 1/4 = 0.25; D = |0.25 + 1/3 − 1| = |−0.41666…| = 0.41666…
        assert_eq!(value["classes"], 4);
        assert_eq!(value["abstract_classes"], 1);
        assert_eq!(value["abstractness"], 0.25);
        assert_eq!(value["distance"], (0.25 + 1.0 / 3.0 - 1.0_f64).abs());
    }

    #[test]
    fn fanout_flags_over_full_directory_once() {
        let config = Config::default();
        let selector = config.prepare().unwrap();

        let mut over: Vec<FileResult> = (0..5)
            .map(|i| empty_result(&format!("pkg/m{i}.py")))
            .collect();
        attribute_fanout(&mut over, &selector, 3);
        let flagged: usize = over.iter().map(|r| r.diagnostics.len()).sum();
        assert_eq!(flagged, 1, "exactly one SLP090 for the over-full directory");

        let mut under: Vec<FileResult> = (0..3)
            .map(|i| empty_result(&format!("pkg/m{i}.py")))
            .collect();
        attribute_fanout(&mut under, &selector, 3);
        assert_eq!(
            under.iter().map(|r| r.diagnostics.len()).sum::<usize>(),
            0,
            "at the limit is fine"
        );
    }
}

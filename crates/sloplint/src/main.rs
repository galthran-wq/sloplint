//! sloplint CLI.
//!
//! - `parse` — debug aid that dumps the AST and token stream for a file.
//! - `check` — discover config, run the shipped per-file rules over Python files, then
//!   run cross-file clone detection (SLP020), and report all findings.

mod imports;
mod stdlib;

mod hook;
mod init;

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use ignore::WalkBuilder;
use sloplint_clone::{extract_functions, find_clones, CloneConfig, FunctionUnit};
use sloplint_diagnostics::render::render_diagnostics;
use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_linter::config::{BadgeSettings, Config, Selector};
use sloplint_linter::lint::{check_file, FileContext, Rule};
use sloplint_linter::registry::Registry;
use sloplint_metrics::badge::{Badge, Color};
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
        } => {
            // Agent-loop mode: file path comes from stdin, output goes to stderr, and the exit
            // code (2) is what an editor's PostToolUse / afterFileEdit hook reads. The `paths`
            // and `format` args are ignored here — the contract is fixed.
            let _ = (paths, format);
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
        } => match run_check(&paths, config.as_deref(), preview, format) {
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
        } => match run_metrics(
            &paths,
            format,
            badges.as_deref(),
            config.as_deref(),
            max_cyclomatic,
            max_cognitive,
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
        limits: config.limits,
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

/// Returns `Ok(true)` when the run is clean, `Ok(false)` when there are findings or
/// per-file read/parse errors, and `Err` only when the run could not start (bad config).
fn run_check(
    paths: &[String],
    config_path: Option<&str>,
    preview: bool,
    format: Format,
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
        let parsed = match parse(&source) {
            Ok(parsed) => parsed,
            Err(err) => {
                eprintln!("error: {display}: {err}");
                had_error = true;
                continue;
            }
        };
        let rules = registry.enabled_for(&selector, &display);
        let refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();
        let ctx = FileContext {
            path: &display,
            source: &source,
            parsed: &parsed,
            limits: config.limits,
        };
        let diagnostics = check_file(&ctx, &refs);

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
        results.push(FileResult {
            path: display,
            source,
            diagnostics,
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

    Ok(findings == 0 && !had_error)
}

/// One file's parsed source and accumulated diagnostics.
struct FileResult {
    path: String,
    source: String,
    diagnostics: Vec<Diagnostic>,
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
) -> anyhow::Result<bool> {
    let (files, _) = discover_python_files(paths);
    // Keep path + source alongside metrics so the gate can name offending functions with a
    // resolved `path:line` location.
    let mut per_file: Vec<MeasuredFile> = Vec::new();
    for path in files {
        let display = path.to_string_lossy().to_string();
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = parse(&source) else {
            continue;
        };
        let metrics = file_metrics(&source, &parsed);
        per_file.push(MeasuredFile {
            path: display,
            source,
            metrics,
        });
    }

    if let MetricsFormat::Functions = format {
        print_function_rows(&per_file);
    } else {
        let just_metrics: Vec<FileMetrics> = per_file.iter().map(|f| f.metrics.clone()).collect();
        let repo = aggregate(&just_metrics);
        match format {
            MetricsFormat::Text => print_metrics_table(&repo),
            MetricsFormat::Json => println!("{}", metrics_json(&repo)),
            MetricsFormat::Github => println!("{}", metrics_markdown(&repo)),
            MetricsFormat::Functions => unreachable!(),
        }
        if let Some(dir) = badges {
            let settings = load_badge_settings(config_path)?;
            write_badges(dir, &repo, &settings)?;
        }
    }

    // CI gates: run both so all offenders are reported, then fail if either tripped.
    let over_cyclomatic = gate(&per_file, max_cyclomatic, "cyclomatic", |f| f.cyclomatic);
    let over_cognitive = gate(&per_file, max_cognitive, "cognitive", |f| f.cognitive);
    Ok(!over_cyclomatic && !over_cognitive)
}

/// Read `[badges]` from the config. An explicit `--config` is strict (a parse error fails the
/// run), but *discovery* is best-effort: an unrelated or malformed ancestor `sloplint.toml`
/// must not break `metrics --badges`, so we fall back to defaults with a warning.
fn load_badge_settings(config_path: Option<&str>) -> anyhow::Result<BadgeSettings> {
    match config_path {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|e| anyhow!("reading config {path}: {e}"))?;
            let config =
                Config::from_toml_str(&text).map_err(|e| anyhow!("parsing config {path}: {e}"))?;
            Ok(config.badges)
        }
        None => {
            let cwd =
                env::current_dir().map_err(|e| anyhow!("resolving working directory: {e}"))?;
            match Config::discover(&cwd) {
                Ok(config) => Ok(config.badges),
                Err(err) => {
                    eprintln!("sloplint: ignoring discovered config for badges ({err})");
                    Ok(BadgeSettings::default())
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

/// A measured file: its display path, source, and per-function metrics.
struct MeasuredFile {
    path: String,
    source: String,
    metrics: FileMetrics,
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
fn print_function_rows(per_file: &[MeasuredFile]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for file in per_file {
        for function in &file.metrics.functions {
            let _ = writeln!(out, "{}", function_row(&file.path, &file.metrics, function));
        }
    }
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
        "cyclomatic": function.cyclomatic,
        "cognitive": function.cognitive,
        "max_nesting": function.max_nesting,
        "params": function.params,
        "file_loc": file.loc,
        "file_comment_density": comment_density,
    })
}

fn print_metrics_table(repo: &RepoMetrics) {
    println!("sloplint metrics");
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
    println!("  max cognitive       {}", repo.max_cognitive);
    println!("  max nesting         {}", repo.max_nesting);
    println!("  comment density     {:.1}%", repo.comment_density * 100.0);
}

fn metrics_json(repo: &RepoMetrics) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
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
        "max_cognitive": repo.max_cognitive,
        "max_nesting": repo.max_nesting,
        "comment_density": repo.comment_density,
    }))
    .unwrap()
}

/// GitHub-flavored markdown for the PR summary: the cyclomatic risk block from
/// `sloplint_metrics`, under a heading. Pairs with the `cyclomatic-risk` badge.
fn metrics_markdown(repo: &RepoMetrics) -> String {
    format!("### sloplint metrics\n\n{}", repo.cyclomatic_markdown())
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
        "avg-function-loc" => "loc",
        "max-nesting" => "nesting",
        "comment-density" => "density",
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
        // Regression: WalkBuilder::new(".") yields "./tests/t.py"; a `tests/**` override
        // must still apply after normalization.
        let config =
            Config::from_toml_str("[[overrides]]\npath = \"tests/**\"\nignore = [\"SLP010\"]\n")
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
        }
    }

    #[test]
    fn function_row_has_features_and_file_comment_density() {
        let source = "# a comment\ndef f(a, b):\n    if a:\n        return b\n    return a\n";
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
        // 1 comment line over the file's physical lines.
        let density = row["file_comment_density"].as_f64().unwrap();
        assert!(density > 0.0 && density < 1.0, "got {density}");
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

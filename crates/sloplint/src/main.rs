//! sloplint CLI.
//!
//! - `parse` — debug aid that dumps the AST and token stream for a file.
//! - `check` — discover config, run the shipped per-file rules over Python files, then
//!   run cross-file clone detection (SLP020), and report all findings.

// The `metrics --format json` panel is one large `serde_json::json!` literal; as it has grown
// (the per-unit metric distributions) it exceeds the default macro recursion limit.
#![recursion_limit = "256"]

mod args;
mod badges;
mod commands;
mod compute;
mod corrupted;
mod cross_file;
mod discover;
mod gates;
mod hook;
mod init;
mod output;
mod results;
mod rule_docs;
pub(crate) use args::Scope;
use args::{Cli, Command, Format, InitTool, RuleFormat};
use cross_file::{
    attribute_clones, attribute_fanout, attribute_ghost_scaffolding, attribute_undeclared_imports,
};
pub(crate) use discover::module_name;
use discover::{discover_python_files, is_python};
pub(crate) use results::{CloneStats, FileResult, MeasuredFile};

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::anyhow;
use clap::Parser;
use ignore::WalkBuilder;
use sloplint_clone::{extract_functions, CloneConfig, FunctionUnit};
use sloplint_diagnostics::fix;
use sloplint_diagnostics::render::render_diagnostics;
use sloplint_linter::clones;
use sloplint_linter::config::{Config, Selector};
use sloplint_linter::ghost;
use sloplint_linter::imports;
use sloplint_linter::lint::{check_file, FileContext, Rule};
use sloplint_linter::registry::{Registry, WholeProjectRule};
use sloplint_linter::suppression::Suppressions;
use sloplint_python::{parse, Ranged};
use sloplint_report::ReportEntry;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Parse { file } => match run_parse(&file) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => tool_error(err),
        },
        Command::Rule { rule, format } => {
            match rule_docs::run_rule(rule.as_deref(), matches!(format, RuleFormat::Json)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => tool_error(err),
            }
        }
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
        } => match commands::metrics::run_metrics(
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

/// Load the config: from `config_path` if given, else discovered from the cwd up. When
/// `strict_discovery` is false (the `metrics` command), a discovery error degrades to the
/// default config with a warning rather than failing — metrics should still run without a
/// readable config. `preview` forces preview rules on.
pub(crate) fn load_config(
    config_path: Option<&str>,
    preview: bool,
    strict_discovery: bool,
) -> anyhow::Result<Config> {
    let mut config = match config_path {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|e| anyhow!("reading config {path}: {e}"))?;
            Config::from_toml_str(&text).map_err(|e| anyhow!("parsing config {path}: {e}"))?
        }
        None => {
            let cwd =
                env::current_dir().map_err(|e| anyhow!("resolving working directory: {e}"))?;
            match Config::discover(&cwd) {
                Ok(config) => config,
                Err(err) if strict_discovery => return Err(err.into()),
                Err(err) => {
                    eprintln!("sloplint: ignoring discovered config for metrics ({err})");
                    Config::default()
                }
            }
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

    let config = load_config(config_path, preview, true)?;
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
        // Per-file thresholds: the file's profile deltas over the global limits.
        limits: selector.limits(&display),
        security_extra: &config.security.extra,
        placeholders_extra: &config.placeholders.extra,
        comment_phrases_extra: &config.comments.extra,
        crosslang_allow: &config.crosslang.allow,
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

/// Per-file lint pass for `check`: read+parse each file, run the enabled per-file rules,
/// and collect the inputs the whole-project passes need — clone units (SLP020), import
/// scans (SLP180), and ghost scans (SLP240). `had_error` flags any read/parse failure.
struct Scanned {
    results: Vec<FileResult>,
    units: Vec<FunctionUnit>,
    unit_result: Vec<usize>,
    import_scans: Vec<(String, Vec<imports::ImportRef>)>,
    ghost_scans: Vec<ghost::FileScan>,
    had_error: bool,
}

fn scan_files(
    files: &[PathBuf],
    selector: &Selector,
    registry: &Registry,
    clone_config: &CloneConfig,
    config: &Config,
) -> Scanned {
    let mut had_error = false;
    // Per-file results are collected first; cross-file clone detection (SLP020) needs every
    // file's functions before it can report duplicates, so we render only at the end.
    let mut results: Vec<FileResult> = Vec::new();
    let mut units: Vec<FunctionUnit> = Vec::new();
    let mut unit_result: Vec<usize> = Vec::new();
    // SLP180 (preview) is a whole-project rule: collect every file's module-level imports,
    // then resolve them against the manifest after the loop.
    let mut import_scans: Vec<(String, Vec<imports::ImportRef>)> = Vec::new();
    // SLP240 (preview) is a whole-project rule: collect each file's defs/refs/config keys, then
    // resolve ghost (unreferenced) scaffolding after the loop.
    let mut ghost_scans: Vec<ghost::FileScan> = Vec::new();

    for path in files {
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
        let slp220 = selector.preview()
            && selector.is_enabled(sloplint_linter::corrupted::Corrupted.code(), &display);
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
        let rules = registry.enabled_for(selector, &display);
        let refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();
        let ctx = FileContext {
            path: &display,
            source: &source,
            parsed: &parsed,
            // Per-file thresholds: the file's profile deltas over the global limits.
            limits: selector.limits(&display),
            security_extra: &config.security.extra,
            placeholders_extra: &config.placeholders.extra,
            comment_phrases_extra: &config.comments.extra,
            crosslang_allow: &config.crosslang.allow,
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
        if selector.is_enabled(clones::Clones.code(), &display) {
            for unit in extract_functions(&display, &source, &parsed, clone_config.shingle_k) {
                units.push(unit);
                unit_result.push(result_index);
            }
        }
        // Collect imports for all files when preview is on; emission is gated per-path later.
        if selector.preview() {
            import_scans.push((display.clone(), imports::scan_imports(&parsed)));
            ghost_scans.push(ghost::scan(&display, &parsed));
        }
        let suppressions = Suppressions::parse(&source, &parsed);
        results.push(FileResult {
            path: display,
            source,
            diagnostics,
            suppressions,
        });
    }
    Scanned {
        results,
        units,
        unit_result,
        import_scans,
        ghost_scans,
        had_error,
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
    let config = load_config(config_path, preview, true)?;
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

    let Scanned {
        mut results,
        units,
        unit_result,
        import_scans,
        ghost_scans,
        had_error: scan_err,
    } = scan_files(&files, &selector, &registry, &clone_config, &config);
    had_error |= scan_err;

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
        attribute_ghost_scaffolding(&ghost_scans, &selector, &mut results);
    }

    // Inline `# sloplint: allow` suppression runs last, so it filters whole-tree findings
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

/// First-party (project-local) top-level module names found by walking the project `root`.
///
/// Honors `.gitignore` (so `.venv/` etc. are skipped) via the same `ignore` walker used for
/// discovery. Names are computed from paths relative to `root`. Over-collecting is safe — a
/// name treated as first-party is never flagged, preserving the false-negative bias.
pub(crate) fn first_party_under(root: &Path) -> std::collections::HashSet<String> {
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
pub(crate) fn line_of(source: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(source.len());
    source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

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

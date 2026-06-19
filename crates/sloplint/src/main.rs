//! sloplint CLI.
//!
//! - `parse` — debug aid that dumps the AST and token stream for a file.
//! - `check` — discover config, run the shipped per-file rules over Python files, then
//!   run cross-file clone detection (SLP020), and report all findings.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use ignore::WalkBuilder;
use sloplint_clone::{extract_functions, find_clones, CloneConfig, FunctionUnit};
use sloplint_diagnostics::render::render_diagnostics;
use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_linter::config::{Config, Selector};
use sloplint_linter::lint::{check_file, FileContext, Rule};
use sloplint_linter::registry::Registry;
use sloplint_metrics::badge::{Badge, Color};
use sloplint_metrics::{aggregate, file_metrics, FileMetrics, RepoMetrics};
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
}

/// Output format for `metrics`.
#[derive(Clone, Copy, clap::ValueEnum)]
enum MetricsFormat {
    Text,
    Json,
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
        } => match run_check(&paths, config.as_deref(), preview, format) {
            Ok(true) => ExitCode::SUCCESS,  // clean
            Ok(false) => ExitCode::from(1), // findings or read/parse errors
            Err(err) => tool_error(err),    // could not run at all
        },
        Command::Metrics {
            paths,
            format,
            badges,
        } => match run_metrics(&paths, format, badges.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
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

/// Returns `Ok(true)` when the run is clean, `Ok(false)` when there are findings or
/// per-file read/parse errors, and `Err` only when the run could not start (bad config).
fn run_check(
    paths: &[String],
    config_path: Option<&str>,
    preview: bool,
    format: Format,
) -> anyhow::Result<bool> {
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

    // Per-file results are collected first; cross-file clone detection (SLP020) needs every
    // file's functions before it can report duplicates, so we render only at the end.
    let mut results: Vec<FileResult> = Vec::new();
    let mut units: Vec<FunctionUnit> = Vec::new();
    let mut unit_result: Vec<usize> = Vec::new();

    for path in files {
        let display = path.to_string_lossy().to_string();
        let source = match fs::read_to_string(&path) {
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
            placeholders: &config.placeholders.extra,
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
        results.push(FileResult {
            path: display,
            source,
            diagnostics,
        });
    }

    attribute_clones(&units, &unit_result, &clone_config, &mut results);
    attribute_fanout(&mut results, &selector, config.limits.dir_max_modules);

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

/// Compute and report software-quality metrics; optionally emit badges.
fn run_metrics(
    paths: &[String],
    format: MetricsFormat,
    badges: Option<&str>,
) -> anyhow::Result<()> {
    let (files, _) = discover_python_files(paths);
    let mut per_file: Vec<FileMetrics> = Vec::new();
    for path in files {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = parse(&source) else {
            continue;
        };
        per_file.push(file_metrics(&source, &parsed));
    }
    let repo = aggregate(&per_file);

    match format {
        MetricsFormat::Text => print_metrics_table(&repo),
        MetricsFormat::Json => println!("{}", metrics_json(&repo)),
    }

    if let Some(dir) = badges {
        write_badges(dir, &repo)?;
    }
    Ok(())
}

fn print_metrics_table(repo: &RepoMetrics) {
    println!("sloplint metrics");
    println!("  files               {}", repo.files);
    println!("  functions           {}", repo.functions);
    println!("  total lines         {}", repo.total_loc);
    println!("  avg function LoC    {:.1}", repo.avg_function_loc);
    println!("  max function LoC    {}", repo.max_function_loc);
    println!("  max cyclomatic      {}", repo.max_cyclomatic);
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
        "max_cyclomatic": repo.max_cyclomatic,
        "max_cognitive": repo.max_cognitive,
        "max_nesting": repo.max_nesting,
        "comment_density": repo.comment_density,
    }))
    .unwrap()
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

fn write_badges(dir: &str, repo: &RepoMetrics) -> anyhow::Result<()> {
    fs::create_dir_all(dir).map_err(|e| anyhow!("creating {dir}: {e}"))?;
    let badges = metric_badges(repo);
    for (slug, badge) in &badges {
        let svg_path = Path::new(dir).join(format!("{slug}.svg"));
        let json_path = Path::new(dir).join(format!("{slug}.json"));
        fs::write(&svg_path, badge.svg())
            .map_err(|e| anyhow!("writing {}: {e}", svg_path.display()))?;
        fs::write(&json_path, badge.endpoint_json())
            .map_err(|e| anyhow!("writing {}: {e}", json_path.display()))?;
    }
    eprintln!("sloplint: wrote {} badges to {dir}", badges.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn empty_result(path: &str) -> FileResult {
        FileResult {
            path: path.to_string(),
            source: String::new(),
            diagnostics: Vec::new(),
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

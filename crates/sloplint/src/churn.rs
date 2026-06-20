//! `sloplint churn` — git-history churn signals (issue #18).
//!
//! Two signals a single static snapshot can't see, computed from git history (intended to run
//! in CI, but a deterministic subcommand so it's testable and reusable):
//!
//! - **Volatility**: over a history window, the dispersion of per-file change frequency —
//!   the coefficient of variation (stddev / mean) of how many commits touch each file. Low =
//!   cohesive, churn spread evenly; high = a few files regenerated wholesale commit after
//!   commit while the rest lie abandoned. Rendered as a badge (lower is better).
//! - **Churn-vs-delta anomaly**: for a PR diff, files whose churn (`+`/`-` lines) vastly
//!   exceeds their net delta (added − deleted) — code rewritten in place without adding
//!   capability. Motivated by Munson & Elbaum (1998): replacing modules with equivalent ones
//!   leaves the delta near zero while churn is large.
//!
//! Both restrict to `.py` files (this is a Python tool) and are deterministic given the commit
//! range. The pure parsing/scoring lives here and is unit-tested; `run` adds the git I/O.

use std::process::Command;

use anyhow::{anyhow, Context};
use sloplint_metrics::badge::{Badge, Color};

/// One `git numstat` row: a file touched, with lines added/deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub added: u64,
    pub deleted: u64,
}

/// A `Change` tagged with the index of the commit it belongs to (so distinct commits per file
/// can be counted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub commit: usize,
    pub change: Change,
}

/// Per-file history summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChurn {
    pub path: String,
    /// Number of distinct commits that touched the file (its change frequency).
    pub commits: usize,
    /// Total lines added + deleted across those commits.
    pub churn: u64,
}

/// A file rewritten far more than it changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    pub path: String,
    pub churn: u64,
    pub delta: i64,
    pub ratio: f64,
}

/// Parse one numstat row (`<added>\t<deleted>\t<path>`). Returns `None` for commit-hash lines,
/// blanks, and binary rows (`-\t-\tpath`).
fn parse_row(line: &str) -> Option<Change> {
    let mut fields = line.splitn(3, '\t');
    let added = fields.next()?.parse::<u64>().ok()?;
    let deleted = fields.next()?.parse::<u64>().ok()?;
    let path = fields.next()?;
    // Defense in depth: the git invocations pass `--no-renames`, but if a rename row
    // (`old => new`) ever reaches here its path is garbage, so skip it rather than count it.
    if path.is_empty() || path.contains(" => ") {
        return None;
    }
    Some(Change {
        path: path.to_string(),
        added,
        deleted,
    })
}

/// Parse `git log --numstat --format=%H` output. Every non-row, non-blank line (a commit hash)
/// starts a new commit; the numstat rows under it are tagged with that commit's index.
pub fn parse_log_numstat(log: &str) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    let mut commit = 0usize;
    for line in log.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_row(line) {
            Some(change) => entries.push(LogEntry { commit, change }),
            // A non-row, non-blank line is a commit boundary (the `%H` hash line).
            None => commit += 1,
        }
    }
    entries
}

/// Parse `git diff --numstat` output (just numstat rows, no commit lines).
pub fn parse_diff_numstat(diff: &str) -> Vec<Change> {
    diff.lines().filter_map(parse_row).collect()
}

/// Collapse log entries into per-file summaries, sorted by churn (desc) then path (asc) so
/// output is deterministic.
pub fn file_churn(entries: &[LogEntry]) -> Vec<FileChurn> {
    use std::collections::BTreeMap;
    // BTreeMap keeps a stable path order before the churn sort.
    let mut by_path: BTreeMap<&str, (Vec<usize>, u64)> = BTreeMap::new();
    for entry in entries {
        let slot = by_path.entry(entry.change.path.as_str()).or_default();
        slot.0.push(entry.commit);
        slot.1 += entry.change.added + entry.change.deleted;
    }
    let mut files: Vec<FileChurn> = by_path
        .into_iter()
        .map(|(path, (mut commits, churn))| {
            commits.sort_unstable();
            commits.dedup();
            FileChurn {
                path: path.to_string(),
                commits: commits.len(),
                churn,
            }
        })
        .collect();
    files.sort_by(|a, b| b.churn.cmp(&a.churn).then_with(|| a.path.cmp(&b.path)));
    files
}

/// Volatility = coefficient of variation (stddev / mean) of per-file commit counts. `0.0` when
/// there are no files or every file changed equally. Higher = churn concentrated in few files.
pub fn volatility(files: &[FileChurn]) -> f64 {
    if files.is_empty() {
        return 0.0;
    }
    let counts: Vec<f64> = files.iter().map(|f| f.commits as f64).collect();
    let n = counts.len() as f64;
    let mean = counts.iter().sum::<f64>() / n;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt() / mean
}

/// A "volatility" badge, colored lower-is-better.
pub fn volatility_badge(cv: f64) -> Badge {
    Badge::new(
        "volatility",
        format!("{cv:.2}"),
        Color::for_value(cv, 1.0, 1.75),
    )
}

/// Files whose churn (`added + deleted`) is at least `min_churn` and at least `max_ratio` times
/// their `|net delta|` — rewritten far more than they changed. Sorted by ratio (desc).
pub fn anomalies(changes: &[Change], min_churn: u64, max_ratio: f64) -> Vec<Anomaly> {
    let mut out: Vec<Anomaly> = changes
        .iter()
        .filter_map(|c| {
            let churn = c.added + c.deleted;
            let delta = c.added as i64 - c.deleted as i64;
            // |delta| floored at 1 so a pure in-place rewrite (delta 0) gets the full ratio.
            let ratio = churn as f64 / delta.unsigned_abs().max(1) as f64;
            (churn >= min_churn && ratio >= max_ratio).then_some(Anomaly {
                path: c.path.clone(),
                churn,
                delta,
                ratio,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.ratio
            .partial_cmp(&a.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

fn is_python(path: &str) -> bool {
    path.ends_with(".py")
}

// ---- git I/O + rendering -------------------------------------------------------------------

/// Output format for `churn`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ChurnFormat {
    /// Human-readable text (default).
    Text,
    /// JSON object.
    Json,
    /// GitHub-flavored markdown (for a PR summary).
    Github,
}

/// Arguments for the `churn` subcommand.
pub struct ChurnArgs<'a> {
    pub repo: &'a str,
    /// Limit history to the last N commits (`None` = full history).
    pub window: Option<usize>,
    /// If set, also report churn-vs-delta anomalies for `base...HEAD`.
    pub base: Option<&'a str>,
    pub min_churn: u64,
    pub anomaly_ratio: f64,
    pub format: ChurnFormat,
    /// If set, write `volatility.svg` + `volatility.json` into this directory.
    pub badges: Option<&'a str>,
}

fn git_output(repo: &str, args: &[&str]) -> anyhow::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        // Emit raw UTF-8 paths (no `"\303\251"` octal-quoting) so non-ASCII filenames survive.
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .with_context(|| format!("running `git {}`", args.join(" ")))
}

fn git(repo: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = git_output(repo, args)?;
    if !output.status.success() {
        return Err(anyhow!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

/// `git log` output, treating a repo with no commits yet as empty history (so the Action
/// doesn't fail on a brand-new repo's first PR). Other git failures still propagate.
fn git_log(repo: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = git_output(repo, args)?;
    if output.status.success() {
        return String::from_utf8(output.stdout).context("git output was not valid UTF-8");
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("does not have any commits") {
        return Ok(String::new());
    }
    Err(anyhow!("`git log` failed: {}", stderr.trim()))
}

/// Run the churn analysis: read git history, compute volatility (+ optional anomalies), render.
pub fn run(args: ChurnArgs) -> anyhow::Result<()> {
    let window_flag = args.window.map(|n| format!("-n{n}"));
    // `--no-renames` keeps numstat paths plain (a rename becomes delete + add) instead of
    // `old => new`, which would otherwise pollute the per-file counts.
    let mut log_args = vec![
        "log",
        "--no-merges",
        "--no-renames",
        "--numstat",
        "--format=%H",
    ];
    if let Some(flag) = &window_flag {
        log_args.push(flag);
    }
    let log = git_log(args.repo, &log_args)?;
    let entries: Vec<LogEntry> = parse_log_numstat(&log)
        .into_iter()
        .filter(|e| is_python(&e.change.path))
        .collect();
    let files = file_churn(&entries);
    let cv = volatility(&files);
    let window_desc = match args.window {
        Some(n) => format!("last {n} commits"),
        None => "full history".to_string(),
    };

    let anomalies = match args.base {
        Some(base) => {
            let diff = git(
                args.repo,
                &[
                    "diff",
                    "--numstat",
                    "--no-renames",
                    &format!("{base}...HEAD"),
                ],
            )?;
            let changes: Vec<Change> = parse_diff_numstat(&diff)
                .into_iter()
                .filter(|c| is_python(&c.path))
                .collect();
            Some((
                base.to_string(),
                self::anomalies(&changes, args.min_churn, args.anomaly_ratio),
            ))
        }
        None => None,
    };

    if let Some(dir) = args.badges {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {dir}"))?;
        let badge = volatility_badge(cv);
        std::fs::write(format!("{dir}/volatility.svg"), badge.svg())
            .with_context(|| format!("writing {dir}/volatility.svg"))?;
        std::fs::write(format!("{dir}/volatility.json"), badge.endpoint_json())
            .with_context(|| format!("writing {dir}/volatility.json"))?;
        eprintln!("sloplint: wrote volatility badge to {dir}");
    }

    match args.format {
        ChurnFormat::Text => print_text(cv, &window_desc, &files, anomalies.as_ref()),
        ChurnFormat::Json => println!(
            "{}",
            render_json(cv, &window_desc, &files, anomalies.as_ref())
        ),
        ChurnFormat::Github => print_github(cv, &window_desc, &files, anomalies.as_ref()),
    }
    Ok(())
}

const TOP_N: usize = 10;

fn print_text(
    cv: f64,
    window: &str,
    files: &[FileChurn],
    anomalies: Option<&(String, Vec<Anomaly>)>,
) {
    println!(
        "Volatility: {cv:.2} (coefficient of variation of per-file commit counts over {window})"
    );
    if !files.is_empty() {
        println!("Top-churn files:");
        for f in files.iter().take(TOP_N) {
            println!(
                "  {:>4} commits  {:>6} lines  {}",
                f.commits, f.churn, f.path
            );
        }
    }
    if let Some((base, anomalies)) = anomalies {
        println!("\nChurn-vs-delta anomalies ({base}...HEAD):");
        if anomalies.is_empty() {
            println!("  none");
        }
        for a in anomalies {
            println!(
                "  {:>5.1}x  churn {:>5}  net {:+}  {}",
                a.ratio, a.churn, a.delta, a.path
            );
        }
    }
}

fn print_github(
    cv: f64,
    window: &str,
    files: &[FileChurn],
    anomalies: Option<&(String, Vec<Anomaly>)>,
) {
    let color = volatility_badge(cv).color.keyword();
    println!("### Code volatility\n");
    println!("**{cv:.2}** — coefficient of variation of per-file commit counts ({window}, `{color}`). Lower is more cohesive.\n");
    if !files.is_empty() {
        println!("<details><summary>Top-churn files</summary>\n");
        println!("| commits | churn | file |");
        println!("| ---: | ---: | --- |");
        for f in files.iter().take(TOP_N) {
            println!("| {} | {} | `{}` |", f.commits, f.churn, f.path);
        }
        println!("\n</details>\n");
    }
    if let Some((base, anomalies)) = anomalies {
        println!("### Churn-vs-delta anomalies\n");
        if anomalies.is_empty() {
            println!("No files rewritten far more than they changed (`{base}...HEAD`). ✅\n");
        } else {
            println!("Files rewritten far more than they changed — large churn, near-zero net delta (`{base}...HEAD`):\n");
            println!("| ratio | churn | net Δ | file |");
            println!("| ---: | ---: | ---: | --- |");
            for a in anomalies {
                println!(
                    "| {:.1}× | {} | {:+} | `{}` |",
                    a.ratio, a.churn, a.delta, a.path
                );
            }
            println!();
        }
    }
}

fn render_json(
    cv: f64,
    window: &str,
    files: &[FileChurn],
    anomalies: Option<&(String, Vec<Anomaly>)>,
) -> String {
    let top: Vec<serde_json::Value> = files
        .iter()
        .take(TOP_N)
        .map(|f| serde_json::json!({ "path": f.path, "commits": f.commits, "churn": f.churn }))
        .collect();
    let mut root = serde_json::json!({
        "volatility": { "cv": cv, "window": window, "top_churn": top },
    });
    if let Some((base, anomalies)) = anomalies {
        let items: Vec<serde_json::Value> = anomalies
            .iter()
            .map(|a| serde_json::json!({ "path": a.path, "churn": a.churn, "delta": a.delta, "ratio": a.ratio }))
            .collect();
        root["anomalies"] = serde_json::json!({ "base": base, "files": items });
    }
    root.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
aaa111
10\t2\tsrc/hot.py
3\t0\tsrc/cool.py

bbb222
40\t38\tsrc/hot.py

ccc333
5\t5\tsrc/hot.py
2\t2\tREADME.md
";

    #[test]
    fn parses_log_and_counts_distinct_commits_per_file() {
        let entries = parse_log_numstat(LOG);
        let files = file_churn(&entries);
        let hot = files.iter().find(|f| f.path == "src/hot.py").unwrap();
        // touched in all three commits; churn 12 + 78 + 10 = 100.
        assert_eq!(hot.commits, 3);
        assert_eq!(hot.churn, 100);
        let cool = files.iter().find(|f| f.path == "src/cool.py").unwrap();
        assert_eq!(cool.commits, 1);
        assert_eq!(cool.churn, 3);
        // Sorted by churn desc -> hot first.
        assert_eq!(files[0].path, "src/hot.py");
    }

    #[test]
    fn skips_binary_and_blank_rows() {
        let log = "aaa\n-\t-\timg.png\n5\t1\tsrc/a.py\n";
        let entries = parse_log_numstat(log);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].change.path, "src/a.py");
    }

    #[test]
    fn defensively_skips_rename_rows() {
        // The git calls pass `--no-renames`, but a stray `old => new` row must not be counted
        // as a bogus path.
        let log = "aaa\n1\t0\tsrc/old.py => src/new.py\n4\t2\tsrc/a.py\n";
        let entries = parse_log_numstat(log);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].change.path, "src/a.py");
        assert!(parse_diff_numstat("1\t0\t{a => b}/c.py\n").is_empty());
    }

    #[test]
    fn volatility_is_zero_when_all_files_change_equally() {
        let files = vec![
            FileChurn {
                path: "a.py".into(),
                commits: 2,
                churn: 10,
            },
            FileChurn {
                path: "b.py".into(),
                commits: 2,
                churn: 8,
            },
        ];
        assert_eq!(volatility(&files), 0.0);
    }

    #[test]
    fn volatility_rises_when_churn_concentrates() {
        let even = vec![
            FileChurn {
                path: "a.py".into(),
                commits: 3,
                churn: 1,
            },
            FileChurn {
                path: "b.py".into(),
                commits: 3,
                churn: 1,
            },
            FileChurn {
                path: "c.py".into(),
                commits: 3,
                churn: 1,
            },
        ];
        let skewed = vec![
            FileChurn {
                path: "a.py".into(),
                commits: 9,
                churn: 1,
            },
            FileChurn {
                path: "b.py".into(),
                commits: 1,
                churn: 1,
            },
            FileChurn {
                path: "c.py".into(),
                commits: 1,
                churn: 1,
            },
        ];
        assert_eq!(volatility(&even), 0.0);
        assert!(volatility(&skewed) > 1.0, "cv={}", volatility(&skewed));
    }

    #[test]
    fn volatility_empty_is_zero() {
        assert_eq!(volatility(&[]), 0.0);
    }

    #[test]
    fn anomaly_flags_in_place_rewrite_but_not_genuine_growth() {
        let changes = vec![
            // Rewritten in place: 50 added, 48 deleted -> churn 98, net +2, ratio 49.
            Change {
                path: "rewrite.py".into(),
                added: 50,
                deleted: 48,
            },
            // Genuine growth: 60 added, 0 deleted -> churn 60, net +60, ratio 1.
            Change {
                path: "feature.py".into(),
                added: 60,
                deleted: 0,
            },
            // Tiny edit below the churn floor.
            Change {
                path: "tiny.py".into(),
                added: 1,
                deleted: 1,
            },
        ];
        let found = anomalies(&changes, 20, 5.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "rewrite.py");
        assert_eq!(found[0].churn, 98);
        assert_eq!(found[0].delta, 2);
        assert!((found[0].ratio - 49.0).abs() < 1e-9);
    }

    #[test]
    fn pure_deletion_is_not_an_anomaly() {
        // Removing a lot of code is a real delta, not churn-without-benefit.
        let changes = vec![Change {
            path: "gone.py".into(),
            added: 0,
            deleted: 80,
        }];
        assert!(anomalies(&changes, 20, 5.0).is_empty());
    }

    #[test]
    fn diff_numstat_parses_rows_only() {
        let diff = "50\t48\trewrite.py\n0\t10\tdeleted.py\n";
        let changes = parse_diff_numstat(diff);
        assert_eq!(changes.len(), 2);
        assert_eq!(
            changes[0],
            Change {
                path: "rewrite.py".into(),
                added: 50,
                deleted: 48
            }
        );
    }
}

//! End-to-end tests for the metrics polish: the `--max-cognitive` CI gate and configurable
//! `[badges]` output, exercised against the real built binary on real Python.

use std::path::Path;
use std::process::Command;

/// Write `src` + optional `sloplint.toml` into a fresh temp dir and run `sloplint metrics`
/// there; return (stdout, stderr, exit code).
fn run(tag: &str, py: &str, toml: Option<&str>, extra: &[&str]) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!("sloplint_mb_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.py"), py).unwrap();
    if let Some(toml) = toml {
        std::fs::write(dir.join("sloplint.toml"), toml).unwrap();
    }
    let mut args = vec!["metrics", "m.py"];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .current_dir(&dir)
        .output()
        .expect("run sloplint");
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

// A deeply-branched function — high cognitive complexity.
const TANGLED: &str = "\
def tangled(xs):
    total = 0
    for x in xs:
        if x > 0:
            for y in range(x):
                if y % 2 == 0:
                    if y > 5:
                        total += y
    return total
";

#[test]
fn max_cognitive_gate_trips_and_names_the_offender() {
    let (_out, err, code) = run("cog_fail", TANGLED, None, &["--max-cognitive", "3"]);
    assert_eq!(code, 1, "stderr: {err}");
    assert!(err.contains("cognitive ceiling of 3"), "stderr: {err}");
    assert!(err.contains("`tangled`"), "stderr: {err}");
    assert!(err.contains("m.py:1"), "offender at the def line: {err}");
}

#[test]
fn max_cognitive_gate_passes_under_the_ceiling() {
    let (_o, _e, code) = run("cog_pass", TANGLED, None, &["--max-cognitive", "100"]);
    assert_eq!(code, 0);
}

#[test]
fn badges_default_to_all_individual() {
    let dir = std::env::temp_dir().join(format!("sloplint_mb_{}_def", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.py"), TANGLED).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(["metrics", "m.py", "--badges", "b"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let b = dir.join("b");
    // All six per-metric badges, no summary.
    assert!(b.join("max-cyclomatic.json").exists());
    assert!(b.join("max-cognitive.json").exists());
    assert!(b.join("comment-density.json").exists());
    assert!(!b.join("summary.json").exists());
}

#[test]
fn badges_only_summary_when_include_empty() {
    let toml = "[badges]\ninclude = []\nsummary = [\"max-cyclomatic\", \"max-cognitive\", \"comment-density\"]\n";
    let (_o, _e, code) = run("only_summary", TANGLED, Some(toml), &["--badges", "b"]);
    assert_eq!(code, 0);
    let b = std::env::temp_dir()
        .join(format!("sloplint_mb_{}_only_summary", std::process::id()))
        .join("b");
    // Only the combined badge — no individual ones.
    assert!(b.join("summary.json").exists(), "summary badge written");
    assert!(
        !b.join("max-cyclomatic.json").exists(),
        "no individual badges"
    );
    let json = std::fs::read_to_string(b.join("summary.json")).unwrap();
    assert!(json.contains("\"label\":\"sloplint\""), "json: {json}");
    assert!(
        json.contains("CC ") && json.contains("· CoCo ") && json.contains("· density "),
        "combined message: {json}"
    );
    let _ = Path::new(".");
}

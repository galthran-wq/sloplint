//! End-to-end tests for the duplication-density aggregate (#123): SLP020 clone detection surfaced
//! as a `metrics` cohort metric (`clone_ratio` + pairs + largest cluster), per profile. Exercises
//! the real binary in JSON, text, and the GitHub markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A function body large enough to clear SLP020's min-statements guard, parameterized by a tag so
/// distinct bodies aren't accidentally clones.
fn dup_fn(name: &str) -> String {
    format!(
        "def {name}(url):\n    resp = fetch(url)\n    items = []\n    for row in resp.rows:\n        if row.valid:\n            items.append(row.price)\n    return items\n"
    )
}

fn unique_fn(name: &str, k: usize) -> String {
    format!(
        "def {name}(x):\n    total = {k}\n    for i in range(x):\n        total += i * {k}\n    return total\n"
    )
}

/// Write `files` (relative path → contents) into a temp dir and run `sloplint metrics .  <extra>`
/// from inside it. Returns (stdout, code).
fn run(tag: &str, files: &[(&str, String)], extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_clone_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (rel, contents) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let mut args = vec!["metrics", "."];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&dir)
        .args(&args)
        .output()
        .expect("run sloplint");
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

/// Two byte-identical-shaped scrapers (a clone pair) + one unique function = 3 production funcs.
fn scraper_project() -> Vec<(&'static str, String)> {
    vec![(
        "shop.py",
        format!(
            "{}{}{}",
            dup_fn("scrape_amazon"),
            dup_fn("scrape_flipkart"),
            unique_fn("tally", 7),
        ),
    )]
}

#[test]
fn json_reports_clone_density() {
    let (stdout, code) = run("json", &scraper_project(), &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let dup = &value["profiles"]["production"]["duplication"];

    assert_eq!(dup["clone_pairs"], 1, "the two scrapers are one clone pair");
    assert_eq!(dup["functions_in_clones"], 2);
    assert_eq!(dup["functions"], 3);
    assert_eq!(dup["largest_clone_cluster"], 2);
    // ratio = 2 of 3 functions in clones.
    let ratio = dup["clone_ratio"].as_f64().unwrap();
    assert!((ratio - 2.0 / 3.0).abs() < 1e-9, "clone_ratio = {ratio}");
}

#[test]
fn clean_project_has_zero_clone_ratio() {
    // Two *structurally* different functions — the clone engine normalizes identifiers/literals,
    // so non-clones must differ in control flow, not just names/numbers.
    let clean = "\
def accumulate(xs):
    total = 0
    for x in xs:
        total += x
    return total

def classify(value):
    if value > 0:
        return \"pos\"
    if value < 0:
        return \"neg\"
    return \"zero\"
";
    let files = vec![("clean.py", clean.to_string())];
    let (stdout, code) = run("clean", &files, &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let dup = &value["profiles"]["production"]["duplication"];
    assert_eq!(dup["clone_pairs"], 0);
    assert_eq!(dup["clone_ratio"], 0.0);
    assert_eq!(dup["largest_clone_cluster"], 0);
}

#[test]
fn duplication_is_scoped_per_profile() {
    // Production duplication (shop.py) and test duplication (tests/) are reported separately — a
    // clone pair in one profile must not count toward the other's ratio.
    let mut files = scraper_project();
    files.push((
        "tests/test_x.py",
        format!("{}{}", dup_fn("test_one"), dup_fn("test_two")),
    ));
    let (stdout, code) = run("scoped", &files, &["--format", "json"]);
    assert_eq!(code, 0);
    let profiles = &serde_json::from_str::<Value>(&stdout).unwrap()["profiles"];
    // Production: 2 of 3 in clones (the test pair is excluded).
    assert_eq!(profiles["production"]["duplication"]["functions"], 3);
    assert_eq!(profiles["production"]["duplication"]["clone_pairs"], 1);
    // Tests: its own pair, all functions duplicated.
    assert_eq!(profiles["tests"]["duplication"]["clone_pairs"], 1);
    assert_eq!(profiles["tests"]["duplication"]["clone_ratio"], 1.0);
}

#[test]
fn text_and_markdown_surface_clone_density() {
    let (text, code) = run("text", &scraper_project(), &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("clone ratio"),
        "text panel shows the clone ratio:\n{text}"
    );

    let (md, code) = run("md", &scraper_project(), &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        md.contains("**Duplication**") && md.contains("clone ratio"),
        "markdown has the duplication block:\n{md}"
    );
}

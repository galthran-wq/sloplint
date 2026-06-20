//! SLP160: mechanical test-suite mirroring (preview, whole-project).
//!
//! A test module that mirrors production 1:1 — a `test_foo` per `foo`, each exercising one
//! symbol in isolation — *looks* thorough (full structural coverage) but is typically shallow:
//! it tests the shape of the code, not its behavior. Mirroring on its own is fine, so this
//! fires only when the mirrored tests are *also* assertion-free (shallow). Like clone detection
//! it needs a cross-file view (the production symbol set), so it runs as a whole-tree pass in
//! the CLI rather than a per-file rule.
//!
//! Name-based and conservative: a `test_<stem>` mirrors a production symbol when their names
//! match after l-casing and dropping underscores (`test_token_bucket` ↔ `TokenBucket`); a test
//! is "shallow" when its body contains no `assert` and no assertion call (`self.assertX`,
//! `pytest.raises`, …). Production symbols are the top-level classes/functions of non-test
//! modules (methods are not symbols, so `test_<method>` doesn't mirror).
//!
//! Known limitations (acceptable for a preview rule, and they only ever *suppress* a finding,
//! never invent one): a test that asserts only via a called helper reads as shallow; the
//! name normalization can coincidentally match unrelated names; and an assertion via a bare
//! `raises`/`fail` call (not `pytest.raises`/`self.fail`) isn't recognized.

use std::collections::HashSet;

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ModModule, Stmt, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange};

/// One `test_*` function and whether it is a shallow (assertion-free) mirror candidate.
pub struct TestFn {
    /// Normalized name stem after the `test_` prefix.
    pub stem: String,
    pub shallow: bool,
    pub name_range: TextRange,
}

/// What one file contributes to the whole-project mirroring analysis.
#[derive(Default)]
pub struct FileScan {
    pub is_test: bool,
    /// `test_*` functions found (when `is_test`).
    pub tests: Vec<TestFn>,
    /// Normalized top-level class/function names (when not a test file).
    pub production: Vec<String>,
}

/// A confirmed mechanical-mirror finding for one test module.
pub struct Finding {
    pub scan_index: usize,
    pub range: TextRange,
    /// How many of the module's tests are assertion-free mirrors, out of its total.
    pub mirrors: usize,
    pub total: usize,
}

/// Scan one parsed file for mirroring signals.
pub fn scan_file(path: &str, parsed: &Parsed<ModModule>) -> FileScan {
    if is_test_path(path) {
        let mut functions = Vec::new();
        let mut collector = FnCollector {
            out: &mut functions,
        };
        for stmt in &parsed.syntax().body {
            collector.visit_stmt(stmt);
        }
        let tests = functions
            .iter()
            .filter_map(|function| {
                let stem = function.name.as_str().strip_prefix("test_")?;
                (!stem.is_empty()).then(|| TestFn {
                    stem: normalize(stem),
                    shallow: !has_assertions(&function.body),
                    name_range: function.name.range(),
                })
            })
            .collect();
        FileScan {
            is_test: true,
            tests,
            production: Vec::new(),
        }
    } else {
        let production = parsed
            .syntax()
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::ClassDef(class) => Some(normalize(class.name.as_str())),
                Stmt::FunctionDef(function) => Some(normalize(function.name.as_str())),
                _ => None,
            })
            .collect();
        FileScan {
            is_test: false,
            tests: Vec::new(),
            production,
        }
    }
}

/// Flag test modules where the fraction of assertion-free `test_<production-symbol>` mirrors is
/// at/above `max_ratio`, among modules with at least `min_tests` tests. Pure over the scans.
pub fn findings(scans: &[FileScan], min_tests: usize, max_ratio: f64) -> Vec<Finding> {
    let production: HashSet<&str> = scans
        .iter()
        .filter(|scan| !scan.is_test)
        .flat_map(|scan| scan.production.iter().map(String::as_str))
        .collect();

    let mut out = Vec::new();
    for (scan_index, scan) in scans.iter().enumerate() {
        if !scan.is_test || scan.tests.len() < min_tests {
            continue;
        }
        let mirrors: Vec<&TestFn> = scan
            .tests
            .iter()
            .filter(|test| test.shallow && production.contains(test.stem.as_str()))
            .collect();
        let ratio = mirrors.len() as f64 / scan.tests.len() as f64;
        if !mirrors.is_empty() && ratio >= max_ratio {
            out.push(Finding {
                scan_index,
                range: mirrors[0].name_range,
                mirrors: mirrors.len(),
                total: scan.tests.len(),
            });
        }
    }
    out
}

/// `token_bucket` and `TokenBucket` both normalize to `tokenbucket`.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_test_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());
    file.starts_with("test_")
        || file.ends_with("_test.py")
        || file == "conftest.py"
        || lower.split('/').any(|segment| segment == "tests")
}

/// Whether a function body contains any assertion — an `assert` statement or an assertion call
/// (`self.assertX(...)`, `pytest.raises(...)`, `self.fail()`, …).
fn has_assertions(body: &[Stmt]) -> bool {
    let mut scan = AssertScan { found: false };
    for stmt in body {
        scan.visit_stmt(stmt);
    }
    scan.found
}

struct AssertScan {
    found: bool,
}

impl<'a> Visitor<'a> for AssertScan {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if matches!(stmt, Stmt::Assert(_)) {
            self.found = true;
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if is_assertion_call(call.func.as_ref()) {
                self.found = true;
            }
        }
        visitor::walk_expr(self, expr);
    }
}

fn is_assertion_call(func: &Expr) -> bool {
    match func {
        // `self.assertEqual(...)`, `pytest.raises(...)`, `mock.assert_called_once()`, `self.fail()`.
        Expr::Attribute(attribute) => {
            let name = attribute.attr.as_str();
            name.starts_with("assert") || matches!(name, "fail" | "raises" | "warns")
        }
        // A directly-imported `assertEqual(...)` — but NOT a bare `raises`/`fail` call, which is
        // more likely a production function than an assertion helper.
        Expr::Name(name) => name.id.as_str().starts_with("assert"),
        _ => false,
    }
}

/// Collects module-level functions and class methods — but NOT functions nested inside other
/// functions (closures aren't tests). Descends through classes and other compound statements,
/// just not into a function body.
struct FnCollector<'a, 'b> {
    out: &'b mut Vec<&'a StmtFunctionDef>,
}

impl<'a> Visitor<'a> for FnCollector<'a, '_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(function) => self.out.push(function), // don't descend into its body
            other => visitor::walk_stmt(self, other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn scan(path: &str, source: &str) -> FileScan {
        scan_file(path, &parse(source).expect("valid python"))
    }

    #[test]
    fn classifies_test_functions_by_shallowness() {
        let s = scan(
            "tests/test_thing.py",
            "\
def test_run():
    assert run() == 1

def test_build():
    build()
",
        );
        assert!(s.is_test);
        assert_eq!(s.tests.len(), 2);
        let run = s.tests.iter().find(|t| t.stem == "run").unwrap();
        let build = s.tests.iter().find(|t| t.stem == "build").unwrap();
        assert!(!run.shallow, "test_run asserts");
        assert!(build.shallow, "test_build is assertion-free");
    }

    #[test]
    fn assertion_calls_count_as_assertions() {
        let s = scan(
            "tests/test_x.py",
            "\
class TestThing:
    def test_a(self):
        self.assertEqual(a(), 1)

    def test_b(self):
        with pytest.raises(ValueError):
            b()
",
        );
        assert!(s.tests.iter().all(|t| !t.shallow), "both have assertions");
    }

    #[test]
    fn production_file_yields_top_level_symbols() {
        let s = scan(
            "pkg/thing.py",
            "class TokenBucket:\n    pass\n\ndef build_default():\n    return 1\n",
        );
        assert!(!s.is_test);
        assert!(s.production.contains(&"tokenbucket".to_string()));
        assert!(s.production.contains(&"builddefault".to_string()));
    }

    fn prod(symbols: &str) -> FileScan {
        scan("pkg/p.py", symbols)
    }

    #[test]
    fn flags_a_mechanically_mirrored_shallow_suite() {
        let production =
            prod("def alpha():\n    pass\n\ndef beta():\n    pass\n\ndef gamma():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "\
def test_alpha():
    alpha()

def test_beta():
    beta()

def test_gamma():
    gamma()
",
        );
        let result = findings(&[production, tests], 3, 0.7);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].mirrors, 3);
        assert_eq!(result[0].total, 3);
    }

    #[test]
    fn behavior_suite_with_cross_cutting_tests_is_not_flagged() {
        let production = prod("def alpha():\n    pass\n\ndef beta():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "\
def test_end_to_end():
    assert pipeline() == expected

def test_rejects_negative_input():
    with pytest.raises(ValueError):
        run(-1)

def test_handles_empty():
    assert run([]) == []
",
        );
        assert!(findings(&[production, tests], 3, 0.7).is_empty());
    }

    #[test]
    fn mirrored_but_asserting_suite_is_not_flagged() {
        // 1:1 mirror, but each test actually asserts behavior -> not shallow -> fine.
        let production =
            prod("def alpha():\n    pass\n\ndef beta():\n    pass\n\ndef gamma():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "\
def test_alpha():
    assert alpha() == 1

def test_beta():
    assert beta() == 2

def test_gamma():
    assert gamma() == 3
",
        );
        assert!(findings(&[production, tests], 3, 0.7).is_empty());
    }

    #[test]
    fn zero_ratio_threshold_does_not_panic_or_fire_without_mirrors() {
        // Regression: `max_ratio = 0.0` must not index an empty mirror list.
        let production = prod("def alpha():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "def test_x():\n    assert one() == 1\n\ndef test_y():\n    assert two() == 2\n\ndef test_z():\n    assert three() == 3\n",
        );
        assert!(findings(&[production, tests], 3, 0.0).is_empty());
    }

    #[test]
    fn nested_closures_are_not_counted_as_tests() {
        // A `def test_helper` nested inside a test must not inflate the test count / mirrors.
        let production = prod("def alpha():\n    pass\n\ndef beta():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "\
def test_alpha():
    def test_beta():
        return beta()
    alpha()
",
        );
        assert_eq!(tests.tests.len(), 1, "only test_alpha is a test");
        // One mirror over one test would be 100%, but min_tests gates it out at the floor.
        assert!(findings(&[production, tests], 3, 0.7).is_empty());
    }

    #[test]
    fn small_suite_below_the_floor_is_not_flagged() {
        let production = prod("def alpha():\n    pass\n\ndef beta():\n    pass\n");
        let tests = scan(
            "tests/test_p.py",
            "def test_alpha():\n    alpha()\n\ndef test_beta():\n    beta()\n",
        );
        assert!(
            findings(&[production, tests], 3, 0.7).is_empty(),
            "only 2 tests"
        );
    }

    #[test]
    fn mirror_stem_matches_class_name_ignoring_case_and_underscores() {
        let production = prod("class TokenBucket:\n    pass\n\nclass RateLimiter:\n    pass\n\nclass LeakyBucket:\n    pass\n");
        let tests = scan(
            "tests/test_buckets.py",
            "\
def test_token_bucket():
    TokenBucket()

def test_rate_limiter():
    RateLimiter()

def test_leaky_bucket():
    LeakyBucket()
",
        );
        assert_eq!(findings(&[production, tests], 3, 0.7).len(), 1);
    }
}

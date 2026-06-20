//! Static test proxies (issue #86): **test:code ratio** and **assertion density**.
//!
//! ## What this is — and is NOT
//!
//! These are *static* heuristics computed without ever running the test suite. They are
//! **NOT test coverage**: real coverage requires *executing* the tests and recording which
//! production lines ran, which a static linter cannot do. Treat both numbers as descriptive
//! *proxies*:
//!
//! - A low test:code ratio and a low assertion density *suggest* under-testing.
//! - But they **cannot** reliably tell a shallow test from a thorough one — a test can carry
//!   many asserts and still verify nothing meaningful, or few asserts and be excellent.
//!
//! Therefore these figures are reported as descriptive cohort statistics and are **never** a
//! pass/fail gate. Their value is *across a cohort*: the slop side of a corpus tends to ship
//! far less test code with shallower assertions, so as distribution signals they add real
//! information even though no single repo's number is a verdict.
//!
//! These aggregate *metrics* are the cohort-level counterpart to the per-file
//! assertion-free-test (SLP070) and test-mirroring (SLP160) *rules*.

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ModModule, Stmt, StmtFunctionDef};
use sloplint_python::parser::Parsed;

/// Classify a file as a test file purely from its path: a `test_*.py` or `*_test.py` filename,
/// a `conftest.py`, or any `tests/` (or `test/`) directory segment. Path-based on purpose — it
/// matches the conventions pytest/unittest discovery already rely on, and needs no parsing.
pub fn is_test_file(path: &str) -> bool {
    // Normalize separators so Windows-style paths classify the same as POSIX ones.
    let normalized = path.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);

    if file_name == "conftest.py"
        || (file_name.starts_with("test_") && file_name.ends_with(".py"))
        || file_name.ends_with("_test.py")
    {
        return true;
    }

    // A `tests/` or `test/` directory segment anywhere in the path (but not the file name).
    normalized
        .split('/')
        .rev()
        .skip(1)
        .any(|segment| segment == "tests" || segment == "test")
}

/// Per-file test signals, gathered in one pass alongside the other metrics. Production files
/// contribute only their size; test files also contribute their test-function and assertion
/// counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTestStats {
    /// Whether [`is_test_file`] classified this path as a test file.
    pub is_test: bool,
    /// Physical lines in the file (matches `FileMetrics::loc`).
    pub loc: usize,
    /// `test_*` functions (module-level or methods of any class). Only meaningful for test files.
    pub test_functions: usize,
    /// Assertions inside those test functions: `assert` statements plus assertion calls
    /// (`self.assertX`, `self.fail`, `pytest.raises`/`warns`/`deprecated_call`). Test files only.
    pub assertions: usize,
}

/// Gather the per-file test signals for one parsed file. `loc` is the file's physical line
/// count (passed in so this shares `FileMetrics`'s definition rather than recomputing it).
pub fn file_test_stats(path: &str, loc: usize, parsed: &Parsed<ModModule>) -> FileTestStats {
    let is_test = is_test_file(path);
    if !is_test {
        return FileTestStats {
            is_test: false,
            loc,
            test_functions: 0,
            assertions: 0,
        };
    }

    let mut tests = Vec::new();
    collect_test_functions(&parsed.syntax().body, &mut tests);
    let assertions = tests.iter().map(|f| count_assertions(&f.body)).sum();

    FileTestStats {
        is_test: true,
        loc,
        test_functions: tests.len(),
        assertions,
    }
}

/// The aggregated test proxies for a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TestProxies {
    pub test_files: usize,
    pub production_files: usize,
    pub test_loc: usize,
    pub production_loc: usize,
    /// `test_loc / production_loc`. `None` when there is no production code to divide by (the
    /// ratio would be undefined/infinite — reported as null rather than a misleading number).
    pub test_code_ratio: Option<f64>,
    pub test_functions: usize,
    pub assertions: usize,
    /// `assertions / test_functions`. `None` when there are no test functions (undefined).
    pub assertion_density: Option<f64>,
}

/// Roll per-file test signals up into the project-level proxies.
pub fn aggregate_test_proxies(stats: &[FileTestStats]) -> TestProxies {
    let mut proxies = TestProxies::default();
    for file in stats {
        if file.is_test {
            proxies.test_files += 1;
            proxies.test_loc += file.loc;
            proxies.test_functions += file.test_functions;
            proxies.assertions += file.assertions;
        } else {
            proxies.production_files += 1;
            proxies.production_loc += file.loc;
        }
    }
    proxies.test_code_ratio = if proxies.production_loc == 0 {
        None
    } else {
        Some(proxies.test_loc as f64 / proxies.production_loc as f64)
    };
    proxies.assertion_density = if proxies.test_functions == 0 {
        None
    } else {
        Some(proxies.assertions as f64 / proxies.test_functions as f64)
    };
    proxies
}

/// Collect `test_*` functions: module-level functions and methods of any class, but not
/// functions nested inside another function (a local helper named `test_*` is not a test).
fn collect_test_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(function) => {
                if is_test_name(&function.name) {
                    out.push(function);
                }
                // Do not descend into the body: nested defs are helpers, not separate tests.
            }
            Stmt::ClassDef(class) => collect_test_functions(&class.body, out),
            _ => {}
        }
    }
}

/// A test function by pytest/unittest convention: exactly `test`, a `test_*` name (pytest), or
/// a `testCamelCase` name (unittest's default loader). The character after `test` must be `_` or
/// uppercase, so ordinary helpers like `testing()` or `tested_value()` are *not* miscounted as
/// tests (which would otherwise inflate the denominator and deflate assertion density).
fn is_test_name(name: &str) -> bool {
    match name.strip_prefix("test") {
        None => false,
        Some("") => true,
        Some(rest) => rest
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_uppercase()),
    }
}

/// Count assertions in a test function's body: `assert` statements plus assertion calls,
/// descending through every nested block (including local helper defs — an assert in a helper
/// the test calls still tests something).
fn count_assertions(body: &[Stmt]) -> usize {
    struct Counter {
        n: usize,
    }
    impl Visitor<'_> for Counter {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            if matches!(stmt, Stmt::Assert(_)) {
                self.n += 1;
            }
            visitor::walk_stmt(self, stmt);
        }
        fn visit_expr(&mut self, expr: &Expr) {
            if is_assertion_call(expr) {
                self.n += 1;
            }
            visitor::walk_expr(self, expr);
        }
    }
    let mut counter = Counter { n: 0 };
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.n
}

/// Whether an expression is a recognized assertion *call* — the same shapes SLP070 treats as
/// proof a test actually checks something:
/// - `<receiver>.assertX(...)` — unittest's `assertEqual`/`assertTrue`/`assertRaises`/...; the
///   `assert` prefix must be followed by an UPPERCASE letter (the unittest camelCase
///   convention), so snake_case lookalikes are excluded: a user helper `assertion_helper()` and
///   `mock.assert_called_with(...)` (a mock-configuration call, not a test assertion) do not
///   count;
/// - `self.fail(...)` / `cls.fail(...)`;
/// - `pytest.raises(...)` / `pytest.warns(...)` / `pytest.deprecated_call(...)` (incl. as the
///   context expression of a `with`, which is visited as a normal call).
fn is_assertion_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    let Expr::Attribute(attribute) = call.func.as_ref() else {
        return false;
    };
    let method = attribute.attr.as_str();

    if is_unittest_assert(method) {
        return true;
    }
    if method == "fail" && receiver_is(&attribute.value, &["self", "cls"]) {
        return true;
    }
    if matches!(method, "raises" | "warns" | "deprecated_call")
        && receiver_is(&attribute.value, &["pytest"])
    {
        return true;
    }
    false
}

/// A unittest `assertX` method name: `assert` followed by an uppercase letter (`assertEqual`,
/// `assertTrue`, `assertRaises`). Excludes `assert_called_with` (mock) and `assertion_helper`.
fn is_unittest_assert(method: &str) -> bool {
    method
        .strip_prefix("assert")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// Whether `expr` is a bare name matching one of `names` (the receiver of an attribute access).
fn receiver_is(expr: &Expr, names: &[&str]) -> bool {
    matches!(expr, Expr::Name(name) if names.contains(&name.id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn parse_src(source: &str) -> Parsed<ModModule> {
        parse(source).expect("valid python")
    }

    #[test]
    fn classifies_test_files_by_path() {
        assert!(is_test_file("test_foo.py"));
        assert!(is_test_file("foo_test.py"));
        assert!(is_test_file("conftest.py"));
        assert!(is_test_file("pkg/tests/thing.py"));
        assert!(is_test_file("a/b/test/thing.py"));
        assert!(is_test_file("src/tests/sub/helpers.py"));

        assert!(!is_test_file("foo.py"));
        assert!(!is_test_file("src/contest.py"));
        // A `test`/`tests` substring that isn't a full segment must not match.
        assert!(!is_test_file("src/latest/thing.py"));
        assert!(!is_test_file("src/attestation.py"));
        // `testing.py` is not `test_*` and `testing` is not a `tests`/`test` segment.
        assert!(!is_test_file("src/testing.py"));
    }

    #[test]
    fn counts_test_functions_and_assertions() {
        let source = "\
import pytest

def helper():
    assert True  # not a test function — not counted

def test_one():
    assert 1 == 1
    assert 2 == 2

def test_two():
    with pytest.raises(ValueError):
        do()

class TestThing:
    def test_method(self):
        self.assertEqual(1, 1)
        self.assertTrue(True)
    def not_a_test(self):
        assert False  # not test_* — not counted
";
        let parsed = parse_src(source);
        let stats = file_test_stats("test_mod.py", source.lines().count(), &parsed);
        assert!(stats.is_test);
        // test_one, test_two, TestThing.test_method.
        assert_eq!(stats.test_functions, 3);
        // 2 asserts + 1 pytest.raises + 2 self.assertX.
        assert_eq!(stats.assertions, 5);
    }

    #[test]
    fn production_file_carries_only_size() {
        let source = "def test_looks_like_a_test():\n    assert True\n";
        let parsed = parse_src(source);
        // Path is not a test path, so the test-shaped contents are ignored.
        let stats = file_test_stats("src/module.py", source.lines().count(), &parsed);
        assert!(!stats.is_test);
        assert_eq!(stats.test_functions, 0);
        assert_eq!(stats.assertions, 0);
        assert_eq!(stats.loc, 2);
    }

    #[test]
    fn test_name_matches_pytest_and_unittest_but_not_lookalikes() {
        // pytest + unittest conventions.
        assert!(is_test_name("test"));
        assert!(is_test_name("test_add"));
        assert!(is_test_name("testAddition")); // unittest camelCase
                                               // Helpers that merely start with the letters "test" must not count.
        assert!(!is_test_name("testing"));
        assert!(!is_test_name("tested_value"));
        assert!(!is_test_name("teardown"));
    }

    #[test]
    fn unittest_assert_excludes_mock_and_helper_lookalikes() {
        // camelCase unittest assertions count; snake_case lookalikes do not.
        assert!(is_unittest_assert("assertEqual"));
        assert!(is_unittest_assert("assertTrue"));
        assert!(!is_unittest_assert("assert_called_with")); // mock configuration, not a test
        assert!(!is_unittest_assert("assertion_helper")); // user helper
        assert!(!is_unittest_assert("assert")); // bare (not a real method name anyway)

        // End-to-end through the counter: only the real unittest assertions count.
        let source = "\
def test_mock_calls():
    mock.assert_called_with(1)  # not a test assertion
    self.assertion_helper()     # user helper, not a test assertion
    self.assertEqual(a, b)      # counts
";
        let parsed = parse_src(source);
        let stats = file_test_stats("test_mocks.py", source.lines().count(), &parsed);
        assert_eq!(stats.assertions, 1);
    }

    #[test]
    fn self_fail_counts_but_unrelated_fail_does_not() {
        let source = "\
def test_fail_path():
    self.fail('boom')
    job.fail()  # unrelated .fail() — not an assertion
";
        let parsed = parse_src(source);
        let stats = file_test_stats("test_x.py", source.lines().count(), &parsed);
        assert_eq!(stats.assertions, 1);
    }

    #[test]
    fn aggregate_ratio_and_density() {
        let stats = vec![
            FileTestStats {
                is_test: false,
                loc: 100,
                test_functions: 0,
                assertions: 0,
            },
            FileTestStats {
                is_test: false,
                loc: 100,
                test_functions: 0,
                assertions: 0,
            },
            FileTestStats {
                is_test: true,
                loc: 50,
                test_functions: 4,
                assertions: 10,
            },
        ];
        let proxies = aggregate_test_proxies(&stats);
        assert_eq!(proxies.production_files, 2);
        assert_eq!(proxies.test_files, 1);
        assert_eq!(proxies.production_loc, 200);
        assert_eq!(proxies.test_loc, 50);
        // 50 / 200 = 0.25.
        assert_eq!(proxies.test_code_ratio, Some(0.25));
        // 10 / 4 = 2.5.
        assert_eq!(proxies.assertion_density, Some(2.5));
    }

    #[test]
    fn undefined_ratios_are_none_not_zero() {
        // No production code → ratio undefined.
        let only_tests = vec![FileTestStats {
            is_test: true,
            loc: 30,
            test_functions: 0,
            assertions: 0,
        }];
        let proxies = aggregate_test_proxies(&only_tests);
        assert_eq!(proxies.test_code_ratio, None);
        // No test functions → density undefined.
        assert_eq!(proxies.assertion_density, None);
    }
}

//! Static test proxies: **test:code ratio** and **assertion density**, plus an
//! **assertion-free-test rate** test-substance signal.
//!
//! ## What this is — and is NOT
//!
//! These are *static* heuristics computed without ever running the test suite. They are
//! **NOT test coverage**: real coverage requires *executing* the tests and recording which
//! production lines ran, which a static linter cannot do. Treat every number as a descriptive
//! *proxy*:
//!
//! - A low test:code ratio and a low assertion density *suggest* under-testing.
//! - But they **cannot** reliably tell a shallow test from a thorough one — a test can carry
//!   many asserts and still verify nothing meaningful, or few asserts and be excellent.
//!
//! ## Test-substance: the assertion-free-test rate
//!
//! `test:code` and `assertion_density` both reward *volume*, so an inflated suite can read as
//! "well-tested" even when individual tests verify nothing. The **assertion-free-test rate** is
//! the counterweight: the fraction of test functions whose body contains **no assertion at all**
//! (no `assert`, no `self.assertX`, no `pytest.raises`, …). That is the shape "test theater"
//! actually takes across the vibe cohort — print-spam "tests" that exercise code but check
//! nothing, and assertion-free stubs. A rate near 1.0 means a suite that *looks* tested (files,
//! functions, LoC) but asserts almost nothing.
//!
//! An earlier cut of this signal keyed on **cognitive complexity** instead, calling a test
//! "trivial" when its body had little branching. That was exactly backwards: a disciplined
//! arrange-act-assert unit test is *deliberately* branch-free, so it scored as 100% trivial,
//! while an assertion-free test that loops over cases and `print()`s them scored as substantive.
//! This signal corrects that: low cognitive complexity in a *test* is good, not a smell — the quality
//! signal is whether a test **asserts**, not whether it **branches**.
//!
//! Like the others this stays purely descriptive: a test *can* assert through a helper it calls
//! (so a 0-assertion body is not proof of a weak test), so a high rate is a prompt to look, never
//! a verdict.
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
    /// Of those test functions, how many contain **no assertion at all** in their body —
    /// the numerator for the assertion-free-test rate. Test files only.
    pub assertion_free_tests: usize,
    /// Assertions inside those test functions: `assert` statements plus assertion calls
    /// (`self.assertX`, `self.fail`, `pytest.raises`/`warns`/`deprecated_call`). Test files only.
    pub assertions: usize,
}

/// Gather the per-file test signals for one parsed file. `loc` is the file's physical line count
/// (passed in so this shares `FileMetrics`'s definition rather than recomputing it). `is_test` is
/// the caller's classification — the CLI binds it to the `tests` profile so the proxies and
/// the metric panels agree; [`is_test_file`] is the path heuristic that classifier defaults to.
pub fn file_test_stats(is_test: bool, loc: usize, parsed: &Parsed<ModModule>) -> FileTestStats {
    if !is_test {
        return FileTestStats {
            is_test: false,
            loc,
            test_functions: 0,
            assertion_free_tests: 0,
            assertions: 0,
        };
    }

    let mut tests = Vec::new();
    collect_test_functions(&parsed.syntax().body, &mut tests);
    // Per-test assertion counts, computed once: their sum is the file's assertion total, and the
    // number that are zero is the assertion-free-test count.
    let per_test_assertions: Vec<usize> = tests.iter().map(|f| count_assertions(&f.body)).collect();
    let assertions = per_test_assertions.iter().sum();
    let assertion_free_tests = per_test_assertions.iter().filter(|&&n| n == 0).count();

    FileTestStats {
        is_test: true,
        loc,
        test_functions: tests.len(),
        assertion_free_tests,
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
    /// Test functions whose body contains no assertion at all. The numerator for
    /// [`Self::assertion_free_rate`].
    pub assertion_free_tests: usize,
    /// `assertion_free_tests / test_functions` (0.0–1.0): the fraction of the suite that asserts
    /// nothing — "test theater". `None` when there are no test functions (undefined). A high value
    /// alongside a high `test_code_ratio` flags a suite that looks tested but verifies little —
    /// descriptive, never a gate.
    pub assertion_free_rate: Option<f64>,
}

/// Roll per-file test signals up into the project-level proxies.
pub fn aggregate_test_proxies(stats: &[FileTestStats]) -> TestProxies {
    let mut proxies = TestProxies::default();
    for file in stats {
        if file.is_test {
            proxies.test_files += 1;
            proxies.test_loc += file.loc;
            proxies.test_functions += file.test_functions;
            proxies.assertion_free_tests += file.assertion_free_tests;
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
    proxies.assertion_free_rate = if proxies.test_functions == 0 {
        None
    } else {
        Some(proxies.assertion_free_tests as f64 / proxies.test_functions as f64)
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
        let stats = file_test_stats(true, source.lines().count(), &parsed);
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
        // Classified as production (is_test = false), so the test-shaped contents are ignored.
        let stats = file_test_stats(false, source.lines().count(), &parsed);
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
        let stats = file_test_stats(true, source.lines().count(), &parsed);
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
        let stats = file_test_stats(true, source.lines().count(), &parsed);
        assert_eq!(stats.assertions, 1);
    }

    #[test]
    fn aggregate_ratio_and_density() {
        let stats = vec![
            FileTestStats {
                is_test: false,
                loc: 100,
                test_functions: 0,
                assertion_free_tests: 0,
                assertions: 0,
            },
            FileTestStats {
                is_test: false,
                loc: 100,
                test_functions: 0,
                assertion_free_tests: 0,
                assertions: 0,
            },
            FileTestStats {
                is_test: true,
                loc: 50,
                test_functions: 4,
                assertion_free_tests: 1,
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
        // 1 of 4 test functions assertion-free → 0.25.
        assert_eq!(proxies.assertion_free_tests, 1);
        assert_eq!(proxies.assertion_free_rate, Some(0.25));
    }

    #[test]
    fn undefined_ratios_are_none_not_zero() {
        // No production code → ratio undefined.
        let only_tests = vec![FileTestStats {
            is_test: true,
            loc: 30,
            test_functions: 0,
            assertion_free_tests: 0,
            assertions: 0,
        }];
        let proxies = aggregate_test_proxies(&only_tests);
        assert_eq!(proxies.test_code_ratio, None);
        // No test functions → density undefined.
        assert_eq!(proxies.assertion_density, None);
        // No test functions → assertion-free rate undefined (never a misleading 0).
        assert_eq!(proxies.assertion_free_rate, None);
    }

    #[test]
    fn assertion_free_rate_flags_tests_that_check_nothing() {
        // The regression this guards against: a disciplined arrange-act-assert unit test (branch-free,
        // cognitive 0) is NOT theater, while an assertion-free `print`-spam loop (cognitive > 0)
        // IS. The signal must key on assertions, not branching.
        let source = "\
import pytest

def test_good_simple():
    # Clean unit test — cognitive 0, but it asserts. Must NOT be flagged.
    assert f(2) == 4

def test_good_raises():
    with pytest.raises(ValueError):
        f(-1)

def test_theater_print():
    # 'Test theater': loops and prints, asserts nothing. MUST be flagged.
    for x in (0, 1, 2):
        print(f(x))

def test_theater_stub():
    pass
";
        let parsed = parse_src(source);
        let stats = file_test_stats(true, source.lines().count(), &parsed);
        assert_eq!(stats.test_functions, 4);
        // test_theater_print and test_theater_stub assert nothing; the two good tests do.
        assert_eq!(stats.assertion_free_tests, 2);

        let proxies = aggregate_test_proxies(&[stats]);
        assert_eq!(proxies.assertion_free_rate, Some(0.5));
    }

    #[test]
    fn assertion_via_helper_in_body_counts_as_asserting() {
        // `count_assertions` descends into a local helper defined inside the test, so a test that
        // asserts through such a helper is not assertion-free — matching the assertion-density
        // definition.
        let source = "\
def test_uses_local_helper():
    def check(v):
        assert v > 0
    check(f(3))
";
        let parsed = parse_src(source);
        let stats = file_test_stats(true, source.lines().count(), &parsed);
        assert_eq!(stats.test_functions, 1);
        assert_eq!(stats.assertion_free_tests, 0);
    }

    #[test]
    fn assertion_free_rate_scores_unittest_class_methods() {
        // The check runs over every collected test, including `test*` methods of a unittest class.
        let source = "\
class TestThing:
    def test_asserts(self):
        self.assertEqual(f(1), 1)

    def test_theater(self):
        result = f(2)
        print(result)
";
        let parsed = parse_src(source);
        let stats = file_test_stats(true, source.lines().count(), &parsed);
        assert_eq!(stats.test_functions, 2);
        // test_asserts has self.assertEqual; test_theater asserts nothing.
        assert_eq!(stats.assertion_free_tests, 1);
    }

    #[test]
    fn production_file_reports_no_assertion_free_tests() {
        // Test-shaped contents in a production file are ignored, so it contributes no
        // assertion-free tests (the denominator stays production-free).
        let source = "def test_looks_like_a_test():\n    print('noop')\n";
        let parsed = parse_src(source);
        let stats = file_test_stats(false, source.lines().count(), &parsed);
        assert_eq!(stats.assertion_free_tests, 0);
    }
}

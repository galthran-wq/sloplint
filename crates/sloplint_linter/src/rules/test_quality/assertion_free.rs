//! SLP070: assertion-free and tautological tests.
//!
//! A test that executes code but verifies nothing gives false confidence — it drives the
//! coverage number up while catching no regression. Coverage tools are blind to it (the
//! lines *are* executed). Ruff's `PT*`/`B101` flag bare-`assert` *style*, never "this test
//! asserts nothing" or "this assertion is tautological", so this is an open lane.
//!
//! We fire on two high-precision shapes, one finding per test function:
//!
//! - **assertion-free**: a test that runs work (a call, an assignment, a loop, …) but
//!   contains no assertion at all. A test whose body is only `pass`/`...`/a docstring is an
//!   empty stub — left to the SATD/stub detector, not flagged here.
//! - **tautological**: a test whose *every* assertion constant-folds to a fixed literal
//!   (`assert True`, `assert 1 == 1`) or asserts a value trivially implied by a preceding
//!   literal assignment in the same scope (`x = 5` … `assert x == 5`).
//!
//! To keep precision high, any "real" assertion spares the function: a plain `assert`,
//! a `self.assert*`/mock `assert_called*` call, a `with pytest.raises(...)` block, or a
//! call to a `check*`/`verify*`/`expect*` helper (delegated verification).

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{ExceptHandler, Expr, ExprCall, Stmt, StmtClassDef, StmtFunctionDef};
use sloplint_python::Ranged;

use crate::lint::{FileContext, Rule};

pub struct AssertionFreeTest;

impl Rule for AssertionFreeTest {
    fn code(&self) -> &'static str {
        "SLP070"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        for stmt in &ctx.parsed.syntax().body {
            match stmt {
                Stmt::FunctionDef(func) => check_function(func, diagnostics),
                Stmt::ClassDef(class) if is_test_class(class) => {
                    for member in &class.body {
                        if let Stmt::FunctionDef(method) = member {
                            check_function(method, diagnostics);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Examine one function; push a finding if it is a test that verifies nothing.
fn check_function(func: &StmtFunctionDef, diagnostics: &mut Vec<Diagnostic>) {
    if !is_test_name(func.name.as_str()) || is_skipped(func) {
        return;
    }

    let mut found = Collected::default();
    collect(&func.body, &mut found);
    // Tautology is classified only over the function's top-level statements, in order,
    // with a live literal map — assertions nested inside control flow are conservatively
    // treated as real, so we never flag a test that might genuinely verify something.
    let (top_real, top_taut) = classify_top_level(&func.body);
    let nested_asserts = found.assert_count.saturating_sub(top_real + top_taut);

    let has_any_assertion = found.assert_count > 0 || found.has_verification;
    let has_real_assertion = found.has_verification || top_real > 0 || nested_asserts > 0;

    let message = if !has_any_assertion {
        // No assertion anywhere. Only a finding if the test actually runs something;
        // an empty stub (`pass`/`...`/docstring only) is a different smell.
        found
            .executes_work
            .then(|| format!("test `{}` runs code but asserts nothing", func.name))
    } else if !has_real_assertion {
        // Has assertions, but every one constant-folds (`assert True`, `assert 1 == 1`) —
        // its truth is fixed regardless of the code under test, so it checks nothing.
        Some(format!(
            "every assertion in test `{}` checks only constants — it verifies nothing about the code under test",
            func.name
        ))
    } else {
        None
    };

    if let Some(message) = message {
        diagnostics.push(Diagnostic::new(
            "SLP070",
            message,
            func.name.range(),
            Severity::Warning,
        ));
    }
}

/// What a recursive scan of a test body turns up. Assertions and verification calls are
/// counted across control flow; nested function/class definitions are separate scopes and
/// are not descended into.
#[derive(Default)]
struct Collected {
    /// Count of `assert` statements anywhere in the body.
    assert_count: usize,
    /// A non-`assert` verification was seen: a `self.assert*`/mock `assert_called*` call,
    /// a `with raises(...)` block, or a `check*`/`verify*`/`expect*` helper call.
    has_verification: bool,
    /// The body runs something observable (a call, assignment, loop, …) — distinguishes a
    /// real-but-unverified test from an empty stub.
    executes_work: bool,
}

fn collect(body: &[Stmt], found: &mut Collected) {
    for stmt in body {
        match stmt {
            Stmt::Assert(_) => {
                found.assert_count += 1;
                found.executes_work = true;
            }
            Stmt::Expr(expr) => match expr.value.as_ref() {
                Expr::Call(call) => {
                    if is_verification_call(call) {
                        found.has_verification = true;
                    }
                    found.executes_work = true;
                }
                value if !is_noop_expr(value) => found.executes_work = true,
                _ => {}
            },
            Stmt::Pass(_) => {}
            // Nested definitions are their own scope; don't count their contents here.
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::If(node) => {
                found.executes_work = true;
                collect(&node.body, found);
                for clause in &node.elif_else_clauses {
                    collect(&clause.body, found);
                }
            }
            Stmt::For(node) => {
                found.executes_work = true;
                collect(&node.body, found);
                collect(&node.orelse, found);
            }
            Stmt::While(node) => {
                found.executes_work = true;
                collect(&node.body, found);
                collect(&node.orelse, found);
            }
            Stmt::With(node) => {
                found.executes_work = true;
                for item in &node.items {
                    if let Expr::Call(call) = &item.context_expr {
                        if is_verification_call(call) {
                            found.has_verification = true;
                        }
                    }
                }
                collect(&node.body, found);
            }
            Stmt::Try(node) => {
                found.executes_work = true;
                collect(&node.body, found);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    collect(&handler.body, found);
                }
                collect(&node.orelse, found);
                collect(&node.finalbody, found);
            }
            Stmt::Match(node) => {
                found.executes_work = true;
                for case in &node.cases {
                    collect(&case.body, found);
                }
            }
            // Assign, AugAssign, Return, Raise, Delete, Import, … all run something.
            _ => found.executes_work = true,
        }
    }
}

/// Classify the top-level `assert` statements of a body as (real, tautological), tracking
/// names bound to literals so `x = 5` … `assert x == 5` is recognized as tautological.
fn classify_top_level(body: &[Stmt]) -> (usize, usize) {
    let mut literals: HashSet<&str> = HashSet::new();
    let (mut real, mut taut) = (0usize, 0usize);
    for stmt in body {
        match stmt {
            Stmt::Assign(assign) => {
                if let [Expr::Name(target)] = assign.targets.as_slice() {
                    update_literal(&mut literals, target.id.as_str(), &assign.value);
                } else {
                    // Tuple / chained targets: anything they bind is no longer a known literal.
                    for target in &assign.targets {
                        if let Expr::Name(name) = target {
                            literals.remove(name.id.as_str());
                        }
                    }
                }
            }
            Stmt::AnnAssign(assign) => {
                if let (Expr::Name(target), Some(value)) =
                    (assign.target.as_ref(), assign.value.as_ref())
                {
                    update_literal(&mut literals, target.id.as_str(), value);
                }
            }
            Stmt::AugAssign(assign) => {
                if let Expr::Name(target) = assign.target.as_ref() {
                    literals.remove(target.id.as_str());
                }
            }
            Stmt::Assert(node) => {
                if is_tautological(&node.test, &literals) {
                    taut += 1;
                } else {
                    real += 1;
                }
            }
            _ => {}
        }
    }
    (real, taut)
}

/// Record `name` as literal-bound if `value` is a literal, otherwise forget it.
fn update_literal<'a>(literals: &mut HashSet<&'a str>, name: &'a str, value: &Expr) {
    if is_literal(value) {
        literals.insert(name);
    } else {
        literals.remove(name);
    }
}

/// An `assert` whose truth is fixed regardless of the code under test: a bare literal
/// (`assert True`, `assert 1`), a name bound to a literal, or a comparison whose operands
/// are all literals / literal-bound names (`assert 1 == 1`, `assert x == 5`).
fn is_tautological(test: &Expr, literals: &HashSet<&str>) -> bool {
    match test {
        Expr::BooleanLiteral(node) => node.value,
        Expr::NumberLiteral(_) | Expr::StringLiteral(_) => true,
        Expr::Name(name) => literals.contains(name.id.as_str()),
        Expr::Compare(node) => {
            is_constish(&node.left, literals)
                && node
                    .comparators
                    .iter()
                    .all(|cmp| is_constish(cmp, literals))
        }
        _ => false,
    }
}

/// A constant literal or a name known to hold one.
fn is_constish(expr: &Expr, literals: &HashSet<&str>) -> bool {
    is_literal(expr) || matches!(expr, Expr::Name(name) if literals.contains(name.id.as_str()))
}

fn is_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BooleanLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::StringLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::EllipsisLiteral(_)
    )
}

/// An expression statement that does nothing observable: a docstring or other bare literal,
/// or a `...` placeholder.
fn is_noop_expr(expr: &Expr) -> bool {
    is_literal(expr)
}

/// A call that performs verification: a `self.assert*`/mock `assert_called*` method, a
/// `pytest.raises`/`assertRaises` context manager, `pytest.fail()`, or a `check*`/`verify*`/
/// `expect*` helper (delegated verification we cannot see into).
fn is_verification_call(call: &ExprCall) -> bool {
    let trailing = trailing_name(&call.func).is_some_and(is_verification_name);
    trailing || root_name(&call.func).is_some_and(is_verification_name)
}

fn is_verification_name(name: &str) -> bool {
    name.starts_with("assert")
        || name.starts_with("check")
        || name.starts_with("verify")
        || name.starts_with("expect")
        || name == "fail"
        || name == "raises"
        || name == "warns"
}

/// The rightmost identifier of a callee: `a.b.c(...)` → `c`, `f(...)` → `f`.
fn trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => trailing_name(&call.func),
        _ => None,
    }
}

/// The leftmost identifier of a callee: `pytest.raises(...)` → `pytest`,
/// `expect(x).to_be(y)` → `expect`.
fn root_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => root_name(&attribute.value),
        Expr::Call(call) => root_name(&call.func),
        _ => None,
    }
}

/// Pytest collects functions/methods named `test*`.
fn is_test_name(name: &str) -> bool {
    name.starts_with("test")
}

/// A class that holds tests: pytest's `Test*` naming, or a `unittest.TestCase` subclass.
fn is_test_class(class: &StmtClassDef) -> bool {
    if class.name.as_str().starts_with("Test") {
        return true;
    }
    class.arguments.as_ref().is_some_and(|arguments| {
        arguments
            .args
            .iter()
            .any(|base| trailing_name(base).is_some_and(|name| name.contains("TestCase")))
    })
}

/// A test deliberately marked skipped/expected-to-fail is intentionally not running, so it
/// is exempt: `@pytest.mark.skip`, `@pytest.mark.skipif`, `@pytest.mark.xfail`,
/// `@unittest.skip`, …
fn is_skipped(func: &StmtFunctionDef) -> bool {
    func.decorator_list.iter().any(|decorator| {
        matches!(
            trailing_name(&decorator.expression),
            Some("skip" | "skipif" | "xfail")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::AssertionFreeTest;
    use crate::lint::{check_file, FileContext, Rule};
    use sloplint_python::parse;

    /// Run SLP070 over `source` and return the rendered diagnostic messages.
    fn findings(source: &str) -> Vec<String> {
        let parsed = parse(source).expect("source parses");
        let ctx = FileContext {
            path: "tests/test_x.py",
            source,
            parsed: &parsed,
            limits: Default::default(),
        };
        let rule = AssertionFreeTest;
        check_file(&ctx, &[&rule as &dyn Rule])
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn flags_assertion_free_test_that_runs_code() {
        let msgs = findings("def test_it():\n    result = compute()\n    print(result)\n");
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].contains("asserts nothing"), "{}", msgs[0]);
    }

    #[test]
    fn spares_empty_stub_body() {
        // No assertion, but nothing executes either — that's a stub, not this rule's smell.
        assert!(findings("def test_todo():\n    pass\n").is_empty());
        assert!(findings("def test_todo():\n    ...\n").is_empty());
        assert!(findings("def test_todo():\n    \"\"\"later\"\"\"\n").is_empty());
    }

    #[test]
    fn flags_assert_true_and_tautological_compare() {
        assert!(findings("def test_a():\n    foo()\n    assert True\n")[0]
            .contains("checks only constants"));
        assert!(findings("def test_b():\n    foo()\n    assert 1 == 1\n")[0]
            .contains("checks only constants"));
    }

    #[test]
    fn flags_assert_on_preceding_literal() {
        let msgs = findings("def test_c():\n    x = 5\n    assert x == 5\n");
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].contains("checks only constants"));
    }

    #[test]
    fn spares_real_assert() {
        assert!(findings("def test_real():\n    assert add(2, 3) == 5\n").is_empty());
    }

    #[test]
    fn spares_when_value_comes_from_a_call() {
        // `x` is bound to a call result, not a literal — `assert x == 5` is a real check.
        assert!(findings("def test_real():\n    x = compute()\n    assert x == 5\n").is_empty());
    }

    #[test]
    fn one_real_assert_among_tautological_spares_the_test() {
        assert!(
            findings("def test_mixed():\n    assert True\n    assert add(2, 3) == 5\n").is_empty()
        );
    }

    #[test]
    fn recognizes_unittest_assert_methods() {
        let src = "class TestThing(unittest.TestCase):\n    def test_eq(self):\n        self.assertEqual(add(2, 3), 5)\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn recognizes_pytest_raises_block() {
        let src = "def test_raises():\n    with pytest.raises(ValueError):\n        parse('bad')\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn recognizes_delegated_verification_helper() {
        let src = "def test_via_helper():\n    result = run()\n    check_result(result)\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn skipped_tests_are_exempt() {
        let src = "@pytest.mark.skip\ndef test_wip():\n    foo()\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn ignores_non_test_functions() {
        // A `test`-prefixed name is required; ordinary helpers are not tests.
        assert!(findings("def helper():\n    compute()\n").is_empty());
    }

    #[test]
    fn flags_unittest_method_with_no_assert() {
        let src = "class TestThing(unittest.TestCase):\n    def test_runs(self):\n        self.thing.do()\n";
        let msgs = findings(src);
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].contains("asserts nothing"));
    }
}

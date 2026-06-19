//! SLP110: pass-through wrapper functions (preview).
//!
//! Flags a function or method whose entire body just forwards its own arguments to a single
//! other call, adding nothing — no branching, no transformation, no extra arguments. Such a
//! wrapper is pure indirection: a layer the reader must step through to reach where the work
//! actually happens, and an abstraction to maintain for no benefit. Complexity metrics can't
//! see it (the code is trivially simple); the slop is the *needless layer*, not the difficulty.
//!
//! High-precision by construction — it fires only when **every** non-`self`/`cls` parameter is
//! forwarded verbatim (reordering allowed) to one call and nothing else happens. A wrapper that
//! validates, logs, transforms an argument, drops a parameter, adds one, or is decorated (the
//! decorator adds behavior) is left alone. Preview until tuned against real corpora.

use std::collections::BTreeSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{Expr, ExprCall, Parameters, Stmt, StmtFunctionDef};
use sloplint_python::Ranged;

use crate::ast_util::collect_functions;
use crate::lint::{FileContext, Rule};

pub struct PassthroughWrapper;

impl Rule for PassthroughWrapper {
    fn code(&self) -> &'static str {
        "SLP110"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut functions = Vec::new();
        collect_functions(&ctx.parsed.syntax().body, &mut functions);
        for function in functions {
            if let Some(callee) = passthrough_callee(function, ctx.source) {
                diagnostics.push(Diagnostic::new(
                    "SLP110",
                    format!(
                        "function `{}` is a pass-through wrapper: it only forwards its arguments \
                         to `{callee}` — inline the call or drop the layer",
                        function.name
                    ),
                    function.name.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// If `function` is a pure pass-through wrapper, the source text of the call it forwards to
/// (for the message); otherwise `None`.
fn passthrough_callee(function: &StmtFunctionDef, source: &str) -> Option<String> {
    // A decorator adds behavior, so a decorated forwarder isn't pure indirection.
    if !function.decorator_list.is_empty() {
        return None;
    }
    // Dunder methods (`__init__`, `__enter__`, operator protocols, …) are forwarding by design
    // far more often than by accident.
    if is_dunder(function.name.as_str()) {
        return None;
    }
    // Need at least one real parameter for anything to "pass through".
    let params = forwardable_params(&function.parameters);
    if params.is_empty() {
        return None;
    }

    // Exactly one statement once an optional leading docstring is set aside.
    let body = match function.body.first() {
        Some(first) if is_docstring(first) => &function.body[1..],
        _ => &function.body[..],
    };
    let [only] = body else { return None };

    // …which is `return <call>` or a bare `<call>` (optionally awaited).
    let value = match only {
        Stmt::Return(node) => node.value.as_deref()?,
        Stmt::Expr(node) => node.value.as_ref(),
        _ => return None,
    };
    let call = match unwrap_await(value) {
        Expr::Call(call) => call,
        _ => return None,
    };

    // `super().__init__(...)` and direct recursion are forwarding shapes that aren't slop.
    // Detected structurally so whitespace (`super ().run`) or a same-named attribute
    // (`mod.run`) can't fool a textual check.
    if is_super_call(&call.func) || is_self_recursion(&call.func, function.name.as_str()) {
        return None;
    }

    // Every argument must be a bare parameter reference (`x`, `*args`, `**kwargs`, `k=x`) — no
    // literals, transforms, or nested calls — and together they must forward each parameter
    // exactly once (a permutation; comparison is by multiset so a repeat like `f(a, a, b)` is
    // NOT a clean pass-through).
    let mut forwarded = forwarded_names(call)?;
    let mut expected: Vec<&str> = params.iter().copied().collect();
    forwarded.sort_unstable();
    expected.sort_unstable();
    if forwarded != expected {
        return None;
    }

    // Collapse any interior whitespace so a multi-line callee renders on one line.
    let callee = source[call.func.range()]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(callee)
}

/// `super().method(...)` — an attribute access on a bare `super()` call.
fn is_super_call(func: &Expr) -> bool {
    let Expr::Attribute(attribute) = func else {
        return false;
    };
    let Expr::Call(inner) = attribute.value.as_ref() else {
        return false;
    };
    matches!(inner.func.as_ref(), Expr::Name(name) if name.id.as_str() == "super")
}

/// A direct recursive call: the callee is a bare name equal to the function's own name.
fn is_self_recursion(func: &Expr, function_name: &str) -> bool {
    matches!(func, Expr::Name(name) if name.id.as_str() == function_name)
}

/// Strip one layer of `await`, so `await impl(x)` is treated like `impl(x)`.
fn unwrap_await(expr: &Expr) -> &Expr {
    match expr {
        Expr::Await(node) => &node.value,
        other => other,
    }
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

fn is_docstring(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(node) if matches!(node.value.as_ref(), Expr::StringLiteral(_)))
}

/// The parameter names that ought to be forwarded — every declared parameter except the
/// implicit `self`/`cls` receiver.
fn forwardable_params(parameters: &Parameters) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    for param in parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .chain(&parameters.kwonlyargs)
    {
        names.insert(param.parameter.name.as_str());
    }
    if let Some(vararg) = &parameters.vararg {
        names.insert(vararg.name.as_str());
    }
    if let Some(kwarg) = &parameters.kwarg {
        names.insert(kwarg.name.as_str());
    }
    names.remove("self");
    names.remove("cls");
    names
}

/// The names a call's arguments forward, **with duplicates preserved** (so a repeated
/// argument can be detected as not-a-clean-forward), or `None` if any argument is anything
/// other than a bare name reference (`x`), a `*x` / `**x` unpack, or a `k=x` keyword forward.
fn forwarded_names(call: &ExprCall) -> Option<Vec<&str>> {
    let mut names = Vec::new();
    for arg in call.arguments.args.iter() {
        let name = match arg {
            Expr::Name(name) => name.id.as_str(),
            Expr::Starred(starred) => match starred.value.as_ref() {
                Expr::Name(name) => name.id.as_str(),
                _ => return None,
            },
            _ => return None,
        };
        names.push(name);
    }
    for keyword in call.arguments.keywords.iter() {
        match &keyword.value {
            Expr::Name(name) => names.push(name.id.as_str()),
            _ => return None,
        };
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    /// Count of SLP110 findings over `source`.
    fn findings(source: &str) -> usize {
        let parsed = parse(source).expect("valid python");
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            limits: Default::default(),
        };
        let mut diagnostics = Vec::new();
        PassthroughWrapper.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    #[test]
    fn flags_plain_forwarding_function() {
        assert_eq!(findings("def wrap(a, b):\n    return impl(a, b)\n"), 1);
    }

    #[test]
    fn flags_reordered_and_kwargs_forwarding() {
        assert_eq!(findings("def wrap(a, b):\n    return impl(b, a)\n"), 1);
        assert_eq!(
            findings("def wrap(*args, **kwargs):\n    return impl(*args, **kwargs)\n"),
            1
        );
        assert_eq!(findings("def wrap(a, b):\n    return impl(x=a, y=b)\n"), 1);
    }

    #[test]
    fn flags_method_delegation_and_async() {
        assert_eq!(
            findings("class C:\n    def save(self, row):\n        return self._repo.save(row)\n"),
            1
        );
        assert_eq!(
            findings("async def wrap(a):\n    return await impl(a)\n"),
            1,
            "await forwarding still counts"
        );
        // Bare expression-statement forwarding (no return).
        assert_eq!(findings("def wrap(a):\n    impl(a)\n"), 1);
    }

    #[test]
    fn ignores_added_logic() {
        // Transforms an argument.
        assert_eq!(findings("def wrap(a, b):\n    return impl(a + 1, b)\n"), 0);
        // Nested call (extra work).
        assert_eq!(findings("def wrap(a):\n    return impl(clean(a))\n"), 0);
        // Adds a literal argument.
        assert_eq!(
            findings("def wrap(a):\n    return impl(a, verbose=True)\n"),
            0
        );
        // More than one statement (logging/validation).
        assert_eq!(
            findings("def wrap(a):\n    log(a)\n    return impl(a)\n"),
            0
        );
        // Drops a parameter — not a pure pass-through.
        assert_eq!(findings("def wrap(a, b):\n    return impl(a)\n"), 0);
    }

    #[test]
    fn ignores_duplicated_arguments() {
        // A repeated argument isn't a clean 1:1 forward — multiset comparison must reject it.
        assert_eq!(findings("def wrap(a, b):\n    return impl(a, a, b)\n"), 0);
        assert_eq!(findings("def wrap(a):\n    return impl(a, x=a)\n"), 0);
    }

    #[test]
    fn super_exclusion_is_not_fooled_by_whitespace() {
        // `super ()` (legal) must still be recognized as super-delegation, not flagged.
        assert_eq!(
            findings("class C:\n    def run(self, a):\n        return super ().run(a)\n"),
            0
        );
        // A same-named attribute on something else is NOT recursion — it's a real forward.
        assert_eq!(findings("def run(a):\n    return mod.run(a)\n"), 1);
    }

    #[test]
    fn nested_forwarding_function_is_flagged() {
        // A nested function that only forwards is still needless indirection.
        assert_eq!(
            findings(
                "def outer(a):\n    def inner(b):\n        return impl(b)\n    return inner\n"
            ),
            1
        );
    }

    #[test]
    fn ignores_non_call_bodies() {
        assert_eq!(findings("def wrap(a, b):\n    return (a, b)\n"), 0);
        assert_eq!(
            findings("def wrap(self, k):\n    return self._data[k]\n"),
            0
        );
        assert_eq!(findings("def wrap(a):\n    return a\n"), 0);
    }

    #[test]
    fn ignores_dunder_super_decorated_and_recursion() {
        // Dunder forwarding (constructor delegation) is idiomatic, not slop.
        assert_eq!(
            findings(
                "class C:\n    def __init__(self, *a, **k):\n        super().__init__(*a, **k)\n"
            ),
            0
        );
        // super() delegation by name.
        assert_eq!(
            findings("class C:\n    def run(self, a):\n        return super().run(a)\n"),
            0
        );
        // Decorated — the decorator adds behavior.
        assert_eq!(
            findings("import functools\n@functools.lru_cache\ndef wrap(a):\n    return impl(a)\n"),
            0
        );
        // Direct recursion.
        assert_eq!(findings("def wrap(a):\n    return wrap(a)\n"), 0);
    }

    #[test]
    fn ignores_zero_parameter_factories() {
        // Nothing passes through, so a no-arg accessor/factory isn't a wrapper.
        assert_eq!(findings("def make():\n    return Thing()\n"), 0);
        assert_eq!(
            findings("class C:\n    def value(self):\n        return self._compute()\n"),
            0
        );
    }

    #[test]
    fn forwarding_wrapper_with_docstring_still_flags() {
        assert_eq!(
            findings("def wrap(a, b):\n    \"\"\"Forward to impl.\"\"\"\n    return impl(a, b)\n"),
            1
        );
    }
}

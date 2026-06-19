//! SLP034: self-admitted-debt + stub-body "unfinished generation" detector.
//!
//! A confident-looking function whose body is an empty stub, topped with a comment
//! admitting it isn't finished ("implement the others too, but I'm not sure how"), is a
//! maintenance trap: it reads as real code, passes a glance in review, and silently does
//! nothing at runtime. The *combination* — admitted uncertainty + trivial body — is a
//! high-precision marker that the scope was left unfinished and shipped anyway.
//!
//! This is the slice Ruff doesn't cover: `TD*`/`FIX*` match the literal `TODO`/`FIXME`
//! tags and `ERA001` flags commented-out code, but none classify natural-language
//! *uncertainty phrasing* or correlate a comment with a **stub body**. We fire only on the
//! conjunction, so a plain `# TODO: refactor` over real code stays Ruff's territory.
//!
//! Precision guards: only **comments** count (a docstring like "Subclasses must implement
//! this" does not), and `@abstractmethod`/`@overload` stubs are exempt — those bodies are
//! *meant* to be trivial.

use sloplint_python::ast::{ExceptHandler, Expr, Stmt, StmtClassDef, StmtFunctionDef, StmtRaise};
use sloplint_python::{Ranged, TextSize, TokenKind};

use sloplint_diagnostics::{Diagnostic, Severity};

use crate::lint::{FileContext, Rule};

/// One comment, pre-parsed: its 1-based line, whether it is the only content on that line
/// (an "own-line" comment vs a trailing one), and its lowercased text.
struct Comment {
    line: usize,
    own_line: bool,
    text: String,
}

pub struct UnfinishedStub;

impl Rule for UnfinishedStub {
    fn code(&self) -> &'static str {
        "SLP034"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let comments: Vec<Comment> = ctx
            .parsed
            .tokens()
            .iter()
            .filter(|token| token.kind() == TokenKind::Comment)
            .map(|token| {
                let start = usize::from(token.range().start()).min(ctx.source.len());
                let line_start = ctx.source[..start].rfind('\n').map_or(0, |i| i + 1);
                Comment {
                    line: line_at(ctx.source, token.range().start()),
                    own_line: ctx.source[line_start..start].trim().is_empty(),
                    text: ctx.source[token.range()].to_lowercase(),
                }
            })
            .collect();
        if comments.is_empty() {
            return;
        }

        // `@abstractmethod`/`Protocol`/ABC interface methods are *meant* to be trivial, so
        // their whole subtree is skipped; we gather only the candidate stub functions.
        let mut functions = Vec::new();
        gather_functions(&ctx.parsed.syntax().body, &mut functions);
        for function in functions {
            if is_exempt(function) || !is_stub_body(&function.body) {
                continue;
            }
            // A comment counts when it sits inside the function (between its header and end,
            // trailing or not) or is an *own-line* comment directly above the header — a
            // trailing comment on the preceding statement is not "attached" to the function.
            let header_start = function
                .decorator_list
                .iter()
                .map(|decorator| decorator.range().start())
                .min()
                .unwrap_or_else(|| function.range().start());
            let header_line = line_at(ctx.source, header_start);
            let end = line_at(ctx.source, function.range().end());

            if comments.iter().any(|comment| {
                admits_unfinished(&comment.text)
                    && ((comment.line >= header_line && comment.line <= end)
                        || (comment.own_line && comment.line + 1 == header_line))
            }) {
                diagnostics.push(Diagnostic::new(
                    "SLP034",
                    format!(
                        "function `{}` is an unfinished stub: a trivial body plus a comment \
                         admitting it isn't done — finish it or drop the placeholder",
                        function.name
                    ),
                    function.name.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Collect every function reachable in `body` (methods + nested functions), but prune the
/// subtree of any interface class (`Protocol`/ABC), whose method stubs are legitimate.
fn gather_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(function) => {
                out.push(function);
                gather_functions(&function.body, out);
            }
            Stmt::ClassDef(class) => {
                if !is_interface_class(class) {
                    gather_functions(&class.body, out);
                }
            }
            Stmt::If(node) => {
                gather_functions(&node.body, out);
                for clause in &node.elif_else_clauses {
                    gather_functions(&clause.body, out);
                }
            }
            Stmt::For(node) => {
                gather_functions(&node.body, out);
                gather_functions(&node.orelse, out);
            }
            Stmt::While(node) => {
                gather_functions(&node.body, out);
                gather_functions(&node.orelse, out);
            }
            Stmt::With(node) => gather_functions(&node.body, out),
            Stmt::Try(node) => {
                gather_functions(&node.body, out);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    gather_functions(&handler.body, out);
                }
                gather_functions(&node.orelse, out);
                gather_functions(&node.finalbody, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    gather_functions(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// A class declaring an interface — a `Protocol`/`ABC` subclass or `metaclass=ABCMeta`.
/// Its method stubs are declarations, not unfinished work.
fn is_interface_class(class: &StmtClassDef) -> bool {
    let Some(arguments) = &class.arguments else {
        return false;
    };
    let base_is_interface = arguments
        .args
        .iter()
        .any(|base| matches!(trailing_name(base), Some("Protocol" | "ABC")));
    let metaclass_is_abc = arguments.keywords.iter().any(|keyword| {
        keyword.arg.as_ref().map(|arg| arg.as_str()) == Some("metaclass")
            && trailing_name(&keyword.value) == Some("ABCMeta")
    });
    base_is_interface || metaclass_is_abc
}

/// 1-based line number for a byte offset.
fn line_at(source: &str, offset: TextSize) -> usize {
    let offset = usize::from(offset).min(source.len());
    source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// `@abstractmethod`/`@overload`/`@abstractproperty` bodies are *meant* to be trivial, so
/// they are not unfinished work.
fn is_exempt(function: &StmtFunctionDef) -> bool {
    function.decorator_list.iter().any(|decorator| {
        matches!(
            trailing_name(&decorator.expression),
            Some("abstractmethod" | "abstractproperty" | "overload")
        )
    })
}

/// A body that does nothing: a single `pass`/`...`/`raise NotImplementedError`/bare
/// `return`/`return None`/lone `print`/log call, optionally preceded by a docstring (and a
/// docstring-only body counts too).
fn is_stub_body(body: &[Stmt]) -> bool {
    let rest = match body.split_first() {
        Some((first, rest)) if is_docstring(first) => rest,
        _ => body,
    };
    match rest {
        [] => true,
        [only] => is_trivial_stmt(only),
        _ => false,
    }
}

fn is_docstring(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_)))
}

fn is_trivial_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr) => {
            matches!(expr.value.as_ref(), Expr::EllipsisLiteral(_)) || is_print_or_log(&expr.value)
        }
        Stmt::Return(node) => match node.value.as_deref() {
            None => true,
            Some(value) => matches!(value, Expr::NoneLiteral(_)),
        },
        Stmt::Raise(node) => is_not_implemented(node),
        _ => false,
    }
}

fn is_not_implemented(raise: &StmtRaise) -> bool {
    let exc = match raise.exc.as_deref() {
        Some(Expr::Call(call)) => call.func.as_ref(),
        Some(other) => other,
        None => return false,
    };
    matches!(exc, Expr::Name(name) if name.id.as_str() == "NotImplementedError"
        || name.id.as_str() == "NotImplemented")
}

fn is_print_or_log(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else { return false };
    match call.func.as_ref() {
        Expr::Name(name) => name.id.as_str() == "print",
        Expr::Attribute(attribute) => matches!(
            attribute.attr.as_str(),
            "debug" | "info" | "warning" | "warn" | "error" | "exception" | "critical" | "log"
        ),
        _ => false,
    }
}

/// The rightmost identifier of a decorator expression: `@a.b.c` → `c`, `@f(...)` → `f`.
fn trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => trailing_name(&call.func),
        _ => None,
    }
}

/// Curated natural-language phrases that admit the code is unfinished or uncertain. These
/// are deliberately *phrases*, not the bare `TODO`/`FIXME` tags Ruff already covers — and
/// the conjunction with a stub body keeps precision high.
const UNFINISHED_PHRASES: &[&str] = &[
    "implement later",
    "implement this",
    "implement the rest",
    "implement me",
    "to be implemented",
    "not yet implemented",
    "not implemented yet",
    "needs implementation",
    "need to implement",
    "should implement",
    "finish this",
    "finish later",
    "i don't know",
    "i dont know",
    "not sure how",
    "no idea how",
    "don't know how",
    "dont know how",
    "not quite right",
    "fix this",
    "needs fixing",
    "needs work",
    "fill in",
    "fill this in",
    "fill me in",
    "figure out",
    "figure this out",
    "come back to this",
    "revisit this",
    "uncomment",
    "placeholder",
    "stub out",
    "just a stub",
    "work in progress",
];

fn admits_unfinished(comment_lower: &str) -> bool {
    UNFINISHED_PHRASES
        .iter()
        .any(|phrase| comment_lower.contains(phrase))
}

#[cfg(test)]
mod tests {
    use super::UnfinishedStub;
    use crate::lint::{check_file, FileContext, Rule};
    use sloplint_python::parse;

    fn findings(source: &str) -> Vec<String> {
        let parsed = parse(source).expect("source parses");
        let ctx = FileContext {
            path: "m.py",
            source,
            parsed: &parsed,
            limits: Default::default(),
        };
        let rule = UnfinishedStub;
        check_file(&ctx, &[&rule as &dyn Rule])
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn flags_uncertainty_comment_over_not_implemented() {
        let src = "def handle(event):\n    # not sure how to do this yet\n    raise NotImplementedError\n";
        assert_eq!(findings(src).len(), 1);
    }

    #[test]
    fn flags_comment_directly_above_stub() {
        let src = "# TODO: implement this properly\ndef parse(s):\n    pass\n";
        let msgs = findings(src);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(msgs[0].contains("unfinished stub"));
    }

    #[test]
    fn flags_ellipsis_and_bare_return_stubs() {
        assert_eq!(
            findings("def a():\n    # fill in later\n    ...\n").len(),
            1
        );
        assert_eq!(
            findings("def b():\n    # figure this out\n    return None\n").len(),
            1
        );
    }

    #[test]
    fn plain_todo_over_real_code_is_not_flagged() {
        // Bare tag + a real body — Ruff's TD territory, not ours.
        let src = "def add(a, b):\n    # TODO: refactor\n    return a + b\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn finished_function_is_not_flagged() {
        assert!(findings("def add(a, b):\n    return a + b\n").is_empty());
    }

    #[test]
    fn stub_without_admitting_comment_is_not_flagged() {
        // A trivial body alone is not enough — the conjunction requires the comment.
        assert!(findings("def later():\n    raise NotImplementedError\n").is_empty());
        assert!(findings("def noop():\n    pass  # noqa\n").is_empty());
    }

    #[test]
    fn docstring_uncertainty_does_not_count_only_comments() {
        // An abstract-style docstring is not a comment; must not fire.
        let src = "def area(self):\n    \"\"\"Subclasses must implement this.\"\"\"\n    raise NotImplementedError\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn abstractmethod_stub_is_exempt() {
        let src = "class Shape:\n    @abstractmethod\n    def area(self):\n        # implement this in subclasses\n        ...\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn overload_stub_is_exempt() {
        let src = "@overload\ndef f(x):\n    # implement this\n    ...\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn docstring_plus_stub_with_comment_is_flagged() {
        let src = "def f():\n    \"\"\"Do the thing.\"\"\"\n    # implement this later\n    raise NotImplementedError\n";
        assert_eq!(findings(src).len(), 1);
    }

    #[test]
    fn real_body_with_uncertainty_comment_is_not_flagged() {
        // The comment admits uncertainty, but the body actually does work.
        let src =
            "def f(xs):\n    # not sure this handles every case\n    return [x * 2 for x in xs]\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn trailing_comment_on_preceding_statement_is_not_attributed() {
        // The comment is a *trailing* comment on `items`, not an own-line comment above f.
        let src = "items = [1, 2]  # fill in the rest later\ndef warm_cache(keys):\n    pass\n";
        assert!(findings(src).is_empty(), "{:?}", findings(src));
    }

    #[test]
    fn protocol_method_stub_is_exempt() {
        let src = "class Reader(Protocol):\n    def read(self, key):\n        # implement this\n        ...\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn abc_metaclass_method_stub_is_exempt() {
        let src = "class Base(metaclass=ABCMeta):\n    def run(self):\n        # implement this later\n        raise NotImplementedError\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn nested_stub_is_flagged() {
        // Deliberate: an unfinished nested closure is still unfinished code.
        let src = "def outer():\n    def inner():\n        # figure this out\n        pass\n    return inner\n";
        assert_eq!(findings(src).len(), 1);
    }
}

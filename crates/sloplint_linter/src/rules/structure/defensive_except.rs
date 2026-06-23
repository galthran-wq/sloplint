//! SLP030: overly defensive try/except.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{ExceptHandler, Expr, Stmt};
use sloplint_python::Ranged;

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags a broad `except` whose body does nothing useful — a single `pass`, a bare re-raise,
/// or a lone log call that swallows the error.
///
/// ## Why is this bad?
/// Such a handler adds no value and hides failures. Ruff's `BLE001` flags the blind
/// `except Exception`; this adds the slice it doesn't — the no-value body. A handler that logs
/// **and** re-raises (two statements) is legitimate and left alone.
#[derive(ViolationMetadata)]
pub struct DefensiveExcept;

impl Rule for DefensiveExcept {
    fn code(&self) -> &'static str {
        "SLP030"
    }

    fn check_stmt(&self, stmt: &Stmt, _ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let Stmt::Try(node) = stmt else { return };
        for handler in &node.handlers {
            let ExceptHandler::ExceptHandler(handler) = handler;
            if is_broad(handler.type_.as_deref()) && is_low_value(&handler.body) {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    "broad except whose body only passes, logs, or re-raises adds no value",
                    handler.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// A handler is "broad" if it's bare (`except:`) or catches `Exception`/`BaseException`.
fn is_broad(exception_type: Option<&Expr>) -> bool {
    match exception_type {
        None => true,
        Some(Expr::Name(name)) => {
            matches!(name.id.as_str(), "Exception" | "BaseException")
        }
        _ => false,
    }
}

/// A body adds no value if it is exactly one statement that passes, bare/no-op re-raises,
/// or merely logs. A `raise NewError(...) from exc` (exception *translation*) is real work
/// and is NOT low-value; nor is any two-or-more-statement body (e.g. log *then* raise).
fn is_low_value(body: &[Stmt]) -> bool {
    match body {
        [Stmt::Pass(_)] => true,
        // Bare `raise`, or `raise <caught name>` — both just re-raise. A `raise <Call>`
        // constructs/translates an exception and is spared.
        [Stmt::Raise(node)] => matches!(node.exc.as_deref(), None | Some(Expr::Name(_))),
        [Stmt::Expr(expr)] => is_log_call(&expr.value),
        _ => false,
    }
}

fn is_log_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else { return false };
    match call.func.as_ref() {
        Expr::Attribute(attribute) => matches!(
            attribute.attr.as_str(),
            "debug" | "info" | "warning" | "warn" | "error" | "exception" | "critical" | "log"
        ),
        Expr::Name(name) => name.id.as_str() == "print",
        _ => false,
    }
}

//! Exception-handling hygiene: classify every `except` handler in a module as bare (no type),
//! broad (`Exception`/`BaseException`), and/or swallow (a body of exactly `pass`/`continue`/`...`).

use crate::expr_trailing_name;
use crate::ExceptionStats;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{ExceptHandler, Expr, Stmt};

/// Exception-handling hygiene counts for a module body: every `except` handler, anywhere
/// (module level or nested in functions/classes), classified bare / broad / swallow. A bare
/// `except:` has no type; broad catches `Exception`/`BaseException` (or a tuple containing one);
/// swallow is a body of exactly `pass`, `continue`, or `...`.
pub(crate) fn exception_stats(body: &[Stmt]) -> ExceptionStats {
    #[derive(Default)]
    struct Counter {
        stats: ExceptionStats,
    }
    impl Visitor<'_> for Counter {
        fn visit_except_handler(&mut self, handler: &ExceptHandler) {
            let ExceptHandler::ExceptHandler(h) = handler;
            self.stats.handlers += 1;
            match &h.type_ {
                None => self.stats.bare += 1,
                Some(ty) if is_broad_except(ty) => self.stats.broad += 1,
                Some(_) => {}
            }
            if is_swallow_body(&h.body) {
                self.stats.swallow += 1;
            }
            visitor::walk_except_handler(self, handler);
        }
    }
    let mut counter = Counter::default();
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.stats
}

/// Whether an `except` type expression is "broad": it names `Exception`/`BaseException` (by
/// trailing identifier, so `builtins.Exception` counts too), or is a tuple containing one.
fn is_broad_except(expr: &Expr) -> bool {
    match expr {
        Expr::Tuple(tuple) => tuple.elts.iter().any(is_broad_except),
        other => matches!(
            expr_trailing_name(other),
            Some("Exception" | "BaseException")
        ),
    }
}

/// Whether a handler body silently swallows the error: a single `pass`, `continue`, or `...`
/// statement and nothing else. (A bare logging-only body is deliberately *not* counted — kept
/// strict to avoid false positives.)
fn is_swallow_body(body: &[Stmt]) -> bool {
    match body {
        [Stmt::Pass(_)] | [Stmt::Continue(_)] => true,
        [Stmt::Expr(expr)] => matches!(expr.value.as_ref(), Expr::EllipsisLiteral(_)),
        _ => false,
    }
}

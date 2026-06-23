//! SLP040: redundant type hint (preview).
//!
//! Flags an annotation that merely restates a trivially-inferable literal type, e.g.
//! `count: int = 0` or `name: str = "x"`. The annotation carries no information the literal
//! doesn't already. Conservative: only the builtin scalar types paired with a matching
//! literal are flagged; anything non-obvious is left alone. Preview until tuned (module
//! constants legitimately annotate sometimes).

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{Expr, Number, Stmt};
use sloplint_python::Ranged;

use crate::ast_util::walk_statements;
use crate::lint::{FileContext, Rule};

pub struct RedundantTypeHint;

impl Rule for RedundantTypeHint {
    fn code(&self) -> &'static str {
        "SLP040"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        walk_statements(&ctx.parsed.syntax().body, &mut |stmt| {
            let Stmt::AnnAssign(node) = stmt else { return };
            let Some(value) = &node.value else { return };
            let annotation: &Expr = &node.annotation;
            let value: &Expr = value;
            if let Expr::Name(name) = annotation {
                if restates_literal(name.id.as_str(), value) {
                    diagnostics.push(Diagnostic::new(
                        self.code(),
                        format!(
                            "type hint `{}` restates the literal value",
                            name.id.as_str()
                        ),
                        node.annotation.range(),
                        Severity::Warning,
                    ));
                }
            }
        });
    }
}

/// True when `type_name` is the trivially-inferable type of the literal `value`.
fn restates_literal(type_name: &str, value: &Expr) -> bool {
    match (type_name, value) {
        ("int", Expr::NumberLiteral(n)) => matches!(n.value, Number::Int(_)),
        ("float", Expr::NumberLiteral(n)) => matches!(n.value, Number::Float(_)),
        ("complex", Expr::NumberLiteral(n)) => matches!(n.value, Number::Complex { .. }),
        ("str", Expr::StringLiteral(_)) => true,
        ("bytes", Expr::BytesLiteral(_)) => true,
        ("bool", Expr::BooleanLiteral(_)) => true,
        _ => false,
    }
}

//! SLP120: low-cohesion "god class" detector (preview).

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_metrics::cohesion::class_cohesion;
use sloplint_python::ast::{Expr, Stmt, StmtClassDef};
use sloplint_python::Ranged;

use crate::ast_util::walk_statements;
use crate::lint::{FileContext, Rule};

/// ## What it does
/// Flags a class whose methods split into two or more unrelated groups by **LCOM4**
/// (connected components of methods linked by shared `self` state) — a catch-all
/// `Utils`/`Manager`/`Service` that bundles unrelated methods around no shared state.
///
/// ## Why is this bad?
/// Coincidental binding hides dependencies, resists reuse, and forces unrelated concerns to
/// change together; a plain method/attribute *count* can't see it — a tidy-looking 5-method
/// class can still be two disjoint concepts. Only classes with at least `lcom4_min_methods`
/// methods are judged, and data/interface classes (`@dataclass`/`attrs`, `Protocol`, `ABC`,
/// `Enum`, `NamedTuple`, `TypedDict`) are allowlisted, since their low method-cohesion is by
/// design. Preview.
pub struct GodClass;

impl Rule for GodClass {
    fn code(&self) -> &'static str {
        "SLP120"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut classes = Vec::new();
        collect_classes(&ctx.parsed.syntax().body, &mut classes);

        for class in classes {
            if is_allowlisted(class) {
                continue;
            }
            let cohesion = class_cohesion(class);
            if cohesion.methods >= ctx.limits.lcom4_min_methods
                && cohesion.components > ctx.limits.lcom4_max_components
            {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    format!(
                        "class `{}` has low cohesion: its methods split into {} unrelated \
                         groups (LCOM4={}) — consider splitting it",
                        class.name, cohesion.components, cohesion.components,
                    ),
                    class.name.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Collect every `ClassDef` reachable in `body` (nested classes included).
fn collect_classes<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtClassDef>) {
    walk_statements(body, &mut |stmt| {
        if let Stmt::ClassDef(class) = stmt {
            out.push(class);
        }
    });
}

/// Data/interface classes whose low method-cohesion is by design are not "god classes".
fn is_allowlisted(class: &StmtClassDef) -> bool {
    // Data-class decorators: the stdlib `@dataclass` (unambiguous), or any decorator rooted at
    // `attr`/`attrs` (`@attr.s`, `@attrs.define`). Bare `@define`/`@frozen` are NOT matched —
    // those are common generic decorator names and would silently exempt a real god class.
    for decorator in &class.decorator_list {
        let expression = &decorator.expression;
        if trailing_name(expression) == Some("dataclass")
            || matches!(root_name(expression), Some("attr" | "attrs"))
        {
            return true;
        }
    }
    let Some(arguments) = &class.arguments else {
        return false;
    };
    // Base classes that mark an interface or data container.
    for base in arguments.args.iter() {
        if let Some(name) = trailing_name(base) {
            if is_container_base(name) {
                return true;
            }
        }
    }
    // `metaclass=ABCMeta`.
    for keyword in arguments.keywords.iter() {
        let is_metaclass = keyword.arg.as_ref().map(|arg| arg.as_str()) == Some("metaclass");
        if is_metaclass && trailing_name(&keyword.value) == Some("ABCMeta") {
            return true;
        }
    }
    false
}

fn is_container_base(name: &str) -> bool {
    matches!(
        name,
        "Protocol"
            | "ABC"
            | "ABCMeta"
            | "Enum"
            | "IntEnum"
            | "IntFlag"
            | "Flag"
            | "StrEnum"
            | "NamedTuple"
            | "TypedDict"
    )
}

/// The trailing identifier of a dotted/called/subscripted name expression — e.g. `Protocol`
/// from `typing.Protocol`, `dataclass` from `@dataclass(frozen=True)`, `Protocol` from
/// `Protocol[T]`.
fn trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => trailing_name(&call.func),
        Expr::Subscript(subscript) => trailing_name(&subscript.value),
        _ => None,
    }
}

/// The leftmost (root) identifier of a dotted/called/subscripted name — e.g. `attrs` from
/// `attrs.define(...)`, `attr` from `attr.s`.
fn root_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => root_name(&attribute.value),
        Expr::Call(call) => root_name(&call.func),
        Expr::Subscript(subscript) => root_name(&subscript.value),
        _ => None,
    }
}

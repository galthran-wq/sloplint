//! SLP120: low-cohesion "god class" detector (preview).
//!
//! Flags a class whose methods split into ≥2 unrelated groups by **LCOM4** (connected
//! components — see [`sloplint_metrics::cohesion`]). A catch-all `Utils`/`Manager`/`Service`
//! that bundles unrelated methods around no shared state is "coincidental binding": it hides
//! dependencies, resists reuse, and forces unrelated concerns to change together. A pure
//! method/attribute *count* can't see it — a tidy-looking 5-method class can still be two
//! disjoint concepts.
//!
//! High-precision guards: only classes with at least `lcom4_min_methods` methods are judged
//! (small classes are too noisy), and data/interface classes — `@dataclass`/`attrs`,
//! `Protocol`, `ABC`, `Enum`, `NamedTuple`, `TypedDict` — are allowlisted, since their low
//! method-cohesion is by design. Both thresholds are configurable under `[limits]`; Preview.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_metrics::cohesion::class_cohesion;
use sloplint_python::ast::{Expr, Stmt, StmtClassDef};
use sloplint_python::Ranged;

use crate::ast_util::walk_statements;
use crate::lint::{FileContext, Rule};

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
                    "SLP120",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Limits;
    use sloplint_python::parse;

    fn findings(source: &str) -> usize {
        findings_with(source, Limits::default())
    }

    fn findings_with(source: &str, limits: Limits) -> usize {
        let parsed = parse(source).expect("valid python");
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            limits,
            security_extra: &[],
            placeholders_extra: &[],
            comment_phrases_extra: &[],
            crosslang_allow: &[],
        };
        let mut diagnostics = Vec::new();
        GodClass.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    const TWO_CONCEPT_UTILS: &str = "\
class Utils:
    def parse(self, text):
        return self.parser.run(text)

    def tokenize(self, text):
        return self.parser.split(text)

    def render(self, node):
        return self.formatter.render(node)
";

    #[test]
    fn flags_two_concept_god_class() {
        assert_eq!(findings(TWO_CONCEPT_UTILS), 1);
    }

    #[test]
    fn cohesive_class_is_not_flagged() {
        let cohesive = "\
class Counter:
    def __init__(self):
        self.total = 0

    def add(self, n):
        self.total += n

    def double(self):
        self.add(self.total)

    def value(self):
        return self.total
";
        assert_eq!(findings(cohesive), 0);
    }

    #[test]
    fn small_class_below_min_methods_is_not_flagged() {
        // Two disjoint methods, but under the default 3-method floor.
        let small = "\
class Pair:
    def left(self):
        return self.a

    def right(self):
        return self.b
";
        assert_eq!(findings(small), 0);
    }

    #[test]
    fn dataclass_and_protocol_are_allowlisted() {
        let dataclass = "\
import dataclasses

@dataclasses.dataclass
class Config:
    def host(self):
        return self.h

    def port(self):
        return self.p

    def scheme(self):
        return self.s
";
        assert_eq!(findings(dataclass), 0, "dataclass is allowlisted");

        let protocol = "\
from typing import Protocol

class Store(Protocol):
    def get(self, k):
        return self.a

    def put(self, k):
        return self.b

    def drop(self, k):
        return self.c
";
        assert_eq!(findings(protocol), 0, "Protocol is allowlisted");
    }

    #[test]
    fn attrs_decorator_is_allowlisted_but_bare_user_decorators_are_not() {
        let attrs = format!("import attrs\n\n@attrs.define\n{TWO_CONCEPT_UTILS}");
        assert_eq!(findings(&attrs), 0, "@attrs.define is allowlisted");

        // A user-defined `@frozen` must NOT silently exempt a real god class.
        let user_frozen = format!("@frozen\n{TWO_CONCEPT_UTILS}");
        assert_eq!(
            findings(&user_frozen),
            1,
            "bare @frozen is not an allowlist"
        );
    }

    #[test]
    fn thresholds_are_configurable() {
        // Raising the component ceiling silences the smell.
        let lax = Limits {
            lcom4_max_components: 2,
            ..Limits::default()
        };
        assert_eq!(findings_with(TWO_CONCEPT_UTILS, lax), 0);
    }
}

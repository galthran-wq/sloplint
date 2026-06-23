//! SLP210: phantom security-guard calls and decorators.

use std::collections::HashMap;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Decorator, ExceptHandler, Expr, ExprContext, Parameters, Stmt};
use sloplint_python::{Ranged, TextRange};

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags a call to / decorator of a known security-guard name (`validate_token`,
/// `sanitize_input`, `@requires_auth`, `@login_required`) that is never defined, imported, or
/// otherwise bound in the module.
///
/// ## Why is this bad?
/// AI emits fake security scaffolding — the code *reads* as defended but at runtime is a
/// `NameError` or a no-op (CWE-693, Protection Mechanism Failure). Curated to security-guard
/// names (not a general undefined-name lint — that's Ruff's `F821`); only bare-name
/// calls/decorators are considered, and a near-miss is reported as a likely typo.
///
/// ## Example
/// ```python
/// @requires_auth          # never defined or imported in this module
/// def delete_account(user):
///     ...
/// ```
#[derive(ViolationMetadata)]
pub struct PhantomGuard;

impl Rule for PhantomGuard {
    fn code(&self) -> &'static str {
        "SLP210"
    }

    fn check_source(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let module = ctx.parsed.syntax();

        // 1. Collect every name bound anywhere in the module (over-approximate — more bound names
        //    means fewer findings, the safe direction for a "this guard is undefined" rule).
        let mut bindings = Bindings {
            names: Vec::new().into_iter().collect(),
        };
        bindings.visit_body(&module.body);
        let bound = bindings.names;

        // 2. Find guard calls/decorators whose name isn't bound. Keyed by the offending name's
        //    start offset so a decorator-call (`@rate_limit(...)`) reported by both the decorator
        //    pass and the generic call pass collapses to one finding.
        let mut finder = Finder {
            bound: &bound,
            extra: ctx.security_extra,
            found: HashMap::new(),
        };
        finder.visit_body(&module.body);

        let mut findings: Vec<Phantom> = finder.found.into_values().collect();
        findings.sort_by_key(|p| u32::from(p.range.start()));
        for phantom in findings {
            diagnostics.push(Diagnostic::new(
                self.code(),
                phantom.message(),
                phantom.range,
                Severity::Warning,
            ));
        }
    }
}

/// A phantom guard occurrence: where it is, its name, whether it's a decorator, and a near-miss
/// bound symbol if one exists (suggesting a typo rather than wholly-absent scaffolding).
struct Phantom {
    range: TextRange,
    name: String,
    is_decorator: bool,
    near_miss: Option<String>,
}

impl Phantom {
    fn message(&self) -> String {
        let what = if self.is_decorator {
            format!("decorator `@{}`", self.name)
        } else {
            format!("call to security guard `{}`", self.name)
        };
        match &self.near_miss {
            Some(similar) => format!(
                "{what} is not defined or imported in this module — likely a typo of `{similar}` \
                 (an undefined security control is still broken; CWE-693)"
            ),
            None => format!(
                "{what} is not defined or imported in this module (phantom security control — the \
                 code looks defended but the guard does not exist; CWE-693)"
            ),
        }
    }
}

/// Collects names bound anywhere in the module across all scopes (a deliberate over-approximation:
/// SLP210 fires only when a guard name is bound *nowhere*, so over-collecting bindings is the
/// false-negative-biased, safe direction).
struct Bindings<'a> {
    names: std::collections::HashSet<&'a str>,
}

impl<'a> Bindings<'a> {
    fn visit_body(&mut self, body: &'a [Stmt]) {
        for stmt in body {
            self.visit_stmt(stmt);
        }
    }

    fn add_params(&mut self, params: &'a Parameters) {
        for param in params
            .posonlyargs
            .iter()
            .chain(&params.args)
            .chain(&params.kwonlyargs)
        {
            self.names.insert(param.parameter.name.as_str());
        }
        for variadic in [&params.vararg, &params.kwarg].into_iter().flatten() {
            self.names.insert(variadic.name.as_str());
        }
    }
}

impl<'a> Visitor<'a> for Bindings<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(func) => {
                self.names.insert(func.name.as_str());
                self.add_params(&func.parameters);
            }
            Stmt::ClassDef(class) => {
                self.names.insert(class.name.as_str());
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    // `import a.b.c` binds `a`; `import a.b.c as x` binds `x`.
                    let bound = alias.asname.as_ref().map_or_else(
                        || alias.name.as_str().split('.').next().unwrap_or(""),
                        |asname| asname.as_str(),
                    );
                    self.names.insert(bound);
                }
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    let bound = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    self.names.insert(bound);
                }
            }
            Stmt::Global(global) => {
                for name in &global.names {
                    self.names.insert(name.as_str());
                }
            }
            Stmt::Nonlocal(nonlocal) => {
                for name in &nonlocal.names {
                    self.names.insert(name.as_str());
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(name) if matches!(name.ctx, ExprContext::Store) => {
                self.names.insert(name.id.as_str());
            }
            Expr::Lambda(lambda) => {
                if let Some(params) = &lambda.parameters {
                    self.add_params(params);
                }
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }

    fn visit_except_handler(&mut self, handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(except) = handler;
        if let Some(name) = &except.name {
            self.names.insert(name.as_str());
        }
        visitor::walk_except_handler(self, handler);
    }
}

/// Walks the module looking for bare-name guard calls and decorators not in `bound`.
struct Finder<'a> {
    bound: &'a std::collections::HashSet<&'a str>,
    extra: &'a [String],
    found: HashMap<u32, Phantom>,
}

impl<'a> Finder<'a> {
    fn visit_body(&mut self, body: &'a [Stmt]) {
        for stmt in body {
            self.visit_stmt(stmt);
        }
    }

    /// Record a phantom guard at `range` if `name` is a guard and isn't bound. Deduplicated by
    /// start offset (a decorator-call is seen by both the decorator and call passes).
    fn record(&mut self, name: &str, range: TextRange, is_decorator: bool) {
        if self.bound.contains(name) || !self.is_guard(name) {
            return;
        }
        let key = u32::from(range.start());
        let near_miss = self.near_miss(name);
        // Prefer the decorator framing when either pass marks the site as a decorator.
        self.found
            .entry(key)
            .and_modify(|existing| existing.is_decorator |= is_decorator)
            .or_insert(Phantom {
                range,
                name: name.to_string(),
                is_decorator,
                near_miss,
            });
    }

    fn is_guard(&self, name: &str) -> bool {
        GUARDS.binary_search(&name).is_ok() || self.extra.iter().any(|e| e == name)
    }

    /// A bound name within edit distance 1 of `name` (a likely typo/rename), if any. Only
    /// length-similar candidates are compared, both to bound the cost and because a one-edit typo
    /// can't change length by more than one. When several bound names qualify, the lexicographically
    /// smallest is chosen so the suggestion is **deterministic** — `bound` is a `HashSet` with
    /// run-varying iteration order, so picking the first match would make the message flap.
    fn near_miss(&self, name: &str) -> Option<String> {
        self.bound
            .iter()
            .filter(|cand| cand.len().abs_diff(name.len()) <= 1 && **cand != name)
            .filter(|cand| within_edit_distance_1(name, cand))
            .min()
            .map(|cand| (*cand).to_string())
    }
}

impl<'a> Visitor<'a> for Finder<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let decorators = match stmt {
            Stmt::FunctionDef(func) => Some(&func.decorator_list),
            Stmt::ClassDef(class) => Some(&class.decorator_list),
            _ => None,
        };
        if let Some(decorators) = decorators {
            for decorator in decorators {
                if let Some((name, range)) = decorator_name(decorator) {
                    self.record(name, range, true);
                }
            }
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if let Expr::Name(name) = call.func.as_ref() {
                self.record(name.id.as_str(), name.range(), false);
            }
        }
        visitor::walk_expr(self, expr);
    }
}

/// The bare guard name a decorator applies, with its range — `@requires_auth` (a name) or
/// `@rate_limit(...)` (a call of a name). A dotted decorator (`@app.login_required`) resolves via
/// its receiver and yields `None` (never flagged).
fn decorator_name(decorator: &Decorator) -> Option<(&str, TextRange)> {
    match &decorator.expression {
        Expr::Name(name) => Some((name.id.as_str(), name.range())),
        Expr::Call(call) => match call.func.as_ref() {
            Expr::Name(name) => Some((name.id.as_str(), name.range())),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `a` and `b` are within Levenshtein distance 1 (one insert, delete, or substitution).
/// Short-circuits on a length gap > 1.
fn within_edit_distance_1(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    if la == lb {
        // Substitution: at most one differing position.
        return a.iter().zip(&b).filter(|(x, y)| x != y).count() <= 1;
    }
    // One insert/delete: the longer must equal the shorter with one char skipped.
    let (short, long) = if la < lb { (&a, &b) } else { (&b, &a) };
    let (mut i, mut j, mut skipped) = (0usize, 0usize, false);
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
        } else if skipped {
            return false;
        } else {
            skipped = true;
            j += 1;
        }
    }
    true
}

/// Curated catalog of security-guard / sanitizer / authz names AI models routinely *call* without
/// implementing. **Kept sorted** for `binary_search`. Deliberately security-specific (not a general
/// undefined-name set) so a hit reads as a missing protection mechanism, not a typo. Extend per
/// project via `[security] extra = [...]`.
const GUARDS: &[&str] = &[
    "authenticate",
    "authenticate_request",
    "authenticate_user",
    "authorize",
    "authorize_request",
    "check_access",
    "check_api_key",
    "check_auth",
    "check_credentials",
    "check_csrf",
    "check_password",
    "check_permission",
    "check_permissions",
    "check_rate_limit",
    "check_signature",
    "check_token",
    "clean_input",
    "csrf_protect",
    "decode_jwt",
    "enforce_https",
    "ensure_authenticated",
    "ensure_authorized",
    "ensure_csrf",
    "ensure_permission",
    "escape_html",
    "escape_input",
    "escape_sql",
    "escape_string",
    "has_permission",
    "has_role",
    "hash_password",
    "html_escape",
    "is_authenticated",
    "is_authorized",
    "is_safe_url",
    "login_required",
    "permission_required",
    "prevent_sql_injection",
    "rate_limit",
    "rate_limited",
    "require_api_key",
    "require_auth",
    "require_https",
    "require_login",
    "require_permission",
    "require_role",
    "requires_auth",
    "requires_login",
    "requires_permission",
    "requires_role",
    "sanitize",
    "sanitize_html",
    "sanitize_input",
    "sanitize_user_input",
    "secure_filename",
    "strip_tags",
    "throttle",
    "validate_api_key",
    "validate_csrf",
    "validate_email",
    "validate_input",
    "validate_jwt",
    "validate_password",
    "validate_request",
    "validate_signature",
    "validate_token",
    "validate_user",
    "verify_api_key",
    "verify_credentials",
    "verify_csrf",
    "verify_jwt",
    "verify_password",
    "verify_signature",
    "verify_token",
    "verify_user",
    "xss_clean",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_are_sorted_for_binary_search() {
        let mut sorted = GUARDS.to_vec();
        sorted.sort_unstable();
        assert_eq!(GUARDS, sorted.as_slice(), "GUARDS must stay sorted");
    }

    #[test]
    fn edit_distance_1() {
        assert!(within_edit_distance_1("validate_token", "validate_tokens")); // insert
        assert!(within_edit_distance_1("validate_token", "validate_toke")); // delete
        assert!(within_edit_distance_1("sanitize", "sanitise")); // substitution
        assert!(within_edit_distance_1("abc", "abc")); // identical
        assert!(!within_edit_distance_1("validate_token", "verify_jwt"));
        assert!(!within_edit_distance_1("abc", "axyz")); // length gap > 1
    }

    #[test]
    fn near_miss_is_deterministic_with_multiple_candidates() {
        // Two bound names are each one edit from `validate_token`; the lexicographically smallest
        // is chosen so the suggestion never flaps with HashSet iteration order.
        let bound: std::collections::HashSet<&str> =
            ["validate_tokens", "validate_toke"].into_iter().collect();
        let finder = Finder {
            bound: &bound,
            extra: &[],
            found: HashMap::new(),
        };
        assert_eq!(
            finder.near_miss("validate_token").as_deref(),
            Some("validate_toke")
        );
    }
}

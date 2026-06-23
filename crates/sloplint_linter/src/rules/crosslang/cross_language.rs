//! SLP250: cross-language pollution.

use std::collections::HashMap;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ExprContext, Stmt};
use sloplint_python::{Ranged, TextRange};

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags wrong-language idioms leaking into Python: camelCase methods (`toString`, `charAt`,
/// `forEach`), foreign attributes (`.length`, `.prototype`), and foreign bare builtins
/// (`array_push`, `println`). `console.log` is deliberately *not* flagged (it collides with
/// `rich.Console.log`).
///
/// ## Why is this bad?
/// A model fluent in JS/Java/PHP/C# emits these in Python; it is simply wrong code that often
/// runs on a duck-typed object or fails only at runtime, slipping past review. Deliberately
/// narrow — only names that are never idiomatic Python — to avoid false positives; allowlist
/// via `[crosslang] allow`. Preview (the FP-riskiest rule).
#[derive(ViolationMetadata)]
pub struct CrossLanguage;

impl Rule for CrossLanguage {
    fn code(&self) -> &'static str {
        "SLP250"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let module = ctx.parsed.syntax();
        // Qt's Python bindings (PyQt/PySide) are camelCase by design — `toString`/`indexOf`/… are
        // their real API, not foreign idioms — so skip a file that imports them entirely.
        if imports_qt(&module.body) {
            return;
        }
        let mut finder = Finder {
            allow: ctx.crosslang_allow,
            found: HashMap::new(),
        };
        for stmt in &module.body {
            finder.visit_stmt(stmt);
        }
        let mut findings: Vec<(TextRange, String)> = finder.found.into_values().collect();
        findings.sort_by_key(|(range, _)| u32::from(range.start()));
        for (range, message) in findings {
            diagnostics.push(Diagnostic::new(
                self.code(),
                message,
                range,
                Severity::Warning,
            ));
        }
    }
}

struct Finder<'a> {
    allow: &'a [String],
    found: HashMap<u32, (TextRange, String)>,
}

impl<'a> Finder<'a> {
    fn allowed(&self, name: &str) -> bool {
        ALLOW.contains(&name) || self.allow.iter().any(|a| a == name)
    }

    fn record(&mut self, range: TextRange, name: &str, lang: &str, suggest: &str) {
        if self.allowed(name) {
            return;
        }
        let message = if suggest.is_empty() {
            format!("cross-language idiom `{name}` ({lang}) — not idiomatic Python")
        } else {
            format!("cross-language idiom `{name}` ({lang}) — use {suggest} in Python")
        };
        self.found
            .entry(u32::from(range.start()))
            .or_insert((range, message));
    }
}

impl<'a> Visitor<'a> for Finder<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Call(call) => match call.func.as_ref() {
                // `x.toString()`. (No `console.log` heuristic — `console` is the canonical name for
                // a `rich.console.Console`, whose `.log()`/`.print()` are real methods, and a stdlib
                // `Logger` is often named `console` too — too collision-prone to flag.)
                Expr::Attribute(attr) => {
                    let method = attr.attr.as_str();
                    if let Some((lang, suggest)) = foreign_method(method) {
                        self.record(attr.attr.range(), method, lang, suggest);
                    }
                }
                // Bare `array_push(...)`.
                Expr::Name(name) => {
                    if let Some((lang, suggest)) = foreign_func(name.id.as_str()) {
                        self.record(name.range(), name.id.as_str(), lang, suggest);
                    }
                }
                _ => {}
            },
            // Foreign attribute access `s.length` / `obj.prototype` (not a call).
            Expr::Attribute(attr) if matches!(attr.ctx, ExprContext::Load) => {
                if let Some((lang, suggest)) = foreign_attr(attr.attr.as_str()) {
                    self.record(attr.attr.range(), attr.attr.as_str(), lang, suggest);
                }
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

/// Whether the module imports a Qt binding (PyQt/PySide/qtpy), whose camelCase API would otherwise
/// false-positive. Checks top-level `import`/`from` statements only — Qt is imported at module top.
fn imports_qt(body: &[Stmt]) -> bool {
    const QT: &[&str] = &[
        "PyQt4", "PyQt5", "PyQt6", "PySide", "PySide2", "PySide6", "qtpy",
    ];
    let top = |dotted: &str| dotted.split('.').next().unwrap_or("").to_string();
    body.iter().any(|stmt| match stmt {
        Stmt::Import(import) => import
            .names
            .iter()
            .any(|a| QT.contains(&top(a.name.as_str()).as_str())),
        Stmt::ImportFrom(import) => import
            .module
            .as_ref()
            .is_some_and(|m| QT.contains(&top(m.as_str()).as_str())),
        _ => false,
    })
}

/// camelCase foreign methods → `(language, suggestion)`. camelCase makes these un-Pythonic
/// regardless of receiver, so they're safe to flag by name alone. FP-prone lower/snake names
/// (`push`/`size`/`contains`/`sub`/`echo`/…) are deliberately absent.
fn foreign_method(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "toString" => ("JavaScript/Java", "`str(x)`"),
        "valueOf" => ("JavaScript/Java", "the value directly"),
        "hashCode" => ("Java", "`hash(x)`"),
        "getClass" => ("Java", "`type(x)`"),
        "charAt" => ("JavaScript/Java", "indexing `s[i]`"),
        "charCodeAt" => ("JavaScript", "`ord(s[i])`"),
        "codePointAt" => ("JavaScript", "`ord(s[i])`"),
        "substring" => ("JavaScript/Java", "slicing `s[a:b]`"),
        "substr" => ("JavaScript", "slicing `s[a:b]`"),
        "toUpperCase" => ("JavaScript/Java", "`.upper()`"),
        "toLowerCase" => ("JavaScript/Java", "`.lower()`"),
        "toLocaleUpperCase" => ("JavaScript", "`.upper()`"),
        "toLocaleLowerCase" => ("JavaScript", "`.lower()`"),
        "indexOf" => ("JavaScript/Java", "`.index()` / `.find()`"),
        "lastIndexOf" => ("JavaScript/Java", "`.rindex()` / `.rfind()`"),
        "forEach" => ("JavaScript", "a `for` loop"),
        "parseInt" => ("JavaScript", "`int(x)`"),
        "parseFloat" => ("JavaScript", "`float(x)`"),
        "toFixed" => ("JavaScript", "`f\"{x:.2f}\"`"),
        "splice" => ("JavaScript", "slice assignment / `del`"),
        "unshift" => ("JavaScript", "`list.insert(0, x)`"),
        "padStart" => ("JavaScript", "`.rjust()`"),
        "padEnd" => ("JavaScript", "`.ljust()`"),
        "trimStart" => ("JavaScript", "`.lstrip()`"),
        "trimEnd" => ("JavaScript", "`.rstrip()`"),
        "toLocaleString" => ("JavaScript", "explicit formatting"),
        "getElementById" => ("JavaScript (DOM)", "a real DOM/HTML library"),
        "querySelector" => ("JavaScript (DOM)", "a real DOM/HTML library"),
        "addEventListener" => ("JavaScript (DOM)", "a real event API"),
        _ => return None,
    })
}

/// Foreign attribute access → `(language, suggestion)`.
fn foreign_attr(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "length" => ("JavaScript/Java", "`len(x)`"),
        "prototype" => ("JavaScript", "a class"),
        "__proto__" => ("JavaScript", "`type(x)` / `x.__class__`"),
        _ => return None,
    })
}

/// Foreign bare builtins → `(language, suggestion)`. Names a Python program essentially never
/// defines itself.
fn foreign_func(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "array_push" => ("PHP", "`list.append()`"),
        "array_pop" => ("PHP", "`list.pop()`"),
        "array_merge" => ("PHP", "`list +` / `dict |`"),
        "array_keys" => ("PHP", "`dict.keys()`"),
        "array_values" => ("PHP", "`dict.values()`"),
        "array_map" => ("PHP", "`map()` / a comprehension"),
        "array_filter" => ("PHP", "`filter()` / a comprehension"),
        "in_array" => ("PHP", "the `in` operator"),
        "is_array" => ("PHP", "`isinstance(x, list)`"),
        "var_dump" => ("PHP", "`print()` / `repr()`"),
        "print_r" => ("PHP", "`print()` / `pprint`"),
        "var_export" => ("PHP", "`repr()`"),
        "json_encode" => ("PHP", "`json.dumps()`"),
        "json_decode" => ("PHP", "`json.loads()`"),
        "str_replace" => ("PHP", "`str.replace()`"),
        "strlen" => ("PHP/C", "`len(x)`"),
        "strpos" => ("PHP", "`str.find()`"),
        "sprintf" => ("C/PHP", "an f-string"),
        "println" => ("Java/Kotlin/Go", "`print()`"),
        "printf" => ("C", "`print()` / an f-string"),
        "puts" => ("Ruby", "`print()`"),
        _ => return None,
    })
}

/// FP-prone names that are legitimate Python and must never be flagged, even via `[crosslang]
/// allow` extras or a future blocklist addition. The reusable allow-list from the prior art.
const ALLOW: &[&str] = &[
    "push",
    "pop",
    "size",
    "count",
    "contains",
    "sub",
    "echo",
    "map",
    "filter",
    "reduce",
    "find",
    "split",
    "join",
    "keys",
    "values",
    "items",
    "get",
    "set",
    "add",
    "remove",
    "append",
    "update",
    "sort",
    "index",
    "format",
    "strip",
    "replace",
    "match",
    "search",
    "log",
    "name",
    "type",
    "id",
    "length_of",
    "slice",
    "insert",
    "extend",
    "clear",
    "copy",
];

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn findings(src: &str) -> Vec<String> {
        let parsed = parse(src).unwrap();
        let mut diags = Vec::new();
        let ctx = FileContext {
            path: "m.py",
            source: src,
            parsed: &parsed,
            limits: Default::default(),
            security_extra: &[],
            placeholders_extra: &[],
            comment_phrases_extra: &[],
            crosslang_allow: &[],
        };
        CrossLanguage.check(&ctx, &mut diags);
        diags.into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn flags_unambiguous_foreign_idioms() {
        assert!(findings("x = obj.toString()")
            .iter()
            .any(|m| m.contains("toString")));
        assert!(findings("c = s.charAt(0)")
            .iter()
            .any(|m| m.contains("charAt")));
        assert!(findings("n = arr.length")
            .iter()
            .any(|m| m.contains("length")));
        assert!(findings("array_push(arr, 1)")
            .iter()
            .any(|m| m.contains("array_push")));
        assert!(findings("for_each = items.forEach(f)")
            .iter()
            .any(|m| m.contains("forEach")));
    }

    #[test]
    fn fp_prone_names_are_not_flagged() {
        assert!(findings("y = re.sub(p, r, s)").is_empty());
        assert!(findings("click.echo('hi')").is_empty());
        assert!(findings("stack.push(item)").is_empty());
        assert!(findings("m = df.contains('x')").is_empty());
        assert!(findings("n = queue.size()").is_empty());
        assert!(findings("v = d.get('k')").is_empty());
        assert!(findings("xs.append(1)").is_empty());
        // `console.log` collides with rich.Console.log() — never flagged.
        assert!(findings("console.log('a real rich log')").is_empty());
    }

    #[test]
    fn qt_imports_suppress_the_rule() {
        // PyQt/PySide are camelCase by design — their API isn't foreign there.
        assert!(findings("from PyQt5.QtCore import QObject\n\ns = v.toString()\n").is_empty());
        assert!(findings("import PySide6\n\ni = combo.indexOf(x)\n").is_empty());
        // …but a non-Qt module with the same call is still flagged.
        assert!(!findings("s = v.toString()").is_empty());
    }

    #[test]
    fn allow_list_extra_suppresses() {
        let parsed = parse("x = obj.toString()").unwrap();
        let mut diags = Vec::new();
        let allow = vec!["toString".to_string()];
        let ctx = FileContext {
            path: "m.py",
            source: "x = obj.toString()",
            parsed: &parsed,
            limits: Default::default(),
            security_extra: &[],
            placeholders_extra: &[],
            comment_phrases_extra: &[],
            crosslang_allow: &allow,
        };
        CrossLanguage.check(&ctx, &mut diags);
        assert!(diags.is_empty(), "allow extra suppresses toString");
    }
}

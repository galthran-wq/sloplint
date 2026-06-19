//! SLP130: docstring drift — `Raises:`/`Returns:` sections that no longer match the body.
//!
//! A fully-sectioned docstring that has drifted from the code actively misleads: a documented
//! exception the body never `raise`s, or a `Returns:` description for a function that returns
//! nothing, sends maintainers down wrong paths and survives review because the prose *looks*
//! authoritative. This rule cross-checks the two sections that existing tools handle poorly —
//! it deliberately does **not** touch `Args:`↔signature matching (darglint/pydoclint's job).
//!
//! Parses Google (`Returns:` / `Raises:` blocks), NumPy (`Returns`/`Raises` underlined), and
//! Sphinx (`:returns:` / `:raises X:`) styles. High-precision by construction: the `Raises`
//! check is suppressed whenever the body has a dynamic raise (`raise` re-raise, `raise var`)
//! the analysis can't resolve, and both checks skip stubs, `@abstractmethod`, and `@overload`.
//! Preview.
//!
//! Known blind spots (acceptable for a no-LLM heuristic): a documented exception raised only as
//! a *subclass* (`Raises: ValueError` while the body raises a custom `ValueError` subclass) is
//! matched by name, so it can read as drift; a `Raises:`/`Returns:` header appearing inside an
//! embedded doctest/code block in the docstring is parsed as a real section; and a documented
//! `Raises: Warning` is suppressed, since `Warning` doubles as a docstring admonition word
//! (raising the `Warning` builtin is rare — `warnings.warn` is idiomatic).

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Stmt, StmtFunctionDef, StmtRaise, StmtReturn};
use sloplint_python::Ranged;

use crate::ast_util::collect_functions;
use crate::lint::{FileContext, Rule};

pub struct DocstringDrift;

impl Rule for DocstringDrift {
    fn code(&self) -> &'static str {
        "SLP130"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut functions = Vec::new();
        collect_functions(&ctx.parsed.syntax().body, &mut functions);

        for function in functions {
            let Some(doc) = docstring(function) else {
                continue;
            };
            let sections = DocSections::parse(doc);
            if !sections.documents_return && sections.raises.is_empty() {
                continue; // nothing structured to drift.
            }
            // Stubs / abstract / overload declare a contract; their docstring leads the body.
            if is_skipped(function) {
                continue;
            }

            let body = BodyFacts::collect(function);

            if sections.documents_return && !body.returns_value && !body.is_generator {
                diagnostics.push(Diagnostic::new(
                    "SLP130",
                    format!(
                        "function `{}` documents a `Returns:` section but never returns a \
                         value — update or drop the docstring section",
                        function.name
                    ),
                    function.name.range(),
                    Severity::Warning,
                ));
            }

            // Only when every raise is statically resolvable can we be sure a documented
            // exception is never raised.
            if !body.dynamic_raise {
                for documented in &sections.raises {
                    if !body.raised.iter().any(|r| r == documented) {
                        diagnostics.push(Diagnostic::new(
                            "SLP130",
                            format!(
                                "function `{}` documents `Raises: {documented}` but never \
                                 raises it — update or drop the docstring section",
                                function.name
                            ),
                            function.name.range(),
                            Severity::Warning,
                        ));
                    }
                }
            }
        }
    }
}

/// The function's docstring text (logical string value), if its first statement is one.
fn docstring(function: &StmtFunctionDef) -> Option<&str> {
    let Stmt::Expr(expr) = function.body.first()? else {
        return None;
    };
    match expr.value.as_ref() {
        Expr::StringLiteral(string) => Some(string.value.to_str()),
        _ => None,
    }
}

/// Whether to skip a function entirely — its body legitimately doesn't match its docstring.
fn is_skipped(function: &StmtFunctionDef) -> bool {
    let abstract_or_overload = function.decorator_list.iter().any(|decorator| {
        matches!(
            trailing_name(&decorator.expression),
            Some("abstractmethod" | "overload" | "abstractproperty")
        )
    });
    abstract_or_overload || is_stub(function)
}

/// A stub body: only a docstring, `...`, `pass`, or `raise NotImplementedError`.
fn is_stub(function: &StmtFunctionDef) -> bool {
    function
        .body
        .iter()
        .enumerate()
        .all(|(i, stmt)| match stmt {
            Stmt::Expr(expr) => {
                i == 0 && matches!(expr.value.as_ref(), Expr::StringLiteral(_))
                    || matches!(expr.value.as_ref(), Expr::EllipsisLiteral(_))
            }
            Stmt::Pass(_) => true,
            Stmt::Raise(raise) => raise
                .exc
                .as_ref()
                .and_then(|exc| exception_type(exc))
                .is_some_and(|name| name == "NotImplementedError"),
            _ => false,
        })
}

fn trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Call(call) => trailing_name(&call.func),
        _ => None,
    }
}

/// The exception type name of a `raise` target, if statically resolvable and PascalCase (so a
/// `raise some_var` / `raise self.err` instance re-raise is treated as unresolvable, not a
/// type). `raise ValueError(...)` -> `ValueError`; `raise errors.NotFound` -> `NotFound`.
fn exception_type(exc: &Expr) -> Option<&str> {
    let target = match exc {
        Expr::Call(call) => call.func.as_ref(),
        other => other,
    };
    let name = match target {
        Expr::Name(name) => name.id.as_str(),
        Expr::Attribute(attribute) => attribute.attr.as_str(),
        _ => return None,
    };
    name.chars()
        .next()
        .is_some_and(|c| c.is_uppercase())
        .then_some(name)
}

/// Facts gathered from a function's own body (nested defs/classes excluded).
#[derive(Default)]
struct BodyFacts {
    /// Resolvable exception type names raised.
    raised: Vec<String>,
    /// A `raise` whose type couldn't be resolved (bare re-raise, `raise variable`) — makes the
    /// "documented-but-never-raised" check unsound, so it's suppressed.
    dynamic_raise: bool,
    /// Any `return <value>` (a value other than `None`).
    returns_value: bool,
    /// Contains `yield`/`yield from` — a generator (documents `Yields:`, not `Returns:`).
    is_generator: bool,
}

impl BodyFacts {
    fn collect(function: &StmtFunctionDef) -> Self {
        let mut facts = BodyFacts::default();
        for stmt in &function.body {
            facts.visit_stmt(stmt);
        }
        facts
    }
}

impl<'a> Visitor<'a> for BodyFacts {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            // Nested scopes have their own raises/returns.
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::Raise(raise) => self.record_raise(raise),
            Stmt::Return(ret) => {
                if returns_a_value(ret) {
                    self.returns_value = true;
                }
                visitor::walk_stmt(self, stmt);
            }
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if matches!(expr, Expr::Yield(_) | Expr::YieldFrom(_)) {
            self.is_generator = true;
        }
        visitor::walk_expr(self, expr);
    }
}

impl BodyFacts {
    fn record_raise(&mut self, raise: &StmtRaise) {
        match &raise.exc {
            None => self.dynamic_raise = true, // bare `raise` re-raise.
            Some(exc) => match exception_type(exc) {
                Some(name) => self.raised.push(name.to_string()),
                None => self.dynamic_raise = true,
            },
        }
    }
}

/// A `return` carrying a real value (not bare `return` and not `return None`).
fn returns_a_value(ret: &StmtReturn) -> bool {
    match ret.value.as_deref() {
        None | Some(Expr::NoneLiteral(_)) => false,
        Some(_) => true,
    }
}

/// The `Returns:`/`Raises:` content extracted from a docstring across Google/NumPy/Sphinx.
struct DocSections {
    documents_return: bool,
    raises: Vec<String>,
}

impl DocSections {
    fn parse(doc: &str) -> Self {
        let lines: Vec<&str> = doc.lines().collect();
        let mut raises = documented_raises(&lines);
        raises.sort();
        raises.dedup();
        DocSections {
            documents_return: returns_documented(&lines),
            raises,
        }
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_underline(s: &str) -> bool {
    s.len() >= 3 && (s.bytes().all(|b| b == b'-') || s.bytes().all(|b| b == b'='))
}

/// True if a `Returns`/`Return` section is present in any supported style.
fn returns_documented(lines: &[&str]) -> bool {
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        // Sphinx field.
        if lower.starts_with(":returns:") || lower.starts_with(":return:") {
            return true;
        }
        // Google header.
        if matches!(lower.as_str(), "returns:" | "return:") {
            return true;
        }
        // NumPy header + underline.
        if matches!(lower.as_str(), "returns" | "return")
            && lines.get(i + 1).is_some_and(|n| is_underline(n.trim()))
        {
            return true;
        }
    }
    false
}

/// Exception type names documented in any `Raises` section.
fn documented_raises(lines: &[&str]) -> Vec<String> {
    let mut types = Vec::new();

    // Sphinx `:raises X:` / `:raise X:` / `:except X:` fields.
    for line in lines {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        for keyword in [":raises", ":raise", ":exception", ":except"] {
            if let Some(rest) = lower.strip_prefix(keyword) {
                // Require a word boundary so `:raisesfoo:` isn't mistaken for `:raises`.
                if rest.starts_with(' ') || rest.starts_with(':') {
                    if let Some(end) = trimmed[keyword.len()..].find(':') {
                        push_types(&trimmed[keyword.len()..keyword.len() + end], &mut types);
                    }
                }
                break;
            }
        }
    }

    // Google / NumPy blocks.
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let lower = trimmed.to_ascii_lowercase();
        let header_indent = indent(lines[i]);
        if matches!(lower.as_str(), "raises:" | "raise:") {
            i += 1;
            collect_block_types(lines, &mut i, header_indent, false, &mut types);
        } else if matches!(lower.as_str(), "raises" | "raise")
            && lines.get(i + 1).is_some_and(|n| is_underline(n.trim()))
        {
            i += 2;
            collect_block_types(lines, &mut i, header_indent, true, &mut types);
        } else {
            i += 1;
        }
    }

    types
}

/// Collect exception type names from an indented Google block (`numpy = false`, entries are
/// indented deeper than the header) or a NumPy block (`numpy = true`, type lines sit at the
/// header's own indent). Advances `i` past the block.
fn collect_block_types(
    lines: &[&str],
    i: &mut usize,
    header_indent: usize,
    numpy: bool,
    types: &mut Vec<String>,
) {
    let mut entry_indent: Option<usize> = None;
    while *i < lines.len() {
        let line = lines[*i];
        if line.trim().is_empty() {
            *i += 1;
            continue;
        }
        let ind = indent(line);
        // End of section: a line dedented to/under the header (Google) or under it (NumPy), or
        // the start of the next NumPy section.
        let dedented = if numpy {
            ind < header_indent
        } else {
            ind <= header_indent
        };
        if dedented {
            break;
        }
        if numpy && lines.get(*i + 1).is_some_and(|n| is_underline(n.trim())) {
            break; // next NumPy section header.
        }
        let level = *entry_indent.get_or_insert(ind);
        if ind == level {
            // An entry line: the type(s) appear before the first ':' (Google `Type: desc`) or
            // are the whole line (NumPy `Type`).
            let trimmed = line.trim();
            let spec = trimmed.split(':').next().unwrap_or(trimmed);
            push_types(spec, types);
        }
        *i += 1;
    }
}

/// Common docstring section / admonition words that can sit at the entry indent of a `Raises`
/// block (e.g. a `Note:` line) and must NOT be mistaken for an exception type.
const NON_EXCEPTION_WORDS: &[&str] = &[
    "note",
    "notes",
    "warning",
    "warnings",
    "warns",
    "todo",
    "example",
    "examples",
    "see",
    "yields",
    "yield",
    "returns",
    "return",
    "args",
    "arguments",
    "parameters",
    "param",
    "params",
    "attributes",
    "references",
    "reference",
    "keyword",
    "keywords",
    "raises",
    "raise",
];

/// Push the PascalCase exception type names from a comma-separated `spec` (taking each name's
/// trailing dotted component) onto `types`. Skips admonition/section words so a `Note:` line in
/// a `Raises:` block isn't captured as a phantom exception.
fn push_types(spec: &str, types: &mut Vec<String>) {
    for segment in spec.split(',') {
        let name = segment.trim().rsplit('.').next().unwrap_or("").trim();
        if !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && name.chars().next().is_some_and(|c| c.is_uppercase())
            && !NON_EXCEPTION_WORDS.contains(&name.to_ascii_lowercase().as_str())
        {
            types.push(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn findings(source: &str) -> Vec<String> {
        let parsed = parse(source).expect("valid python");
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            limits: Default::default(),
        };
        let mut diagnostics = Vec::new();
        DocstringDrift.check(&ctx, &mut diagnostics);
        diagnostics.into_iter().map(|d| d.message).collect()
    }

    // --- docstring parser ---

    #[test]
    fn parses_google_sections() {
        let doc = "Summary.\n\n    Returns:\n        The total.\n\n    Raises:\n        ValueError: if bad.\n        KeyError: if missing.\n    ";
        let s = DocSections::parse(doc);
        assert!(s.documents_return);
        assert_eq!(s.raises, vec!["KeyError", "ValueError"]);
    }

    #[test]
    fn parses_numpy_sections() {
        let doc = "Summary.\n\n    Returns\n    -------\n    int\n        The total.\n\n    Raises\n    ------\n    ValueError\n        If bad.\n    ";
        let s = DocSections::parse(doc);
        assert!(s.documents_return);
        assert_eq!(s.raises, vec!["ValueError"]);
    }

    #[test]
    fn parses_sphinx_sections() {
        let doc =
            ":returns: the total\n:raises ValueError: if bad\n:raises lib.KeyError: if missing\n";
        let s = DocSections::parse(doc);
        assert!(s.documents_return);
        assert_eq!(s.raises, vec!["KeyError", "ValueError"]);
    }

    #[test]
    fn no_structured_sections() {
        let s = DocSections::parse("Just a one-line summary.");
        assert!(!s.documents_return);
        assert!(s.raises.is_empty());
    }

    #[test]
    fn admonition_lines_are_not_captured_as_exceptions() {
        // Regression: a `Note:` line inside a Raises block must not become a phantom exception.
        let doc = "Summary.\n\n    Raises:\n        ValueError: if bad.\n\n        Note: this is a note, not an exception.\n    ";
        let s = DocSections::parse(doc);
        assert_eq!(s.raises, vec!["ValueError"]);
    }

    #[test]
    fn sphinx_word_boundary_is_required() {
        // `:raisesfoo:` is not a `:raises` field.
        let s = DocSections::parse(":raisesfoo: nonsense\n");
        assert!(s.raises.is_empty());
    }

    // --- raises drift ---

    #[test]
    fn flags_documented_but_unraised_exception() {
        let src = "\
def f(x):
    \"\"\"Do it.

    Raises:
        ValueError: if bad.
    \"\"\"
    return x
";
        let f = findings(src);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("Raises: ValueError") && f[0].contains("never raises"));
    }

    #[test]
    fn correct_raises_is_not_flagged() {
        let src = "\
def f(x):
    \"\"\"Do it.

    Raises:
        ValueError: if bad.
    \"\"\"
    if x < 0:
        raise ValueError(\"bad\")
    return x
";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn dynamic_raise_suppresses_the_check() {
        // A bare re-raise means we can't prove ValueError is never raised.
        let src = "\
def f(x):
    \"\"\"Do it.

    Raises:
        ValueError: if bad.
    \"\"\"
    try:
        return x
    except Exception:
        raise
";
        assert!(findings(src).is_empty());
    }

    // --- returns drift ---

    #[test]
    fn flags_returns_on_valueless_function() {
        let src = "\
def f(x) -> None:
    \"\"\"Do it.

    Returns:
        The total.
    \"\"\"
    print(x)
";
        let f = findings(src);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("Returns:") && f[0].contains("never returns a value"));
    }

    #[test]
    fn correct_returns_is_not_flagged() {
        let src = "\
def f(x):
    \"\"\"Do it.

    Returns:
        The doubled value.
    \"\"\"
    return x * 2
";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn generator_documenting_returns_is_not_flagged() {
        let src = "\
def f(xs):
    \"\"\"Stream them.

    Returns:
        Each item.
    \"\"\"
    for x in xs:
        yield x
";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn return_none_literal_counts_as_valueless() {
        let src = "\
def f(x):
    \"\"\"Do it.

    Returns:
        Nothing really.
    \"\"\"
    return None
";
        assert_eq!(findings(src).len(), 1);
    }

    // --- skips ---

    #[test]
    fn abstract_and_stub_methods_are_skipped() {
        let abstract_method = "\
import abc

class A(abc.ABC):
    @abc.abstractmethod
    def f(self):
        \"\"\"Contract.

        Returns:
            A value.

        Raises:
            ValueError: maybe.
        \"\"\"
        ...
";
        assert!(findings(abstract_method).is_empty());

        let stub = "\
def f(x):
    \"\"\"Contract.

    Raises:
        ValueError: maybe.
    \"\"\"
    raise NotImplementedError
";
        assert!(findings(stub).is_empty());
    }

    #[test]
    fn nested_function_raises_do_not_count_for_parent() {
        // The parent documents ValueError but only the nested function raises it.
        let src = "\
def outer(x):
    \"\"\"Outer.

    Raises:
        ValueError: if bad.
    \"\"\"
    def inner():
        raise ValueError(\"bad\")
    return inner
";
        let f = findings(src);
        assert_eq!(f.len(), 1, "parent should be flagged: {f:?}");
        assert!(f[0].contains("outer"));
    }
}

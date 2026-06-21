//! SLP230: mock / placeholder data left in production code.
//!
//! Agents routinely seed plausible-looking placeholder data to make code "run", then never replace
//! it: `user@example.com` emails, `123-456-7890` phones, all-zero UUIDs, `password123`/`changeme`
//! credentials, and dummy returns like `return {"foo": "bar"}` / `return "placeholder"`. It
//! compiles, the test asserts the placeholder, and it ships — structurally-shallow-but-green slop.
//!
//! These are cheap, deterministic, high-precision literal checks with essentially no Ruff overlap
//! (bandit's `S105`/`S106` flag *any* hardcoded password, not the placeholder *class*). The rule is
//! restricted to **non-test** paths — a fixture's `test@example.com` is expected, not slop.

use std::collections::HashMap;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Stmt};
use sloplint_python::{Ranged, TextRange};

use crate::lint::{FileContext, Rule};

pub struct MockData;

impl Rule for MockData {
    fn code(&self) -> &'static str {
        "SLP230"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        // Placeholder data is *expected* in tests; only production code is slop.
        if is_test_path(ctx.path) {
            return;
        }
        // Docstrings legitimately contain example emails ("send to user@example.com"); collect
        // their offsets so the literal scan skips them.
        let mut docstrings = std::collections::HashSet::new();
        collect_docstrings(&ctx.parsed.syntax().body, &mut docstrings);
        let mut finder = Finder {
            extra: ctx.placeholders_extra,
            docstrings,
            found: HashMap::new(),
        };
        for stmt in &ctx.parsed.syntax().body {
            finder.visit_stmt(stmt);
        }
        let mut findings: Vec<(TextRange, String)> = finder.found.into_values().collect();
        findings.sort_by_key(|(range, _)| u32::from(range.start()));
        for (range, message) in findings {
            diagnostics.push(Diagnostic::new("SLP230", message, range, Severity::Warning));
        }
    }
}

/// Whether `path` is test code (where placeholder data is expected). Mirrors the test/production
/// path split used elsewhere: a `test_*.py` / `*_test.py` / `conftest.py` file, or any path under a
/// plural `tests` directory segment. Windows separators are normalized. Only the **plural** `tests`
/// segment counts (not singular `test`), so the rule's own `resources/test/…` fixture still fires —
/// the same lesson the path-exemption rules already learned.
fn is_test_path(path: &str) -> bool {
    let norm = path.replace('\\', "/");
    let file = norm.rsplit('/').next().unwrap_or(&norm);
    if file == "conftest.py"
        || (file.starts_with("test_") && file.ends_with(".py"))
        || file.ends_with("_test.py")
    {
        return true;
    }
    norm.split('/').any(|seg| seg == "tests")
}

struct Finder<'a> {
    /// Extra placeholder literal values from `[placeholders] extra`.
    extra: &'a [String],
    /// Start offsets of docstring string literals, skipped by the email/UUID/phone scan (example
    /// emails in docs are legitimate, not slop).
    docstrings: std::collections::HashSet<u32>,
    /// Deduplicated findings keyed by start offset.
    found: HashMap<u32, (TextRange, String)>,
}

/// Record the start offset of each docstring (the first statement of a module/function/class body
/// when it's a bare string literal), recursing into nested defs/classes.
fn collect_docstrings(body: &[Stmt], out: &mut std::collections::HashSet<u32>) {
    if let Some(Stmt::Expr(expr)) = body.first() {
        if let Expr::StringLiteral(string) = expr.value.as_ref() {
            out.insert(u32::from(string.range().start()));
        }
    }
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(func) => collect_docstrings(&func.body, out),
            Stmt::ClassDef(class) => collect_docstrings(&class.body, out),
            _ => {}
        }
    }
}

impl<'a> Finder<'a> {
    fn record(&mut self, range: TextRange, message: String) {
        self.found
            .entry(u32::from(range.start()))
            .or_insert((range, message));
    }
}

impl<'a> Visitor<'a> for Finder<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            // Credential context: `password = "changeme"`, `api_key: str = "your_api_key"`.
            Stmt::Assign(assign) => {
                if let Expr::StringLiteral(value) = assign.value.as_ref() {
                    let literal = value.value.to_str();
                    for target in &assign.targets {
                        self.check_credential(target, literal, value.range());
                    }
                }
            }
            Stmt::AnnAssign(ann) => {
                if let Some(Expr::StringLiteral(value)) = ann.value.as_deref() {
                    self.check_credential(&ann.target, value.value.to_str(), value.range());
                }
            }
            // Dummy production return: `return {"foo": "bar"}`, `return "placeholder"`.
            Stmt::Return(ret) => {
                if let Some(value) = ret.value.as_deref() {
                    if let Some(reason) = self.dummy_return(value) {
                        self.record(value.range(), reason);
                    }
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            // Placeholder emails / phones / UUIDs are unambiguous wherever they appear — except in
            // docstrings, where example emails are legitimate documentation.
            Expr::StringLiteral(string) => {
                if !self.docstrings.contains(&u32::from(string.range().start())) {
                    if let Some(message) = classify_literal(string.value.to_str()) {
                        self.record(string.range(), message);
                    }
                }
            }
            // Credential context via keyword: `connect(password="changeme")`.
            Expr::Call(call) => {
                for keyword in &call.arguments.keywords {
                    if let (Some(name), Expr::StringLiteral(value)) = (&keyword.arg, &keyword.value)
                    {
                        self.check_keyword_credential(
                            name.as_str(),
                            value.value.to_str(),
                            value.range(),
                        );
                    }
                }
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

impl<'a> Finder<'a> {
    fn check_credential(&mut self, target: &Expr, literal: &str, range: TextRange) {
        if let Expr::Name(name) = target {
            self.check_keyword_credential(name.id.as_str(), literal, range);
        }
    }

    fn check_keyword_credential(&mut self, name: &str, literal: &str, range: TextRange) {
        if is_credential_name(name) && self.is_weak_credential(literal) {
            self.record(
                range,
                format!(
                    "placeholder credential `{}` assigned to a credential-like name `{name}` — \
                     replace before production",
                    truncate(literal)
                ),
            );
        }
    }

    fn is_weak_credential(&self, literal: &str) -> bool {
        let lower = literal.trim().to_ascii_lowercase();
        WEAK_CREDENTIALS.contains(&lower.as_str())
            || self
                .extra
                .iter()
                .any(|e| e.eq_ignore_ascii_case(literal.trim()))
            || lower.starts_with("your_")
            || lower.starts_with("your-")
            || lower.starts_with("changeme")
            || lower.contains("placeholder")
            || is_single_char_run(&lower)
            || is_word_then_digits(&lower, "password")
    }

    /// A dummy production return value: a placeholder string, or a tiny dict of placeholder tokens.
    fn dummy_return(&self, value: &Expr) -> Option<String> {
        match value {
            Expr::StringLiteral(string) => {
                let literal = string.value.to_str().trim().to_ascii_lowercase();
                if PLACEHOLDER_RETURNS.contains(&literal.as_str())
                    || self
                        .extra
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(literal.as_str()))
                {
                    Some(format!(
                        "production function returns the placeholder value {} — likely unfinished",
                        truncate(string.value.to_str())
                    ))
                } else {
                    None
                }
            }
            Expr::Dict(dict) if !dict.items.is_empty() && dict.items.len() <= 3 => {
                let token = |expr: Option<&Expr>| match expr {
                    Some(Expr::StringLiteral(s)) => PLACEHOLDER_TOKENS
                        .contains(&s.value.to_str().trim().to_ascii_lowercase().as_str()),
                    _ => false,
                };
                // Require BOTH keys and values to be placeholder tokens so `{"foo": "bar"}` /
                // `{"key": "value"}` fire but a real `{"value": self.value}` (token key, real value)
                // does not.
                let all_keys = dict.items.iter().all(|it| token(it.key.as_ref()));
                let all_values = dict.items.iter().all(|it| token(Some(&it.value)));
                if all_keys && all_values {
                    Some(
                        "production function returns a dummy placeholder dict (e.g. {\"foo\": \
                         \"bar\"}) — likely unfinished"
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Classify a standalone string literal as a placeholder email / phone / UUID, if it is one.
fn classify_literal(literal: &str) -> Option<String> {
    let text = literal.trim();
    if let Some(domain) = placeholder_email_domain(text) {
        return Some(format!(
            "placeholder email address (domain `{domain}`) in production code"
        ));
    }
    if is_placeholder_uuid(text) {
        return Some("placeholder / low-entropy UUID in production code".to_string());
    }
    if is_placeholder_phone(text) {
        return Some("placeholder phone number in production code".to_string());
    }
    None
}

/// The domain of a placeholder email, if `text` is an email whose domain is a documentation
/// placeholder. Restricted to RFC 2606 reserved names (`example.com/.net/.org`, the `.test`/
/// `.example`/`.invalid`/`.localhost` TLDs) and unambiguous `your*`/`my*` placeholders — NOT
/// real, registerable domains like `company.com`/`foo.com`, which legitimate apps may use.
fn placeholder_email_domain(text: &str) -> Option<String> {
    let at = text.find('@')?;
    let domain_part = &text[at + 1..];
    // Stop at the first character that can't be in a bare domain (so `"a@example.com bob"` works).
    let domain: String = domain_part
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let reserved_tld = [".test", ".example", ".invalid", ".localhost"]
        .iter()
        .any(|tld| domain.ends_with(tld));
    if PLACEHOLDER_EMAIL_DOMAINS.contains(&domain.as_str()) || reserved_tld {
        Some(domain)
    } else {
        None
    }
}

/// Whether `text` is a UUID-shaped string with ≤2 distinct hex digits (all-zero nil UUID,
/// repeated, or otherwise obviously fake). Real UUIDs are high-entropy, so this rarely false-fires.
fn is_placeholder_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let mut distinct = [false; 16];
    for (i, b) in bytes.iter().enumerate() {
        let is_dash_pos = matches!(i, 8 | 13 | 18 | 23);
        if is_dash_pos {
            if *b != b'-' {
                return false;
            }
            continue;
        }
        match (*b as char).to_digit(16) {
            Some(d) => distinct[d as usize] = true,
            None => return false,
        }
    }
    distinct.iter().filter(|&&seen| seen).count() <= 2
}

/// Whether `text` is a placeholder phone number: a curated fake, or a phone-shaped string whose
/// digits collapse to ≤2 distinct values (`000-000-0000`, `111-111-1111`). Conservative to avoid
/// flagging real numbers.
fn is_placeholder_phone(text: &str) -> bool {
    const FAKES: &[&str] = &[
        "123-456-7890",
        "(123) 456-7890",
        "123.456.7890",
        "1234567890",
        "123-4567",
        "555-0100",
        "555-0123",
        "000-000-0000",
    ];
    if FAKES.iter().any(|f| text.contains(f)) {
        return true;
    }
    // Phone-shaped: only digits and common separators, and 7/10/11 digits that are all the same
    // digit (`000-000-0000`, `111-111-1111`). A single-digit number is unmistakably fake; broader
    // low-entropy (≤2 distinct) would catch real toll-free numbers like 800-800-8000.
    if !text
        .chars()
        .all(|c| c.is_ascii_digit() || " -.()+".contains(c))
    {
        return false;
    }
    let digits: Vec<char> = text.chars().filter(|c| c.is_ascii_digit()).collect();
    if !matches!(digits.len(), 7 | 10 | 11) {
        return false;
    }
    let distinct: std::collections::HashSet<char> = digits.iter().copied().collect();
    distinct.len() == 1
}

/// Whether an identifier names a credential. Matched as a **suffix** (the head noun), so
/// `password` / `db_password` / `api_key` / `auth_token` match, but `token_pattern` / `tokenizer` /
/// `password_hint_label` / `api_key_name` (where the credential word isn't the trailing noun) do
/// not — avoiding false positives on identifiers that merely contain a credential word.
fn is_credential_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    CREDENTIAL_NAMES.iter().any(|n| lower.ends_with(n))
}

/// A run of one repeated character of length ≥3 (`xxxx`, `0000`).
fn is_single_char_run(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => s.len() >= 3 && chars.all(|c| c == first),
        None => false,
    }
}

/// `word` followed only by digits (`password123`).
fn is_word_then_digits(s: &str, word: &str) -> bool {
    s.strip_prefix(word)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Truncate a literal for the message (placeholder values are short, but be safe).
fn truncate(s: &str) -> String {
    if s.chars().count() > 32 {
        format!("\"{}…\"", s.chars().take(32).collect::<String>())
    } else {
        format!("\"{s}\"")
    }
}

const PLACEHOLDER_EMAIL_DOMAINS: &[&str] = &[
    // RFC 2606 reserved + unambiguous `your*`/`my*` placeholders only — not real registerable
    // domains (no `company.com`/`foo.com`/`acme.com`/`test.com`, which legitimate apps may use).
    "domain.com",
    "example.com",
    "example.net",
    "example.org",
    "mycompany.com",
    "yourcompany.com",
    "yourdomain.com",
];

const CREDENTIAL_NAMES: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "credential",
    "auth_key",
];

const WEAK_CREDENTIALS: &[&str] = &[
    "123456",
    "12345678",
    "admin",
    "apikey",
    "api_key",
    "change_me",
    "changeme",
    "dummy",
    "example",
    "foo",
    "foobar",
    "letmein",
    "mysecret",
    "passwd",
    "password",
    "placeholder",
    "pwd",
    "qwerty",
    "root",
    "secret",
    "test",
    "test123",
    "test_api_key",
    "testing",
    "token",
    "topsecret",
    "your_api_key",
    "your_password",
    "your_secret",
];

const PLACEHOLDER_RETURNS: &[&str] = &[
    "bar",
    "baz",
    "changeme",
    "dummy",
    "example",
    "fixme",
    "foo",
    "foo bar",
    "placeholder",
    "tbd",
    "todo",
];

const PLACEHOLDER_TOKENS: &[&str] = &[
    "bar",
    "baz",
    "example",
    "foo",
    "key",
    "placeholder",
    "value",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_are_excluded() {
        assert!(is_test_path("tests/test_x.py"));
        assert!(is_test_path("pkg/test_auth.py"));
        assert!(is_test_path("pkg/auth_test.py"));
        assert!(is_test_path("conftest.py"));
        assert!(!is_test_path("src/app.py"));
        assert!(!is_test_path("src/latest.py")); // "test" only as a substring, not a segment
                                                 // Only the PLURAL `tests` segment excludes, so the rule's own resources/test/ fixture fires.
        assert!(!is_test_path(
            "crates/x/resources/test/fixtures/placeholders/SLP230.py"
        ));
    }

    #[test]
    fn email_domains() {
        assert_eq!(
            placeholder_email_domain("user@example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            placeholder_email_domain("Bob <a@example.org>"),
            Some("example.org".to_string())
        );
        // RFC 2606 reserved TLD.
        assert_eq!(
            placeholder_email_domain("x@foo.test"),
            Some("foo.test".to_string())
        );
        // Real, registerable domains are NOT placeholders (no FP on real contact data).
        assert_eq!(placeholder_email_domain("real@gmail.com"), None);
        assert_eq!(placeholder_email_domain("help@company.com"), None);
        assert_eq!(placeholder_email_domain("not an email"), None);
    }

    #[test]
    fn credential_name_is_suffix_matched() {
        assert!(is_credential_name("password"));
        assert!(is_credential_name("db_password"));
        assert!(is_credential_name("api_key"));
        assert!(is_credential_name("DB_SECRET"));
        // Credential word not the trailing noun → not a credential name (no FP on regex/labels).
        assert!(!is_credential_name("token_pattern"));
        assert!(!is_credential_name("tokenizer"));
        assert!(!is_credential_name("password_hint_label"));
        assert!(!is_credential_name("secretary"));
    }

    #[test]
    fn uuids() {
        assert!(is_placeholder_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_placeholder_uuid("11111111-1111-1111-1111-111111111111"));
        assert!(!is_placeholder_uuid("f47ac10b-58cc-4372-a567-0e02b2c3d479")); // real
        assert!(!is_placeholder_uuid("not-a-uuid"));
    }

    #[test]
    fn phones() {
        assert!(is_placeholder_phone("123-456-7890")); // curated fake
        assert!(is_placeholder_phone("000-000-0000")); // 1 distinct digit
        assert!(is_placeholder_phone("111-111-1111")); // 1 distinct digit
        assert!(is_placeholder_phone("555-0100")); // curated reserved fake
        assert!(!is_placeholder_phone("415-826-3199")); // realistic
        assert!(!is_placeholder_phone("hello world"));
    }

    #[test]
    fn weak_credential_values() {
        let f = Finder {
            extra: &[],
            docstrings: std::collections::HashSet::new(),
            found: HashMap::new(),
        };
        assert!(f.is_weak_credential("changeme"));
        assert!(f.is_weak_credential("password123"));
        assert!(f.is_weak_credential("your_api_key"));
        assert!(f.is_weak_credential("xxxx"));
        assert!(!f.is_weak_credential("a7Fq9zLp2KdM")); // looks real
    }
}

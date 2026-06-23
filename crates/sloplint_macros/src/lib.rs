//! Procedural macros for sloplint, mirroring ruff's `ruff_macros`.
//!
//! Currently just [`ViolationMetadata`], the derive that turns a rule struct's doc-comment into
//! machine-readable metadata (the rule's name and its rendered `## What it does` explanation), so
//! the docs have a single source of truth — the doc-comment — exactly as ruff does.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Expr, ExprLit, Lit, Meta};

/// Derive `sloplint_diagnostics::ViolationMetadata` for a rule struct.
///
/// `rule_name()` returns the struct's identifier; `explanation()` returns the struct's
/// doc-comment (the `## What it does` / `## Why is this bad?` block), or `None` if it has none.
/// One source of truth for a rule's prose: the doc-comment that also documents it in the code.
#[proc_macro_derive(ViolationMetadata)]
pub fn derive_violation_metadata(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    // Collect the `#[doc = "..."]` attributes (what `///` lines desugar to), strip the single
    // leading space rustdoc inserts, and join them back into the original doc block.
    let doc_lines: Vec<String> = input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_string())
        .collect();
    let explanation = doc_lines.join("\n");

    let explanation_tokens = if explanation.trim().is_empty() {
        quote! { ::core::option::Option::None }
    } else {
        quote! { ::core::option::Option::Some(#explanation) }
    };

    quote! {
        impl ::sloplint_diagnostics::ViolationMetadata for #name {
            fn rule_name(&self) -> &'static str {
                #name_str
            }
            fn explanation(&self) -> ::core::option::Option<&'static str> {
                #explanation_tokens
            }
        }
    }
    .into()
}

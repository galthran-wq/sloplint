//! Renderings of the `metrics` command: the per-panel JSON tree (`json`), the markdown PR
//! summary (`markdown`), the human stdout tables (`table`), and the small `Option<f64>` ratio
//! formatter shared across them.

mod json;
mod markdown;
mod table;

pub(crate) use json::metrics_json;
pub(crate) use markdown::metrics_markdown;
pub(crate) use table::{
    print_class_rows, print_clone_density, print_concentration, print_function_rows,
    print_metrics_panel, print_package_rows, print_test_proxies_table,
};

/// Render an optional ratio: a fixed-precision number, or `n/a` when undefined (no denominator).
pub(crate) fn opt_ratio(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2}"),
        None => "n/a".to_string(),
    }
}

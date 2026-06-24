//! Shields.io badge emission: per-metric badges with "lower is better" color thresholds, the
//! combined summary badge, and writing them to the badges directory.

use std::fs;
use std::path::Path;

use anyhow::Context;
use sloplint_linter::config::BadgeSettings;
use sloplint_metrics::badge::{Badge, Color};
use sloplint_metrics::RepoMetrics;

/// Badges for the headline metrics, each with a "lower is better" color threshold.
fn metric_badges(repo: &RepoMetrics) -> Vec<(&'static str, Badge)> {
    vec![
        (
            "avg-function-loc",
            Badge::new(
                "avg function LoC",
                format!("{:.0}", repo.avg_function_loc),
                Color::for_value(repo.avg_function_loc, 30.0, 60.0),
            ),
        ),
        (
            "max-cyclomatic",
            Badge::new(
                "max complexity",
                repo.max_cyclomatic.to_string(),
                Color::for_value(repo.max_cyclomatic as f64, 10.0, 20.0),
            ),
        ),
        // Headline cyclomatic risk, colored by McCabe's tier rather than a flat threshold.
        ("cyclomatic-risk", repo.cyclomatic_badge()),
        (
            "max-cognitive",
            Badge::new(
                "max cognitive",
                repo.max_cognitive.to_string(),
                Color::for_value(repo.max_cognitive as f64, 15.0, 30.0),
            ),
        ),
        // Headline cognitive risk, colored by SonarSource's band rather than a flat threshold
        // — the cognitive counterpart to `cyclomatic-risk`.
        ("cognitive-risk", repo.cognitive_badge()),
        (
            "max-nesting",
            Badge::new(
                "max nesting",
                repo.max_nesting.to_string(),
                Color::for_value(repo.max_nesting as f64, 4.0, 6.0),
            ),
        ),
        (
            "comment-density",
            Badge::new(
                "comment density",
                format!("{:.0}%", repo.comment_density * 100.0),
                Color::for_value(repo.comment_density * 100.0, 20.0, 40.0),
            ),
        ),
        // Documentation coverage: higher is better, so green at high coverage.
        (
            "docstring-coverage",
            Badge::new(
                "docstring coverage",
                format!("{:.0}%", repo.docstring_coverage * 100.0),
                Color::for_value_high(repo.docstring_coverage * 100.0, 50.0, 80.0),
            ),
        ),
    ]
}

pub(crate) fn write_badges(
    dir: &str,
    repo: &RepoMetrics,
    settings: &BadgeSettings,
) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {dir}"))?;
    let all = metric_badges(repo);

    let mut written = 0usize;
    // Individual per-metric badges: `include` is None => all, Some(list) => only those.
    for (slug, badge) in &all {
        let keep = settings
            .include
            .as_ref()
            .is_none_or(|list| list.iter().any(|s| s.as_str() == *slug));
        if keep {
            write_badge_files(dir, slug, badge)?;
            written += 1;
        }
    }
    // One combined badge over the `summary` metrics, colored by the worst tier among them.
    if !settings.summary.is_empty() {
        let badge = summary_badge(&all, &settings.summary);
        // Skip if no slug resolved (e.g. all unknown) — an empty badge is meaningless.
        if !badge.message.is_empty() {
            write_badge_files(dir, "summary", &badge)?;
            written += 1;
        }
    }
    eprintln!("sloplint: wrote {written} badge(s) to {dir}");
    Ok(())
}

fn write_badge_files(dir: &str, slug: &str, badge: &Badge) -> anyhow::Result<()> {
    let svg_path = Path::new(dir).join(format!("{slug}.svg"));
    let json_path = Path::new(dir).join(format!("{slug}.json"));
    fs::write(&svg_path, badge.svg()).with_context(|| format!("writing {}", svg_path.display()))?;
    fs::write(&json_path, badge.endpoint_json())
        .with_context(|| format!("writing {}", json_path.display()))?;
    Ok(())
}

/// Combine the named metrics into a single `sloplint` badge, e.g. `CC 8 · CoCo 14 · density
/// 18%`, colored by the worst tier among them. Unknown slugs are skipped.
fn summary_badge(all: &[(&'static str, Badge)], slugs: &[String]) -> Badge {
    let mut parts = Vec::new();
    let mut worst = Color::Green;
    for slug in slugs {
        if let Some(entry) = all.iter().find(|e| e.0 == slug.as_str()) {
            parts.push(format!(
                "{} {}",
                badge_short_label(entry.0),
                entry.1.message
            ));
            if color_rank(entry.1.color) > color_rank(worst) {
                worst = entry.1.color;
            }
        }
    }
    Badge::new("sloplint", parts.join(" · "), worst)
}

/// Worst-is-highest ranking so a summary badge takes the most severe color among its metrics.
fn color_rank(color: Color) -> u8 {
    match color {
        Color::Green => 0,
        Color::Yellow => 1,
        Color::Red => 2,
    }
}

/// Short label for a metric slug, used in the combined summary badge.
fn badge_short_label(slug: &str) -> &str {
    match slug {
        "max-cyclomatic" => "CC",
        "cyclomatic-risk" => "risk",
        "max-cognitive" => "CoCo",
        "cognitive-risk" => "CoCo risk",
        "avg-function-loc" => "loc",
        "max-nesting" => "nesting",
        "comment-density" => "density",
        "docstring-coverage" => "docs",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_badges() -> Vec<(&'static str, Badge)> {
        vec![
            (
                "max-cyclomatic",
                Badge::new("max complexity", "8", Color::Green),
            ),
            (
                "max-cognitive",
                Badge::new("max cognitive", "14", Color::Yellow),
            ),
            (
                "comment-density",
                Badge::new("comment density", "18%", Color::Red),
            ),
        ]
    }

    #[test]
    fn summary_badge_joins_short_labels() {
        let badge = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "max-cognitive".to_string()],
        );
        assert_eq!(badge.label, "sloplint");
        assert_eq!(badge.message, "CC 8 · CoCo 14");
    }

    #[test]
    fn summary_badge_takes_the_worst_color() {
        // green + yellow -> yellow; adding red -> red.
        let two = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "max-cognitive".to_string()],
        );
        assert_eq!(two.color, Color::Yellow);
        let three = summary_badge(
            &sample_badges(),
            &[
                "max-cyclomatic".to_string(),
                "max-cognitive".to_string(),
                "comment-density".to_string(),
            ],
        );
        assert_eq!(three.color, Color::Red);
    }

    #[test]
    fn summary_badge_skips_unknown_slugs() {
        let badge = summary_badge(
            &sample_badges(),
            &["max-cyclomatic".to_string(), "nope".to_string()],
        );
        assert_eq!(badge.message, "CC 8");
    }

    #[test]
    fn summary_badge_all_unknown_is_empty() {
        // All-unknown slugs -> empty message; write_badges skips emitting it.
        let badge = summary_badge(&sample_badges(), &["nope".to_string()]);
        assert!(badge.message.is_empty());
    }
}

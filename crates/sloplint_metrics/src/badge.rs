//! Badge rendering: color thresholds, shields.io endpoint JSON, and a flat SVG.

/// Badge color. Green = healthy, yellow = watch, red = over budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Green,
    Yellow,
    Red,
}

impl Color {
    /// Hex fill used in the SVG.
    pub fn hex(self) -> &'static str {
        match self {
            Color::Green => "#4c1",
            Color::Yellow => "#dfb317",
            Color::Red => "#e05d44",
        }
    }

    /// shields.io color keyword used in the endpoint JSON.
    pub fn keyword(self) -> &'static str {
        match self {
            Color::Green => "brightgreen",
            Color::Yellow => "yellow",
            Color::Red => "red",
        }
    }

    /// Color for a "lower is better" metric: green below `warn`, yellow up to `fail`, red at
    /// or above `fail`.
    pub fn for_value(value: f64, warn: f64, fail: f64) -> Color {
        if value >= fail {
            Color::Red
        } else if value >= warn {
            Color::Yellow
        } else {
            Color::Green
        }
    }

    /// Color for a "higher is better" metric (e.g. docstring coverage): green at or above
    /// `good`, yellow down to `warn`, red below `warn`. The mirror of [`Self::for_value`].
    pub fn for_value_high(value: f64, warn: f64, good: f64) -> Color {
        if value >= good {
            Color::Green
        } else if value >= warn {
            Color::Yellow
        } else {
            Color::Red
        }
    }
}

/// A label/message/color badge.
#[derive(Debug, Clone)]
pub struct Badge {
    pub label: String,
    pub message: String,
    pub color: Color,
}

impl Badge {
    pub fn new(label: impl Into<String>, message: impl Into<String>, color: Color) -> Self {
        Self {
            label: label.into(),
            message: message.into(),
            color,
        }
    }

    /// shields.io [endpoint](https://shields.io/endpoint) JSON — host this and point a
    /// shields URL at it for a dynamic badge that never needs committing.
    pub fn endpoint_json(&self) -> String {
        format!(
            r#"{{"schemaVersion":1,"label":"{}","message":"{}","color":"{}"}}"#,
            json_escape(&self.label),
            json_escape(&self.message),
            self.color.keyword()
        )
    }

    /// A self-contained flat-style SVG badge (no external assets).
    pub fn svg(&self) -> String {
        let label = xml_escape(&self.label);
        let message = xml_escape(&self.message);
        let label_w = text_width(&self.label);
        let message_w = text_width(&self.message);
        let total = label_w + message_w;
        let label_mid = label_w / 2;
        let message_mid = label_w + message_w / 2;
        let color = self.color.hex();
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" role="img" aria-label="{label}: {message}">
  <rect width="{label_w}" height="20" fill="#555"/>
  <rect x="{label_w}" width="{message_w}" height="20" fill="{color}"/>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="{label_mid}" y="14">{label}</text>
    <text x="{message_mid}" y="14">{message}</text>
  </g>
</svg>
"##
        )
    }
}

/// Approximate pixel width of badge text (~7px/char + padding) — good enough for layout.
fn text_width(text: &str) -> usize {
    text.chars().count() * 7 + 10
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_thresholds() {
        assert_eq!(Color::for_value(5.0, 10.0, 20.0), Color::Green);
        assert_eq!(Color::for_value(12.0, 10.0, 20.0), Color::Yellow);
        assert_eq!(Color::for_value(25.0, 10.0, 20.0), Color::Red);
    }

    #[test]
    fn higher_is_better_thresholds() {
        // Mirror of `for_value`: green at/above `good`, yellow down to `warn`, red below.
        assert_eq!(Color::for_value_high(90.0, 50.0, 80.0), Color::Green);
        assert_eq!(Color::for_value_high(80.0, 50.0, 80.0), Color::Green);
        assert_eq!(Color::for_value_high(60.0, 50.0, 80.0), Color::Yellow);
        assert_eq!(Color::for_value_high(40.0, 50.0, 80.0), Color::Red);
    }

    #[test]
    fn endpoint_json_is_well_formed() {
        let badge = Badge::new("avg LoC", "189", Color::Green);
        assert_eq!(
            badge.endpoint_json(),
            r#"{"schemaVersion":1,"label":"avg LoC","message":"189","color":"brightgreen"}"#
        );
    }

    #[test]
    fn svg_contains_label_and_message() {
        let svg = Badge::new("max complexity", "12", Color::Yellow).svg();
        assert!(svg.contains("max complexity"));
        assert!(svg.contains(">12<"));
        assert!(svg.contains("#dfb317"));
    }
}

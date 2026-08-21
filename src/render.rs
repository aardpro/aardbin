//! Server-side HTML rendering via minijinja (PRD §3.1).

use minijinja::{AutoEscape, Environment};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct Renderer {
    env: Arc<Environment<'static>>,
}

impl Renderer {
    pub fn new(templates_dir: &Path) -> Renderer {
        let mut env = Environment::new();
        env.set_loader(minijinja::path_loader(templates_dir));
        env.set_auto_escape_callback(|name| {
            if name.ends_with(".html") {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        Renderer { env: Arc::new(env) }
    }

    pub fn render<S: Serialize>(
        &self,
        template: &str,
        ctx: S,
    ) -> Result<String, minijinja::Error> {
        self.env.get_template(template)?.render(&ctx)
    }
}

/// Char-boundary-safe truncation.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Display title per PRD §10.3: explicit title → first content line → "Untitled".
/// Returns (title, is_untitled_placeholder).
pub fn display_title(title: &str, content: &str) -> (String, bool) {
    let t = title.trim();
    if !t.is_empty() {
        return (truncate_chars(t, 200), false);
    }
    let first_line = content.lines().next().unwrap_or("").trim();
    if !first_line.is_empty() {
        return (truncate_chars(first_line, 80), false);
    }
    ("Untitled".into(), true)
}

/// One-line preview of content for list cards.
pub fn snippet(content: &str) -> String {
    let one_line: String = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    truncate_chars(&one_line, 160)
}

/// Absolute UTC timestamp text, e.g. `2026-08-20 17:22:31`.
/// Client JS replaces this with a local relative time on load.
pub fn format_ts_utc(ts: i64) -> String {
    use time::format_description::FormatItem;
    use time::OffsetDateTime;
    static FORMAT: std::sync::OnceLock<Vec<FormatItem<'static>>> = std::sync::OnceLock::new();
    let fmt = FORMAT.get_or_init(|| {
        time::format_description::parse_borrowed::<2>(
            "[year]-[month]-[day] [hour]:[minute]:[second]",
        )
        .expect("valid format")
    });
    OffsetDateTime::from_unix_timestamp(ts)
        .map(|dt| {
            let mut s = dt.format(fmt).unwrap_or_default();
            s.push_str(" UTC");
            s
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_fallback_chain() {
        assert_eq!(display_title("  Hi  ", "body"), ("Hi".into(), false));
        assert_eq!(
            display_title("", "first line\nsecond"),
            ("first line".into(), false)
        );
        assert_eq!(display_title("", "  \n  "), ("Untitled".into(), true));
        assert_eq!(display_title("  ", ""), ("Untitled".into(), true));
    }

    #[test]
    fn truncation_is_char_safe() {
        let s = "汉字".repeat(100);
        let t = truncate_chars(&s, 10);
        assert_eq!(t.chars().count(), 11); // 10 + ellipsis
    }

    #[test]
    fn snippet_collapses_lines() {
        assert_eq!(snippet("a\n\nb\nc"), "a · b · c");
        assert_eq!(snippet(""), "");
    }

    #[test]
    fn ts_format() {
        assert_eq!(format_ts_utc(0), "1970-01-01 00:00:00 UTC");
    }
}

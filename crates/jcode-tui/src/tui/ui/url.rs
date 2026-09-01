use regex::Regex;
use std::sync::OnceLock;
use unicode_width::UnicodeWidthStr;

fn compile_static_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(error) => {
            crate::logging::warn(&format!("Failed to compile static {label} regex: {error}"));
            None
        }
    }
}

pub(crate) fn url_regex() -> Option<&'static Regex> {
    static URL_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    URL_REGEX
        .get_or_init(|| {
            compile_static_regex(r#"(?i)(?:https?://|mailto:|file://)[^\s<>'\"]+"#, "URL")
        })
        .as_ref()
}

fn markdown_link_regex() -> Option<&'static Regex> {
    static MARKDOWN_LINK_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    MARKDOWN_LINK_REGEX
        .get_or_init(|| {
            compile_static_regex(
                r#"\[([^]\n]+)\]\(([^\s)]+)(?:\s+[^)]*)?\)"#,
                "Markdown link",
            )
        })
        .as_ref()
}

fn file_path_regex() -> Option<&'static Regex> {
    static FILE_PATH_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    FILE_PATH_REGEX
        .get_or_init(|| {
            compile_static_regex(r#"(?:^|[\s'\"`(\[{])([^\s'\"`()<>\[\]{}]+)"#, "file path")
        })
        .as_ref()
}

fn file_path_without_location(path: &str) -> &str {
    let without_anchor = path.split_once('#').map_or(path, |(path, _)| path);
    let Some((candidate, suffix)) = without_anchor.rsplit_once(':') else {
        return without_anchor;
    };
    if suffix.parse::<u64>().is_err() {
        return without_anchor;
    }

    let Some((path, line)) = candidate.rsplit_once(':') else {
        return candidate;
    };
    if line.parse::<u64>().is_ok() {
        path
    } else {
        candidate
    }
}

fn is_supported_file_path(path: &str) -> bool {
    let path = file_path_without_location(path);
    if path.contains('/') || path.starts_with(['.', '~']) {
        return true;
    }

    if matches!(
        path.to_ascii_lowercase().as_str(),
        "dockerfile" | "gemfile" | "justfile" | "makefile" | "procfile" | "rakefile" | "readme"
    ) {
        return true;
    }

    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "bash"
                | "c"
                | "cc"
                | "cfg"
                | "conf"
                | "cpp"
                | "css"
                | "csv"
                | "fish"
                | "go"
                | "h"
                | "hpp"
                | "html"
                | "ini"
                | "java"
                | "js"
                | "json"
                | "jsx"
                | "log"
                | "markdown"
                | "md"
                | "mdx"
                | "py"
                | "rs"
                | "scss"
                | "sh"
                | "sql"
                | "toml"
                | "ts"
                | "tsx"
                | "txt"
                | "xml"
                | "yaml"
                | "yml"
                | "zsh"
        )
    })
}

pub(crate) fn trim_url_candidate(candidate: &str) -> &str {
    let mut trimmed = candidate;
    loop {
        let next = if trimmed.ends_with(['.', ',', ';', ':', '!', '?'])
            || (trimmed.ends_with(')')
                && trimmed.matches(')').count() > trimmed.matches('(').count())
            || (trimmed.ends_with(']')
                && trimmed.matches(']').count() > trimmed.matches('[').count())
            || (trimmed.ends_with('}')
                && trimmed.matches('}').count() > trimmed.matches('{').count())
        {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };

        if next.len() == trimmed.len() {
            return trimmed;
        }
        trimmed = next;
    }
}

fn trim_file_mention_candidate(candidate: &str) -> &str {
    let mut trimmed = candidate;
    loop {
        let before = trimmed.len();
        trimmed = trim_url_candidate(trimmed);
        if trimmed.ends_with(['\'', '"', '`']) {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        if trimmed.len() == before {
            return trimmed;
        }
    }
}

pub(crate) fn link_target_for_display_column(raw_text: &str, column: usize) -> Option<String> {
    if let Some(regex) = markdown_link_regex() {
        for captures in regex.captures_iter(raw_text) {
            let (Some(label), Some(target)) = (captures.get(1), captures.get(2)) else {
                continue;
            };
            let start_col = raw_text[..label.start()].width();
            let end_col = start_col + label.as_str().width();
            if column >= start_col && column < end_col {
                return Some(target.as_str().to_string());
            }
        }
    }

    for mat in url_regex()?.find_iter(raw_text) {
        let matched = &raw_text[mat.start()..mat.end()];
        let trimmed = trim_url_candidate(matched);
        if trimmed.is_empty() {
            continue;
        }

        let start_col = raw_text[..mat.start()].width();
        let end_col = start_col + trimmed.width();
        if column >= start_col && column < end_col && ::url::Url::parse(trimmed).is_ok() {
            return Some(trimmed.to_string());
        }
    }

    for (mention_start, _) in raw_text.match_indices('@') {
        let starts_token = mention_start == 0
            || raw_text[..mention_start]
                .chars()
                .next_back()
                .is_some_and(|character| {
                    character.is_whitespace()
                        || matches!(character, '\'' | '"' | '`' | '(' | '[' | '{')
                });
        if !starts_token {
            continue;
        }

        let mention_end = raw_text[mention_start..]
            .find(char::is_whitespace)
            .map_or(raw_text.len(), |offset| mention_start + offset);
        let trimmed = trim_file_mention_candidate(&raw_text[mention_start..mention_end]);
        if trimmed.len() <= 1 {
            continue;
        }
        let start_col = raw_text[..mention_start].width();
        let end_col = start_col + trimmed.width();
        if column >= start_col && column < end_col {
            return Some(trimmed.to_string());
        }
    }

    if let Some(regex) = file_path_regex() {
        for captures in regex.captures_iter(raw_text) {
            let Some(path_match) = captures.get(1) else {
                continue;
            };
            let path = trim_file_mention_candidate(path_match.as_str());
            if !is_supported_file_path(path) {
                continue;
            }
            let start_col = raw_text[..path_match.start()].width();
            let end_col = start_col + path.width();
            if column >= start_col && column < end_col {
                return Some(path.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{link_target_for_display_column, trim_url_candidate, url_regex};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn url_regex_matches_supported_link_schemes() {
        let regex = url_regex();
        assert!(regex.is_some(), "test URL regex should initialize");
        let Some(regex) = regex else {
            return;
        };
        let text = "See https://example.com, mailto:user@example.com, and file:///tmp/a.txt";
        let matches: Vec<&str> = regex.find_iter(text).map(|mat| mat.as_str()).collect();

        assert_eq!(
            matches,
            vec![
                "https://example.com,",
                "mailto:user@example.com,",
                "file:///tmp/a.txt"
            ]
        );
    }

    #[test]
    fn trim_url_candidate_removes_trailing_sentence_punctuation() {
        assert_eq!(
            trim_url_candidate("https://example.com,"),
            "https://example.com"
        );
        assert_eq!(
            trim_url_candidate("https://example.com?!"),
            "https://example.com"
        );
        assert_eq!(
            trim_url_candidate("mailto:user@example.com."),
            "mailto:user@example.com"
        );
    }

    #[test]
    fn trim_url_candidate_preserves_balanced_closing_delimiters() {
        assert_eq!(
            trim_url_candidate("https://example.com/path_(draft)"),
            "https://example.com/path_(draft)"
        );
        assert_eq!(
            trim_url_candidate("https://example.com/path_(draft))."),
            "https://example.com/path_(draft)"
        );
        assert_eq!(
            trim_url_candidate("https://example.com/[docs]]"),
            "https://example.com/[docs]"
        );
    }

    #[test]
    fn link_target_for_display_column_returns_trimmed_url_when_inside_url() {
        let text = "Open https://example.com/docs, please";

        assert_eq!(
            link_target_for_display_column(text, "Open https://example".len()),
            Some("https://example.com/docs".to_string())
        );
        assert_eq!(
            link_target_for_display_column(text, "Open ".len() - 1),
            None
        );
        assert_eq!(
            link_target_for_display_column(text, "Open https://example.com/docs".len()),
            None
        );
    }

    #[test]
    fn link_target_for_display_column_uses_display_width_for_wide_prefixes() {
        let text = "🙂 https://example.com";

        assert_eq!(
            link_target_for_display_column(text, 3),
            Some("https://example.com".to_string())
        );
        assert_eq!(link_target_for_display_column(text, 1), None);
    }

    #[test]
    fn link_target_for_display_column_resolves_markdown_label() {
        let text = "Read the [guide](docs/guide.md#setup) today";

        assert_eq!(
            link_target_for_display_column(text, 10),
            Some("docs/guide.md#setup".to_string())
        );
        assert_eq!(link_target_for_display_column(text, 8), None);
    }

    #[test]
    fn link_target_for_display_column_resolves_plain_relative_file_path() {
        let text = "- ' docs/grok-4.6-vs-deepseek-v4-pro-deepswe-cost.md'";

        assert_eq!(
            link_target_for_display_column(text, 12),
            Some("docs/grok-4.6-vs-deepseek-v4-pro-deepswe-cost.md".to_string())
        );
        assert_eq!(link_target_for_display_column(text, 1), None);

        let bare = "See README.md for details";
        assert_eq!(
            link_target_for_display_column(bare, 6),
            Some("README.md".to_string())
        );
    }

    #[test]
    fn link_target_for_display_column_resolves_file_mentions() {
        let text = "Open @docs/guide.md, then @README";

        assert_eq!(
            link_target_for_display_column(text, "Open ".width()),
            Some("@docs/guide.md".to_string())
        );
        assert_eq!(
            link_target_for_display_column(text, "Open @docs/gui".width()),
            Some("@docs/guide.md".to_string())
        );
        assert_eq!(
            link_target_for_display_column(text, "Open @docs/guide.md".width()),
            None,
            "trailing punctuation is outside the mention hit target"
        );
        assert_eq!(
            link_target_for_display_column(text, "Open @docs/guide.md, then ".width()),
            Some("@README".to_string())
        );
    }

    #[test]
    fn file_mention_hit_testing_uses_display_width_without_reclassifying_emails() {
        let text = "🙂 @../src/main.rs user@example.com";

        assert_eq!(
            link_target_for_display_column(text, 3),
            Some("@../src/main.rs".to_string())
        );
        assert_eq!(
            link_target_for_display_column(text, "🙂 @../src/main.rs us".width()),
            None,
            "email domains are not file paths"
        );
    }

    #[test]
    fn ordinary_dotted_prose_is_not_a_file_target() {
        for text in [
            "This is a.b in prose",
            "Try foo.bar next",
            "Visit example.com later",
            "Inspect self.input before continuing",
        ] {
            let dotted = text.find('.').expect("fixture has a dotted word");
            assert_eq!(link_target_for_display_column(text, dotted), None, "{text}");
        }

        let known_file = "Edit config.toml next";
        assert_eq!(
            link_target_for_display_column(known_file, 8),
            Some("config.toml".to_string())
        );
    }

    #[test]
    fn supported_file_targets_preserve_common_path_formats() {
        for (text, expected) in [
            ("Open ~/notes.md", "~/notes.md"),
            ("Run Makefile", "Makefile"),
            ("Read 2026-report.md", "2026-report.md"),
            ("See docs/guide.md#anchor", "docs/guide.md#anchor"),
            ("Inspect src/main.rs:42", "src/main.rs:42"),
            ("Inspect src/main.rs:42:7", "src/main.rs:42:7"),
        ] {
            let column = text.find(expected).expect("fixture contains target");
            assert_eq!(
                link_target_for_display_column(text, column),
                Some(expected.to_string()),
                "target should remain intact in {text:?}"
            );
        }
    }

    #[test]
    fn file_mentions_follow_inline_opening_delimiters() {
        for text in [
            "`@src/main.rs`",
            "(@src/main.rs)",
            "[@src/main.rs]",
            "{@src/main.rs}",
            "\"@src/main.rs\"",
            "'@src/main.rs'",
        ] {
            assert_eq!(
                link_target_for_display_column(text, 1),
                Some("@src/main.rs".to_string()),
                "mention should be clickable in {text:?}"
            );
        }
    }
}

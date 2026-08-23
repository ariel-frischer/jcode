use super::*;

/// Expand repository-local `@path` references before sending a prompt.
///
/// The picker only changes the text in the composer. The provider must receive
/// the referenced contents too, matching Claude Code's accepted file-reference
/// behavior. Unresolved references are intentionally preserved:
/// `@someone` and prose containing `@` are not file errors.
pub(in crate::tui::app) fn expand_file_mentions(
    input: &str,
    working_dir: Option<&str>,
    enabled: bool,
) -> String {
    let Some(working_dir) = working_dir.filter(|_| enabled) else {
        return input.to_owned();
    };
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let Some(relative_at) = input[cursor..].find('@') else {
            output.push_str(&input[cursor..]);
            break;
        };
        let at = cursor + relative_at;
        output.push_str(&input[cursor..at]);

        // An @ embedded in an identifier or email address is not a file
        // reference. A file mention starts at the beginning or after whitespace.
        let valid_start = at == 0
            || input[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let end = input[at + 1..]
            .find(char::is_whitespace)
            .map_or(input.len(), |offset| at + 1 + offset);
        let mention = &input[at + 1..end];
        if !valid_start || mention.is_empty() {
            output.push('@');
            cursor = at + 1;
            continue;
        }

        let path = PathBuf::from(mention);
        let resolved = if path.is_absolute() {
            path
        } else {
            PathBuf::from(working_dir).join(path)
        };
        let replacement = resolved
            .metadata()
            .ok()
            .filter(|metadata| {
                metadata.is_file() && metadata.len() <= MAX_SUBMITTED_TEXT_BYTES as u64
            })
            .and_then(|_| std::fs::read_to_string(&resolved).ok())
            .map(|contents| {
                let escaped_path = mention
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                let escaped_contents = contents.replace("</file>", "&lt;/file&gt;");
                format!("<file path=\"{escaped_path}\">\n{escaped_contents}\n</file>")
            })
            .filter(|replacement| {
                output.len() + replacement.len() + input.len().saturating_sub(end)
                    <= MAX_SUBMITTED_TEXT_BYTES
            });
        if let Some(replacement) = replacement {
            output.push_str(&replacement);
        } else {
            output.push_str(&input[at..end]);
        }
        cursor = end;
    }
    output
}

pub(in crate::tui::app) fn expand_file_mentions_for_submit(
    app: &App,
    input: &str,
) -> Result<String, String> {
    let remote_root = app
        .is_remote
        .then(|| std::env::current_dir().ok())
        .flatten()
        .and_then(|path| path.to_str().map(str::to_owned));
    let working_dir = remote_root
        .as_deref()
        .or(app.session.working_dir.as_deref());
    let expanded = expand_file_mentions(
        input,
        working_dir,
        crate::config::config().file_mentions.enabled,
    );
    if let Some(notice) = input_exceeds_submit_limit(&expanded) {
        return Err(notice);
    }
    Ok(expanded)
}

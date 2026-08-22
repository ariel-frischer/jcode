use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

pub fn parse(input: &str) -> Result<Vec<String>, String> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for (index, raw_line) in input.lines().enumerate() {
        let entry = raw_line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        validate_relative_markdown(entry).map_err(|error| {
            format!(
                "invalid runtime documentation entry on line {}: {error}",
                index + 1
            )
        })?;
        if !seen.insert(entry.to_string()) {
            return Err(format!(
                "duplicate runtime documentation entry on line {}: {entry}",
                index + 1
            ));
        }
        entries.push(entry.to_string());
    }

    if entries.is_empty() {
        return Err("runtime documentation manifest is empty".to_string());
    }
    Ok(entries)
}

pub fn generate(repo: &Path, entries: &[String]) -> Result<String, String> {
    let mut generated = String::from("pub(crate) static JCODE_DOCS: &[(&str, &str)] = &[\n");
    for entry in entries {
        validate_relative_markdown(entry)?;
        let path = repo.join(entry);
        fs::read_to_string(&path)
            .map_err(|error| format!("runtime documentation {entry} is unavailable: {error}"))?;
        generated.push_str(&format!(
            "    ({entry:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../\", {entry:?}))),\n"
        ));
    }
    generated.push_str("];");
    generated.push('\n');
    Ok(generated)
}

fn validate_relative_markdown(entry: &str) -> Result<(), String> {
    let path = Path::new(entry);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
        || path.extension().and_then(|extension| extension.to_str()) != Some("md")
    {
        return Err(format!(
            "runtime documentation path must be a normalized relative Markdown path: {entry}"
        ));
    }
    Ok(())
}

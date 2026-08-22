use super::protocol::{Position, Range, Severity, normalize_diagnostics};

#[test]
fn normalized_diagnostics_preserve_ranges_and_bound_messages() {
    let raw = serde_json::json!({
        "uri": "file:///workspace/src/main.rs",
        "version": 7,
        "diagnostics": [{
            "range": {
                "start": {"line": 1, "character": 2},
                "end": {"line": 1, "character": 8}
            },
            "severity": 1,
            "message": "x".repeat(20000),
            "source": "rust-analyzer"
        }]
    });

    let diagnostics = normalize_diagnostics(&raw, 7, 1024).expect("valid diagnostics");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].range,
        Range {
            start: Position {
                line: 1,
                character: 2
            },
            end: Position {
                line: 1,
                character: 8
            },
        }
    );
    assert_eq!(diagnostics[0].severity, Some(Severity::Error));
    assert_eq!(diagnostics[0].version, Some(7));
    assert_eq!(diagnostics[0].message.len(), 1024);
}

#[test]
fn diagnostic_without_version_is_marked_stale_for_newer_document() {
    let raw = serde_json::json!({
        "uri": "file:///workspace/src/main.rs",
        "diagnostics": [{
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 1}
            },
            "severity": 2,
            "message": "warning"
        }]
    });

    let diagnostics = normalize_diagnostics(&raw, 4, 4096).expect("valid diagnostics");

    assert_eq!(diagnostics[0].severity, Some(Severity::Warning));
    assert!(diagnostics[0].stale);
}

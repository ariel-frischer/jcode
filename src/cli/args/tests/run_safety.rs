use crate::cli::args::{Args, Command};
use clap::Parser;

#[test]
fn schema_mode_safety_flags_are_explicit_and_conflicting() {
    for (flag, value) in [
        ("--max-turns", "1"),
        ("--max-tool-steps", "1"),
        ("--token-budget", "1"),
        ("--deadline", "2030-01-01T00:00:00Z"),
    ] {
        let args = Args::try_parse_from([
            "jcode",
            "run",
            "--schema",
            "schema.json",
            flag,
            value,
            "return data",
        ])
        .expect("schema flags remain parser-compatible before dispatch rejection");
        assert!(matches!(
            args.command,
            Some(Command::Run {
                schema: Some(_),
                ..
            })
        ));
    }
}

#[test]
fn run_safety_flags_preserve_raw_values() {
    let args = Args::try_parse_from([
        "jcode",
        "run",
        "--max-turns",
        " 3 ",
        "--max-tool-steps",
        "7",
        "--token-budget",
        "1000",
        "--deadline",
        "2030-01-01T00:00:00Z",
        "hello",
    ])
    .expect("run safety flags should parse");
    let Some(Command::Run {
        run_safety,
        ..
    }) = args.command
    else {
        panic!("expected run command");
    };
    assert_eq!(run_safety.max_turns.as_deref(), Some(" 3 "));
    assert_eq!(run_safety.max_tool_steps.as_deref(), Some("7"));
    assert_eq!(run_safety.token_budget.as_deref(), Some("1000"));
    assert_eq!(run_safety.deadline.as_deref(), Some("2030-01-01T00:00:00Z"));
}

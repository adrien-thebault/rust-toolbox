use toolbox_server::telemetry::{LogFormat, TelemetryArgs, TelemetryError};

fn args(verbose: u8, quiet: u8) -> TelemetryArgs {
    TelemetryArgs {
        verbose,
        quiet,
        log_format: "pretty".to_owned(),
        log_filter: None,
    }
}

#[test]
fn verbosity_counts_move_the_level_in_both_directions() {
    assert_eq!(args(0, 0).level(), tracing::Level::INFO);
    assert_eq!(args(1, 0).level(), tracing::Level::DEBUG);
    assert_eq!(args(2, 0).level(), tracing::Level::TRACE);
    assert_eq!(
        args(9, 0).level(),
        tracing::Level::TRACE,
        "saturates rather than wrapping"
    );
    assert_eq!(args(0, 1).level(), tracing::Level::WARN);
    assert_eq!(args(0, 2).level(), tracing::Level::ERROR);
    assert_eq!(args(0, 9).level(), tracing::Level::ERROR);
}

#[test]
fn verbose_and_quiet_cancel_out() {
    assert_eq!(args(2, 2).level(), tracing::Level::INFO);
    assert_eq!(args(3, 1).level(), tracing::Level::TRACE);
}

#[test]
fn the_three_formats_parse() {
    assert_eq!("pretty".parse::<LogFormat>().unwrap(), LogFormat::Pretty);
    assert_eq!("compact".parse::<LogFormat>().unwrap(), LogFormat::Compact);
    assert_eq!(
        "JSON".parse::<LogFormat>().unwrap(),
        LogFormat::Json,
        "case insensitive"
    );
}

#[test]
fn an_unknown_format_names_what_was_expected() {
    let err = "yaml".parse::<LogFormat>().unwrap_err();
    assert!(matches!(err, TelemetryError::UnknownFormat(ref s) if s == "yaml"));
    assert!(err.to_string().contains("pretty, compact or json"), "{err}");
}

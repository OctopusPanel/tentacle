use std::collections::HashMap;
use tentacle::core::{
    CircularBuffer, EnvironmentInterpolator, LogEvent, LogScanner,
};

#[test]
fn test_environment_interpolation() {
    let mut vars = HashMap::new();
    vars.insert("SERVER_PORT".to_string(), "25565".to_string());
    vars.insert("MAX_PLAYERS".to_string(), "20".to_string());
    vars.insert("SERVER_JAR".to_string(), "server.jar".to_string());

    let template = "java -Xms128M -Xmx2048M -jar {{SERVER_JAR}} --port {{SERVER_PORT}}";
    let interpolated = EnvironmentInterpolator::interpolate(template, &vars);
    assert_eq!(
        interpolated,
        "java -Xms128M -Xmx2048M -jar server.jar --port 25565"
    );

    let dollar_template = "./run.sh --port ${SERVER_PORT} --max ${MAX_PLAYERS}";
    let dollar_interpolated = EnvironmentInterpolator::interpolate(dollar_template, &vars);
    assert_eq!(dollar_interpolated, "./run.sh --port 25565 --max 20");
}

#[test]
fn test_circular_log_buffer() {
    let mut buffer = CircularBuffer::new(3);
    buffer.push_line("Line 1".to_string());
    buffer.push_line("Line 2".to_string());
    buffer.push_line("Line 3".to_string());

    assert_eq!(buffer.snapshot(), vec!["Line 1", "Line 2", "Line 3"]);

    buffer.push_line("Line 4".to_string());
    assert_eq!(buffer.snapshot(), vec!["Line 2", "Line 3", "Line 4"]);

    buffer.clear();
    assert_eq!(buffer.snapshot(), Vec::<String>::new());
}

#[test]
fn test_log_scanner_start_and_crash_detection() {
    let scanner = LogScanner::new(
        Some(r"(?i)Server started on port \d+"),
        Some(r"(?i)(OutOfMemoryError|Segmentation fault|Fatal error)"),
    );

    // Normal line
    assert!(scanner.scan_line("[INFO] Loading libraries, please wait...").is_none());

    // Start detection
    let start_event = scanner.scan_line("[INFO] Server started on port 25565! Enjoy!");
    assert!(matches!(start_event, Some(LogEvent::ServerStarted)));

    // Crash detection
    let crash_event = scanner.scan_line("java.lang.OutOfMemoryError: Java heap space");
    assert!(matches!(crash_event, Some(LogEvent::CrashDetected(_))));
}

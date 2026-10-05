use super::*;
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::CLIAgentSessionStatus;
use crate::terminal::cli_agent_sessions::event::{
    CLIAgentEvent, CLIAgentEventPayload, CLIAgentEventSource, CLIAgentEventType,
};
use instant::Instant;
use std::time::Duration;

fn event(kind: CLIAgentEventType, payload: CLIAgentEventPayload) -> CLIAgentEvent {
    CLIAgentEvent {
        v: 1,
        agent: CLIAgent::Claude,
        event: kind,
        session_id: None,
        cwd: None,
        project: None,
        payload,
        source: CLIAgentEventSource::RichPlugin,
    }
}

#[test]
fn test_format_activity_basic() {
    assert_eq!(format_activity("Bash", Some("npm test")), "Bash: npm test");
}

#[test]
fn test_format_activity_path() {
    assert_eq!(
        format_activity("Edit", Some("/Users/x/proj/calc.py")),
        "Edit: calc.py"
    );
}

#[test]
fn test_format_activity_empty_detail() {
    assert_eq!(format_activity("Read", None), "Read");
    assert_eq!(format_activity("Read", Some("  ")), "Read");
}

#[test]
fn test_format_activity_multiline() {
    assert_eq!(
        format_activity("Bash", Some("echo a\necho b")),
        "Bash: echo a"
    );
}

#[test]
fn test_format_activity_truncate() {
    let detail = "a".repeat(60);
    let result = format_activity("Bash", Some(&detail));
    let expected_detail = "a".repeat(47) + "…";
    assert_eq!(result, format!("Bash: {expected_detail}"));
}

#[test]
fn test_format_ago() {
    assert_eq!(format_ago(Duration::from_secs(12)), "hace 12 s");
    assert_eq!(format_ago(Duration::from_secs(125)), "hace 2 min");
    assert_eq!(format_ago(Duration::from_secs(7300)), "hace 2 h");
}

#[test]
fn test_line_default_none() {
    let activity = MocaActivity::default();
    assert!(
        activity
            .line(&CLIAgentSessionStatus::InProgress, Instant::now())
            .is_none()
    );
}

#[test]
fn test_line_tool_start() {
    let mut activity = MocaActivity::default();
    let t0 = Instant::now();
    let e = event(
        CLIAgentEventType::ToolStart,
        CLIAgentEventPayload {
            tool_name: Some("Bash".to_owned()),
            tool_input_preview: Some("cargo test".to_owned()),
            ..Default::default()
        },
    );
    activity.record(&e, t0);

    let line = activity
        .line(
            &CLIAgentSessionStatus::InProgress,
            t0 + Duration::from_secs(12),
        )
        .unwrap();
    assert_eq!(line.text, "hace 12 s · Bash: cargo test");
    assert!(!line.stale);
}

#[test]
fn test_line_stale() {
    let mut activity = MocaActivity::default();
    let t0 = Instant::now();
    let e = event(
        CLIAgentEventType::ToolStart,
        CLIAgentEventPayload {
            tool_name: Some("Bash".to_owned()),
            tool_input_preview: Some("cargo test".to_owned()),
            ..Default::default()
        },
    );
    activity.record(&e, t0);

    let line = activity
        .line(
            &CLIAgentSessionStatus::InProgress,
            t0 + Duration::from_secs(301),
        )
        .unwrap();
    assert!(line.stale);
    assert!(line.text.starts_with("¿trabado? · "));
}

#[test]
fn test_line_blocked_not_stale() {
    let mut activity = MocaActivity::default();
    let t0 = Instant::now();
    let e = event(
        CLIAgentEventType::ToolStart,
        CLIAgentEventPayload {
            tool_name: Some("Bash".to_owned()),
            tool_input_preview: Some("cargo test".to_owned()),
            ..Default::default()
        },
    );
    activity.record(&e, t0);

    let line = activity
        .line(
            &CLIAgentSessionStatus::Blocked { message: None },
            t0 + Duration::from_secs(301),
        )
        .unwrap();
    assert!(!line.stale);
    assert_eq!(line.text, "esperando respuesta · hace 5 min");
}

#[test]
fn test_line_tasks_progress() {
    let mut activity = MocaActivity::default();
    let t = Instant::now();

    let e1 = event(
        CLIAgentEventType::TasksProgress,
        CLIAgentEventPayload {
            tasks: Some((3, 7)),
            ..Default::default()
        },
    );
    activity.record(&e1, t);

    let e2 = event(
        CLIAgentEventType::ToolStart,
        CLIAgentEventPayload {
            tool_name: Some("Bash".to_owned()),
            tool_input_preview: Some("ls".to_owned()),
            ..Default::default()
        },
    );
    activity.record(&e2, t);

    let line = activity
        .line(
            &CLIAgentSessionStatus::InProgress,
            t + Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(line.text, "hace 1 s · 3/7 · Bash: ls");
}

#[test]
fn test_line_stop_success() {
    let mut activity = MocaActivity::default();
    let t = Instant::now();

    let e1 = event(
        CLIAgentEventType::ToolStart,
        CLIAgentEventPayload {
            tool_name: Some("Bash".to_owned()),
            tool_input_preview: Some("ls".to_owned()),
            ..Default::default()
        },
    );
    activity.record(&e1, t);

    let e2 = event(CLIAgentEventType::Stop, CLIAgentEventPayload::default());
    activity.record(&e2, t);

    let line = activity.line(&CLIAgentSessionStatus::Success, t).unwrap();
    assert_eq!(line.text, "listo · hace 0 s");
}

#[test]
fn test_format_activity_strips_cd_prefix() {
    assert_eq!(
        format_activity("Bash", Some("cd /tmp/moca-qa-demo && ./progreso.sh")),
        "Bash: ./progreso.sh"
    );
    assert_eq!(
        format_activity("Bash", Some("cd /a && cd b && npm test")),
        "Bash: npm test"
    );
    assert_eq!(format_activity("Bash", Some("cd /tmp")), "Bash: cd /tmp");
}

#[test]
fn test_format_activity_strips_leading_assignments() {
    assert_eq!(
        format_activity(
            "Bash",
            Some("cd /tmp/d && s=$(date +%s); ./progreso.sh; echo hecho")
        ),
        "Bash: ./progreso.sh; echo hecho"
    );
}

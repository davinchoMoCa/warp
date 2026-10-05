use super::*;
use std::time::Duration;

fn p(pid: u32, parent: u32, cmd: &[&str], secs: u64) -> RawProcess {
    RawProcess {
        pid,
        parent: Some(parent),
        cmd: cmd.iter().map(|s| s.to_string()).collect(),
        name: String::new(),
        run_time: Duration::from_secs(secs),
        cpu_percent: None,
    }
}

#[test]
fn test_format_command() {
    // 1. Standard path
    let proc1 = RawProcess {
        pid: 1,
        parent: None,
        cmd: vec![
            "/opt/homebrew/bin/opencode".to_string(),
            "run".to_string(),
            "--pure".to_string(),
        ],
        name: String::new(),
        run_time: Duration::from_secs(0),
        cpu_percent: None,
    };
    assert_eq!(format_command(&proc1), "opencode run --pure");

    // 2. Empty cmd, use name
    let proc2 = RawProcess {
        pid: 2,
        parent: None,
        cmd: vec![],
        name: "zsh".to_string(),
        run_time: Duration::from_secs(0),
        cpu_percent: None,
    };
    assert_eq!(format_command(&proc2), "zsh");

    // 3. Long argv
    let long_arg = "a".repeat(60);
    let proc3 = RawProcess {
        pid: 3,
        parent: None,
        cmd: vec!["python3".to_string(), long_arg],
        name: String::new(),
        run_time: Duration::from_secs(0),
        cpu_percent: None,
    };
    let res3 = format_command(&proc3);
    assert_eq!(res3.chars().count(), 40);
    assert!(res3.ends_with('…'));
}

#[test]
fn test_format_run_time() {
    assert_eq!(format_run_time(Duration::from_secs(12)), "12 s");
    assert_eq!(format_run_time(Duration::from_secs(250)), "4 min");
    assert_eq!(format_run_time(Duration::from_secs(7300)), "2 h");
}

#[test]
fn test_format_cpu() {
    assert_eq!(format_cpu(Some(85.4)), "85% CPU");
    assert_eq!(format_cpu(None), "");
}

#[test]
fn test_build_tree_complex() {
    let procs = vec![
        p(200, 100, &["claude"], 300),
        p(300, 200, &["bash"], 20),
        p(301, 200, &["opencode"], 40),
        p(999, 1, &["otro"], 10),
    ];
    let tree = build_tree(&procs, 100);

    assert_eq!(tree.hidden, 0);
    assert_eq!(tree.rows.len(), 3);

    assert_eq!(tree.rows[0].depth, 0);
    assert_eq!(tree.rows[0].command, "claude");

    assert_eq!(tree.rows[1].depth, 1);
    assert_eq!(tree.rows[1].command, "opencode");

    assert_eq!(tree.rows[2].depth, 1);
    assert_eq!(tree.rows[2].command, "bash");
}

#[test]
fn test_build_tree_no_children() {
    let procs = vec![p(200, 1, &["bash"], 10)];
    let tree = build_tree(&procs, 100);
    assert!(tree.rows.is_empty());
    assert_eq!(tree.hidden, 0);
}

#[test]
fn test_build_tree_max_rows() {
    let mut procs = Vec::new();
    for pid in 1..=10 {
        procs.push(p(pid, 100, &[&pid.to_string()], pid as u64));
    }
    let tree = build_tree(&procs, 100);

    assert_eq!(tree.rows.len(), MAX_ROWS);
    assert_eq!(tree.hidden, 2);
    // First row should be the one with 10s
    assert_eq!(tree.rows[0].command, "10");
}

#[test]
fn test_build_tree_cycle() {
    let procs = vec![p(5, 100, &["a"], 5), p(6, 7, &["b"], 5), p(7, 6, &["c"], 5)];
    let tree = build_tree(&procs, 100);

    assert_eq!(tree.rows.len(), 1);
    assert_eq!(tree.rows[0].command, "a");
    assert_eq!(tree.hidden, 0);
}

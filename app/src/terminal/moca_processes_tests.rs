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

    // Los hermanos van del más nuevo al más antiguo.
    assert_eq!(tree.rows[1].depth, 1);
    assert_eq!(tree.rows[1].command, "bash");

    assert_eq!(tree.rows[2].depth, 1);
    assert_eq!(tree.rows[2].command, "opencode");
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
    // La primera fila es la más nueva (1 s).
    assert_eq!(tree.rows[0].command, "1");
}

#[test]
fn test_build_tree_cycle() {
    let procs = vec![p(5, 100, &["a"], 5), p(6, 7, &["b"], 5), p(7, 6, &["c"], 5)];
    let tree = build_tree(&procs, 100);

    assert_eq!(tree.rows.len(), 1);
    assert_eq!(tree.rows[0].command, "a");
    assert_eq!(tree.hidden, 0);
}

#[test]
fn test_build_tree_hides_background_processes() {
    let procs = vec![
        p(200, 100, &["claude"], 300),
        p(210, 200, &["tabularis", "--mcp"], 300),
        p(211, 210, &["postgresql-plugin"], 300),
        p(220, 200, &["caffeinate", "-i", "-t", "300"], 60),
        p(230, 200, &["opencode", "run"], 5),
    ];
    let tree = build_tree(&procs, 100);
    let commands: Vec<&str> = tree.rows.iter().map(|row| row.command.as_str()).collect();
    assert_eq!(commands, vec!["claude", "opencode run"]);
    assert_eq!(tree.hidden, 0);
}

#[test]
fn test_format_command_extracts_agent_shell_command() {
    let script = "source /Users/x/.claude/shell-snapshots/snapshot-zsh-1.sh 2>/dev/null || true \
        && eval 'cd /tmp/moca-qa-demo && ./progreso.sh' < /dev/null && pwd -P >| /tmp/cwd";
    let process = p(300, 200, &["/bin/zsh", "-c", script], 3);
    assert_eq!(format_command(&process), "$ ./progreso.sh");

    let plain = p(301, 200, &["/bin/zsh", "-c", "echo hola"], 3);
    assert_eq!(format_command(&plain), "zsh -c echo hola");
}

#[test]
fn test_build_tree_hides_sleep() {
    let procs = vec![
        p(200, 100, &["claude"], 60),
        p(201, 200, &["sleep", "2"], 1),
    ];
    let tree = build_tree(&procs, 100);
    let commands: Vec<&str> = tree.rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(commands, vec!["claude"]);
}

#[test]
fn test_format_command_script_basename() {
    let proc1 = p(
        1,
        0,
        &[
            "/bin/zsh",
            "/Users/x/.claude/plugins/cache/moca/delegar/run.sh",
            "ollama/glm-5.3:cloud",
            "p.txt",
        ],
        1,
    );
    assert_eq!(
        format_command(&proc1),
        "zsh run.sh ollama/glm-5.3:cloud p.txt"
    );

    let proc2 = p(2, 0, &["python3", "/tmp/a/gen_tests.py"], 1);
    assert_eq!(format_command(&proc2), "python3 gen_tests.py");
}

#[test]
fn test_format_command_opencode_model() {
    let proc1 = p(
        1,
        0,
        &[
            "/opt/homebrew/bin/opencode",
            "run",
            "--pure",
            "--agent",
            "ejecutor",
            "-m",
            "ollama/glm-5.3:cloud",
            "texto largo",
        ],
        1,
    );
    assert_eq!(format_command(&proc1), "opencode · glm-5.3");

    let proc2 = p(
        2,
        0,
        &["opencode", "run", "--model", "opencode-go/glm-5.3"],
        1,
    );
    assert_eq!(format_command(&proc2), "opencode · glm-5.3");

    let proc3 = p(3, 0, &["opencode", "run"], 1);
    assert_eq!(format_command(&proc3), "opencode run");
}

#[test]
fn test_format_command_strips_leading_assignments() {
    let script = "source /x/snap.sh && eval 'cd /tmp/d && s=$(date +%s); ./progreso.sh; echo hecho' < /dev/null && pwd -P";
    let proc = p(1, 0, &["/bin/zsh", "-c", script], 1);
    assert_eq!(format_command(&proc), "$ ./progreso.sh; echo hecho");
}

#[test]
fn test_build_tree_labels_subagent() {
    let procs = vec![
        p(200, 100, &["claude"], 60),
        p(
            210,
            200,
            &[
                "/bin/zsh",
                "-c",
                "source /x/snap.sh && eval 's=$(date +%s); P=/tmp/x; \"$P/run.sh\" m p l' < /dev/null && pwd -P",
            ],
            5,
        ),
        p(211, 210, &["/bin/zsh", "/x/delegar/run.sh", "m"], 5),
        p(
            212,
            211,
            &["opencode", "run", "-m", "ollama/glm-5.3:cloud"],
            5,
        ),
    ];
    let tree = build_tree(&procs, 100);
    let commands: Vec<&str> = tree.rows.iter().map(|r| r.command.as_str()).collect();
    let depths: Vec<usize> = tree.rows.iter().map(|r| r.depth).collect();
    assert_eq!(
        commands,
        vec!["claude", "subagente", "zsh run.sh m", "opencode · glm-5.3"]
    );
    assert_eq!(depths, vec![0, 1, 2, 3]);
}

#[test]
fn test_build_tree_claude_command_without_agent_is_not_subagent() {
    let procs = vec![
        p(200, 100, &["claude"], 60),
        p(
            210,
            200,
            &[
                "/bin/zsh",
                "-c",
                "source /x && eval './progreso.sh' < /dev/null",
            ],
            5,
        ),
        p(211, 210, &["bash", "./progreso.sh"], 5),
    ];
    let tree = build_tree(&procs, 100);
    let commands: Vec<&str> = tree.rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(
        commands,
        vec!["claude", "$ ./progreso.sh", "bash progreso.sh"]
    );
}

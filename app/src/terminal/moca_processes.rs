//! moca: árbol de subprocesos de una pestaña, para la tarjeta de detalle.
//!
//! Muestra qué procesos cuelgan del shell (p. ej. `claude` → `opencode run`), cuánto
//! llevan corriendo y su uso de CPU, para saber si un agente sigue trabajando.
//! La lectura de la tabla de procesos (`sysinfo`) solo existe fuera de wasm.

use std::collections::HashMap;
use std::time::Duration;

use crate::terminal::cli_agent_sessions::moca_activity::strip_cd_prefix;

/// Máximo de filas en la tarjeta; el resto se resume como "+N más".
pub const MAX_ROWS: usize = 8;

/// Largo máximo del comando mostrado por fila.
const MAX_COMMAND_CHARS: usize = 40;

/// Un proceso leído del sistema, sin depender de `sysinfo` (para poder probarlo).
#[derive(Debug, Clone, PartialEq)]
pub struct RawProcess {
    pub pid: u32,
    pub parent: Option<u32>,
    /// Línea de comandos completa (`argv`).
    pub cmd: Vec<String>,
    /// Nombre del ejecutable, si `cmd` viene vacío.
    pub name: String,
    pub run_time: Duration,
    /// Uso de CPU en % (100 = un núcleo completo); `None` si aún no hay dos muestras.
    pub cpu_percent: Option<f32>,
}

/// Una fila lista para mostrar.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRow {
    /// Profundidad bajo el shell: 0 = hijo directo del shell.
    pub depth: usize,
    pub command: String,
    pub run_time: Duration,
    pub cpu_percent: Option<f32>,
}

/// Resultado para la tarjeta: filas en orden de árbol y cuántas quedaron fuera.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcessTree {
    pub rows: Vec<ProcessRow>,
    pub hidden: usize,
}

/// Arma el árbol de descendientes de `shell_pid` (sin incluir el shell), en orden
/// de recorrido en profundidad. Los hermanos van del más nuevo al más antiguo, porque
/// lo recién lanzado es lo que el agente está haciendo ahora; los procesos de fondo
/// (ver [`is_background`]) se omiten con todo su subárbol.
pub fn build_tree(processes: &[RawProcess], shell_pid: u32) -> ProcessTree {
    let mut children: HashMap<u32, Vec<&RawProcess>> = HashMap::new();
    for process in processes {
        if let Some(parent) = process.parent {
            children.entry(parent).or_default().push(process);
        }
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|a, b| a.run_time.cmp(&b.run_time).then(b.pid.cmp(&a.pid)));
    }

    let mut all_rows = Vec::new();
    let mut stack: Vec<(u32, usize)> = vec![(shell_pid, 0)];
    // Evita ciclos si la tabla de procesos llega inconsistente (pids reciclados).
    let mut visited = std::collections::HashSet::new();
    while let Some((pid, depth)) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        if let Some(kids) = children.get(&pid) {
            for kid in kids.iter().rev().filter(|kid| !is_background(kid)) {
                stack.push((kid.pid, depth + 1));
            }
        }
        if pid == shell_pid {
            continue;
        }
        if let Some(process) = processes.iter().find(|p| p.pid == pid) {
            // Un comando de Claude que lanza otro agente (p. ej. delegar → opencode) es
            // ilegible como comando; lo que importa es que ahí trabaja un subagente.
            let command =
                if agent_shell_command(process).is_some() && subtree_has_agent(&children, pid) {
                    "subagente".to_owned()
                } else {
                    format_command(process)
                };
            all_rows.push(ProcessRow {
                depth: depth - 1,
                command,
                run_time: process.run_time,
                cpu_percent: process.cpu_percent,
            });
        }
    }

    let hidden = all_rows.len().saturating_sub(MAX_ROWS);
    all_rows.truncate(MAX_ROWS);
    ProcessTree {
        rows: all_rows,
        hidden,
    }
}

/// Agentes de código que pueden correr como subproceso de otro agente.
const AGENT_PROGRAMS: &[&str] = &["opencode", "claude", "codex", "gemini", "aider", "goose"];

/// Intérpretes cuyo primer argumento suele ser la ruta de un script.
const INTERPRETERS: &[&str] = &["zsh", "bash", "sh", "python", "python3", "node", "ruby"];

fn subtree_has_agent(children: &HashMap<u32, Vec<&RawProcess>>, pid: u32) -> bool {
    let mut pending = vec![pid];
    let mut seen = std::collections::HashSet::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        for kid in children.get(&current).into_iter().flatten() {
            if AGENT_PROGRAMS.contains(&program_name(kid).as_str()) {
                return true;
            }
            pending.push(kid.pid);
        }
    }
    false
}

/// Procesos que no son "trabajo" del agente: servidores MCP, `caffeinate` y esperas
/// (`sleep`) de los scripts.
pub fn is_background(process: &RawProcess) -> bool {
    let program = program_name(process);
    program == "caffeinate"
        || program == "sleep"
        || program.contains("mcp")
        || process
            .cmd
            .iter()
            .skip(1)
            .any(|arg| arg == "--mcp" || arg == "mcp")
}

fn program_name(process: &RawProcess) -> String {
    process
        .cmd
        .first()
        .map(|argv0| argv0.rsplit(['/', '\\']).next().unwrap_or(argv0).to_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| process.name.clone())
}

/// Claude corre cada comando como `zsh -c "source <snapshot> && … && eval '<cmd>' …"`;
/// devuelve `<cmd>` sin los `cd dir &&` del inicio.
fn agent_shell_command(process: &RawProcess) -> Option<String> {
    if !matches!(program_name(process).as_str(), "zsh" | "bash" | "sh") {
        return None;
    }
    let script = process
        .cmd
        .iter()
        .position(|arg| arg == "-c")
        .and_then(|i| process.cmd.get(i + 1))?;
    let start = script.find("eval '")? + "eval '".len();
    let rest = &script[start..];
    let end = rest.find("' < /dev/null").or_else(|| rest.rfind('\''))?;
    let command = strip_leading_assignments(strip_cd_prefix(rest[..end].trim()));
    (!command.is_empty()).then(|| command.to_owned())
}

/// `s=$(date +%s); ./progreso.sh` → `./progreso.sh`: las asignaciones no dicen qué se corre.
fn strip_leading_assignments(command: &str) -> &str {
    let mut rest = command;
    while let Some((head, tail)) = rest.split_once(';') {
        if !is_assignment(head.trim()) {
            break;
        }
        rest = tail.trim_start();
    }
    rest
}

fn is_assignment(segment: &str) -> bool {
    let Some((name, _)) = segment.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `-m ollama/glm-5.3:cloud` → `glm-5.3`.
fn opencode_model(process: &RawProcess) -> Option<String> {
    let model = process
        .cmd
        .iter()
        .position(|arg| arg == "-m" || arg == "--model")
        .and_then(|i| process.cmd.get(i + 1))?;
    let name = model.rsplit('/').next().unwrap_or(model);
    let name = name.split(':').next().unwrap_or(name);
    (!name.is_empty()).then(|| name.to_owned())
}

/// `["/opt/homebrew/bin/opencode", "run", "--pure"]` → `opencode run --pure`.
pub fn format_command(process: &RawProcess) -> String {
    if let Some(command) = agent_shell_command(process) {
        return truncate(&format!("$ {command}"));
    }
    let program = program_name(process);
    if program == "opencode"
        && let Some(model) = opencode_model(process)
    {
        return format!("opencode · {model}");
    }
    let mut args: Vec<String> = process.cmd.iter().skip(1).cloned().collect();
    if INTERPRETERS.contains(&program.as_str())
        && let Some(script) = args.first_mut()
        && !script.starts_with('-')
    {
        *script = script
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(script)
            .to_owned();
    }
    let mut text = program;
    for arg in &args {
        if text.chars().count() >= MAX_COMMAND_CHARS {
            break;
        }
        text.push(' ');
        text.push_str(arg);
    }
    truncate(&text)
}

fn truncate(text: &str) -> String {
    if text.chars().count() > MAX_COMMAND_CHARS {
        let mut cut: String = text.chars().take(MAX_COMMAND_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        text.to_owned()
    }
}

/// `12 s`, `4 min`, `2 h`.
pub fn format_run_time(run_time: Duration) -> String {
    let secs = run_time.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else {
        format!("{} h", secs / 3600)
    }
}

/// `Some(85.4)` → `85% CPU`; `None` → cadena vacía.
pub fn format_cpu(cpu_percent: Option<f32>) -> String {
    cpu_percent
        .map(|cpu| format!("{}% CPU", cpu.round() as i64))
        .unwrap_or_default()
}

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        /// En wasm no hay tabla de procesos local.
        pub fn process_tree(_shell_pid: u32) -> ProcessTree {
            ProcessTree::default()
        }
    } else {
        use std::sync::LazyLock;

        use instant::Instant;
        use parking_lot::Mutex;
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        /// Una lectura de la tabla es cara; la tarjeta se repinta seguido.
        const CACHE_FOR: Duration = Duration::from_secs(2);

        struct Monitor {
            system: System,
            /// Tiempo de CPU acumulado y hora de la muestra anterior, por pid.
            previous_cpu: HashMap<u32, (u64, Instant)>,
            cache: HashMap<u32, (Instant, ProcessTree)>,
        }

        static MONITOR: LazyLock<Mutex<Monitor>> = LazyLock::new(|| {
            Mutex::new(Monitor {
                system: System::new(),
                previous_cpu: HashMap::new(),
                cache: HashMap::new(),
            })
        });

        /// Árbol de subprocesos del shell `shell_pid`, con caché de [`CACHE_FOR`].
        pub fn process_tree(shell_pid: u32) -> ProcessTree {
            let now = Instant::now();
            let mut monitor = MONITOR.lock();
            if let Some((at, tree)) = monitor.cache.get(&shell_pid)
                && now.saturating_duration_since(*at) < CACHE_FOR
            {
                return tree.clone();
            }

            monitor.system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true, /* remove_dead_processes */
                ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet).with_cpu(),
            );

            let Monitor { system, previous_cpu, cache } = &mut *monitor;
            let raw: Vec<RawProcess> = system
                .processes()
                .iter()
                .map(|(pid, process)| {
                    let pid = pid.as_u32();
                    let cpu_ms = process.accumulated_cpu_time();
                    let cpu_percent = previous_cpu.get(&pid).and_then(|(prev_ms, prev_at)| {
                        let wall_ms = now.saturating_duration_since(*prev_at).as_millis() as f32;
                        (wall_ms > 0.).then(|| {
                            cpu_ms.saturating_sub(*prev_ms) as f32 / wall_ms * 100.
                        })
                    });
                    RawProcess {
                        pid,
                        parent: process.parent().map(Pid::as_u32),
                        cmd: process
                            .cmd()
                            .iter()
                            .map(|arg| arg.to_string_lossy().into_owned())
                            .collect(),
                        name: process.name().to_string_lossy().into_owned(),
                        run_time: Duration::from_secs(process.run_time()),
                        cpu_percent,
                    }
                })
                .collect();

            let tree = build_tree(&raw, shell_pid);

            // Solo guarda CPU de los procesos del árbol: la tabla completa es grande.
            let in_tree: std::collections::HashSet<u32> =
                descendant_pids(&raw, shell_pid).into_iter().collect();
            previous_cpu.retain(|pid, _| in_tree.contains(pid));
            for process in system.processes().iter().filter(|(pid, _)| in_tree.contains(&pid.as_u32())) {
                previous_cpu.insert(process.0.as_u32(), (process.1.accumulated_cpu_time(), now));
            }
            cache.insert(shell_pid, (now, tree.clone()));
            tree
        }

        fn descendant_pids(processes: &[RawProcess], root: u32) -> Vec<u32> {
            let mut found = vec![root];
            let mut i = 0;
            while i < found.len() {
                let parent = found[i];
                found.extend(
                    processes
                        .iter()
                        .filter(|p| p.parent == Some(parent) && !found.contains(&p.pid))
                        .map(|p| p.pid)
                        .collect::<Vec<_>>(),
                );
                i += 1;
            }
            found
        }
    }
}

#[cfg(test)]
#[path = "moca_processes_tests.rs"]
mod tests;

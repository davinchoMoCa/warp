//! moca: visibilidad de lo que hace un agente CLI en cada pestaña.
//!
//! Guarda la última herramienta que empezó el agente, el avance de su lista de
//! tareas y la hora del último evento, para mostrar en la pestaña una línea como
//! `3/7 · Bash: npm test · hace 12 s` o avisar `¿trabado?` si lleva mucho rato
//! sin actividad mientras dice estar trabajando.

use std::time::Duration;

use instant::Instant;

use super::CLIAgentSessionStatus;
use super::event::{CLIAgentEvent, CLIAgentEventType};

/// Sin eventos durante este tiempo mientras está "en progreso" = posiblemente trabado.
pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// Cada cuánto se repinta la pestaña para actualizar el "hace X".
pub const REFRESH_EVERY: Duration = Duration::from_secs(15);

/// Largo máximo del detalle de la actividad (comando, archivo, etc.).
const MAX_DETAIL_CHARS: usize = 48;

#[derive(Debug, Clone, Default)]
pub struct MocaActivity {
    /// Última herramienta que empezó el agente, ya formateada (`Bash: npm test`).
    pub current: Option<String>,
    /// Avance de la lista de tareas: `(hechas, total)`.
    pub tasks: Option<(u32, u32)>,
    /// Hora del último evento recibido del agente.
    pub last_event_at: Option<Instant>,
}

/// Línea lista para mostrar en la pestaña.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MocaActivityLine {
    pub text: String,
    /// `true` si lleva más de [`STALE_AFTER`] sin actividad estando en progreso.
    pub stale: bool,
}

impl MocaActivity {
    /// Registra un evento del agente.
    pub fn record(&mut self, event: &CLIAgentEvent, now: Instant) {
        self.last_event_at = Some(now);
        if let Some(tasks) = event.payload.tasks {
            self.tasks = Some(tasks);
        }
        match event.event {
            CLIAgentEventType::ToolStart => {
                self.current =
                    event.payload.tool_name.as_deref().map(|tool| {
                        format_activity(tool, event.payload.tool_input_preview.as_deref())
                    });
            }
            // Un turno nuevo o un final dejan sin "herramienta actual".
            CLIAgentEventType::PromptSubmit
            | CLIAgentEventType::Stop
            | CLIAgentEventType::StopFailure => self.current = None,
            CLIAgentEventType::SessionStart
            | CLIAgentEventType::ToolComplete
            | CLIAgentEventType::PermissionRequest
            | CLIAgentEventType::PermissionReplied
            | CLIAgentEventType::QuestionAsked
            | CLIAgentEventType::IdlePrompt
            | CLIAgentEventType::TasksProgress
            | CLIAgentEventType::Unknown(_) => {}
        }
    }

    /// Arma la línea para la pestaña, o `None` si todavía no hubo eventos.
    pub fn line(&self, status: &CLIAgentSessionStatus, now: Instant) -> Option<MocaActivityLine> {
        let last = self.last_event_at?;
        let elapsed = now.saturating_duration_since(last);
        let stale = matches!(status, CLIAgentSessionStatus::InProgress) && elapsed >= STALE_AFTER;

        // El tiempo va antes que la actividad: la pestaña es angosta y el detalle
        // (comando, archivo) es lo que se puede cortar con "…".
        let mut parts: Vec<String> = Vec::new();
        if stale {
            parts.push("¿trabado?".to_owned());
        }
        let status_word = match status {
            CLIAgentSessionStatus::InProgress => None,
            CLIAgentSessionStatus::Blocked { .. } => Some("esperando respuesta"),
            CLIAgentSessionStatus::Success => Some("listo"),
            CLIAgentSessionStatus::Failed { .. } => Some("falló"),
            CLIAgentSessionStatus::Cancelled => Some("cancelado"),
        };
        if let Some(word) = status_word {
            parts.push(word.to_owned());
        }
        parts.push(format_ago(elapsed));
        if let Some((done, total)) = self.tasks.filter(|(_, total)| *total > 0) {
            parts.push(format!("{done}/{total}"));
        }
        if status_word.is_none()
            && let Some(current) = &self.current
        {
            parts.push(current.clone());
        }

        Some(MocaActivityLine {
            text: parts.join(" · "),
            stale,
        })
    }
}

/// `Bash` + `npm test --watch` → `Bash: npm test --watch`; rutas se reducen al nombre de archivo.
pub fn format_activity(tool: &str, detail: Option<&str>) -> String {
    let Some(detail) = detail.map(str::trim).filter(|d| !d.is_empty()) else {
        return tool.to_owned();
    };
    let first_line =
        strip_leading_assignments(strip_cd_prefix(detail.lines().next().unwrap_or(detail)));
    let detail = if first_line.starts_with('/') || first_line.contains(":\\") {
        first_line
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(first_line)
    } else {
        first_line
    };
    format!("{tool}: {}", truncate(detail, MAX_DETAIL_CHARS))
}

/// Quita los `cd <dir> &&` del inicio: Claude los antepone a casi todos sus comandos.
pub(crate) fn strip_cd_prefix(mut command: &str) -> &str {
    while command.starts_with("cd ")
        && let Some((_, rest)) = command.split_once("&&")
    {
        command = rest.trim_start();
    }
    command
}

/// `s=$(date +%s); ./progreso.sh` → `./progreso.sh`: las asignaciones no dicen qué se corre.
pub(crate) fn strip_leading_assignments(command: &str) -> &str {
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

/// `12 s` → `hace 12 s`, `3 min`, `2 h`.
pub fn format_ago(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("hace {secs} s")
    } else if secs < 3600 {
        format!("hace {} min", secs / 60)
    } else {
        format!("hace {} h", secs / 3600)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "moca_activity_tests.rs"]
mod tests;

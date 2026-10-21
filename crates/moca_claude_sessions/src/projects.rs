//! Agrupación del tablero: cliente → proyecto (carpeta) → sesiones.

use std::path::PathBuf;

use chrono::NaiveDateTime;

use crate::{ClaudeSession, Placement, StatusFilter, is_empty_session, match_session};

/// Nombre de la sección de las sesiones abiertas desde `~` (o la raíz de un cliente) que no
/// tocaron un proyecto claro.
pub const UNASSIGNED: &str = "Sin proyecto";

#[derive(Clone, Debug)]
pub struct ProjectCard {
    pub dir: PathBuf,
    /// Nombre de la carpeta (`elbarretal`).
    pub name: String,
    /// Más reciente primero.
    pub sessions: Vec<ClaudeSession>,
    /// Tarjeta de una sola sesión sin proyecto.
    pub unassigned: bool,
}

impl ProjectCard {
    fn latest(&self) -> Option<NaiveDateTime> {
        self.sessions.first().and_then(|s| s.last_activity)
    }
}

#[derive(Clone, Debug)]
pub struct ClientSection {
    pub client: String,
    /// Más reciente primero.
    pub projects: Vec<ProjectCard>,
}

fn passes_status(session: &ClaudeSession, status: StatusFilter, now: NaiveDateTime) -> bool {
    match status {
        StatusFilter::All => true,
        StatusFilter::Active => session.active,
        StatusFilter::Today => session
            .last_activity
            .is_some_and(|last| last.date() == now.date()),
    }
}

/// Arma las secciones del tablero. Una tarjeta entra si alguna de sus sesiones pasa el filtro
/// de estado y la búsqueda (que también mira el nombre del proyecto); la tarjeta muestra solo
/// esas sesiones. "Sin proyecto" va al final y tiene una tarjeta por sesión.
pub fn group_projects(
    sessions: &[ClaudeSession],
    query: &str,
    status: StatusFilter,
    hide_empty: bool,
    now: NaiveDateTime,
) -> Vec<ClientSection> {
    let mut sections: Vec<ClientSection> = Vec::new();
    let mut unassigned: Vec<ProjectCard> = Vec::new();

    for session in sessions {
        if session.archived || (hide_empty && is_empty_session(session)) {
            continue;
        }
        if !passes_status(session, status, now) {
            continue;
        }
        let is_unassigned = session.placement == Placement::Unassigned;
        let client = if is_unassigned {
            UNASSIGNED.to_string()
        } else {
            session.client.clone()
        };
        let name = session
            .project_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "~".to_string());
        let matches = match_session(session, &client, query).is_some()
            || query
                .split_whitespace()
                .all(|word| crate::normalized_contains(&name, word));
        if !matches {
            continue;
        }

        if is_unassigned {
            unassigned.push(ProjectCard {
                dir: session.project_dir.clone(),
                name,
                sessions: vec![session.clone()],
                unassigned: true,
            });
            continue;
        }
        let section = match sections.iter_mut().position(|s| s.client == client) {
            Some(index) => &mut sections[index],
            None => {
                sections.push(ClientSection {
                    client,
                    projects: Vec::new(),
                });
                sections.last_mut().expect("recién agregada")
            }
        };
        match section
            .projects
            .iter_mut()
            .find(|card| card.dir == session.project_dir)
        {
            Some(card) => card.sessions.push(session.clone()),
            None => section.projects.push(ProjectCard {
                dir: session.project_dir.clone(),
                name,
                sessions: vec![session.clone()],
                unassigned: false,
            }),
        }
    }

    for section in &mut sections {
        for card in &mut section.projects {
            card.sessions
                .sort_by_key(|s| std::cmp::Reverse(s.last_activity));
        }
        section
            .projects
            .sort_by_key(|card| std::cmp::Reverse(card.latest()));
    }
    sections.sort_by_key(|section| {
        std::cmp::Reverse(section.projects.first().and_then(ProjectCard::latest))
    });
    if !unassigned.is_empty() {
        unassigned.sort_by_key(|card| std::cmp::Reverse(card.latest()));
        sections.push(ClientSection {
            client: UNASSIGNED.to_string(),
            projects: unassigned,
        });
    }
    sections
}

/// Sesiones activas para "Continuar donde lo dejaste": más reciente primero, sin vacías.
pub fn continue_sessions(sessions: &[ClaudeSession], limit: usize) -> Vec<ClaudeSession> {
    let mut active: Vec<ClaudeSession> = sessions
        .iter()
        .filter(|s| s.active && !s.archived && !is_empty_session(s))
        .cloned()
        .collect();
    active.sort_by_key(|s| std::cmp::Reverse(s.last_activity));
    active.truncate(limit);
    active
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(value: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn session(
        id: &str,
        title: &str,
        cwd: &str,
        project: &str,
        placement: Placement,
        when: &str,
        active: bool,
    ) -> ClaudeSession {
        let client = project
            .strip_prefix("/Users/t/Documents/Moca/")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("Personal")
            .to_string();
        ClaudeSession {
            client,
            id: id.to_string(),
            title: title.to_string(),
            cwd: PathBuf::from(cwd),
            project_dir: PathBuf::from(project),
            placement,
            last_activity: Some(dt(when)),
            messages: 10,
            active,
            from_desktop_app: false,
            archived: false,
        }
    }

    fn sample() -> Vec<ClaudeSession> {
        let m = "/Users/t/Documents/Moca";
        vec![
            session(
                "a",
                "PuntoDeVenta",
                &format!("{m}/Manifesto/elbarretal"),
                &format!("{m}/Manifesto/elbarretal"),
                Placement::Opened,
                "2026-10-21 11:00:00",
                true,
            ),
            session(
                "b",
                "Inventario",
                &format!("{m}/Manifesto/elbarretal"),
                &format!("{m}/Manifesto/elbarretal"),
                Placement::Opened,
                "2026-10-19 11:00:00",
                false,
            ),
            session(
                "c",
                "MocaWarp",
                "/Users/t",
                &format!("{m}/Personal/warp"),
                Placement::Inferred,
                "2026-10-21 12:00:00",
                true,
            ),
            session(
                "d",
                "Red interna",
                "/Users/t",
                "/Users/t",
                Placement::Unassigned,
                "2026-10-20 07:00:00",
                false,
            ),
            session(
                "e",
                "Odoo",
                &format!("{m}/NeoNet/odoo"),
                &format!("{m}/NeoNet/odoo"),
                Placement::Opened,
                "2026-10-10 07:00:00",
                false,
            ),
        ]
    }

    #[test]
    fn agrupa_por_cliente_y_proyecto() {
        let now = dt("2026-10-21 12:30:00");
        let sections = group_projects(&sample(), "", StatusFilter::All, true, now);
        let clients: Vec<&str> = sections.iter().map(|s| s.client.as_str()).collect();
        assert_eq!(clients, ["Personal", "Manifesto", "NeoNet", UNASSIGNED]);
        let barretal = &sections[1].projects[0];
        assert_eq!(barretal.name, "elbarretal");
        let ids: Vec<&str> = barretal.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(sections[3].projects[0].unassigned);
    }

    #[test]
    fn busca_por_nombre_de_proyecto_y_filtra_estado() {
        let now = dt("2026-10-21 12:30:00");
        let found = group_projects(&sample(), "warp", StatusFilter::All, true, now);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].projects[0].sessions[0].id, "c");

        let active = group_projects(&sample(), "", StatusFilter::Active, true, now);
        let total: usize = active
            .iter()
            .flat_map(|s| &s.projects)
            .map(|p| p.sessions.len())
            .sum();
        assert_eq!(total, 2);

        let today = group_projects(&sample(), "", StatusFilter::Today, true, now);
        assert!(today.iter().all(|s| s.client != "NeoNet"));
    }

    #[test]
    fn continuar_solo_activas_recientes() {
        let ids: Vec<String> = continue_sessions(&sample(), 4)
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["c", "a"]);
    }
}

//! Lógica pura para listar, buscar y agrupar sesiones de Claude Code
//! a partir del JSON de `sesiones-claude --json`.

use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeSession {
    pub id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub last_activity: Option<NaiveDateTime>,
    pub messages: u32,
    pub active: bool,
    pub from_desktop_app: bool,
    pub archived: bool,
}

#[derive(Deserialize)]
struct RawOutput {
    sesiones: Vec<RawSession>,
}

#[derive(Deserialize)]
struct RawSession {
    id: String,
    origen: Option<String>,
    titulo: Option<String>,
    carpeta: Option<String>,
    ultima: Option<String>,
    mensajes: Option<u32>,
    archivada: Option<bool>,
    activa: Option<bool>,
    transcripcion: Option<String>,
}

/// Parsea la salida de `sesiones-claude --json`. Descarta las sesiones que no se pueden
/// retomar con `claude --resume`: sin `carpeta` o sin `transcripcion`.
pub fn parse_sessions_json(json: &str) -> Result<Vec<ClaudeSession>, serde_json::Error> {
    let raw: RawOutput = serde_json::from_str(json)?;
    Ok(raw
        .sesiones
        .into_iter()
        .filter(|s| s.carpeta.is_some() && s.transcripcion.is_some())
        .map(|s| ClaudeSession {
            id: s.id,
            title: s.titulo.unwrap_or_else(|| "(sin título)".to_string()),
            cwd: PathBuf::from(s.carpeta.unwrap_or_default()),
            last_activity: s.ultima.as_deref().and_then(parse_datetime),
            messages: s.mensajes.unwrap_or(0),
            active: s.activa.unwrap_or(false),
            from_desktop_app: s
                .origen
                .as_deref()
                .is_some_and(|o| o.starts_with("Desktop")),
            archived: s.archivada.unwrap_or(false),
        })
        .collect())
}

fn parse_datetime(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

/// Cliente de una carpeta: el primer componente después de `<home>/Documents/Moca/`
/// (p. ej. `/Users/x/Documents/Moca/Manifesto/elbarretal` -> "Manifesto"); cualquier otra ruta -> "Personal".
pub fn client_for(cwd: &Path, home: &Path) -> String {
    let base = home.join("Documents").join("Moca");
    cwd.strip_prefix(&base)
        .ok()
        .and_then(|rest| rest.components().next())
        .and_then(|c| c.as_os_str().to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Personal")
        .to_string()
}

/// Sesión "vacía": menos de 3 mensajes o título "(sin título)".
pub fn is_empty_session(session: &ClaudeSession) -> bool {
    session.messages < 3 || session.title == "(sin título)"
}

/// Minúsculas y sin acentos, 1 carácter -> 1 carácter, para que los índices de carácter
/// del texto normalizado coincidan con los del original.
fn normalize(text: &str) -> Vec<char> {
    text.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            match lower {
                'á' | 'à' | 'ä' | 'â' => 'a',
                'é' | 'è' | 'ë' | 'ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' => 'o',
                'ú' | 'ù' | 'û' => 'u',
                'ü' => 'u',
                'ñ' => 'n',
                other => other,
            }
        })
        .collect()
}

fn contains(haystack: &[char], needle: &[char]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn occurrence_indices(haystack: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let mut indices = Vec::new();
    for (start, window) in haystack.windows(needle.len()).enumerate() {
        if window == needle {
            indices.extend(start..start + needle.len());
        }
    }
    indices
}

/// Búsqueda: cada palabra del query debe aparecer en el título, la ruta o el cliente,
/// sin distinguir mayúsculas ni acentos. Devuelve los índices de carácter del título
/// que coinciden con alguna palabra (sin repetidos, ordenados), o None si no coincide.
pub fn match_session(session: &ClaudeSession, client: &str, query: &str) -> Option<Vec<usize>> {
    let title = normalize(&session.title);
    let cwd = normalize(&session.cwd.to_string_lossy());
    let client = normalize(client);

    let mut indices = Vec::new();
    for word in query.split_whitespace() {
        let word = normalize(word);
        if !contains(&title, &word) && !contains(&cwd, &word) && !contains(&client, &word) {
            return None;
        }
        indices.extend(occurrence_indices(&title, &word));
    }
    indices.sort_unstable();
    indices.dedup();
    Some(indices)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusFilter {
    All,
    Active,
    Today,
}

pub struct SessionGroup {
    pub client: String,
    /// Sesión + índices resaltados del título.
    pub sessions: Vec<(ClaudeSession, Vec<usize>)>,
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

/// Filtra (vacías si `hide_empty`, archivadas siempre fuera, `status`, `client_filter` exacto
/// si Some, y `query`) y agrupa por cliente. Dentro del grupo: más reciente primero (None al
/// final). Grupos ordenados por la sesión más reciente de cada uno. `now` define "Today".
pub fn filter_and_group(
    sessions: &[ClaudeSession],
    home: &Path,
    query: &str,
    status: StatusFilter,
    client_filter: Option<&str>,
    hide_empty: bool,
    now: NaiveDateTime,
) -> Vec<SessionGroup> {
    let mut groups: Vec<SessionGroup> = Vec::new();
    for session in sessions {
        if session.archived || (hide_empty && is_empty_session(session)) {
            continue;
        }
        if !passes_status(session, status, now) {
            continue;
        }
        let client = client_for(&session.cwd, home);
        if client_filter.is_some_and(|c| c != client) {
            continue;
        }
        let Some(indices) = match_session(session, &client, query) else {
            continue;
        };
        match groups.iter_mut().find(|g| g.client == client) {
            Some(group) => group.sessions.push((session.clone(), indices)),
            None => groups.push(SessionGroup {
                client,
                sessions: vec![(session.clone(), indices)],
            }),
        }
    }
    for group in &mut groups {
        group
            .sessions
            .sort_by_key(|(s, _)| std::cmp::Reverse(s.last_activity));
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.sessions[0].0.last_activity));
    groups
}

/// Lista de clientes con conteo (sin vacías si hide_empty, sin archivadas), ordenada por
/// actividad más reciente: (cliente, total, activas).
pub fn clients_summary(
    sessions: &[ClaudeSession],
    home: &Path,
    hide_empty: bool,
) -> Vec<(String, usize, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut rows: Vec<(String, usize, usize, Option<NaiveDateTime>)> = Vec::new();
    for session in sessions {
        if session.archived || (hide_empty && is_empty_session(session)) {
            continue;
        }
        let client = client_for(&session.cwd, home);
        match order.iter().position(|c| *c == client) {
            Some(pos) => {
                let row = &mut rows[pos];
                row.1 += 1;
                row.2 += usize::from(session.active);
                row.3 = row.3.max(session.last_activity);
            }
            None => {
                order.push(client.clone());
                rows.push((
                    client,
                    1,
                    usize::from(session.active),
                    session.last_activity,
                ));
            }
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.3));
    rows.into_iter()
        .map(|(client, total, active, _)| (client, total, active))
        .collect()
}

/// "ahora" (<1 min), "hace N min", "hace N h", "hace N d" (<30 d), si no "dd/mm/aaaa". None => "".
pub fn relative_time(last: Option<NaiveDateTime>, now: NaiveDateTime) -> String {
    let Some(last) = last else {
        return String::new();
    };
    let elapsed = now.signed_duration_since(last);
    let minutes = elapsed.num_minutes();
    if minutes < 1 {
        "ahora".to_string()
    } else if minutes < 60 {
        format!("hace {minutes} min")
    } else if elapsed.num_hours() < 24 {
        format!("hace {} h", elapsed.num_hours())
    } else if elapsed.num_days() < 30 {
        format!("hace {} d", elapsed.num_days())
    } else {
        last.format("%d/%m/%Y").to_string()
    }
}

/// Comando para retomar: `claude --resume <id>`.
pub fn resume_command(session: &ClaudeSession) -> String {
    format!("claude --resume {}", session.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/macuser")
    }

    fn dt(value: &str) -> NaiveDateTime {
        parse_datetime(value).unwrap()
    }

    fn session(id: &str, title: &str, cwd: &str, ultima: Option<&str>) -> ClaudeSession {
        ClaudeSession {
            id: id.to_string(),
            title: title.to_string(),
            cwd: PathBuf::from(cwd),
            last_activity: ultima.map(dt),
            messages: 10,
            active: false,
            from_desktop_app: false,
            archived: false,
        }
    }

    #[test]
    fn parse_descarta_sin_carpeta_o_transcripcion() {
        let json = r#"{"sesiones":[
            {"id":"abc","origen":"Terminal","titulo":"MocaWarp","carpeta":"/Users/macuser",
             "ultima":"2026-10-21 12:08:10.367561","mensajes":2795,"mb":34.2,"version":"2.1.289",
             "rama":"HEAD","modelo":null,"archivada":false,"activa":true,"estado":"busy","pid":1,
             "transcripcion":"/tmp/x.jsonl"},
            {"id":"def","origen":"Desktop · Cowork","titulo":"Algo","carpeta":null,"ultima":null,
             "mensajes":null,"mb":null,"version":null,"rama":null,"modelo":null,"archivada":false,
             "activa":false,"estado":null,"pid":null,"transcripcion":null},
            {"id":"ghi","origen":"Desktop","titulo":"Migración corporativa",
             "carpeta":"/Users/macuser/Documents/Moca/Manifesto/elbarretal",
             "ultima":"2026-10-19 13:00:55","mensajes":null,"archivada":true,"activa":false,
             "transcripcion":"/tmp/y.jsonl"}
        ],"skills":[]}"#;
        let sessions = parse_sessions_json(json).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "abc");
        assert!(sessions[0].active);
        assert!(!sessions[0].archived);
        assert!(!sessions[0].from_desktop_app);
        assert_eq!(sessions[0].messages, 2795);
        assert_eq!(
            sessions[0].last_activity,
            Some(dt("2026-10-21 12:08:10.367561"))
        );
        // mensajes null => 0, fecha sin fracción, origen Desktop
        assert_eq!(sessions[1].id, "ghi");
        assert_eq!(sessions[1].messages, 0);
        assert!(sessions[1].from_desktop_app);
        assert!(sessions[1].archived);
        assert_eq!(sessions[1].last_activity, Some(dt("2026-10-19 13:00:55")));
    }

    #[test]
    fn client_for_clasifica_por_carpeta() {
        let home = home();
        assert_eq!(
            client_for(
                Path::new("/Users/macuser/Documents/Moca/Manifesto/elbarretal"),
                &home
            ),
            "Manifesto"
        );
        assert_eq!(
            client_for(
                Path::new("/Users/macuser/Documents/Moca/PeterPeter/api"),
                &home
            ),
            "PeterPeter"
        );
        assert_eq!(
            client_for(Path::new("/Users/macuser/otra"), &home),
            "Personal"
        );
        assert_eq!(
            client_for(Path::new("/Users/macuser/Documents/Moca"), &home),
            "Personal"
        );
        assert_eq!(client_for(Path::new("/"), &home), "Personal");
    }

    #[test]
    fn match_session_por_palabras_y_acentos() {
        let mut s = session(
            "1",
            "ElBarretalPuntoDeVenta",
            "/Users/macuser/Documents/Moca/Manifesto/elbarretal",
            None,
        );
        // Cada palabra debe aparecer; índices de carácter del título.
        assert_eq!(
            match_session(&s, "Manifesto", "punto venta"),
            Some(vec![10, 11, 12, 13, 14, 17, 18, 19, 20, 21])
        );

        s.title = "Migración corporativa".to_string();
        assert_eq!(
            match_session(&s, "Manifesto", "migracion"),
            Some(vec![0, 1, 2, 3, 4, 5, 6, 7, 8])
        );

        // Coincide solo por cliente: sin índices del título.
        assert_eq!(match_session(&s, "Manifesto", "manifesto"), Some(vec![]));

        assert_eq!(match_session(&s, "Manifesto", "xyz"), None);

        // Query vacía coincide con todo.
        assert_eq!(match_session(&s, "Manifesto", "   "), Some(vec![]));
    }

    #[test]
    fn filter_and_group_ordena_y_filtra() {
        let home = home();
        let moca = |name: &str| format!("/Users/macuser/Documents/Moca/{name}/x");
        let vieja = Some("2026-10-19 10:00:00");
        let hoy_a = Some("2026-10-21 09:00:00");
        let hoy_b = Some("2026-10-21 11:00:00");
        let now = dt("2026-10-21 12:00:00");

        let mut s1 = session("1", "Vieja Manifesto", &moca("Manifesto"), vieja);
        let mut s2 = session("2", "Hoy Manifesto", &moca("Manifesto"), hoy_a);
        let mut s3 = session("3", "Hoy Peter", &moca("PeterPeter"), hoy_b);
        let s4 = session("4", "Personal", "/Users/macuser/otra", hoy_a);
        let mut archivada = session("5", "Archivada Peter", &moca("PeterPeter"), hoy_b);
        archivada.archived = true;
        s1.active = true;
        s2.active = false;
        s3.active = true;
        let sessions = vec![s1, s2, s3, s4, archivada];

        // Orden: PeterPeter (11:00) > Manifesto (09:00) > Personal (09:00). Archivada fuera.
        let groups = filter_and_group(&sessions, &home, "", StatusFilter::All, None, false, now);
        let names: Vec<&str> = groups.iter().map(|g| g.client.as_str()).collect();
        assert_eq!(names, ["PeterPeter", "Manifesto", "Personal"]);
        // Dentro del grupo Manifesto: más reciente primero.
        let manifesto = &groups[1];
        let ids: Vec<&str> = manifesto
            .sessions
            .iter()
            .map(|(s, _)| s.id.as_str())
            .collect();
        assert_eq!(ids, ["2", "1"]);

        // Filtro Active.
        let groups = filter_and_group(&sessions, &home, "", StatusFilter::Active, None, false, now);
        let names: Vec<&str> = groups.iter().map(|g| g.client.as_str()).collect();
        assert_eq!(names, ["PeterPeter", "Manifesto"]);
        assert_eq!(groups[0].sessions[0].0.id, "3");
        assert_eq!(groups[1].sessions[0].0.id, "1");

        // Filtro Today: la sesión vieja queda fuera.
        let groups = filter_and_group(&sessions, &home, "", StatusFilter::Today, None, false, now);
        let manifesto = groups.iter().find(|g| g.client == "Manifesto").unwrap();
        assert_eq!(manifesto.sessions.len(), 1);
        assert_eq!(manifesto.sessions[0].0.id, "2");

        // Filtro por cliente exacto.
        let groups = filter_and_group(
            &sessions,
            &home,
            "",
            StatusFilter::All,
            Some("PeterPeter"),
            false,
            now,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].client, "PeterPeter");

        // hide_empty: "(sin título)" y menos de 3 mensajes quedan fuera.
        let mut vacia = session("6", "(sin título)", &moca("Manifesto"), hoy_b);
        vacia.messages = 50;
        let mut pocos = session("7", "Pocos mensajes", &moca("Manifesto"), hoy_b);
        pocos.messages = 2;
        let mut list = sessions.clone();
        list.extend([vacia, pocos]);
        let groups = filter_and_group(&list, &home, "", StatusFilter::All, None, true, now);
        let ids: Vec<&str> = groups
            .iter()
            .flat_map(|g| g.sessions.iter().map(|(s, _)| s.id.as_str()))
            .collect();
        assert!(!ids.contains(&"6"));
        assert!(!ids.contains(&"7"));
    }

    #[test]
    fn clients_summary_conteo_y_orden() {
        let home = home();
        let now_a = Some("2026-10-21 09:00:00");
        let now_b = Some("2026-10-21 11:00:00");
        let mut a = session("1", "A", "/Users/macuser/Documents/Moca/Manifesto/x", now_a);
        let b = session("2", "B", "/Users/macuser/Documents/Moca/Manifesto/y", now_a);
        let c = session(
            "3",
            "C",
            "/Users/macuser/Documents/Moca/PeterPeter/z",
            now_b,
        );
        a.active = true;
        let mut arch = session(
            "4",
            "D",
            "/Users/macuser/Documents/Moca/PeterPeter/w",
            now_b,
        );
        arch.archived = true;
        let summary = clients_summary(&[a, b, c, arch], &home, false);
        assert_eq!(
            summary,
            vec![
                ("PeterPeter".to_string(), 1, 0),
                ("Manifesto".to_string(), 2, 1),
            ]
        );
    }

    #[test]
    fn relative_time_rangos() {
        let now = dt("2026-10-21 12:00:00");
        assert_eq!(relative_time(None, now), "");
        assert_eq!(relative_time(Some(dt("2026-10-21 11:59:40")), now), "ahora");
        assert_eq!(
            relative_time(Some(dt("2026-10-21 11:45:00")), now),
            "hace 15 min"
        );
        assert_eq!(
            relative_time(Some(dt("2026-10-21 05:00:00")), now),
            "hace 7 h"
        );
        assert_eq!(
            relative_time(Some(dt("2026-10-16 12:00:00")), now),
            "hace 5 d"
        );
        assert_eq!(
            relative_time(Some(dt("2026-03-01 12:00:00")), now),
            "01/03/2026"
        );
    }

    #[test]
    fn resume_command_formato() {
        let s = session("abc-123", "X", "/tmp", None);
        assert_eq!(resume_command(&s), "claude --resume abc-123");
    }

    #[test]
    fn is_empty_session_reglas() {
        let mut s = session("1", "Algo", "/tmp", None);
        s.messages = 3;
        assert!(!is_empty_session(&s));
        s.messages = 2;
        assert!(is_empty_session(&s));
        s.messages = 10;
        s.title = "(sin título)".to_string();
        assert!(is_empty_session(&s));
    }
}

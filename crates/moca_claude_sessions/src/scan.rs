//! Lector nativo de las sesiones de Claude Code directo de `~/.claude`,
//! sin pasar por el script de Python.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::{ClaudeSession, Placement};

/// Lo leído de una transcripción; se reutiliza mientras el archivo no cambie.
#[derive(Clone, Debug, Default)]
struct TranscriptInfo {
    messages: u32,
    custom_title: Option<String>,
    ai_title: Option<String>,
    first_user_message: Option<String>,
    cwd: Option<String>,
    entrypoint: Option<String>,
    /// "Cliente/Proyecto" -> apariciones en tool_use.
    paths: HashMap<String, usize>,
    last_activity: Option<chrono::NaiveDateTime>,
}

#[derive(Default)]
pub struct ScanCache {
    entries: HashMap<PathBuf, (SystemTime, u64, TranscriptInfo)>, // mtime, tamaño
    #[cfg(test)]
    reads: usize,
}

/// Lee todas las sesiones. Solo vuelve a leer las transcripciones cuyo mtime o tamaño cambió
/// desde la última llamada con el mismo `cache` (y quita del cache las que ya no existen).
/// Orden del resultado: más reciente primero. Errores de E/S en un archivo: se salta ese archivo.
pub fn scan_sessions(
    claude_dir: &Path,
    desktop_dir: Option<&Path>,
    home: &Path,
    cache: &mut ScanCache,
) -> Vec<ClaudeSession> {
    let active = scan_active(claude_dir);
    let desktop = scan_desktop(desktop_dir);

    let mut found: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(projects) = fs::read_dir(claude_dir.join("projects")) {
        for project in projects.flatten() {
            if let Ok(files) = fs::read_dir(project.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let Some(id) = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                    else {
                        continue;
                    };
                    found.push((path, id));
                }
            }
        }
    }
    cache
        .entries
        .retain(|path, _| found.iter().any(|(p, _)| p == path));

    // Primera pasada: lo leído de cada transcripción y su carpeta.
    let mut raws: Vec<(String, PathBuf, TranscriptInfo)> = Vec::new();
    for (path, id) in found {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let size = meta.len();
        let info = match cache.entries.get(&path) {
            Some((t, s, info)) if *t == mtime && *s == size => info.clone(),
            _ => {
                #[cfg(test)]
                {
                    cache.reads += 1;
                }
                let Some(info) = read_transcript(&path, home, mtime) else {
                    continue;
                };
                cache.entries.insert(path, (mtime, size, info.clone()));
                info
            }
        };

        let desktop_info = desktop.get(&id);
        let Some(cwd_raw) = desktop_info
            .and_then(|d| d.folder.clone())
            .or_else(|| info.cwd.clone())
        else {
            continue;
        };
        let trimmed = cwd_raw.trim_end_matches('/');
        let cwd = PathBuf::from(if trimmed.is_empty() { "/" } else { trimmed });
        raws.push((id, cwd, info));
    }

    // Segunda pasada: con todas las carpetas a la vista se sabe cuáles son clientes.
    let clients = client_folders(raws.iter().map(|(_, cwd, _)| cwd.as_path()), home);
    let mut sessions = Vec::new();
    for (id, cwd, info) in raws {
        let desktop_info = desktop.get(&id);
        let active_info = active.get(&id);
        let title = desktop_info
            .and_then(|d| d.title.clone())
            .or_else(|| info.custom_title.clone())
            .or_else(|| active_info.and_then(|a| a.name.clone()))
            .or_else(|| info.ai_title.clone())
            .or_else(|| info.first_user_message.clone())
            .unwrap_or_else(|| "(sin título)".to_string());

        let (project_dir, placement) = match orphan_client(&cwd, home, &clients) {
            None => (cwd.clone(), Placement::Opened),
            Some(client) => match infer_project(&info, client.as_deref(), home, &clients) {
                Some(project) => (project, Placement::Inferred),
                None => (cwd.clone(), Placement::Unassigned),
            },
        };
        let client = locate(&project_dir, home, &clients)
            .map(|(client, _)| client)
            .unwrap_or_else(|| "Personal".to_string());

        sessions.push(ClaudeSession {
            id: id.clone(),
            title,
            project_dir,
            placement,
            client,
            last_activity: info.last_activity,
            messages: info.messages,
            active: active_info.is_some(),
            from_desktop_app: desktop_info.is_some()
                || info.entrypoint.as_deref() == Some("claude-desktop"),
            archived: desktop_info.is_some_and(|d| d.archived),
            cwd,
        });
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity));
    sessions
}

/// Carpeta de la app de escritorio de Claude según el sistema: macOS
/// `~/Library/Application Support/Claude`, Windows `%APPDATA%/Claude`, Linux `~/.config/Claude`.
pub fn default_desktop_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Claude")
    } else {
        home.join(".config/Claude")
    }
}

/// Lee una transcripción línea por línea; solo parsea JSON las líneas interesantes.
fn read_transcript(path: &Path, home: &Path, mtime: SystemTime) -> Option<TranscriptInfo> {
    // Sin exigir UTF-8 válido: una línea rota no debe esconder la sesión entera.
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut info = TranscriptInfo {
        last_activity: Some(chrono::DateTime::<chrono::Local>::from(mtime).naive_local()),
        ..Default::default()
    };
    let needle = format!("{}/Documents/Moca/", home.to_string_lossy());
    for line in text.lines() {
        let is_user = line.contains("\"type\":\"user\"");
        let is_assistant = line.contains("\"type\":\"assistant\"");
        if is_user || is_assistant {
            info.messages += 1;
        }
        if line.contains("\"custom-title\"")
            && let Ok(value) = serde_json::from_str::<Value>(line)
            && let Some(title) = value.get("customTitle").and_then(Value::as_str)
            && !title.is_empty()
        {
            info.custom_title = Some(title.to_string());
        }
        if line.contains("\"ai-title\"")
            && let Ok(value) = serde_json::from_str::<Value>(line)
            && let Some(title) = value.get("aiTitle").and_then(Value::as_str)
            && !title.is_empty()
        {
            info.ai_title = Some(title.to_string());
        }
        if is_user && info.first_user_message.is_none() {
            info.first_user_message = first_user_text(line);
        }
        if info.cwd.is_none()
            && line.contains("\"cwd\"")
            && let Ok(value) = serde_json::from_str::<Value>(line)
            && let Some(cwd) = value.get("cwd").and_then(Value::as_str)
        {
            info.cwd = Some(cwd.to_string());
            info.entrypoint = value
                .get("entrypoint")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if is_assistant && line.contains("tool_use") {
            count_paths(line, &needle, &mut info.paths);
        }
    }
    Some(info)
}

/// Primer mensaje del usuario usable como título: string o bloques de texto unidos,
/// sin espacios repetidos, recortado a 100 caracteres.
fn first_user_text(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let content = value.get("message")?.get("content")?;
    let raw = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || collapsed.starts_with('<') {
        return None;
    }
    let mut chars: Vec<char> = collapsed.chars().collect();
    if chars.len() > 100 {
        chars.truncate(99);
        chars.push('…');
    }
    Some(chars.into_iter().collect())
}

/// Cuenta apariciones de `<home>/Documents/Moca/Cliente/Proyecto` en la línea (sin parsear).
fn count_paths(line: &str, needle: &str, counts: &mut HashMap<String, usize>) {
    let mut rest = line;
    while let Some(pos) = rest.find(needle) {
        let after = &rest[pos + needle.len()..];
        let end = after.find(['"', '\\', ' ']).unwrap_or(after.len());
        let mut parts = after[..end].split('/').filter(|p| !p.is_empty());
        if let (Some(client), Some(project)) = (parts.next(), parts.next()) {
            *counts.entry(format!("{client}/{project}")).or_insert(0) += 1;
        }
        rest = after;
    }
}

fn moca_dir(home: &Path) -> PathBuf {
    home.join("Documents").join("Moca")
}

/// Carpetas de `~/Documents/Moca` que son clientes: hay sesiones abiertas en alguna de sus
/// subcarpetas y no son un repo git ellas mismas (`warp` o `Barquito` son proyectos directos).
fn client_folders<'a>(cwds: impl Iterator<Item = &'a Path>, home: &Path) -> HashSet<String> {
    let moca = moca_dir(home);
    cwds.filter_map(|cwd| {
        let mut components = cwd.strip_prefix(&moca).ok()?.components();
        let first = components.next()?.as_os_str().to_str()?.to_string();
        components.next()?;
        (!moca.join(&first).join(".git").exists()).then_some(first)
    })
    .collect()
}

/// Cliente y carpeta de proyecto de una ruta bajo `~/Documents/Moca`: `Cliente/Proyecto` si
/// el primer componente es un cliente; si no, el proyecto es esa carpeta y el cliente "Moca".
fn locate(path: &Path, home: &Path, clients: &HashSet<String>) -> Option<(String, PathBuf)> {
    let moca = moca_dir(home);
    let mut components = path.strip_prefix(&moca).ok()?.components();
    let first = components.next()?.as_os_str().to_str()?.to_string();
    if clients.contains(&first) {
        let second = components.next()?.as_os_str().to_str()?.to_string();
        Some((first.clone(), moca.join(first).join(second)))
    } else {
        Some(("Moca".to_string(), moca.join(first)))
    }
}

/// None si no es huérfana; Some(None) si es `~` o la raíz de Moca; Some(Some(cliente))
/// si es la raíz de un cliente.
fn orphan_client(cwd: &Path, home: &Path, clients: &HashSet<String>) -> Option<Option<String>> {
    let moca = moca_dir(home);
    if cwd == home || cwd == moca {
        return Some(None);
    }
    let mut components = cwd.strip_prefix(&moca).ok()?.components();
    let first = components.next()?.as_os_str().to_str()?.to_string();
    (components.next().is_none() && clients.contains(&first)).then_some(Some(first))
}

/// El proyecto más tocado si tiene al menos 3 apariciones y el 40 % del total. Con `client`,
/// solo cuentan los proyectos de ese cliente.
fn infer_project(
    info: &TranscriptInfo,
    client: Option<&str>,
    home: &Path,
    clients: &HashSet<String>,
) -> Option<PathBuf> {
    let moca = moca_dir(home);
    let mut by_project: HashMap<PathBuf, usize> = HashMap::new();
    for (key, count) in &info.paths {
        let Some((project_client, project)) = locate(&moca.join(key), home, clients) else {
            continue;
        };
        if client.is_some_and(|c| c != project_client) {
            continue;
        }
        *by_project.entry(project).or_insert(0) += count;
    }
    let total: usize = by_project.values().sum();
    let (project, count) = by_project.into_iter().max_by_key(|(_, count)| *count)?;
    (count >= 3 && count * 5 >= total * 2).then_some(project)
}

struct ActiveInfo {
    name: Option<String>,
}

/// Sesiones abiertas ahora: `sessions/*.json` con el proceso vivo.
fn scan_active(claude_dir: &Path) -> HashMap<String, ActiveInfo> {
    let mut map = HashMap::new();
    let Ok(entries) = fs::read_dir(claude_dir.join("sessions")) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(id) = value.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let pid = value.get("pid").and_then(Value::as_i64).unwrap_or(-1) as i32;
        if !pid_alive(pid) {
            continue;
        }
        let name = if value.get("nameSource").and_then(Value::as_str) == Some("user") {
            value
                .get("name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
        } else {
            None
        };
        map.insert(id.to_string(), ActiveInfo { name });
    }
    map
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(not(unix))]
fn pid_alive(_pid: i32) -> bool {
    true
}

#[derive(Default)]
struct DesktopInfo {
    title: Option<String>,
    archived: bool,
    folder: Option<String>,
}

/// Entradas de la app de escritorio: `claude-code-sessions/*/*/local_*.json`.
fn scan_desktop(desktop_dir: Option<&Path>) -> HashMap<String, DesktopInfo> {
    let mut map = HashMap::new();
    let Some(desktop_dir) = desktop_dir else {
        return map;
    };
    let Ok(level1) = fs::read_dir(desktop_dir.join("claude-code-sessions")) else {
        return map;
    };
    for entry1 in level1.flatten() {
        let Ok(level2) = fs::read_dir(entry1.path()) else {
            continue;
        };
        for entry2 in level2.flatten() {
            let name = entry2.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("local_") && name.ends_with(".json")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry2.path()) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(key) = value
                .get("cliSessionId")
                .and_then(Value::as_str)
                .or_else(|| value.get("sessionId").and_then(Value::as_str))
            else {
                continue;
            };
            let archived = match value.get("isArchived") {
                Some(Value::Bool(b)) => *b,
                Some(Value::String(s)) => s == "true",
                _ => false,
            };
            let folder = value
                .get("userSelectedFolders")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .filter(|f| !f.is_empty())
                .or_else(|| {
                    value
                        .get("cwd")
                        .and_then(Value::as_str)
                        .filter(|f| !f.is_empty())
                })
                .map(str::to_string);
            map.insert(
                key.to_string(),
                DesktopInfo {
                    title: value
                        .get("title")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string),
                    archived,
                    folder,
                },
            );
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/t")
    }

    fn write_transcript(claude_dir: &Path, id: &str, lines: &[String]) -> PathBuf {
        let dir = claude_dir.join("projects").join("proyecto");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{id}.jsonl"));
        fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn touch(path: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Read","input":{{"file_path":"{path}"}}}}]}}}}"#
        )
    }

    fn scan(claude_dir: &Path, cache: &mut ScanCache) -> Vec<ClaudeSession> {
        scan_sessions(claude_dir, None, &home(), cache)
    }

    #[test]
    fn sesion_normal_con_titulos() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        write_transcript(
            &claude,
            "abc",
            &[
                r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}"#
                    .to_string(),
                r#"{"type":"user","message":{"content":"hola"}}"#.to_string(),
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}"#
                    .to_string(),
                r#"{"type":"custom-title","customTitle":"MocaWarp","sessionId":"abc"}"#.to_string(),
                r#"{"type":"ai-title","aiTitle":"Otro","sessionId":"abc"}"#.to_string(),
            ],
        );
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.title, "MocaWarp");
        assert_eq!(s.placement, Placement::Opened);
        assert_eq!(
            s.project_dir,
            PathBuf::from("/Users/t/Documents/Moca/Manifesto/elbarretal")
        );
        assert_eq!(s.messages, 2);
    }

    #[test]
    fn huerfana_desde_home_infiere_proyecto() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let mut lines =
            vec![r#"{"type":"system","cwd":"/Users/t","entrypoint":"cli"}"#.to_string()];
        // `warp` cuelga directo de Moca (no hay sesiones en subcarpetas de un cliente
        // `warp`): es un proyecto del grupo "Moca".
        for _ in 0..4 {
            lines.push(touch("/Users/t/Documents/Moca/warp/src/a.rs"));
        }
        lines.push(touch("/Users/t/Documents/Moca/Otro/x/y"));
        write_transcript(&claude, "abc", &lines);
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.placement, Placement::Inferred);
        assert_eq!(s.project_dir, PathBuf::from("/Users/t/Documents/Moca/warp"));
        assert_eq!(s.client, "Moca");
        assert_eq!(s.cwd, PathBuf::from("/Users/t"));
    }

    #[test]
    fn huerfana_sin_rutas_queda_sin_asignar() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        write_transcript(
            &claude,
            "abc",
            &[
                r#"{"type":"system","cwd":"/Users/t","entrypoint":"cli"}"#.to_string(),
                r#"{"type":"user","message":{"content":"hola"}}"#.to_string(),
            ],
        );
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.placement, Placement::Unassigned);
        assert_eq!(s.project_dir, PathBuf::from("/Users/t"));
    }

    #[test]
    fn huerfana_en_raiz_de_cliente_solo_cuenta_su_cliente() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let mut lines = vec![
            r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto","entrypoint":"cli"}"#
                .to_string(),
        ];
        for _ in 0..3 {
            lines.push(touch(
                "/Users/t/Documents/Moca/Manifesto/elbarretal/src/a.rs",
            ));
        }
        for _ in 0..10 {
            lines.push(touch("/Users/t/Documents/Moca/NeoNet/x/y"));
        }
        write_transcript(&claude, "abc", &lines);
        // Otra sesión abierta dentro de Manifesto/elbarretal hace de Manifesto un cliente.
        write_transcript(
            &claude,
            "otra",
            &[r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}"#.to_string()],
        );
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 2);
        let s = sessions.iter().find(|s| s.id == "abc").unwrap();
        assert_eq!(s.client, "Manifesto");
        assert_eq!(s.placement, Placement::Inferred);
        assert_eq!(
            s.project_dir,
            PathBuf::from("/Users/t/Documents/Moca/Manifesto/elbarretal")
        );
    }

    #[test]
    fn titulo_desde_bloques_de_texto() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        write_transcript(
            &claude,
            "abc",
            &[
                r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}"#
                    .to_string(),
                r#"{"type":"user","message":{"content":[{"type":"text","text":"hola"},{"type":"text","text":"mundo"}]}}"#
                    .to_string(),
            ],
        );
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "hola mundo");
    }

    #[test]
    fn cache_evita_releer() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let path = write_transcript(
            &claude,
            "abc",
            &[
                r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}"#
                    .to_string(),
            ],
        );
        let mut cache = ScanCache::default();
        scan(&claude, &mut cache);
        assert_eq!(cache.reads, 1);
        scan(&claude, &mut cache);
        assert_eq!(cache.reads, 1);
        // Al modificar el archivo (tamaño distinto) se relee.
        fs::write(&path, r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}
{"type":"user","message":{"content":"mas"}}"#)
        .unwrap();
        scan(&claude, &mut cache);
        assert_eq!(cache.reads, 2);
    }

    #[test]
    fn sesion_activa_con_nombre_de_usuario() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        write_transcript(
            &claude,
            "abc",
            &[
                r#"{"type":"system","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","entrypoint":"cli"}"#
                    .to_string(),
                r#"{"type":"user","message":{"content":"hola"}}"#.to_string(),
            ],
        );
        let sessions_dir = claude.join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("abc.json"),
            format!(
                r#"{{"pid":{},"sessionId":"abc","cwd":"/Users/t/Documents/Moca/Manifesto/elbarretal","name":"MiSesion","nameSource":"user","status":"shell"}}"#,
                std::process::id()
            ),
        )
        .unwrap();
        let sessions = scan(&claude, &mut ScanCache::default());
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert!(s.active);
        assert_eq!(s.title, "MiSesion");
    }
}

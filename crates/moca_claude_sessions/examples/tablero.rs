//! Imprime el tablero de proyectos con las sesiones reales de esta máquina:
//! `cargo run -p moca_claude_sessions --example tablero [búsqueda]`.
// Herramienta de terminal, nunca corre en wasm.
#![allow(clippy::disallowed_types)]

use std::time::Instant;

use moca_claude_sessions::projects::group_projects;
use moca_claude_sessions::scan::{ScanCache, default_desktop_dir, scan_sessions};
use moca_claude_sessions::{Placement, StatusFilter, relative_time};

fn main() {
    let home = dirs_home();
    let query = std::env::args().nth(1).unwrap_or_default();
    let mut cache = ScanCache::default();
    let started = Instant::now();
    let sessions = scan_sessions(
        &home.join(".claude"),
        Some(&default_desktop_dir(&home)),
        &home,
        &mut cache,
    );
    let first = started.elapsed();
    let started = Instant::now();
    scan_sessions(
        &home.join(".claude"),
        Some(&default_desktop_dir(&home)),
        &home,
        &mut cache,
    );
    println!(
        "{} sesiones · primera lectura {:?} · con cache {:?}\n",
        sessions.len(),
        first,
        started.elapsed()
    );
    let now = chrono::Local::now().naive_local();
    for section in group_projects(&sessions, &query, StatusFilter::All, true, now) {
        println!("== {} ({})", section.client, section.projects.len());
        for card in &section.projects {
            let latest = &card.sessions[0];
            let mark = match latest.placement {
                Placement::Opened => "",
                Placement::Inferred => "  [abierta desde ~]",
                Placement::Unassigned => "  [sin proyecto]",
            };
            println!(
                "   {:<28} {:>2} ses · {:<12} {}{}",
                if card.unassigned {
                    latest.title.as_str()
                } else {
                    card.name.as_str()
                },
                card.sessions.len(),
                relative_time(latest.last_activity, now),
                latest.title,
                mark
            );
        }
    }
}

fn dirs_home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
}

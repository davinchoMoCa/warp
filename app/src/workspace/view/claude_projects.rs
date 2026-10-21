//! moca: tablero "Proyectos". Tarjetas por proyecto (carpeta) agrupadas por cliente, leídas
//! directo de `~/.claude`. Clic en una tarjeta retoma la última sesión de ese proyecto en una
//! pestaña nueva; "+ Nueva" abre `claude` limpio en la carpeta. Reemplaza el área de la
//! terminal mientras está abierto, como la vista de agentes de Warp.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use moca_claude_sessions::projects::{
    ClientSection, ProjectCard, continue_sessions, group_projects,
};
use moca_claude_sessions::scan::{ScanCache, default_desktop_dir, scan_sessions};
use moca_claude_sessions::{ClaudeSession, Placement, StatusFilter, relative_time};
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::ui::theme::color::internal_colors;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    Border, ChildView, Clipped, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, Element, Empty, Fill, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    ScrollbarWidth, Shrinkable, Text, Wrap,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};

const CARD_WIDTH: f32 = 260.;
const CONTINUE_LIMIT: usize = 4;

#[derive(Clone, Debug)]
pub enum ClaudeProjectsAction {
    /// Retoma la sesión `id`.
    Resume {
        id: String,
    },
    /// Abre `claude` limpio en `dir`.
    NewSession {
        dir: PathBuf,
    },
    ToggleSessions {
        dir: PathBuf,
    },
    SetStatus(StatusFilter),
    Close,
}

pub enum Event {
    /// Abrir una pestaña en `cwd` que corra `command`.
    OpenSession {
        cwd: PathBuf,
        command: String,
        title: String,
    },
    Close,
}

enum LoadState {
    Loading,
    Loaded,
    Failed(String),
}

#[derive(Default)]
struct MouseStates {
    cards: HashMap<String, MouseStateHandle>,
    buttons: HashMap<String, MouseStateHandle>,
}

impl MouseStates {
    fn card(&self, key: &str) -> MouseStateHandle {
        self.cards.get(key).cloned().unwrap_or_default()
    }

    fn button(&self, key: &str) -> MouseStateHandle {
        self.buttons.get(key).cloned().unwrap_or_default()
    }
}

pub struct ClaudeProjectsView {
    query_editor: ViewHandle<EditorView>,
    sessions: Vec<ClaudeSession>,
    sections: Vec<ClientSection>,
    continuing: Vec<ClaudeSession>,
    status: StatusFilter,
    expanded: HashSet<PathBuf>,
    load_state: LoadState,
    is_refreshing: bool,
    cache: Arc<Mutex<ScanCache>>,
    mouse_states: MouseStates,
    scroll_state: ClippedScrollStateHandle,
}

impl ClaudeProjectsView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let query_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(14.), appearance),
                    select_all_on_focus: true,
                    clear_selections_on_blur: true,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    propagate_horizontal_navigation_keys: PropagateHorizontalNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("Busca un proyecto, sesión o cliente", ctx);
            editor
        });
        ctx.subscribe_to_view(&query_editor, |me, _, event, ctx| match event {
            EditorEvent::Edited(_) => {
                me.rebuild(ctx);
                ctx.notify();
            }
            EditorEvent::Escape => ctx.emit(Event::Close),
            EditorEvent::Enter => me.resume_first_match(ctx),
            _ => {}
        });

        Self {
            query_editor,
            sessions: Vec::new(),
            sections: Vec::new(),
            continuing: Vec::new(),
            status: StatusFilter::All,
            expanded: HashSet::new(),
            load_state: LoadState::Loading,
            is_refreshing: false,
            cache: Arc::new(Mutex::new(ScanCache::default())),
            mouse_states: MouseStates::default(),
            scroll_state: Default::default(),
        }
    }

    /// Al abrir el tablero: enfoca el buscador y relee las sesiones (solo los archivos que
    /// cambiaron desde la última vez).
    pub fn on_opened(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.query_editor);
        self.refresh(ctx);
    }

    fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        if self.is_refreshing {
            return;
        }
        let Some(home) = dirs::home_dir() else {
            self.load_state = LoadState::Failed("No se encontró la carpeta personal.".into());
            ctx.notify();
            return;
        };
        self.is_refreshing = true;
        let cache = self.cache.clone();
        ctx.spawn(
            async move {
                let claude_dir = home.join(".claude");
                if !claude_dir.is_dir() {
                    return Err("No hay sesiones de Claude Code en esta máquina.".to_string());
                }
                let desktop_dir = default_desktop_dir(&home);
                let mut cache = cache.lock().map_err(|_| "Error interno".to_string())?;
                Ok(scan_sessions(
                    &claude_dir,
                    Some(&desktop_dir),
                    &home,
                    &mut cache,
                ))
            },
            |me, result, ctx| {
                me.is_refreshing = false;
                match result {
                    Ok(sessions) => {
                        me.sessions = sessions;
                        me.load_state = LoadState::Loaded;
                    }
                    Err(message) => me.load_state = LoadState::Failed(message),
                }
                me.rebuild(ctx);
                ctx.notify();
            },
        );
    }

    fn rebuild(&mut self, ctx: &mut ViewContext<Self>) {
        let query = self.query_editor.as_ref(ctx).buffer_text(ctx);
        let now = chrono::Local::now().naive_local();
        self.sections = group_projects(&self.sessions, &query, self.status, true, now);
        self.continuing = continue_sessions(&self.sessions, CONTINUE_LIMIT);
        for section in &self.sections {
            for card in &section.projects {
                let key = card_key(card);
                self.mouse_states.cards.entry(key.clone()).or_default();
                for suffix in ["new", "more"] {
                    self.mouse_states
                        .buttons
                        .entry(format!("{key}:{suffix}"))
                        .or_default();
                }
                for session in &card.sessions {
                    self.mouse_states
                        .buttons
                        .entry(format!("s:{}", session.id))
                        .or_default();
                }
            }
        }
        for session in &self.continuing {
            self.mouse_states
                .cards
                .entry(format!("c:{}", session.id))
                .or_default();
        }
    }

    fn has_query(&self, app: &AppContext) -> bool {
        !self
            .query_editor
            .as_ref(app)
            .buffer_text(app)
            .trim()
            .is_empty()
    }

    fn resume(&mut self, id: &str, ctx: &mut ViewContext<Self>) {
        let Some(session) = self.sessions.iter().find(|s| s.id == id) else {
            return;
        };
        // `claude --resume` busca la conversación en la carpeta donde se abrió.
        ctx.emit(Event::OpenSession {
            cwd: session.cwd.clone(),
            command: format!("claude --resume {}", session.id),
            title: session.title.clone(),
        });
    }

    fn resume_first_match(&mut self, ctx: &mut ViewContext<Self>) {
        let first = self
            .sections
            .first()
            .and_then(|section| section.projects.first())
            .and_then(|card| card.sessions.first())
            .map(|session| session.id.clone());
        if let Some(id) = first {
            self.resume(&id, ctx);
        }
    }
}

fn card_key(card: &ProjectCard) -> String {
    // Las tarjetas "sin proyecto" son una por sesión y comparten carpeta.
    match card.sessions.first() {
        Some(session) if card.unassigned => format!("u:{}", session.id),
        _ => format!("p:{}", card.dir.display()),
    }
}

fn project_title(card: &ProjectCard, home: &Path) -> String {
    if card.unassigned {
        return card
            .sessions
            .first()
            .map(|s| s.title.clone())
            .unwrap_or_default();
    }
    if card.dir == home {
        return "~".to_string();
    }
    card.name.clone()
}

struct Palette {
    main: ThemeFill,
    sub: ThemeFill,
    card: ThemeFill,
    card_hover: ThemeFill,
    border: ThemeFill,
    green: ThemeFill,
    accent: ThemeFill,
}

impl Palette {
    fn new(appearance: &Appearance) -> Self {
        let theme = appearance.theme();
        Self {
            main: theme.main_text_color(theme.background()),
            sub: theme.sub_text_color(theme.background()),
            card: theme.surface_1(),
            card_hover: theme.surface_2(),
            border: theme.surface_3(),
            green: ThemeFill::Solid(theme.ansi_fg_green()),
            accent: internal_colors::accent(theme),
        }
    }
}

fn label(text: impl Into<String>, size: f32, color: ThemeFill, appearance: &Appearance) -> Text {
    Text::new_inline(text.into(), appearance.ui_font_family(), size)
        .with_color(color.into())
        .with_clip(ClipConfig::ellipsis())
}

fn status_dot(active: bool, palette: &Palette) -> Box<dyn Element> {
    let dot = Container::new(
        ConstrainedBox::new(Empty::new().finish())
            .with_width(6.)
            .with_height(6.)
            .finish(),
    )
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
    if active {
        dot.with_background(palette.green)
    } else {
        dot.with_border(Border::all(1.).with_border_fill(palette.sub))
    }
    .finish()
}

fn section_title(text: String, detail: String, appearance: &Appearance) -> Box<dyn Element> {
    let palette = Palette::new(appearance);
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.)
            .with_child(
                label(text.to_uppercase(), 11., palette.sub, appearance)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .with_child(label(detail, 11., palette.sub, appearance).finish())
            .finish(),
    )
    .with_padding_top(18.)
    .with_padding_bottom(8.)
    .finish()
}

fn small_button(
    text: String,
    mouse_state: MouseStateHandle,
    action: ClaudeProjectsAction,
    is_active: bool,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let palette = Palette::new(appearance);
    Hoverable::new(mouse_state, move |state| {
        let mut container = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_child(label(text.clone(), 12., palette.sub, appearance).finish())
                .finish(),
        )
        .with_vertical_padding(5.)
        .with_border(Border::all(1.).with_border_fill(palette.border))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)));
        if state.is_hovered() || is_active {
            container = container.with_background(palette.card_hover);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
    .finish()
}

fn render_continue_card(
    session: &ClaudeSession,
    mouse_state: MouseStateHandle,
    home: &Path,
    now: chrono::NaiveDateTime,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let palette = Palette::new(appearance);
    let project = session
        .project_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|_| session.project_dir != home)
        .unwrap_or_else(|| "~".to_string());
    let content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(6.)
        .with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(status_dot(true, &palette))
                .with_child(
                    Shrinkable::new(
                        1.0,
                        label(
                            format!("{project} · {}", relative_time(session.last_activity, now)),
                            12.,
                            palette.accent,
                            appearance,
                        )
                        .finish(),
                    )
                    .finish(),
                )
                .finish(),
        )
        .with_child(
            label(session.title.clone(), 14., palette.main, appearance)
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_child(label("Retomar ↵", 12., palette.sub, appearance).finish())
        .finish();

    let id = session.id.clone();
    ConstrainedBox::new(
        Hoverable::new(mouse_state, move |state| {
            Container::new(content)
                .with_uniform_padding(12.)
                .with_background(if state.is_hovered() {
                    palette.card_hover
                } else {
                    palette.card
                })
                .with_border(Border::all(1.).with_border_fill(palette.accent))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(ClaudeProjectsAction::Resume { id: id.clone() })
        })
        .finish(),
    )
    .with_width(CARD_WIDTH)
    .finish()
}

#[allow(clippy::too_many_arguments)]
fn render_project_card(
    card: &ProjectCard,
    mouse_states: &MouseStates,
    is_expanded: bool,
    home: &Path,
    home_str: Option<&str>,
    now: chrono::NaiveDateTime,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let palette = Palette::new(appearance);
    let key = card_key(card);
    let Some(latest) = card.sessions.first() else {
        return Empty::new().finish();
    };
    let any_active = card.sessions.iter().any(|s| s.active);

    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.)
        .with_child(
            Shrinkable::new(
                1.0,
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(status_dot(any_active, &palette))
                    .with_child(
                        Shrinkable::new(
                            1.0,
                            label(project_title(card, home), 15., palette.main, appearance)
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .finish(),
        )
        .with_child(
            label(
                relative_time(latest.last_activity, now),
                11.,
                palette.sub,
                appearance,
            )
            .finish(),
        )
        .finish();

    let path = latest.project_dir.to_string_lossy();
    let mut path_line = user_friendly_path(&path, home_str).into_owned();
    if latest.placement == Placement::Inferred {
        path_line.push_str("  ·  abierta desde ");
        path_line.push_str(&user_friendly_path(&latest.cwd.to_string_lossy(), home_str));
    }
    let subtitle = if card.unassigned {
        format!("{} mensajes", latest.messages)
    } else {
        latest.title.clone()
    };

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(4.)
        .with_child(header)
        .with_child(label(subtitle, 13., palette.main, appearance).finish())
        .with_child(label(path_line, 11., palette.sub, appearance).finish())
        .finish();

    let latest_id = latest.id.clone();
    let clickable = Hoverable::new(mouse_states.card(&key), move |state| {
        let mut container = Container::new(body)
            .with_padding(Padding::uniform(12.).with_bottom(8.))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)));
        if state.is_hovered() {
            container = container.with_background(palette.card_hover);
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(ClaudeProjectsAction::Resume {
            id: latest_id.clone(),
        })
    })
    .finish();

    let session_count = card.sessions.len();
    let more_label = if session_count == 1 {
        "1 sesión ▾".to_string()
    } else {
        format!("{session_count} sesiones ▾")
    };
    let buttons = Container::new(
        Flex::row()
            .with_spacing(6.)
            .with_child(
                Shrinkable::new(
                    1.0,
                    small_button(
                        "+ Nueva sesión".to_string(),
                        mouse_states.button(&format!("{key}:new")),
                        ClaudeProjectsAction::NewSession {
                            dir: latest.project_dir.clone(),
                        },
                        false,
                        appearance,
                    ),
                )
                .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.0,
                    small_button(
                        more_label,
                        mouse_states.button(&format!("{key}:more")),
                        ClaudeProjectsAction::ToggleSessions {
                            dir: card.dir.clone(),
                        },
                        is_expanded,
                        appearance,
                    ),
                )
                .finish(),
            )
            .finish(),
    )
    .with_horizontal_padding(10.)
    .with_padding_bottom(10.)
    .finish();

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(clickable)
        .with_child(buttons);

    if is_expanded {
        let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for session in &card.sessions {
            let id = session.id.clone();
            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    Shrinkable::new(
                        1.0,
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(6.)
                            .with_child(status_dot(session.active, &palette))
                            .with_child(
                                Shrinkable::new(
                                    1.0,
                                    label(session.title.clone(), 12., palette.main, appearance)
                                        .finish(),
                                )
                                .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
                )
                .with_child(
                    label(
                        relative_time(session.last_activity, now),
                        11.,
                        palette.sub,
                        appearance,
                    )
                    .finish(),
                )
                .finish();
            list.add_child(
                Hoverable::new(
                    mouse_states.button(&format!("s:{}", session.id)),
                    move |state| {
                        let mut container = Container::new(row)
                            .with_horizontal_padding(8.)
                            .with_vertical_padding(6.)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)));
                        if state.is_hovered() {
                            container = container.with_background(palette.card_hover);
                        }
                        container.finish()
                    },
                )
                .with_cursor(Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(ClaudeProjectsAction::Resume { id: id.clone() })
                })
                .finish(),
            );
        }
        column.add_child(
            Container::new(list.finish())
                .with_horizontal_padding(4.)
                .with_padding_bottom(6.)
                .finish(),
        );
    }

    ConstrainedBox::new(
        Container::new(column.finish())
            .with_background(palette.card)
            .with_border(Border::all(1.).with_border_fill(palette.border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
            .finish(),
    )
    .with_width(CARD_WIDTH)
    .finish()
}

impl Entity for ClaudeProjectsView {
    type Event = Event;
}

impl TypedActionView for ClaudeProjectsView {
    type Action = ClaudeProjectsAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ClaudeProjectsAction::Resume { id } => self.resume(id, ctx),
            ClaudeProjectsAction::NewSession { dir } => {
                let title = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Claude".to_string());
                ctx.emit(Event::OpenSession {
                    cwd: dir.clone(),
                    command: "claude".to_string(),
                    title,
                });
            }
            ClaudeProjectsAction::ToggleSessions { dir } => {
                if !self.expanded.remove(dir) {
                    self.expanded.insert(dir.clone());
                }
                ctx.notify();
            }
            ClaudeProjectsAction::SetStatus(status) => {
                self.status = *status;
                self.rebuild(ctx);
                ctx.notify();
            }
            ClaudeProjectsAction::Close => ctx.emit(Event::Close),
        }
    }
}

impl View for ClaudeProjectsView {
    fn ui_name() -> &'static str {
        "ClaudeProjectsView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let palette = Palette::new(appearance);
        let home = dirs::home_dir().unwrap_or_default();
        let home_str = home.to_string_lossy().into_owned();
        let now = chrono::Local::now().naive_local();

        // Encabezado: título, buscador, filtro y cerrar.
        let active_count = self.sessions.iter().filter(|s| s.active).count();
        let project_count: usize = self.sections.iter().map(|s| s.projects.len()).sum();
        let title = Flex::column()
            .with_spacing(2.)
            .with_child(
                label("Proyectos", 20., palette.main, appearance)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .with_child(
                label(
                    format!("{project_count} proyectos · {active_count} sesiones activas"),
                    12.,
                    palette.sub,
                    appearance,
                )
                .finish(),
            )
            .finish();
        let search = ConstrainedBox::new(
            Container::new(Clipped::new(ChildView::new(&self.query_editor).finish()).finish())
                .with_padding(Padding::uniform(7.).with_left(12.).with_right(12.))
                .with_border(Border::all(1.).with_border_fill(palette.border))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .finish(),
        )
        .with_width(340.)
        .finish();
        let mut filters = Flex::row().with_spacing(4.);
        for (status, text) in [
            (StatusFilter::All, "Todos"),
            (StatusFilter::Active, "Activos"),
            (StatusFilter::Today, "Hoy"),
        ] {
            filters.add_child(
                ConstrainedBox::new(small_button(
                    text.to_string(),
                    self.mouse_states.button(&format!("f:{text}")),
                    ClaudeProjectsAction::SetStatus(status),
                    self.status == status,
                    appearance,
                ))
                .with_width(72.)
                .finish(),
            );
        }
        let close = ConstrainedBox::new(small_button(
            "✕".to_string(),
            self.mouse_states.button("close"),
            ClaudeProjectsAction::Close,
            false,
            appearance,
        ))
        .with_width(34.)
        .finish();
        let header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(12.)
            .with_child(Shrinkable::new(1.0, title).finish())
            .with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(10.)
                    .with_child(search)
                    .with_child(filters.finish())
                    .with_child(close)
                    .finish(),
            )
            .finish();

        let mut body = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        match &self.load_state {
            LoadState::Loading => {
                body.add_child(label("Cargando sesiones…", 13., palette.sub, appearance).finish())
            }
            LoadState::Failed(message) => {
                body.add_child(label(message.clone(), 13., palette.sub, appearance).finish())
            }
            LoadState::Loaded => {
                let searching = self.has_query(app);
                if !searching && !self.continuing.is_empty() {
                    body.add_child(section_title(
                        "Continuar donde lo dejaste".to_string(),
                        String::new(),
                        appearance,
                    ));
                    let mut row = Wrap::row().with_spacing(10.).with_run_spacing(10.);
                    for session in &self.continuing {
                        row.add_child(render_continue_card(
                            session,
                            self.mouse_states.card(&format!("c:{}", session.id)),
                            &home,
                            now,
                            appearance,
                        ));
                    }
                    body.add_child(row.finish());
                }
                if self.sections.is_empty() {
                    body.add_child(
                        Container::new(
                            label(
                                "Ningún proyecto coincide. Prueba con el nombre de la carpeta.",
                                13.,
                                palette.sub,
                                appearance,
                            )
                            .finish(),
                        )
                        .with_padding_top(24.)
                        .finish(),
                    );
                }
                for section in &self.sections {
                    let count = section.projects.len();
                    let detail = if section.client == moca_claude_sessions::projects::UNASSIGNED {
                        format!("{count} sesiones abiertas desde ~ sin proyecto claro")
                    } else if count == 1 {
                        "1 proyecto".to_string()
                    } else {
                        format!("{count} proyectos")
                    };
                    body.add_child(section_title(section.client.clone(), detail, appearance));
                    let mut grid = Wrap::row().with_spacing(10.).with_run_spacing(10.);
                    for card in &section.projects {
                        grid.add_child(render_project_card(
                            card,
                            &self.mouse_states,
                            self.expanded.contains(&card.dir),
                            &home,
                            Some(&home_str),
                            now,
                            appearance,
                        ));
                    }
                    body.add_child(grid.finish());
                }
                body.add_child(
                    Container::new(
                        label(
                            "Clic en un proyecto retoma su última sesión · Enter abre el primero · Esc cierra",
                            11.,
                            palette.sub,
                            appearance,
                        )
                        .finish(),
                    )
                    .with_padding_top(20.)
                    .finish(),
                );
            }
        }

        let scrollable = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            Container::new(body.finish())
                .with_padding_bottom(24.)
                .finish(),
            ScrollbarWidth::Auto,
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            Fill::None,
        )
        .finish();

        Container::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(header)
                .with_child(Shrinkable::new(1.0, scrollable).finish())
                .finish(),
        )
        .with_padding(Padding::uniform(24.).with_bottom(0.))
        .with_background(theme.background())
        .finish()
    }
}

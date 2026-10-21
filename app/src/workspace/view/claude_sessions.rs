//! moca: panel "Sesiones de Claude". Lista las sesiones de Claude Code de esta máquina
//! (leídas directo de `~/.claude`), agrupadas por cliente, y abre cada una en una pestaña
//! nueva con `claude --resume <id>` en su carpeta.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use moca_claude_sessions::scan::{ScanCache, default_desktop_dir, scan_sessions};
use moca_claude_sessions::{
    ClaudeSession, SessionGroup, StatusFilter, filter_and_group, relative_time, resume_command,
};
use warp_core::ui::theme::color::internal_colors;
use warp_util::path::user_friendly_path;
use warpui::elements::{
    Border, ChildView, Clipped, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, Element, Empty, Fill, Flex, Highlight, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    ScrollbarWidth, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::{ClipConfig, TextStyle};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};

#[derive(Clone, Debug)]
pub enum ClaudeSessionsAction {
    Open { id: String },
    ToggleGroup(String),
}

pub enum Event {
    /// Abrir una pestaña en `cwd` que corra `command`.
    OpenSession {
        cwd: PathBuf,
        command: String,
        title: String,
    },
}

enum LoadState {
    Loading,
    Loaded,
    Failed(String),
}

pub struct ClaudeSessionsView {
    query_editor: ViewHandle<EditorView>,
    sessions: Vec<ClaudeSession>,
    groups: Vec<SessionGroup>,
    load_state: LoadState,
    is_refreshing: bool,
    cache: Arc<Mutex<ScanCache>>,
    collapsed: HashSet<String>,
    item_mouse_states: HashMap<String, MouseStateHandle>,
    header_mouse_states: HashMap<String, MouseStateHandle>,
    scroll_state: ClippedScrollStateHandle,
}

impl ClaudeSessionsView {
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
            editor.set_placeholder_text("Busca por nombre, carpeta o cliente", ctx);
            editor
        });
        ctx.subscribe_to_view(&query_editor, |me, _, event, ctx| {
            if let EditorEvent::Edited(_) = event {
                me.rebuild_groups(ctx);
                ctx.notify();
            }
        });

        let mut view = Self {
            query_editor,
            sessions: Vec::new(),
            groups: Vec::new(),
            load_state: LoadState::Loading,
            is_refreshing: false,
            cache: Arc::new(Mutex::new(ScanCache::default())),
            collapsed: HashSet::new(),
            item_mouse_states: HashMap::new(),
            header_mouse_states: HashMap::new(),
            scroll_state: Default::default(),
        };
        view.refresh(ctx);
        view
    }

    /// Al abrir el panel: recarga la lista y enfoca el buscador.
    pub fn on_left_panel_focused(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.query_editor);
        self.refresh(ctx);
    }

    /// Vuelve a leer las sesiones en segundo plano (solo los archivos que cambiaron).
    pub fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        if self.is_refreshing {
            return;
        }
        self.is_refreshing = true;
        let cache = self.cache.clone();
        ctx.spawn(async move { load_sessions(&cache) }, |me, result, ctx| {
            me.is_refreshing = false;
            match result {
                Ok(sessions) => {
                    me.sessions = sessions;
                    me.load_state = LoadState::Loaded;
                }
                Err(message) => {
                    if me.sessions.is_empty() {
                        me.load_state = LoadState::Failed(message);
                    } else {
                        log::warn!("moca: no se pudo recargar sesiones de Claude: {message}");
                    }
                }
            }
            me.rebuild_groups(ctx);
            ctx.notify();
        });
    }

    fn rebuild_groups(&mut self, ctx: &mut ViewContext<Self>) {
        let query = self.query_editor.as_ref(ctx).buffer_text(ctx);
        let now = chrono::Local::now().naive_local();
        self.groups = filter_and_group(&self.sessions, &query, StatusFilter::All, None, true, now);
        for group in &self.groups {
            self.header_mouse_states
                .entry(group.client.clone())
                .or_default();
            for (session, _) in &group.sessions {
                self.item_mouse_states
                    .entry(session.id.clone())
                    .or_default();
            }
        }
    }

    fn open_session(&mut self, id: &str, ctx: &mut ViewContext<Self>) {
        let Some(session) = self.sessions.iter().find(|s| s.id == id) else {
            return;
        };
        ctx.emit(Event::OpenSession {
            cwd: session.cwd.clone(),
            command: resume_command(session),
            title: session.title.clone(),
        });
    }

    fn has_query(&self, app: &AppContext) -> bool {
        !self
            .query_editor
            .as_ref(app)
            .buffer_text(app)
            .trim()
            .is_empty()
    }
}

fn load_sessions(cache: &Mutex<ScanCache>) -> Result<Vec<ClaudeSession>, String> {
    let home = dirs::home_dir().ok_or_else(|| "No se encontró la carpeta personal.".to_string())?;
    let claude_dir = home.join(".claude");
    if !claude_dir.is_dir() {
        return Err("No hay sesiones de Claude Code en esta máquina.".to_string());
    }
    let mut cache = cache.lock().map_err(|_| "Error interno".to_string())?;
    Ok(scan_sessions(
        &claude_dir,
        Some(&default_desktop_dir(&home)),
        &home,
        &mut cache,
    ))
}

fn render_search_box(query_editor: &ViewHandle<EditorView>, app: &AppContext) -> Box<dyn Element> {
    let theme = Appearance::as_ref(app).theme();
    let search = Container::new(
        Shrinkable::new(
            1.0,
            Clipped::new(ChildView::new(query_editor).finish()).finish(),
        )
        .finish(),
    )
    .with_padding(Padding::uniform(6.).with_left(12.).with_right(12.))
    .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .finish();

    Container::new(search)
        .with_horizontal_padding(12.)
        .with_vertical_padding(4.)
        .finish()
}

fn render_message(message: &str, app: &AppContext) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    Container::new(
        Text::new(
            message.to_string(),
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_color(theme.sub_text_color(theme.background()).into())
        .finish(),
    )
    .with_horizontal_padding(12.)
    .with_vertical_padding(8.)
    .finish()
}

fn render_group_header(
    group: &SessionGroup,
    is_collapsed: bool,
    mouse_state: MouseStateHandle,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let sub_color = theme.sub_text_color(theme.background());
    let font_family = appearance.ui_font_family();

    let chevron = if is_collapsed {
        warp_core::ui::Icon::ChevronRight
    } else {
        warp_core::ui::Icon::ChevronDown
    };
    let active = group.sessions.iter().filter(|(s, _)| s.active).count();
    let count = if active > 0 {
        format!("● {active}  ·  {}", group.sessions.len())
    } else {
        group.sessions.len().to_string()
    };

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.)
                .with_child(
                    ConstrainedBox::new(chevron.to_warpui_icon(sub_color).finish())
                        .with_width(12.)
                        .with_height(12.)
                        .finish(),
                )
                .with_child(
                    Text::new_inline(group.client.to_uppercase(), font_family, 11.)
                        .with_color(sub_color.into())
                        .finish(),
                )
                .finish(),
        )
        .with_child(
            Text::new_inline(count, font_family, 11.)
                .with_color(sub_color.into())
                .finish(),
        )
        .finish();

    let client = group.client.clone();
    Hoverable::new(mouse_state, move |state| {
        let mut container = Container::new(row)
            .with_horizontal_padding(12.)
            .with_padding_top(12.)
            .with_padding_bottom(6.);
        if state.is_hovered() {
            container = container.with_background(theme.surface_overlay_1());
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(ClaudeSessionsAction::ToggleGroup(client.clone()));
    })
    .finish()
}

fn render_session(
    session: &ClaudeSession,
    highlight_indices: &[usize],
    mouse_state: MouseStateHandle,
    home: Option<&str>,
    now: chrono::NaiveDateTime,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let font_family = appearance.ui_font_family();
    let font_size = appearance.ui_font_size();
    let main_color = theme.main_text_color(theme.background());
    let sub_color = theme.sub_text_color(theme.background());

    // Punto verde si la sesión sigue abierta en alguna terminal; aro gris si no.
    let dot = Container::new(
        ConstrainedBox::new(Empty::new().finish())
            .with_width(6.)
            .with_height(6.)
            .finish(),
    )
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
    let dot = if session.active {
        dot.with_background(Fill::Solid(theme.ansi_fg_green()))
    } else {
        dot.with_border(Border::all(1.).with_border_fill(sub_color))
    }
    .finish();

    let mut title = Text::new_inline(session.title.clone(), font_family, font_size + 1.)
        .with_color(main_color.into())
        .with_clip(ClipConfig::ellipsis());
    if !highlight_indices.is_empty() {
        let highlight = Highlight::new()
            .with_properties(Properties::default().weight(Weight::Bold))
            .with_text_style(
                TextStyle::new()
                    .with_foreground_color(main_color.into())
                    .with_background_color(internal_colors::accent_overlay_3(theme).into_solid()),
            );
        title = title.with_single_highlight(highlight, highlight_indices.to_vec());
    }

    // Punto y título a la izquierda; el "hace X" alineado a la derecha.
    let dot_and_title = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.)
        .with_child(dot)
        .with_child(Shrinkable::new(1.0, title.finish()).finish())
        .finish();
    let top_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.)
        .with_child(Shrinkable::new(1.0, dot_and_title).finish())
        .with_child(
            Text::new_inline(
                relative_time(session.last_activity, now),
                font_family,
                font_size - 2.,
            )
            .with_color(sub_color.into())
            .finish(),
        )
        .finish();

    let path = session.cwd.to_string_lossy();
    let mut detail = format!(
        "{}  ·  {} msj",
        user_friendly_path(&path, home),
        session.messages
    );
    if session.from_desktop_app {
        detail.push_str("  ·  App");
    }
    let bottom_row = Container::new(
        Text::new_inline(detail, font_family, font_size - 2.)
            .with_color(sub_color.into())
            .with_clip(ClipConfig::ellipsis())
            .finish(),
    )
    .with_padding_left(14.)
    .finish();

    let content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.)
        .with_child(top_row)
        .with_child(bottom_row)
        .finish();

    let id = session.id.clone();
    Hoverable::new(mouse_state, move |state| {
        let mut container = Container::new(content)
            .with_horizontal_padding(12.)
            .with_vertical_padding(7.);
        if state.is_hovered() {
            container = container.with_background(theme.surface_overlay_1());
        }
        container.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(ClaudeSessionsAction::Open { id: id.clone() });
    })
    .finish()
}

impl Entity for ClaudeSessionsView {
    type Event = Event;
}

impl TypedActionView for ClaudeSessionsView {
    type Action = ClaudeSessionsAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ClaudeSessionsAction::Open { id } => self.open_session(id, ctx),
            ClaudeSessionsAction::ToggleGroup(client) => {
                if !self.collapsed.remove(client) {
                    self.collapsed.insert(client.clone());
                }
                ctx.notify();
            }
        }
    }
}

impl View for ClaudeSessionsView {
    fn ui_name() -> &'static str {
        "ClaudeSessionsView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let content: Box<dyn Element> = match &self.load_state {
            LoadState::Loading => render_message("Cargando sesiones…", app),
            LoadState::Failed(message) => render_message(message, app),
            LoadState::Loaded if self.groups.is_empty() => {
                if self.has_query(app) {
                    render_message("Nada coincide. Prueba con el nombre de la carpeta.", app)
                } else {
                    render_message("No hay sesiones de Claude en esta máquina.", app)
                }
            }
            LoadState::Loaded => {
                let home_dir = dirs::home_dir().map(|home| home.to_string_lossy().into_owned());
                let now = chrono::Local::now().naive_local();
                // Con búsqueda se muestran todos los grupos aunque estén plegados.
                let searching = self.has_query(app);
                let mut column =
                    Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                for group in &self.groups {
                    let is_collapsed = !searching && self.collapsed.contains(&group.client);
                    let header_state = self
                        .header_mouse_states
                        .get(&group.client)
                        .cloned()
                        .unwrap_or_default();
                    column.add_child(render_group_header(group, is_collapsed, header_state, app));
                    if is_collapsed {
                        continue;
                    }
                    for (session, indices) in &group.sessions {
                        let state = self
                            .item_mouse_states
                            .get(&session.id)
                            .cloned()
                            .unwrap_or_default();
                        column.add_child(render_session(
                            session,
                            indices,
                            state,
                            home_dir.as_deref(),
                            now,
                            app,
                        ));
                    }
                }
                ClippedScrollable::vertical(
                    self.scroll_state.clone(),
                    Container::new(column.finish())
                        .with_padding_bottom(8.)
                        .finish(),
                    ScrollbarWidth::Auto,
                    theme.nonactive_ui_detail().into(),
                    theme.active_ui_detail().into(),
                    Fill::None,
                )
                .finish()
            }
        };

        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(render_search_box(&self.query_editor, app))
            .with_child(Shrinkable::new(1.0, content).finish())
            .finish()
    }
}

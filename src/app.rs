#![allow(dead_code)]

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use adw::prelude::*;
use gtk::{gdk, glib};
use livesplit_core::{
    layout::{Layout, LayoutState},
    rendering::software::Renderer,
    settings::ImageCache,
    HotkeySystem, SharedTimer, Timer,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::{
    config::Config,
    platform::{DisplayBackend, Platform, TimerWindowPlatform},
};

const HOTKEY_PERMISSION_WARNING: &str = "Granting this permission allows every program running \
as your account to read raw keyboard and controller input, including passwords and other \
sensitive keystrokes. Keep this in mind when following these instructions.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyAvailability {
    Available,
    MissingInputGroup,
    InputGroupUnavailable,
    InitializationFailed(String),
}

impl HotkeyAvailability {
    fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

fn classify_wayland_hotkey_permission(
    backend: DisplayBackend,
    input_group_exists: bool,
    is_input_group_member: bool,
) -> HotkeyAvailability {
    if backend != DisplayBackend::WaylandStandard {
        HotkeyAvailability::Available
    } else if !input_group_exists {
        HotkeyAvailability::InputGroupUnavailable
    } else if !is_input_group_member {
        HotkeyAvailability::MissingInputGroup
    } else {
        HotkeyAvailability::Available
    }
}

fn hotkey_permission_status(backend: DisplayBackend) -> HotkeyAvailability {
    use nix::unistd::{getgroups, Group};

    let Ok(Some(input_group)) = Group::from_name("input") else {
        return classify_wayland_hotkey_permission(backend, false, false);
    };
    let is_member = getgroups()
        .map(|groups| groups.contains(&input_group.gid))
        .unwrap_or(false);
    classify_wayland_hotkey_permission(backend, true, is_member)
}

#[derive(Clone)]
pub struct LayoutDraft(Arc<Mutex<Option<Layout>>>);

#[derive(Clone)]
pub struct RunDraft(Arc<Mutex<Option<livesplit_core::Run>>>);

impl std::fmt::Debug for RunDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RunDraft(..)")
    }
}

impl std::fmt::Debug for LayoutDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LayoutDraft(..)")
    }
}

pub struct LayoutData {
    pub layout: Layout,
    pub layout_state: LayoutState,
    pub is_modified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorKind {
    Run,
    Layout,
    Window,
    Hotkeys,
    Notes,
    NotesViewer,
    #[cfg(feature = "auto-splitting")]
    AutoSplitter,
}

#[derive(Default)]
pub struct OpenEditors(HashMap<EditorKind, adw::ApplicationWindow>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intent {
    #[default]
    None,
    NewSplits,
    OpenSplits,
    Exit,
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    NewSplits,
    OpenSplits(PathBuf),
    NewLayout,
    OpenLayout(PathBuf),
    Exit,
}

struct PendingActionState {
    action: PendingAction,
    documents: VecDeque<PendingDocument>,
    current_document: Option<PendingDocument>,
}

fn relevant_changes(action: &PendingAction, splits: bool, layout: bool) -> (bool, bool) {
    match action {
        PendingAction::NewSplits => (splits, false),
        PendingAction::OpenSplits(_) | PendingAction::Exit => (splits, layout),
        PendingAction::NewLayout | PendingAction::OpenLayout(_) => (false, layout),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TimerCommand {
    StartOrSplit,
    Reset,
    Pause,
    UndoSplit,
    SkipSplit,
    UndoAllPauses,
}

#[derive(Clone, Debug)]
pub enum ResetAction {
    ResetOnly,
    SaveSplits,
    SaveSplitsAs(PathBuf),
}

#[derive(Clone, Copy)]
enum FileChoice {
    OpenSplits,
    SaveSplits,
    OpenLayout,
    SaveLayout,
}

#[derive(Clone, Copy)]
enum DirectAction {
    NewSplits,
    SaveSplits,
    NewLayout,
    SaveLayout,
}

#[derive(Debug)]
pub enum AppMsg {
    RenderTick,
    TimerCommand(TimerCommand),
    ResetConfirmationFinished(bool, ResetAction),
    SaveSplitsConfirmed,
    SaveSplitsAsConfirmed(PathBuf),
    Scroll(f64),
    SetComparison(String),
    SetTimingMethod(livesplit_core::TimingMethod),
    WorldRecordResponse {
        index: usize,
        url: String,
        body: Option<String>,
    },
    BeginIntent(Intent),
    OpenEditor(EditorKind),
    EditorFinished(EditorKind),
    MainWindowConfigured,
    MainWindowCloseRequested,
    RequestAction(PendingAction),
    ExecuteAction(PendingAction),
    ContinuePendingAction,
    ResolvePendingChange(PendingChangeDecision),
    SavePendingSplitsAs(PathBuf),
    SavePendingLayoutAs(PathBuf),
    NewSplits,
    OpenSplits(PathBuf),
    SaveSplits,
    SaveSplitsAs(PathBuf),
    NewLayout,
    OpenLayout(PathBuf),
    SaveLayout,
    SaveLayoutAs(PathBuf),
    ApplyWindowSettings(bool),
    ApplyLayout(LayoutDraft),
    ApplyRun(RunDraft),
    ApplyHotkeys(livesplit_core::HotkeyConfig),
    #[cfg(feature = "auto-splitting")]
    StoreAutoSplitterSettings,
}

pub struct AppModel {
    pub timer: SharedTimer,
    pub layout: RefCell<LayoutData>,
    pub config: RefCell<Config>,
    pub image_cache: Rc<RefCell<ImageCache>>,
    pub hotkey_system: Option<HotkeySystem<SharedTimer>>,
    pub hotkey_availability: HotkeyAvailability,
    pub render_size: Cell<(u32, u32)>,
    pub open_editors: OpenEditors,
    pub pending_intent: Intent,
    pub pending_world_records: HashMap<usize, Instant>,
    pending_action: Option<PendingActionState>,
    active_layout_editor: Option<Rc<RefCell<Option<livesplit_core::LayoutEditor>>>>,
    active_run_editor: Option<Rc<RefCell<Option<livesplit_core::RunEditor>>>>,
    notes_viewer: Option<NotesViewerUi>,
    #[cfg(feature = "auto-splitting")]
    pub auto_splitter: Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
    renderer: RefCell<Renderer>,
    picture: gtk::Picture,
    platform: Platform,
    window: gtk::ApplicationWindow,
    closing_allowed: Rc<Cell<bool>>,
    mouse_passthrough: Cell<bool>,
}

struct NotesViewerUi {
    state: crate::notes::NotesViewer,
    title: gtk::Label,
    buffer: gtk::TextBuffer,
}

mod completion_entry;
mod file_dialogs;
mod layout_editor_ui;
mod notes_ui;
mod pending_changes_ui;
mod run_editor_ui;
mod settings_ui;
mod timer_window;

use completion_entry::CompletionEntryRow;
use file_dialogs::{choose_save_path, select_file};
use layout_editor_ui::build_layout_editor;
use notes_ui::{build_notes_editor, build_notes_viewer, set_markdown_buffer};
use pending_changes_ui::{
    confirm_unsaved_layout, confirm_unsaved_splits, PendingChangeDecision, PendingDocument,
};
use run_editor_ui::build_run_editor;
use settings_ui::build_settings_editor;
use timer_window::install_timer_interactions;

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = Config;
    type Input = AppMsg;
    type Output = ();

    view! {
        #[root]
        main_window = gtk::ApplicationWindow {
            set_title: Some("LiveSplit One"),
            set_default_width: width,
            set_default_height: height,
            set_decorated: false,
            set_resizable: true,
            add_css_class: "timer-window",

            #[name(picture)]
            gtk::Picture {
                set_can_shrink: true,
                set_content_fit: gtk::ContentFit::Fill,
            },
        }
    }

    fn init(
        mut config: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        config.setup_logging();
        let run = config.parse_run_or_default();
        let mut timer = Timer::new(run).expect("the default run contains a segment");
        config.configure_timer(&mut timer);
        let layout = config.parse_layout_or_default(&timer);
        let timer = timer.into_shared();
        #[cfg(feature = "auto-splitting")]
        let auto_splitter = Rc::new(livesplit_core::auto_splitting::Runtime::new());
        #[cfg(feature = "auto-splitting")]
        crate::config::or_show_error(config.load_auto_splitter(&auto_splitter, &timer));
        let (window_width, window_height) = config.window_size();
        let width = window_width.round() as i32;
        let height = window_height.round() as i32;
        let widgets = view_output!();
        let picture = widgets.picture.clone();
        let platform = Platform::detect();
        let mut hotkey_availability = hotkey_permission_status(platform.backend());
        let hotkey_system = if hotkey_availability.is_available() {
            match config.configure_hotkeys(timer.clone()) {
                Ok(system) => Some(system),
                Err(error) => {
                    log::error!("Global hotkey initialization failed: {error}");
                    hotkey_availability =
                        HotkeyAvailability::InitializationFailed(error.to_string());
                    None
                }
            }
        } else {
            None
        };
        platform.configure(&root, &config);

        install_timer_interactions(&root, &picture, &timer, &config, platform, &sender);
        let closing_allowed = Rc::new(Cell::new(false));
        let close_guard = closing_allowed.clone();
        let close_sender = sender.clone();
        root.connect_close_request(move |_| {
            if close_guard.get() {
                glib::Propagation::Proceed
            } else {
                close_sender.input(AppMsg::RequestAction(PendingAction::Exit));
                glib::Propagation::Stop
            }
        });

        let model = AppModel {
            timer: timer.clone(),
            layout: RefCell::new(LayoutData {
                layout,
                layout_state: LayoutState::default(),
                is_modified: false,
            }),
            config: RefCell::new(config),
            image_cache: Rc::new(RefCell::new(ImageCache::new())),
            hotkey_system,
            hotkey_availability,
            render_size: Cell::new((width as u32, height as u32)),
            open_editors: OpenEditors::default(),
            pending_intent: Intent::None,
            pending_world_records: HashMap::new(),
            pending_action: None,
            active_layout_editor: None,
            active_run_editor: None,
            notes_viewer: None,
            #[cfg(feature = "auto-splitting")]
            auto_splitter,
            renderer: RefCell::new(Renderer::new()),
            picture,
            platform,
            window: root,
            closing_allowed,
            mouse_passthrough: Cell::new(false),
        };

        let render_sender = sender.clone();
        glib::timeout_add_local(
            std::time::Duration::from_millis(16),
            move || match render_sender.input_sender().send(AppMsg::RenderTick) {
                Ok(()) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            },
        );
        let configured_sender = sender.clone();
        glib::idle_add_local_once(move || {
            configured_sender.input(AppMsg::MainWindowConfigured);
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::RenderTick => {
                self.poll_world_records(&sender);
                self.update_notes_viewer();
                self.update_mouse_passthrough();
                self.render();
            }
            AppMsg::TimerCommand(command) => {
                if matches!(command, TimerCommand::Reset) {
                    self.request_reset(ResetAction::ResetOnly, &sender);
                    return;
                }
                let mut timer = self.timer.write().unwrap();
                match command {
                    TimerCommand::StartOrSplit => drop(timer.split_or_start()),
                    TimerCommand::Reset => unreachable!(),
                    TimerCommand::Pause => drop(timer.toggle_pause_or_start()),
                    TimerCommand::UndoSplit => drop(timer.undo_split()),
                    TimerCommand::SkipSplit => drop(timer.skip_split()),
                    TimerCommand::UndoAllPauses => drop(timer.undo_all_pauses()),
                }
            }
            AppMsg::ResetConfirmationFinished(update_times, action) => {
                let _ = self.timer.write().unwrap().reset(update_times);
                sender.input(match action {
                    ResetAction::ResetOnly => return,
                    ResetAction::SaveSplits => AppMsg::SaveSplitsConfirmed,
                    ResetAction::SaveSplitsAs(path) => AppMsg::SaveSplitsAsConfirmed(path),
                });
            }
            AppMsg::Scroll(delta) => {
                let has_segment_groups = !self
                    .timer
                    .read()
                    .unwrap()
                    .run()
                    .segment_groups()
                    .groups()
                    .is_empty();
                scroll_layout(&self.layout, delta, has_segment_groups);
            }
            AppMsg::SetComparison(comparison) => {
                if self
                    .timer
                    .write()
                    .unwrap()
                    .set_current_comparison(comparison.as_str())
                    .is_ok()
                {
                    self.config.borrow_mut().set_comparison(comparison);
                }
            }
            AppMsg::SetTimingMethod(method) => {
                self.timer
                    .write()
                    .unwrap()
                    .set_current_timing_method(method);
                self.config.borrow_mut().set_timing_method(method);
            }
            AppMsg::WorldRecordResponse { index, url, body } => {
                if let Some(body) = body {
                    self.pending_world_records.remove(&index);
                    let timer = self.timer.read().unwrap();
                    let snapshot = timer.snapshot();
                    let mut layout = self.layout.borrow_mut();
                    let still_current = layout
                        .layout
                        .world_record_request_urls(&snapshot)
                        .into_iter()
                        .any(|(candidate_index, candidate_url)| {
                            candidate_index == index && candidate_url == url
                        });
                    if still_current {
                        layout.layout.world_record_parse_response(index, &body);
                    }
                }
            }
            AppMsg::BeginIntent(intent) => self.pending_intent = intent,
            AppMsg::EditorFinished(kind) => {
                self.open_editors.0.remove(&kind);
                if kind == EditorKind::Layout {
                    self.active_layout_editor = None;
                } else if kind == EditorKind::Run {
                    self.active_run_editor = None;
                } else if kind == EditorKind::Window {
                    if let Some(system) = self.hotkey_system.as_mut() {
                        let _ = system.activate();
                    }
                } else if kind == EditorKind::NotesViewer {
                    self.notes_viewer = None;
                }
            }
            AppMsg::OpenEditor(kind) => match kind {
                EditorKind::Layout => self.open_layout_editor(&sender),
                EditorKind::Run => self.open_run_editor(&sender),
                EditorKind::Window => self.open_settings_editor(&sender),
                EditorKind::Notes => self.open_notes_editor(&sender),
                EditorKind::NotesViewer => self.open_notes_viewer(&sender),
                _ => {}
            },
            AppMsg::MainWindowConfigured => self.show_hotkey_warning(),
            AppMsg::MainWindowCloseRequested => self.pending_intent = Intent::Exit,
            AppMsg::RequestAction(action) => self.request_action(action, &sender),
            AppMsg::ExecuteAction(action) => self.execute_action(action, &sender),
            AppMsg::ContinuePendingAction => self.continue_pending_action(&sender),
            AppMsg::ResolvePendingChange(decision) => {
                self.resolve_pending_change(decision, &sender);
            }
            AppMsg::SavePendingSplitsAs(path) => {
                self.store_auto_splitter_settings();
                let result = self
                    .config
                    .borrow_mut()
                    .save_splits_as(&mut self.timer.write().unwrap(), path);
                if result.is_ok() {
                    self.set_notes_actions_enabled(true);
                    sender.input(AppMsg::ContinuePendingAction);
                }
                crate::config::or_show_error(result);
            }
            AppMsg::SavePendingLayoutAs(path) => {
                let settings = self.layout.borrow().layout.settings();
                let result = self.config.borrow_mut().save_layout_as(
                    &mut self.timer.write().unwrap(),
                    settings,
                    path,
                );
                if result.is_ok() {
                    self.layout.borrow_mut().is_modified = false;
                    sender.input(AppMsg::ContinuePendingAction);
                }
                crate::config::or_show_error(result);
            }
            AppMsg::NewSplits => {
                self.close_notes_windows();
                self.config
                    .borrow_mut()
                    .new_splits(&mut self.timer.write().unwrap());
                #[cfg(feature = "auto-splitting")]
                crate::config::or_show_error(
                    self.config
                        .borrow_mut()
                        .load_auto_splitter(&self.auto_splitter, &self.timer),
                );
                self.set_notes_actions_enabled(false);
            }
            AppMsg::OpenSplits(path) => {
                let result = self.config.borrow_mut().open_splits(
                    &self.timer,
                    &mut self.layout.borrow_mut(),
                    #[cfg(feature = "auto-splitting")]
                    &self.auto_splitter,
                    path,
                );
                let opened = result.is_ok();
                crate::config::or_show_error(result);
                if opened {
                    self.close_notes_windows();
                }
                self.set_notes_actions_enabled(self.config.borrow().splits_path().is_some());
            }
            AppMsg::SaveSplits => {
                self.request_reset(ResetAction::SaveSplits, &sender);
            }
            AppMsg::SaveSplitsConfirmed => {
                if self.config.borrow().can_directly_save_splits() {
                    self.store_auto_splitter_settings();
                    let result = self
                        .config
                        .borrow_mut()
                        .save_splits(&mut self.timer.write().unwrap());
                    crate::config::or_show_error(result);
                } else {
                    select_file(&self.window, true, FileChoice::SaveSplits, &sender);
                }
            }
            AppMsg::SaveSplitsAs(path) => {
                self.request_reset(ResetAction::SaveSplitsAs(path), &sender);
            }
            AppMsg::SaveSplitsAsConfirmed(path) => {
                self.store_auto_splitter_settings();
                let result = self
                    .config
                    .borrow_mut()
                    .save_splits_as(&mut self.timer.write().unwrap(), path);
                if result.is_ok() {
                    self.set_notes_actions_enabled(true);
                }
                crate::config::or_show_error(result);
            }
            AppMsg::NewLayout => self.config.borrow_mut().new_layout(
                Some(&mut self.timer.write().unwrap()),
                &mut self.layout.borrow_mut(),
            ),
            AppMsg::OpenLayout(path) => {
                let result = self.config.borrow_mut().open_layout(
                    Some(&mut self.timer.write().unwrap()),
                    &mut self.layout.borrow_mut(),
                    &path,
                );
                crate::config::or_show_error(result);
            }
            AppMsg::SaveLayout => {
                if self.config.borrow().can_directly_save_layout() {
                    let settings = self.layout.borrow().layout.settings();
                    let result = self.config.borrow().save_layout(settings);
                    if result.is_ok() {
                        self.layout.borrow_mut().is_modified = false;
                    }
                    crate::config::or_show_error(result);
                } else {
                    select_file(&self.window, true, FileChoice::SaveLayout, &sender);
                }
            }
            AppMsg::SaveLayoutAs(path) => {
                let settings = self.layout.borrow().layout.settings();
                let result = self.config.borrow_mut().save_layout_as(
                    &mut self.timer.write().unwrap(),
                    settings,
                    path,
                );
                if result.is_ok() {
                    self.layout.borrow_mut().is_modified = false;
                }
                crate::config::or_show_error(result);
            }
            AppMsg::ApplyWindowSettings(enabled) => {
                self.config
                    .borrow_mut()
                    .set_mouse_pass_through_while_running(enabled);
            }
            AppMsg::ApplyLayout(draft) => {
                if let Some(layout) = draft.0.lock().unwrap().take() {
                    let mut layout_data = self.layout.borrow_mut();
                    layout_data.layout = layout;
                    layout_data.is_modified = true;
                }
            }
            AppMsg::ApplyRun(draft) => {
                if let Some(run) = draft.0.lock().unwrap().take() {
                    let _ = self.timer.write().unwrap().set_run(run);
                }
            }
            AppMsg::ApplyHotkeys(config) => {
                if let Some(system) = self.hotkey_system.as_mut() {
                    let _ = system.set_config(config);
                }
                self.config.borrow_mut().set_hotkeys(config);
            }
            #[cfg(feature = "auto-splitting")]
            AppMsg::StoreAutoSplitterSettings => self.auto_splitter.store_settings(),
        }
    }
}

fn scroll_layout(layout_data: &RefCell<LayoutData>, delta: f64, has_segment_groups: bool) {
    let mut layout_data = layout_data.borrow_mut();
    if !has_segment_groups {
        enable_flat_scrolling(&mut layout_data.layout.components);
    }
    if delta > 0.0 {
        layout_data.layout.scroll_down();
    } else if delta < 0.0 {
        layout_data.layout.scroll_up();
    }
}

fn enable_flat_scrolling(components: &mut [livesplit_core::layout::Component]) {
    use livesplit_core::{component::splits::SubsplitDisplayMode, layout::Component};

    for component in components {
        match component {
            Component::Splits(splits)
                if splits.settings().subsplit_display_mode
                    == SubsplitDisplayMode::CurrentGroupExpanded =>
            {
                splits.settings_mut().subsplit_display_mode = SubsplitDisplayMode::Flat;
            }
            Component::Group(group) => enable_flat_scrolling(&mut group.components),
            Component::Carousel(carousel) => enable_flat_scrolling(&mut carousel.components),
            _ => {}
        }
    }
}

fn should_mouse_passthrough(
    configured: bool,
    phase: livesplit_core::TimerPhase,
    window_active: bool,
) -> bool {
    configured && phase == livesplit_core::TimerPhase::Running && !window_active
}

fn physical_to_logical_size(size: [f32; 2], scale: u32) -> (i32, i32) {
    let scale = scale.max(1) as f32;
    (
        (size[0] / scale).ceil().max(1.0) as i32,
        (size[1] / scale).ceil().max(1.0) as i32,
    )
}

impl AppModel {
    fn request_reset(&self, action: ResetAction, sender: &ComponentSender<Self>) {
        if !self
            .timer
            .read()
            .unwrap()
            .current_attempt_has_new_best_times()
        {
            sender.input(AppMsg::ResetConfirmationFinished(true, action));
            return;
        }

        let dialog = gtk::AlertDialog::builder()
            .message("Update your best times?")
            .detail("This attempt contains new best times. Should they be saved when resetting?")
            .modal(true)
            .build();
        dialog.set_buttons(&["Cancel", "Don't Update", "Update Times"]);
        dialog.set_cancel_button(0);
        dialog.set_default_button(2);
        let sender = sender.clone();
        dialog.choose(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |response| match response.ok() {
                Some(1) => sender.input(AppMsg::ResetConfirmationFinished(false, action.clone())),
                Some(2) => sender.input(AppMsg::ResetConfirmationFinished(true, action)),
                _ => {}
            },
        );
    }

    fn update_mouse_passthrough(&self) {
        let enabled = should_mouse_passthrough(
            self.config.borrow().get_mouse_pass_through_while_running(),
            self.timer.read().unwrap().current_phase(),
            self.window.is_active(),
        );
        if self.mouse_passthrough.replace(enabled) != enabled {
            self.platform.set_mouse_passthrough(&self.window, enabled);
        }
    }

    fn close_notes_windows(&mut self) {
        for kind in [EditorKind::Notes, EditorKind::NotesViewer] {
            if let Some(window) = self.open_editors.0.remove(&kind) {
                window.close();
            }
        }
        self.notes_viewer = None;
    }

    fn set_notes_actions_enabled(&self, enabled: bool) {
        for name in ["edit-notes", "show-notes"] {
            if let Some(action) = self.window.lookup_action(name) {
                if let Ok(action) = action.downcast::<gio::SimpleAction>() {
                    action.set_enabled(enabled);
                }
            }
        }
    }

    fn notes_context(&self) -> Option<(PathBuf, Vec<String>)> {
        let splits_path = self.config.borrow().splits_path()?.to_owned();
        let names = self
            .timer
            .read()
            .unwrap()
            .run()
            .segments()
            .iter()
            .map(|segment| segment.name().to_owned())
            .collect();
        Some((crate::notes::sidecar_path(&splits_path), names))
    }

    fn open_notes_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Notes) {
            window.present();
            return;
        }
        let Some((path, names)) = self.notes_context() else {
            return;
        };
        let document = Rc::new(RefCell::new(crate::notes::NotesDocument::load(names, path)));
        let window = build_notes_editor(&self.window, document, sender);
        self.open_editors
            .0
            .insert(EditorKind::Notes, window.clone());
        window.present();
    }

    fn open_notes_viewer(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::NotesViewer) {
            window.present();
            return;
        }
        let Some((path, names)) = self.notes_context() else {
            return;
        };
        let state = crate::notes::NotesViewer::load(names, path);
        let (window, title, buffer) = build_notes_viewer(&self.window, &state, sender);
        self.notes_viewer = Some(NotesViewerUi {
            state,
            title,
            buffer,
        });
        self.open_editors
            .0
            .insert(EditorKind::NotesViewer, window.clone());
        window.present();
    }

    fn update_notes_viewer(&mut self) {
        let Some(viewer) = self.notes_viewer.as_mut() else {
            return;
        };
        let index = self
            .timer
            .read()
            .unwrap()
            .current_split_index()
            .unwrap_or(0);
        if viewer.state.follow_split(index) {
            viewer.title.set_label(&viewer.state.title());
            set_markdown_buffer(&viewer.buffer, viewer.state.note());
        }
    }

    fn has_unsaved_changes(&self) -> (bool, bool) {
        let splits = self.timer.read().unwrap().run().has_been_modified();
        let layout = self.layout.borrow().is_modified;
        (splits, layout)
    }

    fn relevant_unsaved_changes(&self, action: &PendingAction) -> (bool, bool) {
        let (splits, layout) = self.has_unsaved_changes();
        relevant_changes(action, splits, layout)
    }

    fn request_action(&mut self, action: PendingAction, sender: &ComponentSender<Self>) {
        let (splits, layout) = self.relevant_unsaved_changes(&action);
        if !splits && !layout {
            sender.input(AppMsg::ExecuteAction(action));
            return;
        }

        let mut documents = VecDeque::new();
        if splits {
            documents.push_back(PendingDocument::Splits);
        }
        if layout {
            documents.push_back(PendingDocument::Layout);
        }
        self.pending_action = Some(PendingActionState {
            action,
            documents,
            current_document: None,
        });
        sender.input(AppMsg::ContinuePendingAction);
    }

    fn continue_pending_action(&mut self, sender: &ComponentSender<Self>) {
        let Some(state) = self.pending_action.as_mut() else {
            return;
        };
        let Some(document) = state.documents.pop_front() else {
            let action = self.pending_action.take().unwrap().action;
            sender.input(AppMsg::ExecuteAction(action));
            return;
        };
        state.current_document = Some(document);

        let sender = sender.clone();
        let decided = move |decision| sender.input(AppMsg::ResolvePendingChange(decision));
        match document {
            PendingDocument::Splits => confirm_unsaved_splits(&self.window, decided),
            PendingDocument::Layout => confirm_unsaved_layout(&self.window, decided),
        }
    }

    fn resolve_pending_change(
        &mut self,
        decision: PendingChangeDecision,
        sender: &ComponentSender<Self>,
    ) {
        match decision {
            PendingChangeDecision::Cancel => self.pending_action = None,
            PendingChangeDecision::Discard => {
                if let Some(state) = self.pending_action.as_mut() {
                    state.current_document = None;
                }
                sender.input(AppMsg::ContinuePendingAction);
            }
            PendingChangeDecision::Save => self.save_current_pending_document(sender),
        }
    }

    fn save_current_pending_document(&self, sender: &ComponentSender<Self>) {
        let Some(state) = self.pending_action.as_ref() else {
            return;
        };
        if state.current_document == Some(PendingDocument::Splits) {
            if self.config.borrow().can_directly_save_splits() {
                self.store_auto_splitter_settings();
                let result = self
                    .config
                    .borrow_mut()
                    .save_splits(&mut self.timer.write().unwrap());
                crate::config::or_show_error(result);
                if !self.timer.read().unwrap().run().has_been_modified() {
                    sender.input(AppMsg::ContinuePendingAction);
                }
            } else {
                let sender = sender.clone();
                choose_save_path(&self.window, "Save Splits", move |path| {
                    sender.input(AppMsg::SavePendingSplitsAs(path));
                });
            }
            return;
        }

        if self.config.borrow().can_directly_save_layout() {
            let settings = self.layout.borrow().layout.settings();
            let result = self.config.borrow().save_layout(settings);
            if result.is_ok() {
                self.layout.borrow_mut().is_modified = false;
                sender.input(AppMsg::ContinuePendingAction);
            }
            crate::config::or_show_error(result);
        } else {
            let sender = sender.clone();
            choose_save_path(&self.window, "Save Layout", move |path| {
                sender.input(AppMsg::SavePendingLayoutAs(path));
            });
        }
    }

    fn execute_action(&self, action: PendingAction, sender: &ComponentSender<Self>) {
        sender.input(match action {
            PendingAction::NewSplits => AppMsg::NewSplits,
            PendingAction::OpenSplits(path) => AppMsg::OpenSplits(path),
            PendingAction::NewLayout => AppMsg::NewLayout,
            PendingAction::OpenLayout(path) => AppMsg::OpenLayout(path),
            PendingAction::Exit => {
                let width = self.window.width();
                let height = self.window.height();
                if width > 0 && height > 0 {
                    let mut config = self.config.borrow_mut();
                    config.set_window_size((width as f64, height as f64));
                    self.platform.persist_placement(&self.window, &mut config);
                }
                self.closing_allowed.set(true);
                self.window.close();
                return;
            }
        });
    }

    fn open_settings_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Window) {
            window.present();
            return;
        }
        if let Some(system) = self.hotkey_system.as_mut() {
            let _ = system.deactivate();
        }
        let draft = Rc::new(RefCell::new(self.config.borrow().hotkeys()));
        let window = build_settings_editor(
            &self.window,
            self.config.borrow().get_mouse_pass_through_while_running(),
            &self.hotkey_availability,
            draft,
            sender,
        );
        self.open_editors
            .0
            .insert(EditorKind::Window, window.clone());
        window.present();
    }

    fn show_hotkey_warning(&self) {
        if self.hotkey_availability.is_available() {
            return;
        }
        let (detail, can_show_instructions) = match &self.hotkey_availability {
            HotkeyAvailability::MissingInputGroup => (
                format!(
                    "On Wayland, LiveSplit needs permission to read Linux input devices for \
system-wide hotkeys. LiveSplit will continue working without global hotkeys.\n\n\
Security warning: {HOTKEY_PERMISSION_WARNING}\n\n\
Manual permission instructions are available in the application."
                ),
                true,
            ),
            HotkeyAvailability::InputGroupUnavailable => (
                "This system has no resolvable input group. LiveSplit will continue working \
without global hotkeys. Consult your distribution's input-device permission documentation; \
do not create a group or make input devices world-readable solely for LiveSplit."
                    .to_owned(),
                false,
            ),
            HotkeyAvailability::InitializationFailed(error) => (
                format!(
                    "LiveSplit could not initialize global hotkeys and will continue without \
them.\n\nDiagnostic: {error}\n\nSee the application log and your distribution's documentation."
                ),
                false,
            ),
            HotkeyAvailability::Available => return,
        };
        let dialog = gtk::AlertDialog::builder()
            .message("Global hotkeys are disabled")
            .detail(detail)
            .modal(true)
            .build();
        if can_show_instructions {
            dialog.set_buttons(&["Continue Without Hotkeys", "Open Instructions"]);
            dialog.set_cancel_button(0);
            dialog.set_default_button(0);
            let parent = self.window.clone();
            dialog.choose(
                Some(&self.window),
                gio::Cancellable::NONE,
                move |response| {
                    if response.ok() == Some(1) {
                        settings_ui::show_hotkey_permission_instructions(&parent);
                    }
                },
            );
        } else {
            dialog.set_buttons(&["Continue Without Hotkeys"]);
            dialog.set_cancel_button(0);
            dialog.set_default_button(0);
            dialog.show(Some(&self.window));
        }
    }

    fn open_run_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Run) {
            window.present();
            return;
        }
        // The editor restores the auto splitter from its run when cancelled,
        // so the run needs to reflect the live settings.
        self.store_auto_splitter_settings();
        let run = self.timer.read().unwrap().run().clone();
        let Ok(editor) = livesplit_core::RunEditor::new(run) else {
            return;
        };
        let editor = Rc::new(RefCell::new(Some(editor)));
        let window = build_run_editor(
            &self.window,
            editor.clone(),
            sender,
            #[cfg(feature = "auto-splitting")]
            self.auto_splitter.clone(),
            #[cfg(feature = "auto-splitting")]
            self.timer.clone(),
        );
        self.active_run_editor = Some(editor);
        self.open_editors.0.insert(EditorKind::Run, window.clone());
        window.present();
    }

    fn open_layout_editor(&mut self, sender: &ComponentSender<Self>) {
        if let Some(window) = self.open_editors.0.get(&EditorKind::Layout) {
            window.present();
            return;
        }
        let layout = self.layout.borrow().layout.clone();
        let Ok(editor) = livesplit_core::LayoutEditor::new(layout) else {
            return;
        };
        let editor = Rc::new(RefCell::new(Some(editor)));
        let window = build_layout_editor(
            &self.window,
            editor.clone(),
            self.image_cache.clone(),
            sender,
        );
        self.active_layout_editor = Some(editor);
        self.open_editors
            .0
            .insert(EditorKind::Layout, window.clone());
        window.present();
    }

    /// Writes the loaded auto splitter and its live settings into the run. When
    /// none is loaded, the run's stored settings are left alone, as they may
    /// belong to an auto splitter this timer can't run, like an ASL script.
    fn store_auto_splitter_settings(&self) {
        #[cfg(feature = "auto-splitting")]
        if self.auto_splitter.loaded_path().is_some() {
            self.auto_splitter.store_settings();
        }
    }

    fn poll_world_records(&mut self, sender: &ComponentSender<Self>) {
        let requests = {
            let timer = self.timer.read().unwrap();
            let snapshot = timer.snapshot();
            self.layout
                .borrow_mut()
                .layout
                .world_record_request_urls(&snapshot)
        };
        for (index, url) in requests {
            let should_request = self
                .pending_world_records
                .get(&index)
                .is_none_or(|started| started.elapsed() >= std::time::Duration::from_secs(30));
            if !should_request {
                continue;
            }
            self.pending_world_records.insert(index, Instant::now());
            let sender = sender.clone();
            std::thread::spawn(move || {
                let body = ureq::get(&url)
                    .call()
                    .ok()
                    .and_then(|response| response.into_string().ok());
                let _ =
                    sender
                        .input_sender()
                        .send(AppMsg::WorldRecordResponse { index, url, body });
            });
        }
    }

    fn render(&self) {
        let logical_width = self.picture.width().max(1) as u32;
        let logical_height = self.picture.height().max(1) as u32;
        let scale = self.picture.scale_factor().max(1) as u32;
        let width = logical_width.saturating_mul(scale);
        let height = logical_height.saturating_mul(scale);
        let timer = self.timer.read().unwrap();
        let snapshot = timer.snapshot();
        let mut layout = self.layout.borrow_mut();
        let mut cache = self.image_cache.borrow_mut();
        if let Some(editor) = &self.active_layout_editor {
            if let Some(editor) = editor.borrow_mut().as_mut() {
                editor.update_layout_state(
                    &mut layout.layout_state,
                    &mut cache,
                    &snapshot,
                    livesplit_core::Lang::English,
                );
            }
        } else {
            layout.layout_state =
                layout
                    .layout
                    .state(&mut cache, &snapshot, livesplit_core::Lang::English);
        }
        let mut renderer = self.renderer.borrow_mut();
        let requested_size = renderer.render(&layout.layout_state, &cache, [width, height]);
        let bytes = glib::Bytes::from_owned(renderer.image_data().to_vec());
        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            (width * 4) as usize,
        );
        self.picture.set_paintable(Some(&texture));
        self.render_size.set((logical_width, logical_height));
        if let Some(size) = requested_size {
            let (width, height) = physical_to_logical_size(size, scale);
            if width != self.window.width() || height != self.window.height() {
                self.window.set_default_size(width, height);
            }
        }
        cache.collect();
    }
}

#[cfg(test)]
mod tests {
    use super::timer_window::timer_resize_edge;
    use super::{
        classify_wayland_hotkey_permission, physical_to_logical_size, relevant_changes,
        scroll_layout, should_mouse_passthrough, HotkeyAvailability, Intent, LayoutData,
        PendingAction,
    };
    use crate::platform::DisplayBackend;
    use gtk::gdk;
    use std::cell::RefCell;

    #[test]
    fn cancel_clears_pending_intent() {
        let mut pending = Intent::Exit;
        assert_eq!(pending, Intent::Exit);
        pending = Intent::None;
        assert_eq!(pending, Intent::None);
    }

    #[test]
    fn wayland_hotkeys_require_the_input_group() {
        assert_eq!(
            classify_wayland_hotkey_permission(DisplayBackend::WaylandStandard, true, true),
            HotkeyAvailability::Available
        );
        assert_eq!(
            classify_wayland_hotkey_permission(DisplayBackend::WaylandStandard, true, false),
            HotkeyAvailability::MissingInputGroup
        );
        assert_eq!(
            classify_wayland_hotkey_permission(DisplayBackend::WaylandStandard, false, false),
            HotkeyAvailability::InputGroupUnavailable
        );
    }

    #[test]
    fn x11_hotkeys_do_not_require_the_wayland_preflight() {
        assert_eq!(
            classify_wayland_hotkey_permission(DisplayBackend::X11, false, false),
            HotkeyAvailability::Available
        );
    }

    #[test]
    fn scrolling_releases_layout_borrow_before_rendering() {
        let layout = RefCell::new(LayoutData {
            layout: livesplit_core::Layout::default_layout(livesplit_core::Lang::English),
            layout_state: Default::default(),
            is_modified: false,
        });

        scroll_layout(&layout, 1.0, false);

        assert!(layout.try_borrow_mut().is_ok());
    }

    #[test]
    fn scrolling_ungrouped_runs_uses_flat_split_scrolling() {
        use livesplit_core::{component::splits::SubsplitDisplayMode, layout::Component};

        let layout = RefCell::new(LayoutData {
            layout: livesplit_core::Layout::default_layout(livesplit_core::Lang::English),
            layout_state: Default::default(),
            is_modified: false,
        });

        scroll_layout(&layout, 1.0, false);

        let layout = layout.borrow();
        let splits = layout
            .layout
            .components
            .iter()
            .find_map(|component| match component {
                Component::Splits(splits) => Some(splits),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            splits.settings().subsplit_display_mode,
            SubsplitDisplayMode::Flat
        );
    }

    #[test]
    fn destructive_actions_only_guard_files_they_replace() {
        assert_eq!(
            relevant_changes(&PendingAction::NewSplits, true, true),
            (true, false)
        );
        assert_eq!(
            relevant_changes(&PendingAction::NewLayout, true, true),
            (false, true)
        );
        assert_eq!(
            relevant_changes(&PendingAction::Exit, true, true),
            (true, true)
        );
        assert_eq!(
            relevant_changes(
                &PendingAction::OpenSplits(std::path::PathBuf::from("next.lss")),
                true,
                true,
            ),
            (true, true)
        );
    }

    #[test]
    fn invisible_resize_border_classifies_edges_and_corners() {
        let edge = |x, y| timer_resize_edge(x, y, 300.0, 500.0, 8.0);
        assert_eq!(edge(0.0, 0.0), Some(gdk::SurfaceEdge::NorthWest));
        assert_eq!(edge(150.0, 0.0), Some(gdk::SurfaceEdge::North));
        assert_eq!(edge(299.0, 0.0), Some(gdk::SurfaceEdge::NorthEast));
        assert_eq!(edge(0.0, 250.0), Some(gdk::SurfaceEdge::West));
        assert_eq!(edge(299.0, 250.0), Some(gdk::SurfaceEdge::East));
        assert_eq!(edge(0.0, 499.0), Some(gdk::SurfaceEdge::SouthWest));
        assert_eq!(edge(150.0, 499.0), Some(gdk::SurfaceEdge::South));
        assert_eq!(edge(299.0, 499.0), Some(gdk::SurfaceEdge::SouthEast));
        assert_eq!(edge(150.0, 250.0), None);
    }

    #[test]
    fn pass_through_requires_running_and_an_inactive_window() {
        use livesplit_core::TimerPhase;

        assert!(should_mouse_passthrough(true, TimerPhase::Running, false));
        assert!(!should_mouse_passthrough(true, TimerPhase::Running, true));
        assert!(!should_mouse_passthrough(true, TimerPhase::Paused, false));
        assert!(!should_mouse_passthrough(false, TimerPhase::Running, false));
    }

    #[test]
    fn renderer_dimensions_are_converted_from_physical_pixels() {
        assert_eq!(physical_to_logical_size([601.0, 999.0], 2), (301, 500));
        assert_eq!(physical_to_logical_size([300.0, 500.0], 1), (300, 500));
        assert_eq!(physical_to_logical_size([0.0, 0.0], 0), (1, 1));
    }
}

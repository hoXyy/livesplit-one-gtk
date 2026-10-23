#![allow(dead_code)]

use anyhow::{Context, Result};
use directories::ProjectDirs;
use livesplit_core::{
    event,
    layout::{self, Layout, LayoutSettings},
    run::{
        parser::{composite, TimerKind},
        saver::livesplit::save_timer,
        LinkedLayout,
    },
    HotkeyConfig, HotkeySystem, Run, RunEditor, Segment, SharedTimer, Timer, TimingMethod,
};
use log::error;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, create_dir_all},
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{app::LayoutData, cli};

pub type SplitsHistory = BTreeMap<Arc<str>, BTreeMap<Arc<str>, BTreeSet<Arc<Path>>>>;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default)]
    splits: Splits,
    #[serde(default)]
    general: General,
    #[serde(default)]
    log: Log,
    #[serde(default)]
    window: Window,
    #[serde(default)]
    hotkeys: HotkeyConfig,
    /// Auto splitter passed on the command line. It replaces the one stored
    /// in the splits file that is loaded on startup.
    #[serde(skip)]
    cli_auto_splitter: Option<PathBuf>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Splits {
    current: Option<PathBuf>,
    #[serde(skip)]
    can_save: bool,
    #[serde(default)]
    history: SplitsHistory,
}

impl Splits {
    fn add_to_history(&mut self, run: &Run) {
        if let Some(current) = &self.current {
            self.history
                .entry(run.game_name().into())
                .or_default()
                .entry(
                    run.extended_category_name(false, false, true)
                        .to_string()
                        .into(),
                )
                .or_default()
                .insert(current.as_path().into());
        }
    }

    fn remove_from_history(&mut self) {
        if let Some(current) = self.current.as_deref() {
            self.history.retain(|_, categories| {
                categories.retain(|_, paths| {
                    paths.remove(current);
                    !paths.is_empty()
                });
                !categories.is_empty()
            });
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SplitsHistoryEntry {
    game: Box<str>,
    category: Box<str>,
    path: Box<Path>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct General {
    layout: Option<PathBuf>,
    #[serde(skip)]
    can_save_layout: bool,
    timing_method: Option<TimingMethod>,
    comparison: Option<String>,
    /// Legacy global auto-splitter path. Migrated on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auto_splitter: Option<PathBuf>,
    /// Legacy per-splits auto splitters. Auto splitters are stored in the
    /// splits file now, so these only migrate splits that predate that. An
    /// entry is dropped once its splits file stores an auto splitter itself.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    auto_splitters: BTreeMap<PathBuf, LegacyAutoSplitterAssociation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum LegacyAutoSplitterAssociation {
    Local {
        path: PathBuf,
    },
    Registry {
        game: String,
        last_url: String,
        cached_path: PathBuf,
    },
}

impl LegacyAutoSplitterAssociation {
    fn path(&self) -> &Path {
        match self {
            Self::Local { path } => path,
            Self::Registry { cached_path, .. } => cached_path,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Log {
    #[serde(default)]
    enable: bool,
    level: Option<log::LevelFilter>,
    #[serde(default)]
    clear: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[serde(default)]
struct Window {
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
    /// Ignore Mouse While Running and Not In Focus
    mouse_pass_through_while_running: bool,
}

impl Default for Window {
    fn default() -> Window {
        Self {
            width: 300.0,
            height: 500.0,
            x: None,
            y: None,
            mouse_pass_through_while_running: false,
        }
    }
}

static CONFIG_PATH: Lazy<PathBuf> = Lazy::new(|| {
    ProjectDirs::from("org", "LiveSplit", "LiveSplit One")
        .map(|dirs| dirs.data_local_dir().join("config.yml"))
        .unwrap_or_default()
});

impl Config {
    pub fn load(cli: cli::Cli) -> Self {
        let mut cfg = Self::parse().unwrap_or_default();
        cfg.load_splits_path(cli.splits);
        cfg.load_layout_path(cli.layout);
        cfg.migrate_legacy_autosplitter();
        cfg.cli_auto_splitter = cli.autosplitter;
        cfg.save_config();
        cfg
    }

    /// Replace the current splits file with the given path during load, before the window is initialized
    /// If the file path is invalid it keeps the split file specified in the config
    fn load_splits_path(&mut self, split_file: Option<PathBuf>) {
        if split_file.is_none() {
            return;
        };
        let split_file = split_file.unwrap();
        // This reads the file twice, once now and again below when splits are opened
        let maybe_run = Config::parse_run_from_path(&split_file);
        if maybe_run.is_none() {
            return;
        };
        let (run, _) = maybe_run.unwrap();
        self.splits.add_to_history(&run);
        self.splits.current = Some(split_file);
    }

    fn load_layout_path(&mut self, layout_file: Option<PathBuf>) {
        if layout_file.is_none() {
            return;
        };
        let layout_file = layout_file.unwrap();
        let maybe_layout = Self::parse_layout_with_path(&layout_file);
        if maybe_layout.is_err() {
            return;
        }
        self.general.can_save_layout = true;
        self.general.layout = Some(layout_file);
    }

    fn migrate_legacy_autosplitter(&mut self) {
        let path = self.general.auto_splitter.take();
        if let (Some(splits), Some(path)) = (self.splits.current.clone(), path) {
            self.general
                .auto_splitters
                .entry(splits)
                .or_insert(LegacyAutoSplitterAssociation::Local { path });
        }
    }

    fn save_config(&self) -> Option<()> {
        create_dir_all(CONFIG_PATH.parent()?).ok()?;
        self.serialize()
    }

    fn parse() -> Option<Self> {
        let buf = fs::read(CONFIG_PATH.as_path()).ok()?;
        serde_yaml::from_slice(&buf).ok()
    }

    fn serialize(&self) -> Option<()> {
        let buf = serde_yaml::to_string(self).ok()?;
        fs::write(CONFIG_PATH.as_path(), buf).ok()
    }

    pub fn splits_history(&self) -> &SplitsHistory {
        &self.splits.history
    }

    fn parse_run_from_path(path: &Path) -> Option<(Run, bool)> {
        let file = fs::read(path).ok()?;
        let parsed_run = composite::parse(&file, Some(path)).ok()?;
        let run = parsed_run.run;
        let can_save = parsed_run.kind == TimerKind::LiveSplit;
        Some((run, can_save))
    }

    fn parse_run(&self) -> Option<(Run, bool)> {
        let path = self.splits.current.clone()?;
        Config::parse_run_from_path(&path)
    }

    pub fn parse_run_or_default(&mut self) -> Run {
        match self.parse_run() {
            Some((run, can_save)) => {
                self.splits.can_save = can_save;
                run
            }
            None => {
                self.splits.can_save = false;
                default_run()
            }
        }
    }

    pub fn is_game_time(&self) -> bool {
        self.general.timing_method == Some(TimingMethod::GameTime)
    }

    fn parse_layout_with_path(path: &Path) -> Result<(Layout, bool)> {
        let file = fs::read_to_string(path).context("Failed reading the file.")?;
        match LayoutSettings::from_json(Cursor::new(&file)) {
            Ok(settings) => return Ok((Layout::from_settings(settings), true)),
            Err(err) => error!("Failed to parse layout as *.ls1l: {err}"),
        }
        layout::parser::parse(&file)
            .context("Failed parsing the layout.")
            .map(|layout| (layout, false))
    }

    fn parse_layout(&mut self, timer: &Timer) -> Option<Layout> {
        if let Some(linked_layout) = timer.run().linked_layout() {
            match linked_layout {
                LinkedLayout::Default => {
                    self.general.can_save_layout = false;
                    self.general.layout = None;
                    self.save_config();
                    return None;
                }
                LinkedLayout::Path(path) => {
                    if let Ok((layout, can_save)) = Self::parse_layout_with_path(Path::new(path)) {
                        self.general.can_save_layout = can_save;
                        self.general.layout = Some(path.into());
                        self.save_config();
                        return Some(layout);
                    }
                }
            }
        }

        let (layout, can_save) =
            Self::parse_layout_with_path(self.general.layout.as_deref()?).ok()?;
        self.general.can_save_layout = can_save;
        Some(layout)
    }

    pub fn parse_layout_or_default(&mut self, timer: &Timer) -> Layout {
        self.parse_layout(timer)
            .unwrap_or_else(|| Layout::default_layout(livesplit_core::Lang::English))
    }

    pub fn window_size(&self) -> (f64, f64) {
        (self.window.width, self.window.height)
    }

    pub fn set_window_size(&mut self, (width, height): (f64, f64)) {
        self.window.width = width;
        self.window.height = height;
        self.save_config();
    }

    pub fn set_window_position(&mut self, (x, y): (f64, f64)) {
        self.window.x = Some(x);
        self.window.y = Some(y);
        self.save_config();
    }

    pub fn get_mouse_pass_through_while_running(&self) -> bool {
        self.window.mouse_pass_through_while_running
    }

    pub fn set_mouse_pass_through_while_running(&mut self, b: bool) {
        self.window.mouse_pass_through_while_running = b;
    }

    // Just directly construct the HotkeySystem from the config.
    pub fn configure_hotkeys<E: event::CommandSink + Clone + Send + 'static>(
        &self,
        command_sink: E,
    ) -> livesplit_core::hotkey::Result<HotkeySystem<E>> {
        HotkeySystem::with_config(command_sink, self.hotkeys)
    }

    pub fn configure_timer(&self, timer: &mut Timer) {
        if self.is_game_time() {
            timer.set_current_timing_method(TimingMethod::GameTime);
        }
        if let Some(comparison) = &self.general.comparison {
            timer.set_current_comparison(comparison.as_str()).ok();
        }
    }

    pub fn set_hotkeys(&mut self, hotkeys: HotkeyConfig) {
        self.hotkeys = hotkeys;
        self.save_config();
    }

    pub fn hotkeys(&self) -> HotkeyConfig {
        self.hotkeys
    }

    pub fn new_splits(&mut self, timer: &mut Timer) {
        timer.set_run(default_run()).map_err(drop).unwrap();
        self.splits.can_save = false;
        self.splits.current = None;
        self.save_config();
    }

    pub fn open_splits(
        &mut self,
        shared_timer: &SharedTimer,
        layout_data: &mut LayoutData,
        #[cfg(feature = "auto-splitting")] auto_splitter: &livesplit_core::auto_splitting::Runtime<
            SharedTimer,
        >,
        path: PathBuf,
    ) -> Result<()> {
        {
            let timer = &mut shared_timer.write().unwrap();
            let file = fs::read(&path).context("Failed reading the file.")?;
            let run = composite::parse(&file, Some(&path)).context("Failed parsing the file.")?;
            timer.set_run(run.run).ok().context(
                "The splits can't be used with the timer because they don't contain a single segment.",
            )?;

            self.splits.can_save = run.kind == TimerKind::LiveSplit;
            self.splits.current = Some(path);
            self.splits.add_to_history(timer.run());

            self.save_config();

            if let Some(linked_layout) = timer.run().linked_layout() {
                match linked_layout {
                    LinkedLayout::Default => self.new_layout(None, layout_data),
                    LinkedLayout::Path(path) => {
                        let _ = self.open_layout(None, layout_data, Path::new(path));
                    }
                }
            }
        }

        // The splits are open at this point, so an auto splitter that fails to
        // load is reported without failing the whole operation.
        #[cfg(feature = "auto-splitting")]
        or_show_error(self.load_auto_splitter(auto_splitter, shared_timer));

        Ok(())
    }

    pub fn can_directly_save_splits(&self) -> bool {
        self.splits.current.is_some() && self.splits.can_save
    }

    pub fn splits_path(&self) -> Option<&Path> {
        self.splits.current.as_deref()
    }

    pub fn save_splits(&mut self, timer: &mut Timer) -> Result<()> {
        if let Some(path) = &self.splits.current {
            let mut buf = String::new();
            save_timer(timer, &mut buf).context("Failed saving the splits.")?;
            fs::write(path, &buf).context("Failed writing the file.")?;
            timer.mark_as_unmodified();

            self.splits.remove_from_history();
            self.splits.add_to_history(timer.run());

            self.save_config();
        }
        Ok(())
    }

    pub fn save_splits_as(&mut self, timer: &mut Timer, path: PathBuf) -> Result<()> {
        let mut buf = String::new();
        save_timer(timer, &mut buf).context("Failed saving the splits.")?;
        fs::write(&path, &buf).context("Failed writing the file.")?;
        timer.mark_as_unmodified();

        if !self.splits.can_save {
            self.splits.remove_from_history();
        }
        self.splits.can_save = true;
        self.splits.current = Some(path);
        self.splits.add_to_history(timer.run());

        self.save_config();
        Ok(())
    }

    pub fn link_layout(&self, run_editor: &mut RunEditor) {
        run_editor.set_linked_layout(Some(
            match &self.general.layout {
                Some(path) => path.to_str().map(|p| LinkedLayout::Path(p.to_owned())),
                None => None,
            }
            .unwrap_or(LinkedLayout::Default),
        ));
    }

    pub fn new_layout(&mut self, timer: Option<&mut Timer>, layout_data: &mut LayoutData) {
        self.general.can_save_layout = false;
        self.general.layout = None;
        layout_data.layout = Layout::default_layout(livesplit_core::Lang::English);
        layout_data.is_modified = false;

        if let Some(timer) = timer {
            timer.layout_path_changed(None::<&str>);
        }

        self.save_config();
    }

    pub fn open_layout(
        &mut self,
        timer: Option<&mut Timer>,
        layout_data: &mut LayoutData,
        path: &Path,
    ) -> Result<()> {
        let (layout, can_save) = Self::parse_layout_with_path(path)?;
        self.general.can_save_layout = can_save;
        self.general.layout = Some(path.into());
        layout_data.layout = layout;
        layout_data.is_modified = false;

        if let Some(timer) = timer {
            timer.layout_path_changed(path.to_str());
        }

        self.save_config();
        Ok(())
    }

    pub fn save_layout(&self, settings: LayoutSettings) -> Result<()> {
        if let Some(path) = &self.general.layout {
            let mut buf = Vec::new();
            settings
                .write_json(&mut buf)
                .context("Failed saving the layout.")?;
            fs::write(path, &buf).context("Failed writing the file.")?;
        }
        Ok(())
    }

    pub fn save_layout_as(
        &mut self,
        timer: &mut Timer,
        settings: LayoutSettings,
        path: PathBuf,
    ) -> Result<()> {
        let mut buf = Vec::new();
        settings
            .write_json(&mut buf)
            .context("Failed saving the layout.")?;
        fs::write(&path, &buf).context("Failed writing the file.")?;

        timer.layout_path_changed(path.to_str());

        self.general.can_save_layout = true;
        self.general.layout = Some(path);
        self.save_config();
        Ok(())
    }

    pub fn can_directly_save_layout(&self) -> bool {
        self.general.layout.is_some() && self.general.can_save_layout
    }

    pub fn set_comparison(&mut self, comparison: String) {
        self.general.comparison = Some(comparison);
        self.save_config();
    }

    pub fn set_timing_method(&mut self, timing_method: TimingMethod) {
        self.general.timing_method = Some(timing_method);
        self.save_config();
    }

    pub fn setup_logging(&self) -> Option<()> {
        if self.log.enable {
            let config_folder = CONFIG_PATH.parent()?;
            create_dir_all(config_folder).ok()?;

            let log_file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(!self.log.clear)
                .truncate(self.log.clear)
                .open(config_folder.join("log.txt"))
                .ok()?;

            fern::Dispatch::new()
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "{}[{}][{}] {}",
                        chrono::Local::now().format("[%Y-%m-%d %H:%M:%S]"),
                        record.target(),
                        record.level(),
                        message
                    ))
                })
                .level(self.log.level.unwrap_or(log::LevelFilter::Warn))
                .chain(log_file)
                .apply()
                .ok()?;

            #[cfg(not(debug_assertions))]
            {
                std::panic::set_hook(Box::new(|panic_info| {
                    log::error!(target: "PANIC", "{}\n{:?}", panic_info, backtrace::Backtrace::new());
                }));
            }
        }
        Some(())
    }

    pub fn window_position(&self) -> Option<(f64, f64)> {
        Some((self.window.x?, self.window.y?))
    }

    /// Returns the auto splitter to load instead of the one stored in the run,
    /// if any. Drops the legacy association of splits that store their own.
    #[cfg(feature = "auto-splitting")]
    fn auto_splitter_override(&mut self, run: &Run) -> Option<PathBuf> {
        if let Some(path) = self.cli_auto_splitter.take() {
            return Some(path);
        }
        let splits = self.splits.current.as_ref()?;
        let stores_own = run
            .stored_auto_splitter_settings()
            .is_ok_and(|stored| stored.script_path().is_some_and(|path| !path.is_empty()));
        if stores_own {
            self.general.auto_splitters.remove(splits);
            return None;
        }
        Some(self.general.auto_splitters.get(splits)?.path().to_owned())
    }

    /// Loads the auto splitter for the timer's run. An auto splitter passed on
    /// the command line takes precedence over the one stored in the splits
    /// file. Splits that don't store one yet fall back to their legacy
    /// association from the config, which is then written into the run, so the
    /// next save migrates it into the splits file.
    #[cfg(feature = "auto-splitting")]
    pub fn load_auto_splitter(
        &mut self,
        runtime: &livesplit_core::auto_splitting::Runtime<SharedTimer>,
        timer: &SharedTimer,
    ) -> Result<()> {
        let legacy_count = self.general.auto_splitters.len();
        let override_path = self.auto_splitter_override(timer.read().unwrap().run());
        if self.general.auto_splitters.len() != legacy_count {
            self.save_config();
        }
        match override_path {
            Some(path) => {
                runtime
                    .load_from_path(timer.clone(), path)
                    .context("Auto Splitter failed to load.")?;
                runtime.store_settings();
            }
            None => {
                runtime
                    .load(timer.clone())
                    .context("Auto Splitter failed to load.")?;
            }
        }
        Ok(())
    }
}

fn default_run() -> Run {
    let mut run = Run::new();
    run.push_segment(Segment::new("Time"));
    run
}

pub fn show_error(error: anyhow::Error) {
    use gtk::prelude::*;

    log::error!("{error:?}");
    let detail = error
        .chain()
        .skip(1)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let dialog = gtk::AlertDialog::builder()
        .message(error.to_string())
        .detail(detail)
        .modal(true)
        .build();
    let parent = gio::Application::default()
        .and_downcast::<gtk::Application>()
        .and_then(|application| application.active_window());
    dialog.show(parent.as_ref());
}

pub fn or_show_error(result: Result<()>) {
    if let Err(e) = result {
        show_error(e);
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, LegacyAutoSplitterAssociation};
    #[cfg(feature = "auto-splitting")]
    use livesplit_core::{run::parser::composite, Run, StoredAutoSplitterSettings};
    use std::path::Path;
    #[cfg(feature = "auto-splitting")]
    use std::path::PathBuf;

    const LEGACY_CONFIG: &str = r#"
splits:
  current: /splits/game.lss
general:
  auto-splitters:
    /splits/game.lss:
      kind: local
      path: /splitters/local.wasm
    /splits/other.lss:
      kind: registry
      game: Other Game
      last_url: https://example.com/other.wasm
      cached_path: /cache/Other_Game/other.wasm
"#;

    #[test]
    fn legacy_associations_deserialize() {
        let config: Config = serde_yaml::from_str(LEGACY_CONFIG).unwrap();
        let associations = &config.general.auto_splitters;
        assert_eq!(
            associations[Path::new("/splits/game.lss")].path(),
            Path::new("/splitters/local.wasm")
        );
        assert!(matches!(
            &associations[Path::new("/splits/other.lss")],
            LegacyAutoSplitterAssociation::Registry { cached_path, .. }
                if cached_path == Path::new("/cache/Other_Game/other.wasm")
        ));
    }

    #[test]
    fn legacy_global_auto_splitter_migrates_to_current_splits() {
        let mut config: Config = serde_yaml::from_str(
            "splits:\n  current: /splits/new.lss\ngeneral:\n  auto-splitter: /splitters/global.wasm\n",
        )
        .unwrap();
        config.migrate_legacy_autosplitter();
        assert_eq!(config.general.auto_splitter, None);
        assert_eq!(
            config.general.auto_splitters[Path::new("/splits/new.lss")].path(),
            Path::new("/splitters/global.wasm")
        );
    }

    #[test]
    fn legacy_global_auto_splitter_keeps_existing_association() {
        let mut config: Config = serde_yaml::from_str(LEGACY_CONFIG).unwrap();
        config.general.auto_splitter = Some("/splitters/global.wasm".into());
        config.migrate_legacy_autosplitter();
        assert_eq!(
            config.general.auto_splitters[Path::new("/splits/game.lss")].path(),
            Path::new("/splitters/local.wasm")
        );
    }

    #[test]
    fn empty_legacy_associations_are_not_serialized() {
        let encoded = serde_yaml::to_string(&Config::default()).unwrap();
        assert!(!encoded.contains("auto-splitter"), "{encoded}");
    }

    #[cfg(feature = "auto-splitting")]
    fn run_with_script_path(path: Option<&str>) -> Run {
        let mut run = Run::new();
        let mut stored = StoredAutoSplitterSettings::new();
        stored.set_script_path(path);
        run.set_stored_auto_splitter_settings(&stored);
        run
    }

    #[cfg(feature = "auto-splitting")]
    #[test]
    fn splits_without_a_stored_auto_splitter_use_the_legacy_association() {
        let mut config: Config = serde_yaml::from_str(LEGACY_CONFIG).unwrap();
        assert_eq!(
            config.auto_splitter_override(&run_with_script_path(None)),
            Some(PathBuf::from("/splitters/local.wasm"))
        );
        // Kept until the splits file has been saved with its auto splitter.
        assert!(config
            .general
            .auto_splitters
            .contains_key(Path::new("/splits/game.lss")));
    }

    #[cfg(feature = "auto-splitting")]
    #[test]
    fn splits_storing_an_auto_splitter_drop_the_legacy_association() {
        let mut config: Config = serde_yaml::from_str(LEGACY_CONFIG).unwrap();
        let run = run_with_script_path(Some("/splitters/stored.wasm"));
        assert_eq!(config.auto_splitter_override(&run), None);
        assert!(!config
            .general
            .auto_splitters
            .contains_key(Path::new("/splits/game.lss")));
        // Other splits files keep theirs.
        assert!(config
            .general
            .auto_splitters
            .contains_key(Path::new("/splits/other.lss")));
    }

    #[cfg(feature = "auto-splitting")]
    #[test]
    fn command_line_auto_splitter_takes_precedence() {
        let mut config: Config = serde_yaml::from_str(LEGACY_CONFIG).unwrap();
        config.cli_auto_splitter = Some("/splitters/cli.wasm".into());
        let run = run_with_script_path(Some("/splitters/stored.wasm"));
        assert_eq!(
            config.auto_splitter_override(&run),
            Some(PathBuf::from("/splitters/cli.wasm"))
        );
        // It only applies to the splits loaded on startup.
        assert_eq!(config.auto_splitter_override(&run), None);
    }

    #[cfg(feature = "auto-splitting")]
    #[test]
    fn splits_without_a_legacy_association_have_no_override() {
        let mut config = Config::default();
        assert_eq!(
            config.auto_splitter_override(&run_with_script_path(None)),
            None
        );
    }

    /// Splits saved by earlier versions store the settings without a script
    /// path. They need to keep parsing, so migrating the legacy association
    /// keeps its settings.
    #[cfg(feature = "auto-splitting")]
    #[test]
    fn settings_from_earlier_versions_parse_without_a_script_path() {
        let lss = r#"<?xml version="1.0" encoding="UTF-8"?>
<Run version="1.8.0">
  <GameIcon />
  <GameName>Game</GameName>
  <CategoryName>Any%</CategoryName>
  <Metadata><Run id="" /><Platform usesEmulator="False"></Platform><Region></Region><Variables /></Metadata>
  <Offset>00:00:00</Offset>
  <AttemptCount>0</AttemptCount>
  <AttemptHistory />
  <Segments>
    <Segment><Name>Time</Name><Icon /><SplitTimes><SplitTime name="Personal Best" /></SplitTimes><BestSegmentTime /><SegmentHistory /></Segment>
  </Segments>
  <AutoSplitterSettings>
    <Version>1.0</Version>
    <CustomSettings>
      <Setting id="start" type="bool">True</Setting>
      <Setting id="mode" type="string" value="fast" />
    </CustomSettings>
  </AutoSplitterSettings>
</Run>"#;
        let run = composite::parse(lss.as_bytes(), None).unwrap().run;
        let stored = run.stored_auto_splitter_settings().unwrap();
        assert_eq!(stored.script_path(), None);
        let settings = stored.into_settings_map();
        assert!(settings.get("start").is_some());
        assert!(settings.get("mode").is_some());
    }

    #[test]
    fn legacy_window_fields_round_trip() {
        let yaml = r#"
window:
  width: 420.0
  height: 240.0
  x: 17.0
  y: 31.0
  mouse-pass-through-while-running: true
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.window_size(), (420.0, 240.0));
        assert_eq!(config.window_position(), Some((17.0, 31.0)));
        assert!(config.get_mouse_pass_through_while_running());

        let encoded = serde_yaml::to_string(&config).unwrap();
        let decoded: Config = serde_yaml::from_str(&encoded).unwrap();
        assert_eq!(decoded.window_position(), Some((17.0, 31.0)));
    }
}

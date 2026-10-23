use super::*;

#[cfg(feature = "auto-splitting")]
fn auto_splitter_summary(path: Option<&std::path::Path>) -> String {
    match path {
        Some(path) if crate::autosplitter_registry::is_managed(path) => {
            format!("Registry — {}", path.display())
        }
        Some(path) => format!("Local — {}", path.display()),
        None => "No auto splitter loaded".to_owned(),
    }
}

#[cfg(feature = "auto-splitting")]
pub(super) fn build_run_auto_splitter_section(
    parent: &adw::ApplicationWindow,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    changed: &Rc<Cell<bool>>,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
    timer: &SharedTimer,
) -> gtk::Widget {
    let loaded_path = runtime.loaded_path();
    let row = adw::ActionRow::builder()
        .title("Auto Splitter")
        .subtitle(auto_splitter_summary(loaded_path.as_deref()))
        .build();
    let settings = gtk::Button::with_label("Settings…");
    settings.set_sensitive(loaded_path.is_some());
    let remove = gtk::Button::from_icon_name("edit-delete-symbolic");
    remove.set_tooltip_text(Some("Remove auto splitter"));
    let registry = gtk::Button::with_label("Find in Registry");
    let registry_editor = editor.clone();
    let registry_changed = changed.clone();
    let registry_runtime = runtime.clone();
    let registry_timer = timer.clone();
    let registry_row = row.clone();
    let registry_settings = settings.clone();
    registry.connect_clicked(move |button| {
        // Read the game name on click, as it may have been edited since the
        // dialog was opened.
        let game = registry_editor
            .borrow()
            .as_ref()
            .map(|editor| editor.run().game_name().trim().to_owned())
            .unwrap_or_default();
        if game.is_empty() {
            registry_row.set_subtitle("Enter a game name before searching the registry");
            return;
        }
        button.set_sensitive(false);
        registry_row.set_subtitle("Checking the LiveSplit auto splitter registry…");
        let download = relm4::spawn(crate::autosplitter_registry::download_for_game(game));
        let changed = registry_changed.clone();
        let runtime = registry_runtime.clone();
        let timer = registry_timer.clone();
        let status = registry_row.clone();
        let button = button.clone();
        let settings = registry_settings.clone();
        glib::spawn_future_local(async move {
            let result = match download.await {
                Ok(result) => result,
                Err(error) => Err(anyhow::anyhow!("Registry lookup failed: {error}")),
            };
            button.set_sensitive(true);
            let (temporary, destination) = match result {
                Ok(paths) => paths,
                Err(error) => {
                    status.set_subtitle(&format!("{error:#}"));
                    return;
                }
            };
            if timer.read().unwrap().current_phase() != livesplit_core::TimerPhase::NotRunning {
                let _ = std::fs::remove_file(temporary);
                status.set_subtitle("Reset the timer before changing the auto splitter");
                return;
            }
            changed.set(true);
            if let Err(error) = runtime.load_from_path(timer.clone(), temporary.clone()) {
                let _ = std::fs::remove_file(temporary);
                status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                settings.set_sensitive(false);
                return;
            }
            // Load once more from the installed location, so that's the path
            // stored in the splits file rather than the temporary download.
            let installed = std::fs::rename(&temporary, &destination)
                .map_err(anyhow::Error::from)
                .and_then(|()| Ok(runtime.load_from_path(timer.clone(), destination.clone())?));
            match installed {
                Ok(()) => {
                    status.set_subtitle(&auto_splitter_summary(Some(&destination)));
                    settings.set_sensitive(true);
                }
                Err(error) => {
                    let _ = std::fs::remove_file(temporary);
                    let _ = runtime.unload();
                    status.set_subtitle(&format!("Could not install auto splitter: {error}"));
                    settings.set_sensitive(false);
                }
            }
        });
    });
    row.add_suffix(&registry);

    let local = gtk::Button::with_label("Select Local…");
    let local_parent = parent.clone();
    let local_changed = changed.clone();
    let local_runtime = runtime.clone();
    let local_timer = timer.clone();
    let local_row = row.clone();
    let local_settings = settings.clone();
    local.connect_clicked(move |_| {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("WASM Auto Splitters"));
        filter.add_pattern("*.wasm");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Select Local WASM Auto Splitter")
            .filters(&filters)
            .build();
        let changed = local_changed.clone();
        let runtime = local_runtime.clone();
        let timer = local_timer.clone();
        let status = local_row.clone();
        let settings = local_settings.clone();
        dialog.open(Some(&local_parent), gio::Cancellable::NONE, move |result| {
            let Some(path) = result.ok().and_then(|file| file.path()) else {
                return;
            };
            if timer.read().unwrap().current_phase() != livesplit_core::TimerPhase::NotRunning {
                status.set_subtitle("Reset the timer before changing the auto splitter");
                return;
            }
            changed.set(true);
            match runtime.load_from_path(timer.clone(), path.clone()) {
                Ok(()) => {
                    status.set_subtitle(&auto_splitter_summary(Some(&path)));
                    settings.set_sensitive(true);
                }
                Err(error) => {
                    status.set_subtitle(&format!("Auto splitter failed to load: {error}"));
                    settings.set_sensitive(false);
                }
            }
        });
    });
    row.add_suffix(&local);

    let settings_parent = parent.clone();
    let settings_changed = changed.clone();
    let settings_runtime = runtime.clone();
    settings.connect_clicked(move |_| {
        open_auto_splitter_settings(&settings_parent, &settings_changed, &settings_runtime);
    });
    row.add_suffix(&settings);
    let remove_changed = changed.clone();
    let remove_runtime = runtime.clone();
    let remove_row = row.clone();
    let remove_settings = settings.clone();
    remove.connect_clicked(move |_| match remove_runtime.unload() {
        Ok(()) => {
            remove_changed.set(true);
            remove_row.set_subtitle(&auto_splitter_summary(None));
            remove_settings.set_sensitive(false);
        }
        Err(error) => remove_row.set_subtitle(&format!("Could not unload: {error}")),
    });
    row.add_suffix(&remove);
    row.upcast()
}

#[cfg(feature = "auto-splitting")]
fn open_auto_splitter_settings(
    parent: &adw::ApplicationWindow,
    changed: &Rc<Cell<bool>>,
    runtime: &Rc<livesplit_core::auto_splitting::Runtime<SharedTimer>>,
) {
    use livesplit_core::auto_splitting::settings::{Value, WidgetKind};

    let window = adw::ApplicationWindow::builder()
        .title("Auto Splitter Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(520)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Auto Splitter")
        .description("Changes are part of the Splits Editor transaction")
        .build();
    let widgets = runtime.settings_widgets().unwrap_or_default();
    let settings_map = runtime.settings_map().unwrap_or_default();
    if widgets.is_empty() {
        let loading = adw::ActionRow::builder()
            .title("Loading settings…")
            .subtitle("Waiting for the auto splitter to initialize")
            .build();
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        loading.add_suffix(&spinner);
        group.add(&loading);
        let attempts = Rc::new(Cell::new(0_u8));
        let retry_changed = changed.clone();
        let retry_runtime = runtime.clone();
        let retry_parent = parent.clone();
        let retry_window = window.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if retry_runtime
                .settings_widgets()
                .is_some_and(|widgets| !widgets.is_empty())
            {
                retry_window.close();
                open_auto_splitter_settings(&retry_parent, &retry_changed, &retry_runtime);
                return glib::ControlFlow::Break;
            }
            let next = attempts.get() + 1;
            attempts.set(next);
            if next >= 20 {
                spinner.set_spinning(false);
                spinner.set_visible(false);
                loading.set_title("No configurable settings");
                loading.set_subtitle("The loaded auto splitter did not expose any settings");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    for widget in widgets.iter() {
        match &widget.kind {
            WidgetKind::Title { .. } => {
                let title = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .build();
                title.add_css_class("property");
                group.add(&title);
            }
            WidgetKind::Bool { default_value } => {
                let value = settings_map
                    .get(widget.key.as_ref())
                    .and_then(|value| match value {
                        Value::Bool(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(*default_value);
                let row = adw::SwitchRow::builder()
                    .title(widget.description.as_ref())
                    .active(value)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let changed = changed.clone();
                let runtime = runtime.clone();
                row.connect_active_notify(move |row| {
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::Bool(row.is_active()));
                    changed.set(true);
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::Choice {
                default_option_key,
                options,
            } => {
                let model = gtk::StringList::new(
                    &options
                        .iter()
                        .map(|option| option.description.as_ref())
                        .collect::<Vec<_>>(),
                );
                let current_key = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .unwrap_or(default_option_key);
                let selected = options
                    .iter()
                    .position(|option| option.key.as_ref() == current_key.as_ref())
                    .unwrap_or(0) as u32;
                let row = adw::ComboRow::builder()
                    .title(widget.description.as_ref())
                    .model(&model)
                    .selected(selected)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let options = options.clone();
                let changed = changed.clone();
                let runtime = runtime.clone();
                row.connect_selected_notify(move |row| {
                    let Some(option) = options.get(row.selected() as usize) else {
                        return;
                    };
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::String(option.key.clone()));
                    changed.set(true);
                    runtime.set_settings_map(map);
                });
                group.add(&row);
            }
            WidgetKind::FileSelect { filters } => {
                let current_path = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .map_or_else(String::new, ToString::to_string);
                let row = adw::ActionRow::builder()
                    .title(widget.description.as_ref())
                    .subtitle(current_path)
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let choose = gtk::Button::with_label("Choose…");
                let key = widget.key.clone();
                let runtime = runtime.clone();
                let changed = changed.clone();
                let parent = window.clone();
                let status = row.clone();
                let filters = filters.clone();
                choose.connect_clicked(move |_| {
                    let dialog = gtk::FileDialog::builder().title("Select File").build();
                    let gtk_filters = gio::ListStore::new::<gtk::FileFilter>();
                    for filter in filters.iter() {
                        let gtk_filter = gtk::FileFilter::new();
                        match filter {
                            livesplit_core::auto_splitting::settings::FileFilter::Name {
                                description,
                                pattern,
                            } => {
                                gtk_filter.set_name(description.as_deref());
                                for pattern in pattern.split_ascii_whitespace() {
                                    gtk_filter.add_pattern(pattern);
                                }
                            }
                            livesplit_core::auto_splitting::settings::FileFilter::MimeType(
                                mime,
                            ) => {
                                gtk_filter.set_name(Some(mime));
                                gtk_filter.add_mime_type(mime);
                            }
                        }
                        gtk_filters.append(&gtk_filter);
                    }
                    if gtk_filters.n_items() != 0 {
                        dialog.set_filters(Some(&gtk_filters));
                    }
                    let key = key.clone();
                    let changed = changed.clone();
                    let runtime = runtime.clone();
                    let status = status.clone();
                    dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                        let Some(path) = result.ok().and_then(|file| file.path()) else {
                            return;
                        };
                        let Some(path_value) =
                            livesplit_core::auto_splitting::wasi_path::from_native(&path)
                        else {
                            return;
                        };
                        let mut map = runtime.settings_map().unwrap_or_default();
                        map.insert(key.clone(), Value::String(path_value.into()));
                        changed.set(true);
                        runtime.set_settings_map(map);
                        status.set_subtitle(&path.to_string_lossy());
                    });
                });
                row.add_suffix(&choose);
                group.add(&row);
            }
            WidgetKind::TextInput { default_value } => {
                let value = settings_map
                    .get(widget.key.as_ref())
                    .and_then(Value::as_string)
                    .unwrap_or(default_value);
                let row = adw::EntryRow::builder()
                    .title(widget.description.as_ref())
                    .text(value.as_ref())
                    .build();
                row.set_tooltip_text(widget.tooltip.as_deref());
                let key = widget.key.clone();
                let changed = changed.clone();
                let runtime = runtime.clone();
                row.connect_changed(move |row| {
                    let mut map = runtime.settings_map().unwrap_or_default();
                    map.insert(key.clone(), Value::String(row.text().as_str().into()));
                    runtime.set_settings_map(map);
                    changed.set(true);
                });
                group.add(&row);
            }
        }
    }
    page.add(&group);
    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));
    window.present();
}

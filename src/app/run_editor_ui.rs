use super::*;

#[cfg(feature = "auto-splitting")]
mod autosplitter_ui;
mod run_details_ui;
mod run_segments;

#[cfg(feature = "auto-splitting")]
use autosplitter_ui::build_run_auto_splitter_section;
use run_details_ui::{
    build_icon_picker, build_run_comparisons_page, build_run_details_page,
    install_run_tool_actions, populate_speedrun_com_variable_rows, run_tools_menu,
};
use run_segments::populate_run_segments;

fn desired_run_row_selection(
    previous: &[i32],
    clicked: i32,
    modifiers: gdk::ModifierType,
    anchor: Option<i32>,
) -> (Vec<i32>, i32) {
    let mut desired = if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
        let anchor = anchor.unwrap_or(clicked);
        let (start, end) = if anchor <= clicked {
            (anchor, clicked)
        } else {
            (clicked, anchor)
        };
        (start..=end).collect::<Vec<_>>()
    } else if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
        let mut desired = previous.to_vec();
        if let Some(position) = desired.iter().position(|&index| index == clicked) {
            if desired.len() > 1 {
                desired.remove(position);
            }
        } else {
            desired.push(clicked);
        }
        desired
    } else {
        vec![clicked]
    };
    desired.sort_unstable();
    desired.dedup();
    let anchor = if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
        anchor.unwrap_or(clicked)
    } else {
        clicked
    };
    (desired, anchor)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunRowClickTarget {
    Row,
    Editable,
    Control,
    DragHandle,
}

fn run_row_click_target(
    mut widget: Option<gtk::Widget>,
    row: &gtk::ListBoxRow,
) -> RunRowClickTarget {
    let row: gtk::Widget = row.clone().upcast();
    while let Some(current) = widget {
        if current == row {
            break;
        }
        if current.has_css_class("drag-handle") {
            return RunRowClickTarget::DragHandle;
        }
        if current.is::<gtk::Entry>() {
            return RunRowClickTarget::Editable;
        }
        if current.is::<gtk::Button>()
            || current.is::<gtk::DropDown>()
            || current.is::<gtk::Switch>()
        {
            return RunRowClickTarget::Control;
        }
        widget = current.parent();
    }
    RunRowClickTarget::Row
}

fn sync_run_row_selection(
    list: &gtk::ListBox,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
) {
    let selected = list.selected_rows();
    let Some((first, rest)) = selected.split_first() else {
        return;
    };
    if let Some(editor) = editor.borrow_mut().as_mut() {
        let state = editor.state(&mut ImageCache::new(), livesplit_core::Lang::English);
        let select_row =
            |editor: &mut livesplit_core::RunEditor, row: &gtk::ListBoxRow, additional: bool| {
                let Some(row) = state.rows.get(row.index() as usize) else {
                    return;
                };
                match row {
                    livesplit_core::run::editor::RowState::Segment(segment) => {
                        if additional {
                            editor.select_additionally(segment.segment_index);
                        } else {
                            editor.select_only(segment.segment_index);
                        }
                    }
                    livesplit_core::run::editor::RowState::SegmentGroup(group) => {
                        if additional {
                            let _ = editor.toggle_segment_group_selection(group.group_index);
                        } else {
                            let _ = editor.select_segment_group(group.group_index);
                        }
                    }
                }
            };
        select_row(editor, first, false);
        for row in rest {
            select_row(editor, row, true);
        }
    }
}

fn apply_run_row_selection(
    list: &gtk::ListBox,
    editor: &Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    updating: &Cell<bool>,
    desired: &[i32],
) {
    updating.set(true);
    list.unselect_all();
    for &index in desired {
        if let Some(row) = list.row_at_index(index) {
            list.select_row(Some(&row));
        }
    }
    updating.set(false);
    sync_run_row_selection(list, editor);
}

pub(super) fn build_run_editor(
    parent: &gtk::ApplicationWindow,
    editor: Rc<RefCell<Option<livesplit_core::RunEditor>>>,
    sender: &ComponentSender<AppModel>,
    #[cfg(feature = "auto-splitting")] auto_splitter: Rc<
        livesplit_core::auto_splitting::Runtime<SharedTimer>,
    >,
    timer: SharedTimer,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .title("Edit Splits")
        .transient_for(parent)
        .default_width(980)
        .default_height(650)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let pages = adw::ViewStack::new();
    pages.set_vexpand(true);
    let page_switcher = adw::ViewSwitcher::builder()
        .stack(&pages)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let splits_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    #[cfg(feature = "auto-splitting")]
    let auto_splitter_changed = Rc::new(Cell::new(false));
    #[cfg(feature = "auto-splitting")]
    let auto_splitter_applied = Rc::new(Cell::new(false));

    let general = adw::PreferencesGroup::builder().title("Run").build();
    general.set_hexpand(true);
    let state = editor
        .borrow()
        .as_ref()
        .unwrap()
        .state(&mut ImageCache::new(), livesplit_core::Lang::English);
    let details = build_run_details_page(&editor, &state);
    // Set while the run's own game and category are matched against
    // speedrun.com when the editor opens. That only loads suggestions and must
    // not modify the run, so the handlers below don't touch the editor then.
    let auto_selecting = Rc::new(Cell::new(false));
    let game = CompletionEntryRow::new("Game", &state.game);
    let game_editor = editor.clone();
    let game_auto_selecting = auto_selecting.clone();
    game.connect_changed(move |row| {
        if game_auto_selecting.get() {
            return;
        }
        if let Some(editor) = game_editor.borrow_mut().as_mut() {
            editor.set_game_name(row.text().as_str());
        }
    });
    general.add(&game.row);
    let src_spinner = gtk::Spinner::new();
    src_spinner.set_tooltip_text(Some("Searching speedrun.com"));
    game.add_suffix(&src_spinner);
    let category = CompletionEntryRow::new("Category", &state.category);
    let category_editor = editor.clone();
    let category_auto_selecting = auto_selecting.clone();
    category.connect_changed(move |row| {
        if category_auto_selecting.get() {
            return;
        }
        if let Some(editor) = category_editor.borrow_mut().as_mut() {
            editor.set_category_name(row.text().as_str());
        }
    });
    general.add(&category.row);
    let src_platforms = details.platform_entry.clone();
    let src_regions = details.region_entry.clone();
    let game_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Game>::new()));
    let category_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Category>::new()));
    let platform_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Choice>::new()));
    let region_results = Rc::new(RefCell::new(Vec::<crate::speedrun_com::Choice>::new()));
    let populating_suggestions = Rc::new(Cell::new(false));
    let search_generation = Rc::new(Cell::new(0_u64));
    let metadata_generation = Rc::new(Cell::new(0_u64));
    let variable_generation = Rc::new(Cell::new(0_u64));
    let initial_game_name = state.game.clone();
    let initial_category_name = state.category.clone();
    let auto_select_game = Rc::new(Cell::new(initial_game_name.trim().len() >= 2));
    let auto_select_category = Rc::new(Cell::new(false));
    let search_results = game_results.clone();
    let search_status = game.clone();
    let search_spinner = src_spinner.clone();
    let search_populating = populating_suggestions.clone();
    let changed_game_completion = game.clone();
    let search_generation_changed = search_generation.clone();
    let search_auto_select_game = auto_select_game.clone();
    let search_auto_select_category = auto_select_category.clone();
    let search_initial_game = initial_game_name.clone();
    let search_auto_selecting = auto_selecting.clone();
    game.connect_changed(move |row| {
        if search_auto_selecting.get() {
            return;
        }
        let query = row.text().trim().to_owned();
        let generation = search_generation_changed.get() + 1;
        search_generation_changed.set(generation);
        let completion = changed_game_completion.clone();
        let auto_select_game = search_auto_select_game.clone();
        let auto_select_category = search_auto_select_category.clone();
        let initial_game = search_initial_game.clone();
        let auto_selecting = search_auto_selecting.clone();
        glib::idle_add_local_once(move || completion.clear_items());
        if query.len() < 2 {
            search_spinner.set_spinning(false);
            search_status.set_tooltip_text(Some("Type at least two characters for suggestions"));
            return;
        }
        search_spinner.set_spinning(true);
        search_status.set_tooltip_text(Some("Waiting to search speedrun.com…"));
        let generation_state = search_generation_changed.clone();
        let results = search_results.clone();
        let status = search_status.clone();
        let spinner = search_spinner.clone();
        let populating = search_populating.clone();
        let completion = changed_game_completion.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
            if generation_state.get() != generation {
                return;
            }
            status.set_tooltip_text(Some("Searching speedrun.com…"));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(
                    crate::speedrun_com::search_games(&query).map_err(|error| error.to_string()),
                );
            });
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                // A worker that died without answering is reported like a failed request.
                let response = match rx.try_recv() {
                    Ok(response) => response,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("the request stopped unexpectedly".to_owned())
                    }
                };
                if generation_state.get() != generation {
                    return glib::ControlFlow::Break;
                }
                spinner.set_spinning(false);
                match response {
                    Ok(games) => {
                        let count = games.len();
                        let names = games
                            .iter()
                            .map(|game| game.name.as_str())
                            .collect::<Vec<_>>();
                        populating.set(true);
                        completion.set_items(&names);
                        *results.borrow_mut() = games;
                        populating.set(false);
                        completion.show_dropdown();
                        if auto_select_game.replace(false) {
                            if let Some(index) = results
                                .borrow()
                                .iter()
                                .position(|game| game.name.eq_ignore_ascii_case(&initial_game))
                            {
                                auto_select_category.set(true);
                                // Selecting replaces the entry's text with
                                // speedrun.com's spelling, so restore it.
                                auto_selecting.set(true);
                                completion.select(index);
                                completion.set_text(&initial_game);
                                auto_selecting.set(false);
                            }
                        }
                        status
                            .set_tooltip_text(Some(&format!("Found {} speedrun.com games", count)));
                    }
                    Err(error) => status
                        .set_tooltip_text(Some(&format!("Speedrun.com search failed: {error}"))),
                }
                glib::ControlFlow::Break
            });
        });
    });
    let selected_games = game_results.clone();
    let selected_categories = category_results.clone();
    let selected_platforms = platform_results.clone();
    let selected_regions = region_results.clone();
    let selected_editor = editor.clone();
    let selected_game_entry = game.clone();
    let selected_status = game.clone();
    let selected_spinner = src_spinner.clone();
    let selected_populating = populating_suggestions.clone();
    let selected_auto_category = auto_select_category.clone();
    let selected_initial_category = initial_category_name.clone();
    let selected_category_completion = category.clone();
    let selected_platform_completion = details.platform_entry.clone();
    let selected_region_completion = details.region_entry.clone();
    let selected_metadata_generation = metadata_generation.clone();
    let selected_auto_selecting = auto_selecting.clone();
    game.connect_selected(move |selected_index| {
        if selected_populating.get() {
            return;
        }
        let Some(game) = selected_games.borrow().get(selected_index).cloned() else {
            return;
        };
        let generation = selected_metadata_generation.get() + 1;
        selected_metadata_generation.set(generation);
        if !selected_auto_selecting.get() {
            selected_game_entry.set_text(&game.name);
            if let Some(editor) = selected_editor.borrow_mut().as_mut() {
                editor.set_game_name(game.name.as_str());
            }
        }
        selected_spinner.set_spinning(true);
        selected_status.set_tooltip_text(Some("Loading speedrun.com metadata…"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                Ok((
                    crate::speedrun_com::categories(&game.id)?,
                    crate::speedrun_com::platforms(&game.id)?,
                    crate::speedrun_com::regions(&game.id)?,
                ))
            })()
            .map_err(|error: anyhow::Error| error.to_string());
            let _ = tx.send(result);
        });
        let categories = selected_categories.clone();
        let platforms = selected_platforms.clone();
        let regions = selected_regions.clone();
        let status = selected_status.clone();
        let spinner = selected_spinner.clone();
        let populating = selected_populating.clone();
        let category_completion = selected_category_completion.clone();
        let platform_completion = selected_platform_completion.clone();
        let region_completion = selected_region_completion.clone();
        let auto_category = selected_auto_category.clone();
        let initial_category = selected_initial_category.clone();
        let generation_state = selected_metadata_generation.clone();
        let auto_selecting = selected_auto_selecting.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            // A worker that died without answering is reported like a failed request.
            let response = match rx.try_recv() {
                Ok(response) => response,
                Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("the request stopped unexpectedly".to_owned())
                }
            };
            if generation_state.get() != generation {
                return glib::ControlFlow::Break;
            }
            spinner.set_spinning(false);
            match response {
                Ok((found_categories, found_platforms, found_regions)) => {
                    populating.set(true);
                    let names = found_categories
                        .iter()
                        .map(|category| category.name.as_str())
                        .collect::<Vec<_>>();
                    category_completion.set_items(&names);

                    let names = found_platforms
                        .iter()
                        .map(|platform| platform.name.as_str())
                        .collect::<Vec<_>>();
                    platform_completion.set_items(&names);

                    let names = found_regions
                        .iter()
                        .map(|region| region.name.as_str())
                        .collect::<Vec<_>>();
                    region_completion.set_items(&names);

                    status.set_tooltip_text(Some(&format!(
                        "Found {} categories, {} platforms, and {} regions",
                        found_categories.len(),
                        found_platforms.len(),
                        found_regions.len()
                    )));
                    *categories.borrow_mut() = found_categories;
                    *platforms.borrow_mut() = found_platforms;
                    *regions.borrow_mut() = found_regions;
                    populating.set(false);
                    if auto_category.replace(false) {
                        if let Some(index) = categories.borrow().iter().position(|category| {
                            category.name.eq_ignore_ascii_case(&initial_category)
                        }) {
                            auto_selecting.set(true);
                            category_completion.select(index);
                            category_completion.set_text(&initial_category);
                            auto_selecting.set(false);
                        }
                    }
                }
                Err(error) => status.set_tooltip_text(Some(&format!(
                    "Speedrun.com metadata lookup failed: {error}"
                ))),
            }
            glib::ControlFlow::Break
        });
    });
    let chosen_platforms = platform_results.clone();
    let chosen_platform_editor = editor.clone();
    let platform_populating = populating_suggestions.clone();
    let selected_platform_entry = src_platforms.clone();
    src_platforms.connect_selected(move |index| {
        if platform_populating.get() {
            return;
        }
        let Some(platform) = chosen_platforms.borrow().get(index).cloned() else {
            return;
        };
        if let Some(editor) = chosen_platform_editor.borrow_mut().as_mut() {
            editor.set_platform_name(platform.name.as_str());
        }
        selected_platform_entry.set_text(&platform.name);
    });
    let chosen_regions = region_results.clone();
    let chosen_region_editor = editor.clone();
    let region_populating = populating_suggestions.clone();
    let selected_region_entry = src_regions.clone();
    src_regions.connect_selected(move |index| {
        if region_populating.get() {
            return;
        }
        let Some(region) = chosen_regions.borrow().get(index).cloned() else {
            return;
        };
        if let Some(editor) = chosen_region_editor.borrow_mut().as_mut() {
            editor.set_region_name(region.name.as_str());
        }
        selected_region_entry.set_text(&region.name);
    });
    let chosen_categories = category_results.clone();
    let chosen_editor = editor.clone();
    let chosen_category_entry = category.clone();
    let chosen_status = category.clone();
    let chosen_spinner = src_spinner.clone();
    let chosen_variables_group = details.src_variables.clone();
    let chosen_empty_variables = details.empty_variables.clone();
    let chosen_variable_rows = details.dynamic_variable_rows.clone();
    let category_populating = populating_suggestions;
    let chosen_variable_generation = variable_generation;
    category.connect_selected(move |index| {
        if category_populating.get() {
            return;
        }
        let Some(category) = chosen_categories.borrow().get(index).cloned() else {
            return;
        };
        let generation = chosen_variable_generation.get() + 1;
        chosen_variable_generation.set(generation);
        // The run's own category only loads the variables for display. Its
        // stored variables are only rewritten when the user picks a category.
        let automatic = auto_selecting.get();
        if !automatic {
            chosen_category_entry.set_text(&category.name);
            if let Some(editor) = chosen_editor.borrow_mut().as_mut() {
                editor.set_category_name(category.name.as_str());
            }
        }
        chosen_spinner.set_spinning(true);
        chosen_status.set_tooltip_text(Some("Loading speedrun.com category variables…"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(
                crate::speedrun_com::variables(&category.id).map_err(|error| error.to_string()),
            );
        });
        let editor = chosen_editor.clone();
        let status = chosen_status.clone();
        let spinner = chosen_spinner.clone();
        let variables_group = chosen_variables_group.clone();
        let empty_variables = chosen_empty_variables.clone();
        let variable_rows = chosen_variable_rows.clone();
        let generation_state = chosen_variable_generation.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            // A worker that died without answering is reported like a failed request.
            let response = match rx.try_recv() {
                Ok(response) => response,
                Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("the request stopped unexpectedly".to_owned())
                }
            };
            if generation_state.get() != generation {
                return glib::ControlFlow::Break;
            }
            spinner.set_spinning(false);
            match response {
                Ok(variables) => {
                    if let Some(editor) = editor.borrow_mut().as_mut().filter(|_| !automatic) {
                        let old_variables = editor
                            .run()
                            .metadata()
                            .speedrun_com_variables()
                            .map(|(name, value)| (name.to_owned(), value.clone()))
                            .collect::<HashMap<_, _>>();
                        for name in old_variables.keys() {
                            editor.remove_speedrun_com_variable(name);
                        }
                        for variable in &variables {
                            let value = old_variables
                                .get(&variable.name)
                                .or(variable.default.as_ref())
                                .or_else(|| {
                                    variable
                                        .mandatory
                                        .then(|| variable.values.first())
                                        .flatten()
                                });
                            if let Some(value) = value {
                                editor.set_speedrun_com_variable(
                                    variable.name.as_str(),
                                    value.as_str(),
                                );
                            }
                        }
                    }
                    populate_speedrun_com_variable_rows(
                        &variables_group,
                        &empty_variables,
                        &variable_rows,
                        &editor,
                        &variables,
                    );
                    status.set_tooltip_text(Some(&format!(
                        "Loaded {} speedrun.com category variables",
                        variables.len()
                    )));
                }
                Err(error) => status.set_tooltip_text(Some(&format!(
                    "Speedrun.com variable lookup failed: {error}"
                ))),
            }
            glib::ControlFlow::Break
        });
    });
    if initial_game_name.trim().len() >= 2 {
        game.entry.emit_by_name::<()>("changed", &[]);
    }
    let offset = adw::EntryRow::builder()
        .title("Start Timer At")
        .text(&state.offset)
        .build();
    let offset_editor = editor.clone();
    offset.connect_changed(move |row| {
        let valid = offset_editor.borrow_mut().as_mut().is_some_and(|editor| {
            editor
                .parse_and_set_offset(row.text().as_str(), livesplit_core::Lang::English)
                .is_ok()
        });
        if valid {
            row.remove_css_class("error");
        } else {
            row.add_css_class("error");
        }
    });
    general.add(&offset);
    let attempts = adw::SpinRow::builder()
        .title("Attempts")
        .adjustment(&gtk::Adjustment::new(
            state.attempts as f64,
            0.0,
            u32::MAX as f64,
            1.0,
            10.0,
            0.0,
        ))
        .digits(0)
        .build();
    let attempts_editor = editor.clone();
    attempts.connect_value_notify(move |row| {
        if let Some(editor) = attempts_editor.borrow_mut().as_mut() {
            editor.set_attempt_count(row.value() as u32);
        }
    });
    general.add(&attempts);
    let game_icon = gtk::Box::new(gtk::Orientation::Vertical, 6);
    game_icon.set_halign(gtk::Align::Center);
    let game_icon_data = editor
        .borrow()
        .as_ref()
        .map(|editor| editor.run().game_icon().data().to_vec())
        .unwrap_or_default();
    let choose_game_editor = editor.clone();
    let remove_game_editor = editor.clone();
    let game_icon_picker = build_icon_picker(
        &game_icon_data,
        96,
        112,
        Rc::new(move |image| {
            if let Some(editor) = choose_game_editor.borrow_mut().as_mut() {
                editor.set_game_icon(image);
            }
        }),
        Rc::new(move || {
            if let Some(editor) = remove_game_editor.borrow_mut().as_mut() {
                editor.remove_game_icon();
            }
        }),
        None,
    );
    let game_icon_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    game_icon_frame.add_css_class("card");
    game_icon_frame.set_size_request(112, 112);
    game_icon_frame.set_halign(gtk::Align::Center);
    game_icon_frame.set_valign(gtk::Align::Start);
    game_icon_frame.append(&game_icon_picker);
    game_icon.append(&game_icon_frame);
    let run_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    run_header.append(&game_icon);
    run_header.append(&general);
    content.append(&run_header);
    content.append(&page_switcher);

    let use_layout = gtk::Switch::new();
    use_layout.set_valign(gtk::Align::Center);
    let linked_layout = editor
        .borrow()
        .as_ref()
        .and_then(|editor| editor.run().linked_layout().cloned())
        .unwrap_or(livesplit_core::run::LinkedLayout::Default);
    use_layout.set_active(
        editor
            .borrow()
            .as_ref()
            .is_some_and(|editor| editor.run().linked_layout().is_some()),
    );
    let linked_editor = editor.clone();
    use_layout.connect_active_notify(move |button| {
        if let Some(editor) = linked_editor.borrow_mut().as_mut() {
            editor.set_linked_layout(button.is_active().then(|| linked_layout.clone()));
        }
    });
    #[cfg(feature = "auto-splitting")]
    splits_page.append(&build_run_auto_splitter_section(
        &window,
        &editor,
        &auto_splitter_changed,
        &auto_splitter,
        &timer,
    ));

    let segment_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let segment_title = gtk::Label::builder()
        .label("Segments")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    segment_header.append(&segment_title);
    let timing = gtk::DropDown::from_strings(&["Real Time", "Game Time"]);
    timing.set_selected(match state.timing_method {
        livesplit_core::TimingMethod::RealTime => 0,
        livesplit_core::TimingMethod::GameTime => 1,
    });
    segment_header.append(&timing);

    let columns = gtk::Grid::builder()
        .column_spacing(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    let column_groups = Rc::new(
        (0..5 + state.comparison_names.len())
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect::<Vec<_>>(),
    );
    for (column, title) in ["Icon", "Name", "Split Time", "Segment Time", "Best Segment"]
        .iter()
        .enumerate()
    {
        let label = gtk::Label::builder().label(*title).xalign(0.0).build();
        label.add_css_class("dim-label");
        column_groups[column].add_widget(&label);
        columns.attach(&label, column as i32, 0, 1, 1);
    }
    for (index, comparison) in state.comparison_names.iter().enumerate() {
        let label = gtk::Label::builder().label(comparison).xalign(0.0).build();
        label.add_css_class("dim-label");
        column_groups[index + 5].add_widget(&label);
        columns.attach(&label, index as i32 + 5, 0, 1, 1);
    }

    let segments = gtk::ListBox::new();
    segments.set_selection_mode(gtk::SelectionMode::Multiple);
    segments.add_css_class("boxed-list");
    populate_run_segments(&segments, &editor, &column_groups);
    let selection_updating = Rc::new(Cell::new(false));
    let selection_anchor = Rc::new(Cell::new(None::<i32>));
    let selection_gesture = gtk::GestureClick::new();
    selection_gesture.set_button(1);
    selection_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let gesture_list = segments.clone();
    let gesture_editor = editor.clone();
    let gesture_updating = selection_updating.clone();
    let gesture_anchor = selection_anchor.clone();
    selection_gesture.connect_pressed(move |gesture, _, x, y| {
        let Some(row) = gesture_list.row_at_y(y as i32) else {
            return;
        };
        let target = run_row_click_target(gesture_list.pick(x, y, gtk::PickFlags::DEFAULT), &row);
        if matches!(
            target,
            RunRowClickTarget::DragHandle | RunRowClickTarget::Control
        ) {
            return;
        }
        let previous = gesture_list
            .selected_rows()
            .iter()
            .map(gtk::ListBoxRow::index)
            .collect::<Vec<_>>();
        let (desired, anchor) = desired_run_row_selection(
            &previous,
            row.index(),
            gesture.current_event_state(),
            gesture_anchor.get(),
        );
        gesture_anchor.set(Some(anchor));

        if target == RunRowClickTarget::Editable {
            let list = gesture_list.clone();
            let editor = gesture_editor.clone();
            let updating = gesture_updating.clone();
            glib::idle_add_local_once(move || {
                apply_run_row_selection(&list, &editor, &updating, &desired);
            });
            return;
        }

        apply_run_row_selection(&gesture_list, &gesture_editor, &gesture_updating, &desired);
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    segments.add_controller(selection_gesture);
    let selection_editor = editor.clone();
    let changed_updating = selection_updating.clone();
    segments.connect_selected_rows_changed(move |list| {
        if changed_updating.get() {
            return;
        }
        sync_run_row_selection(list, &selection_editor);
    });
    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .min_content_height(220)
        .child(&segments)
        .build();
    let table = gtk::Box::new(gtk::Orientation::Vertical, 6);
    table.set_hexpand(true);
    table.append(&segment_header);
    let history_revealer = gtk::Revealer::new();
    history_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    let history_count = editor
        .borrow()
        .as_ref()
        .map_or(0, |editor| editor.run().attempt_history().len());
    let history_summary = adw::ActionRow::builder()
        .title(format!("{history_count} recorded attempts"))
        .subtitle("Use Other… to clear attempt history or all times")
        .build();
    history_revealer.set_child(Some(&history_summary));
    table.append(&history_revealer);
    table.append(&columns);
    table.append(&scroller);

    let command_rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    command_rail.set_size_request(132, -1);
    let history = gtk::Button::with_label("History ▾");
    let shown_history = history_revealer.clone();
    history.connect_clicked(move |button| {
        let reveal = !shown_history.reveals_child();
        shown_history.set_reveal_child(reveal);
        button.set_label(if reveal { "History ▴" } else { "History ▾" });
    });
    command_rail.append(&history);
    let mut segment_action_buttons = Vec::new();
    for (label, operation) in [
        ("Add Split Above", 0_u8),
        ("Add Split Below", 1),
        ("Delete Splits", 2),
        ("Move Up", 3),
        ("Move Down", 4),
        ("Group Selection", 5),
    ] {
        let button = gtk::Button::with_label(label);
        let button_editor = editor.clone();
        let button_list = segments.clone();
        let button_groups = column_groups.clone();
        button.connect_clicked(move |_| {
            if let Some(editor) = button_editor.borrow_mut().as_mut() {
                match operation {
                    0 => editor.insert_segment_above(),
                    1 => editor.insert_segment_below(),
                    2 => editor.remove_segments(),
                    3 => editor.move_segments_up(),
                    4 => editor.move_segments_down(),
                    5 => {
                        let _ = editor.create_segment_group_from_selection(Some("Group"));
                    }
                    _ => unreachable!(),
                }
            }
            populate_run_segments(&button_list, &button_editor, &button_groups);
        });
        command_rail.append(&button);
        segment_action_buttons.push(button);
    }
    let action_editor = editor.clone();
    let update_segment_actions = move || {
        let editor = action_editor.borrow();
        let Some(editor) = editor.as_ref() else {
            return;
        };
        let state = editor.state(&mut ImageCache::new(), livesplit_core::Lang::English);
        for (button, sensitive) in segment_action_buttons.iter().zip([
            true,
            true,
            state.buttons.can_remove,
            state.buttons.can_move_up,
            state.buttons.can_move_down,
            state.buttons.can_create_segment_group,
        ]) {
            button.set_sensitive(sensitive);
        }
    };
    update_segment_actions();
    segments.connect_selected_rows_changed(move |_| update_segment_actions());
    let comparisons_button = gtk::Button::with_label("Comparisons…");
    let comparison_pages = pages.clone();
    comparisons_button.connect_clicked(move |_| {
        comparison_pages.set_visible_child_name("comparisons");
    });
    command_rail.append(&comparisons_button);
    let tools = gtk::MenuButton::builder()
        .label("Other…")
        .menu_model(&run_tools_menu())
        .build();
    install_run_tool_actions(&window, &editor, &segments, &column_groups);
    command_rail.append(&tools);
    let editor_body = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    editor_body.append(&command_rail);
    editor_body.append(&table);
    splits_page.append(&editor_body);
    pages.add_titled_with_icon(&splits_page, Some("splits"), "Splits", "view-list-symbolic");

    pages.add_titled_with_icon(
        &details.page,
        Some("details"),
        "Additional Info",
        "document-properties-symbolic",
    );

    let comparisons_page = build_run_comparisons_page(&editor, &segments, &column_groups);
    pages.add_titled_with_icon(
        &comparisons_page,
        Some("comparisons"),
        "Comparisons",
        "view-sort-ascending-symbolic",
    );
    content.append(&pages);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let link_layout = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    link_layout.set_valign(gtk::Align::Center);
    link_layout.append(&use_layout);
    link_layout.append(&gtk::Label::new(Some("Link Layout")));
    actions.append(&link_layout);
    let action_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    action_spacer.set_hexpand(true);
    actions.append(&action_spacer);
    let cancel = gtk::Button::with_label("Cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&apply);
    content.append(&actions);

    let timing_editor = editor.clone();
    let timing_list = segments.clone();
    let timing_groups = column_groups.clone();
    timing.connect_selected_notify(move |dropdown| {
        if let Some(editor) = timing_editor.borrow_mut().as_mut() {
            editor.select_timing_method(if dropdown.selected() == 0 {
                livesplit_core::TimingMethod::RealTime
            } else {
                livesplit_core::TimingMethod::GameTime
            });
        }
        populate_run_segments(&timing_list, &timing_editor, &timing_groups);
    });

    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));
    let cancel_window = window.clone();
    cancel.connect_clicked(move |_| cancel_window.close());
    let apply_window = window.clone();
    let apply_editor = editor.clone();
    let apply_sender = sender.clone();
    #[cfg(feature = "auto-splitting")]
    let applied_on_apply = auto_splitter_applied.clone();
    #[cfg(feature = "auto-splitting")]
    let changed_on_apply = auto_splitter_changed.clone();
    #[cfg(feature = "auto-splitting")]
    let apply_runtime = auto_splitter.clone();
    let apply_run = Rc::new(move || {
        if let Some(run) = apply_editor
            .borrow_mut()
            .take()
            .map(|editor| editor.close())
        {
            apply_sender.input(AppMsg::ApplyRun(RunDraft(Arc::new(Mutex::new(Some(run))))));
        }
        #[cfg(feature = "auto-splitting")]
        {
            applied_on_apply.set(true);
            // Applying the run replaces its stored auto splitter with the one
            // from when the editor opened, so store the current one again.
            if changed_on_apply.get() || apply_runtime.loaded_path().is_some() {
                apply_sender.input(AppMsg::StoreAutoSplitterSettings);
            }
        }
        apply_window.close();
    });
    // Applying replaces the timer's run with the editor's copy, which resets
    // the timer and drops anything recorded since the editor was opened. Timer
    // controls are disabled meanwhile, but an auto splitter can still use it.
    let initial_attempts = timer.read().unwrap().run().attempt_count();
    let apply_timer = timer.clone();
    let confirm_window = window.clone();
    apply.connect_clicked(move |_| {
        let timer_used = {
            let timer = apply_timer.read().unwrap();
            timer.current_phase() != livesplit_core::TimerPhase::NotRunning
                || timer.run().attempt_count() != initial_attempts
        };
        if !timer_used {
            apply_run();
            return;
        }
        let dialog = gtk::AlertDialog::builder()
            .message("Discard the latest attempt?")
            .detail(
                "The timer was used while the splits editor was open. Applying these changes \
replaces the splits with the ones being edited, which discards that attempt.",
            )
            .modal(true)
            .build();
        dialog.set_buttons(&["Keep Editing", "Apply Anyway"]);
        dialog.set_cancel_button(0);
        dialog.set_default_button(0);
        let apply_run = apply_run.clone();
        dialog.choose(
            Some(&confirm_window),
            gio::Cancellable::NONE,
            move |response| {
                if response.ok() == Some(1) {
                    apply_run();
                }
            },
        );
    });
    let close_sender = sender.clone();
    #[cfg(feature = "auto-splitting")]
    let restore_runtime = auto_splitter.clone();
    #[cfg(feature = "auto-splitting")]
    let restore_timer = timer.clone();
    window.connect_close_request(move |_| {
        #[cfg(feature = "auto-splitting")]
        if !auto_splitter_applied.get() && auto_splitter_changed.get() {
            // The timer's run still stores the auto splitter from when the
            // editor opened.
            if let Err(error) = restore_runtime.load(restore_timer.clone()) {
                log::error!("Auto Splitter failed to load: {error}");
            }
        }
        close_sender.input(AppMsg::EditorFinished(EditorKind::Run));
        glib::Propagation::Proceed
    });
    window
}

#[cfg(test)]
mod selection_tests {
    use super::desired_run_row_selection;
    use gtk::gdk;

    #[test]
    fn plain_click_selects_only_the_clicked_row() {
        let (selection, anchor) =
            desired_run_row_selection(&[1, 2], 4, gdk::ModifierType::empty(), Some(1));
        assert_eq!(selection, [4]);
        assert_eq!(anchor, 4);
    }

    #[test]
    fn control_click_toggles_without_allowing_an_empty_selection() {
        let (selection, _) =
            desired_run_row_selection(&[1, 3], 1, gdk::ModifierType::CONTROL_MASK, Some(3));
        assert_eq!(selection, [3]);

        let (selection, _) =
            desired_run_row_selection(&[3], 3, gdk::ModifierType::CONTROL_MASK, Some(3));
        assert_eq!(selection, [3]);
    }

    #[test]
    fn shift_click_selects_the_range_from_the_anchor() {
        let (selection, anchor) =
            desired_run_row_selection(&[2], 5, gdk::ModifierType::SHIFT_MASK, Some(2));
        assert_eq!(selection, [2, 3, 4, 5]);
        assert_eq!(anchor, 2);
    }
}

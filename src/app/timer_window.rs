use super::*;

pub(super) fn install_timer_interactions(
    window: &gtk::ApplicationWindow,
    picture: &gtk::Picture,
    timer: &SharedTimer,
    config: &Rc<RefCell<Config>>,
    platform: Platform,
    sender: &ComponentSender<AppModel>,
) {
    const RESIZE_MARGIN: f64 = 8.0;

    let move_or_resize = gtk::GestureClick::new();
    move_or_resize.set_button(1);
    let drag_window = window.clone();
    let drag_picture = picture.clone();
    move_or_resize.connect_pressed(move |gesture, _, x, y| {
        let Some(event) = gesture.current_event() else {
            return;
        };
        if let Some(edge) = timer_resize_edge(
            x,
            y,
            drag_picture.width() as f64,
            drag_picture.height() as f64,
            RESIZE_MARGIN,
        ) {
            platform.begin_resize(&drag_window, &event, edge);
        } else {
            platform.begin_drag(&drag_window, &event);
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    picture.add_controller(move_or_resize);

    let resize_cursor = gtk::EventControllerMotion::new();
    let cursor_picture = picture.clone();
    resize_cursor.connect_motion(move |controller, x, y| {
        let cursor = timer_resize_edge(
            x,
            y,
            cursor_picture.width() as f64,
            cursor_picture.height() as f64,
            RESIZE_MARGIN,
        )
        .map(resize_cursor_name);
        if let Some(widget) = controller.widget() {
            widget.set_cursor_from_name(cursor);
        }
    });
    resize_cursor.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor_from_name(None);
        }
    });
    picture.add_controller(resize_cursor);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let scroll_sender = sender.clone();
    scroll.connect_scroll(move |_, _dx, dy| {
        scroll_sender.input(AppMsg::Scroll(dy));
        glib::Propagation::Stop
    });
    picture.add_controller(scroll);

    let menu = gio::Menu::new();

    let splits = gio::Menu::new();
    splits.append(Some("Edit…"), Some("win.edit-splits"));
    let splits_files = gio::Menu::new();
    splits_files.append(Some("New"), Some("win.new-splits"));
    splits_files.append(Some("Open…"), Some("win.open-splits"));
    let recent = gio::Menu::new();
    let open_recent_file =
        gio::SimpleAction::new("open-recent-file", Some(&Vec::<u8>::static_variant_type()));
    let recent_sender = sender.clone();
    open_recent_file.connect_activate(move |_, parameter| {
        use std::os::unix::ffi::OsStringExt;

        let Some(path) = parameter.and_then(|value| value.get::<Vec<u8>>()) else {
            return;
        };
        let path = PathBuf::from(std::ffi::OsString::from_vec(path));
        recent_sender.input(AppMsg::RequestAction(PendingAction::OpenSplits(path)));
    });
    window.add_action(&open_recent_file);
    splits_files.append_submenu(Some("Open Recent"), &recent);
    splits_files.append(Some("Save"), Some("win.save-splits"));
    splits_files.append(Some("Save As…"), Some("win.save-splits-as"));
    splits.append_section(None, &splits_files);
    menu.append_submenu(Some("Splits"), &splits);

    let layout = gio::Menu::new();
    layout.append(Some("Edit…"), Some("win.edit-layout"));
    let layout_files = gio::Menu::new();
    layout_files.append(Some("New"), Some("win.new-layout"));
    layout_files.append(Some("Open…"), Some("win.open-layout"));
    layout_files.append(Some("Save"), Some("win.save-layout"));
    layout_files.append(Some("Save As…"), Some("win.save-layout-as"));
    layout.append_section(None, &layout_files);
    menu.append_submenu(Some("Layout"), &layout);

    let control = gio::Menu::new();
    control.append(Some("Start / Split"), Some("win.start-or-split"));
    control.append(Some("Reset"), Some("win.reset"));
    let split_control = gio::Menu::new();
    split_control.append(Some("Undo Split"), Some("win.undo-split"));
    split_control.append(Some("Skip Split"), Some("win.skip-split"));
    control.append_section(None, &split_control);
    let pause_control = gio::Menu::new();
    pause_control.append(Some("Pause / Resume"), Some("win.pause"));
    pause_control.append(Some("Undo All Pauses"), Some("win.undo-all-pauses"));
    control.append_section(None, &pause_control);
    menu.append_submenu(Some("Control"), &control);

    let comparisons = gio::Menu::new();
    let comparison_action = gio::SimpleAction::new_stateful(
        "comparison",
        Some(&String::static_variant_type()),
        &timer.read().unwrap().current_comparison().to_variant(),
    );
    let comparison_sender = sender.clone();
    comparison_action.connect_activate(move |action, parameter| {
        let Some(comparison) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        action.set_state(&comparison.to_variant());
        comparison_sender.input(AppMsg::SetComparison(comparison));
    });
    window.add_action(&comparison_action);
    menu.append_submenu(Some("Compare Against"), &comparisons);

    let timing = gio::Menu::new();
    for (label, value) in [("Real Time", "real-time"), ("Game Time", "game-time")] {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("win.timing-method"), Some(&value.to_variant()));
        timing.append_item(&item);
    }
    let initial_timing = if timer.read().unwrap().current_timing_method()
        == livesplit_core::TimingMethod::RealTime
    {
        "real-time"
    } else {
        "game-time"
    };
    let timing_action = gio::SimpleAction::new_stateful(
        "timing-method",
        Some(&String::static_variant_type()),
        &initial_timing.to_variant(),
    );
    let timing_sender = sender.clone();
    timing_action.connect_activate(move |action, parameter| {
        let Some(method) = parameter.and_then(|value| value.str()) else {
            return;
        };
        action.set_state(&method.to_variant());
        timing_sender.input(AppMsg::SetTimingMethod(if method == "game-time" {
            livesplit_core::TimingMethod::GameTime
        } else {
            livesplit_core::TimingMethod::RealTime
        }));
    });
    window.add_action(&timing_action);
    menu.append_submenu(Some("Timing Method"), &timing);

    let tools = gio::Menu::new();
    tools.append(Some("Settings"), Some("win.window-settings"));
    tools.append(Some("Edit Split Notes"), Some("win.edit-notes"));
    tools.append(Some("Show Split Notes"), Some("win.show-notes"));
    menu.append_section(None, &tools);

    let application = gio::Menu::new();
    application.append(Some("Exit"), Some("win.exit"));
    menu.append_section(None, &application);

    for (name, command) in [
        ("start-or-split", TimerCommand::StartOrSplit),
        ("undo-split", TimerCommand::UndoSplit),
        ("skip-split", TimerCommand::SkipSplit),
        ("pause", TimerCommand::Pause),
        ("undo-all-pauses", TimerCommand::UndoAllPauses),
        ("reset", TimerCommand::Reset),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| action_sender.input(AppMsg::TimerCommand(command)));
        window.add_action(&action);
    }

    // Keep the former menu discoverable while its editor and file-intent
    // components are being ported. A disabled action is preferable to an item
    // that either disappears or silently discards the user's request.
    let open_recent = gio::SimpleAction::new("open-recent", None);
    open_recent.set_enabled(false);
    window.add_action(&open_recent);

    let window_settings = gio::SimpleAction::new("window-settings", None);
    let settings_sender = sender.clone();
    window_settings.connect_activate(move |_, _| {
        settings_sender.input(AppMsg::OpenEditor(EditorKind::Window));
    });
    window.add_action(&window_settings);

    for (name, kind) in [
        ("edit-notes", EditorKind::Notes),
        ("show-notes", EditorKind::NotesViewer),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(config.borrow().splits_path().is_some());
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            action_sender.input(AppMsg::OpenEditor(kind));
        });
        window.add_action(&action);
    }

    let edit_layout = gio::SimpleAction::new("edit-layout", None);
    let layout_sender = sender.clone();
    edit_layout.connect_activate(move |_, _| {
        layout_sender.input(AppMsg::OpenEditor(EditorKind::Layout));
    });
    window.add_action(&edit_layout);

    let edit_splits = gio::SimpleAction::new("edit-splits", None);
    let splits_sender = sender.clone();
    edit_splits.connect_activate(move |_, _| {
        splits_sender.input(AppMsg::OpenEditor(EditorKind::Run));
    });
    window.add_action(&edit_splits);

    for (name, direct_action) in [
        ("new-splits", DirectAction::NewSplits),
        ("save-splits", DirectAction::SaveSplits),
        ("new-layout", DirectAction::NewLayout),
        ("save-layout", DirectAction::SaveLayout),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            action_sender.input(match direct_action {
                DirectAction::NewSplits => AppMsg::RequestAction(PendingAction::NewSplits),
                DirectAction::SaveSplits => AppMsg::SaveSplits,
                DirectAction::NewLayout => AppMsg::RequestAction(PendingAction::NewLayout),
                DirectAction::SaveLayout => AppMsg::SaveLayout,
            });
        });
        window.add_action(&action);
    }

    for (name, save, choice) in [
        ("open-splits", false, FileChoice::OpenSplits),
        ("save-splits-as", true, FileChoice::SaveSplits),
        ("open-layout", false, FileChoice::OpenLayout),
        ("save-layout-as", true, FileChoice::SaveLayout),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let action_window = window.clone();
        let action_sender = sender.clone();
        action.connect_activate(move |_, _| {
            select_file(&action_window, save, choice, &action_sender);
        });
        window.add_action(&action);
    }

    let exit = gio::SimpleAction::new("exit", None);
    let exit_sender = sender.clone();
    exit.connect_activate(move |_, _| {
        exit_sender.input(AppMsg::RequestAction(PendingAction::Exit));
    });
    window.add_action(&exit);

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(picture);
    popover.set_has_arrow(false);
    let click = gtk::GestureClick::new();
    click.set_button(3);
    let menu_timer = timer.clone();
    let menu_config = config.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        // The splits, their comparisons and the history change while the
        // timer is open, so these are rebuilt whenever the menu is shown.
        fill_recent_menu(&recent, menu_config.borrow().splits_history());
        let timer = menu_timer.read().unwrap();
        fill_comparison_menu(&comparisons, timer.run().comparisons());
        comparison_action.set_state(&timer.current_comparison().to_variant());
        timing_action.set_state(
            &if timer.current_timing_method() == livesplit_core::TimingMethod::RealTime {
                "real-time"
            } else {
                "game-time"
            }
            .to_variant(),
        );
        drop(timer);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    picture.add_controller(click);
}

fn fill_recent_menu(recent: &gio::Menu, history: &crate::config::SplitsHistory) {
    use std::os::unix::ffi::OsStrExt;

    recent.remove_all();
    for (game, categories) in history {
        let game_menu = gio::Menu::new();
        for (category, paths) in categories {
            let category_menu = gio::Menu::new();
            for path in paths {
                let label = path
                    .file_name()
                    .map_or_else(|| "Untitled".into(), |name| name.to_string_lossy());
                let item = gio::MenuItem::new(Some(&label), None);
                item.set_action_and_target_value(
                    Some("win.open-recent-file"),
                    Some(&path.as_os_str().as_bytes().to_variant()),
                );
                category_menu.append_item(&item);
            }
            game_menu.append_submenu(Some(category), &category_menu);
        }
        recent.append_submenu(Some(game), &game_menu);
    }
    if history.is_empty() {
        recent.append(Some("No recent splits available"), Some("win.open-recent"));
    }
}

fn fill_comparison_menu<'a>(comparisons: &gio::Menu, names: impl Iterator<Item = &'a str>) {
    comparisons.remove_all();
    for comparison in names {
        let item = gio::MenuItem::new(Some(comparison), None);
        item.set_action_and_target_value(Some("win.comparison"), Some(&comparison.to_variant()));
        comparisons.append_item(&item);
    }
}

pub(super) fn timer_resize_edge(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    margin: f64,
) -> Option<gdk::SurfaceEdge> {
    let west = x < margin;
    let east = x >= width - margin;
    let north = y < margin;
    let south = y >= height - margin;
    match (west, east, north, south) {
        (true, _, true, _) => Some(gdk::SurfaceEdge::NorthWest),
        (_, true, true, _) => Some(gdk::SurfaceEdge::NorthEast),
        (true, _, _, true) => Some(gdk::SurfaceEdge::SouthWest),
        (_, true, _, true) => Some(gdk::SurfaceEdge::SouthEast),
        (true, _, _, _) => Some(gdk::SurfaceEdge::West),
        (_, true, _, _) => Some(gdk::SurfaceEdge::East),
        (_, _, true, _) => Some(gdk::SurfaceEdge::North),
        (_, _, _, true) => Some(gdk::SurfaceEdge::South),
        _ => None,
    }
}

fn resize_cursor_name(edge: gdk::SurfaceEdge) -> &'static str {
    match edge {
        gdk::SurfaceEdge::NorthWest => "nw-resize",
        gdk::SurfaceEdge::North => "n-resize",
        gdk::SurfaceEdge::NorthEast => "ne-resize",
        gdk::SurfaceEdge::West => "w-resize",
        gdk::SurfaceEdge::East => "e-resize",
        gdk::SurfaceEdge::SouthWest => "sw-resize",
        gdk::SurfaceEdge::South => "s-resize",
        gdk::SurfaceEdge::SouthEast => "se-resize",
        _ => "default",
    }
}

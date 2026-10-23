use std::{cell::Cell, rc::Rc};

use adw::prelude::*;
use livesplit_core::settings::{Field, Value};

pub type SettingChanged = Rc<dyn Fn(Value)>;

pub fn build_setting_row(field: &Field, changed: SettingChanged) -> adw::PreferencesRow {
    match &field.value {
        Value::Bool(value) => {
            let row = adw::SwitchRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .active(*value)
                .build();
            row.connect_active_notify(move |row| changed(Value::Bool(row.is_active())));
            row.upcast()
        }
        Value::UInt(value) => {
            let adjustment =
                gtk::Adjustment::new(*value as f64, 0.0, u32::MAX as f64, 1.0, 10.0, 0.0);
            let row = adw::SpinRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .adjustment(&adjustment)
                .numeric(true)
                .build();
            row.connect_value_notify(move |row| changed(Value::UInt(row.value().round() as u64)));
            row.upcast()
        }
        Value::OptionalUInt(value) => {
            let adjustment = gtk::Adjustment::new(
                value.unwrap_or_default() as f64,
                0.0,
                u32::MAX as f64,
                1.0,
                10.0,
                0.0,
            );
            let row = adw::SpinRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .adjustment(&adjustment)
                .numeric(true)
                .build();
            let enabled = Rc::new(Cell::new(value.is_some()));
            row.set_editable(enabled.get());
            let toggle = gtk::Switch::builder()
                .active(enabled.get())
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&toggle);
            let value_enabled = enabled.clone();
            let value_changed = changed.clone();
            row.connect_value_notify(move |row| {
                if value_enabled.get() {
                    value_changed(Value::OptionalUInt(Some(row.value().round() as u64)));
                }
            });
            let toggle_row = row.clone();
            toggle.connect_active_notify(move |toggle| {
                enabled.set(toggle.is_active());
                toggle_row.set_editable(toggle.is_active());
                changed(Value::OptionalUInt(
                    toggle
                        .is_active()
                        .then(|| toggle_row.value().round() as u64),
                ));
            });
            row.upcast()
        }
        Value::Int(value) => {
            let adjustment = gtk::Adjustment::new(
                *value as f64,
                i32::MIN as f64,
                i32::MAX as f64,
                1.0,
                10.0,
                0.0,
            );
            let row = adw::SpinRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .adjustment(&adjustment)
                .numeric(true)
                .build();
            row.connect_value_notify(move |row| changed(Value::Int(row.value().round() as i64)));
            row.upcast()
        }
        Value::String(value) => {
            let row = adw::EntryRow::builder()
                .title(field.text.as_ref())
                .text(value)
                .tooltip_text(field.tooltip.as_ref())
                .build();
            row.connect_changed(move |row| changed(Value::String(row.text().into())));
            row.upcast()
        }
        Value::OptionalString(value) => {
            let row = adw::EntryRow::builder()
                .title(field.text.as_ref())
                .text(value.as_deref().unwrap_or_default())
                .tooltip_text(field.tooltip.as_ref())
                .build();
            let enabled = Rc::new(Cell::new(value.is_some()));
            row.set_editable(enabled.get());
            let toggle = gtk::Switch::builder()
                .active(enabled.get())
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&toggle);
            let text_enabled = enabled.clone();
            let text_changed = changed.clone();
            row.connect_changed(move |row| {
                if text_enabled.get() {
                    text_changed(Value::OptionalString(Some(row.text().into())));
                }
            });
            let toggle_row = row.clone();
            let toggle_enabled = enabled.clone();
            let toggle_changed = changed.clone();
            toggle.connect_active_notify(move |toggle| {
                toggle_enabled.set(toggle.is_active());
                toggle_row.set_editable(toggle.is_active());
                toggle_changed(Value::OptionalString(
                    toggle.is_active().then(|| toggle_row.text().into()),
                ));
            });
            row.upcast()
        }
        Value::Accuracy(value) => {
            use livesplit_core::timing::formatter::Accuracy::*;
            enum_row(
                field,
                &["Seconds", "Tenths", "Hundredths", "Milliseconds"],
                match value {
                    Seconds => 0,
                    Tenths => 1,
                    Hundredths => 2,
                    Milliseconds => 3,
                },
                changed,
                |index| {
                    Value::Accuracy(match index {
                        0 => Seconds,
                        1 => Tenths,
                        2 => Hundredths,
                        _ => Milliseconds,
                    })
                },
            )
        }
        Value::DigitsFormat(value) => {
            use livesplit_core::timing::formatter::DigitsFormat::*;
            enum_row(
                field,
                &["1", "01", "0:01", "00:01", "0:00:01", "00:00:01"],
                match value {
                    SingleDigitSeconds => 0,
                    DoubleDigitSeconds => 1,
                    SingleDigitMinutes => 2,
                    DoubleDigitMinutes => 3,
                    SingleDigitHours => 4,
                    DoubleDigitHours => 5,
                },
                changed,
                |index| {
                    Value::DigitsFormat(match index {
                        0 => SingleDigitSeconds,
                        1 => DoubleDigitSeconds,
                        2 => SingleDigitMinutes,
                        3 => DoubleDigitMinutes,
                        4 => SingleDigitHours,
                        _ => DoubleDigitHours,
                    })
                },
            )
        }
        Value::ResetChanceMode(value) => {
            use livesplit_core::component::reset_chance::ChanceMode::*;
            enum_row(
                field,
                &["Reset Chance", "Success Chance", "Runs Ended"],
                match value {
                    ResetChance => 0,
                    SuccessChance => 1,
                    RunsEnded => 2,
                },
                changed,
                |index| {
                    Value::ResetChanceMode(match index {
                        1 => SuccessChance,
                        2 => RunsEnded,
                        _ => ResetChance,
                    })
                },
            )
        }
        Value::ResetChanceCalculationBasis(value) => {
            use livesplit_core::component::reset_chance::CalculationBasis::*;
            enum_row(
                field,
                &["All Runs", "Recent Runs", "Recent Split Attempts"],
                match value {
                    AllRuns => 0,
                    RecentRuns => 1,
                    RecentSplitAttempts => 2,
                },
                changed,
                |index| {
                    Value::ResetChanceCalculationBasis(match index {
                        1 => RecentRuns,
                        2 => RecentSplitAttempts,
                        _ => AllRuns,
                    })
                },
            )
        }
        Value::ResetChancePercentageAccuracy(value) => {
            use livesplit_core::component::reset_chance::PercentageAccuracy::*;
            enum_row(
                field,
                &["Whole Percent", "Tenths", "Hundredths"],
                match value {
                    Integer => 0,
                    Tenths => 1,
                    Hundredths => 2,
                },
                changed,
                |index| {
                    Value::ResetChancePercentageAccuracy(match index {
                        1 => Tenths,
                        2 => Hundredths,
                        _ => Integer,
                    })
                },
            )
        }
        Value::OptionalTimingMethod(value) => enum_row(
            field,
            &["Current", "Real Time", "Game Time"],
            match value {
                None => 0,
                Some(livesplit_core::TimingMethod::RealTime) => 1,
                Some(livesplit_core::TimingMethod::GameTime) => 2,
            },
            changed,
            |index| {
                Value::OptionalTimingMethod(match index {
                    1 => Some(livesplit_core::TimingMethod::RealTime),
                    2 => Some(livesplit_core::TimingMethod::GameTime),
                    _ => None,
                })
            },
        ),
        Value::LayoutDirection(direction) => {
            let model = gtk::StringList::new(&["Vertical", "Horizontal"]);
            let row = adw::ComboRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .model(&model)
                .selected(match direction {
                    livesplit_core::layout::LayoutDirection::Vertical => 0,
                    livesplit_core::layout::LayoutDirection::Horizontal => 1,
                })
                .build();
            row.connect_selected_notify(move |row| {
                changed(Value::LayoutDirection(if row.selected() == 0 {
                    livesplit_core::layout::LayoutDirection::Vertical
                } else {
                    livesplit_core::layout::LayoutDirection::Horizontal
                }));
            });
            row.upcast()
        }
        Value::Font(font) => {
            use livesplit_core::settings::{Font, FontStretch, FontStyle, FontWeight};
            let current = Rc::new(std::cell::RefCell::new(font.clone()));
            let expander = adw::ExpanderRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .show_enable_switch(true)
                .enable_expansion(font.is_some())
                .expanded(font.is_some())
                .build();
            let enabled_current = current.clone();
            let enabled_changed = changed.clone();
            expander.connect_enable_expansion_notify(move |row| {
                if row.enables_expansion() {
                    if enabled_current.borrow().is_none() {
                        *enabled_current.borrow_mut() = Some(Font::default());
                    }
                    enabled_changed(Value::Font(enabled_current.borrow().clone()));
                } else {
                    enabled_changed(Value::Font(None));
                }
            });
            let family_row = adw::ActionRow::builder()
                .title("Font")
                .subtitle("Choose an installed family and one of its available faces")
                .build();
            let mut font_description = gtk::pango::FontDescription::new();
            if let Some(font) = font {
                font_description.set_family(&font.family);
                font_description.set_style(match font.style {
                    FontStyle::Normal => gtk::pango::Style::Normal,
                    FontStyle::Italic => gtk::pango::Style::Italic,
                    FontStyle::Oblique => gtk::pango::Style::Oblique,
                });
                font_description.set_weight(match font.weight {
                    FontWeight::Thin => gtk::pango::Weight::Thin,
                    FontWeight::ExtraLight => gtk::pango::Weight::Ultralight,
                    FontWeight::Light => gtk::pango::Weight::Light,
                    FontWeight::SemiLight => gtk::pango::Weight::Semilight,
                    FontWeight::Normal => gtk::pango::Weight::Normal,
                    FontWeight::Medium => gtk::pango::Weight::Medium,
                    FontWeight::SemiBold => gtk::pango::Weight::Semibold,
                    FontWeight::Bold => gtk::pango::Weight::Bold,
                    FontWeight::ExtraBold => gtk::pango::Weight::Ultrabold,
                    FontWeight::Black => gtk::pango::Weight::Heavy,
                    FontWeight::ExtraBlack => gtk::pango::Weight::Ultraheavy,
                });
                font_description.set_stretch(match font.stretch {
                    FontStretch::UltraCondensed => gtk::pango::Stretch::UltraCondensed,
                    FontStretch::ExtraCondensed => gtk::pango::Stretch::ExtraCondensed,
                    FontStretch::Condensed => gtk::pango::Stretch::Condensed,
                    FontStretch::SemiCondensed => gtk::pango::Stretch::SemiCondensed,
                    FontStretch::Normal => gtk::pango::Stretch::Normal,
                    FontStretch::SemiExpanded => gtk::pango::Stretch::SemiExpanded,
                    FontStretch::Expanded => gtk::pango::Stretch::Expanded,
                    FontStretch::ExtraExpanded => gtk::pango::Stretch::ExtraExpanded,
                    FontStretch::UltraExpanded => gtk::pango::Stretch::UltraExpanded,
                });
            }
            let font_dialog = gtk::FontDialog::builder().title("Choose Font").build();
            let family = gtk::FontDialogButton::builder()
                .dialog(&font_dialog)
                .font_desc(&font_description)
                .level(gtk::FontLevel::Face)
                .use_font(true)
                .valign(gtk::Align::Center)
                .build();
            let family_current = current.clone();
            let family_changed = changed.clone();
            family.connect_font_desc_notify(move |button| {
                let Some(description) = button.font_desc() else {
                    return;
                };
                let Some(family) = description.family() else {
                    return;
                };
                let mut current = family_current.borrow_mut();
                let font = current.get_or_insert_with(Font::default);
                font.family = family.into();
                font.style = match description.style() {
                    gtk::pango::Style::Italic => FontStyle::Italic,
                    gtk::pango::Style::Oblique => FontStyle::Oblique,
                    _ => FontStyle::Normal,
                };
                font.weight = match description.weight() {
                    gtk::pango::Weight::Thin => FontWeight::Thin,
                    gtk::pango::Weight::Ultralight => FontWeight::ExtraLight,
                    gtk::pango::Weight::Light => FontWeight::Light,
                    gtk::pango::Weight::Semilight => FontWeight::SemiLight,
                    gtk::pango::Weight::Medium => FontWeight::Medium,
                    gtk::pango::Weight::Semibold => FontWeight::SemiBold,
                    gtk::pango::Weight::Bold => FontWeight::Bold,
                    gtk::pango::Weight::Ultrabold => FontWeight::ExtraBold,
                    gtk::pango::Weight::Heavy => FontWeight::Black,
                    gtk::pango::Weight::Ultraheavy => FontWeight::ExtraBlack,
                    _ => FontWeight::Normal,
                };
                font.stretch = match description.stretch() {
                    gtk::pango::Stretch::UltraCondensed => FontStretch::UltraCondensed,
                    gtk::pango::Stretch::ExtraCondensed => FontStretch::ExtraCondensed,
                    gtk::pango::Stretch::Condensed => FontStretch::Condensed,
                    gtk::pango::Stretch::SemiCondensed => FontStretch::SemiCondensed,
                    gtk::pango::Stretch::SemiExpanded => FontStretch::SemiExpanded,
                    gtk::pango::Stretch::Expanded => FontStretch::Expanded,
                    gtk::pango::Stretch::ExtraExpanded => FontStretch::ExtraExpanded,
                    gtk::pango::Stretch::UltraExpanded => FontStretch::UltraExpanded,
                    _ => FontStretch::Normal,
                };
                family_changed(Value::Font(Some(font.clone())));
            });
            family_row.add_suffix(&family);
            family_row.set_activatable_widget(Some(&family));
            expander.add_row(&family_row);
            expander.upcast()
        }
        Value::Color(color) => color_row(field, *color, changed),
        Value::OptionalColor(color) => {
            let initial = color.unwrap_or_else(livesplit_core::settings::Color::white);
            let current = Rc::new(Cell::new(initial));
            let enabled = Rc::new(Cell::new(color.is_some()));
            let color_current = current.clone();
            let color_enabled = enabled.clone();
            let color_changed = changed.clone();
            let row = color_row(
                field,
                initial,
                Rc::new(move |value| {
                    if let Value::Color(color) = value {
                        color_current.set(color);
                        if color_enabled.get() {
                            color_changed(Value::OptionalColor(Some(color)));
                        }
                    }
                }),
            );
            let row = row
                .downcast::<adw::ActionRow>()
                .expect("color rows are action rows");
            let toggle = gtk::Switch::builder()
                .active(enabled.get())
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&toggle);
            let toggle_current = current.clone();
            toggle.connect_active_notify(move |toggle| {
                enabled.set(toggle.is_active());
                changed(Value::OptionalColor(
                    toggle.is_active().then(|| toggle_current.get()),
                ));
            });
            row.upcast()
        }
        Value::Gradient(gradient) => gradient_row(field, *gradient, changed, Value::Gradient),
        Value::ListGradient(gradient) => {
            use livesplit_core::settings::{Gradient, ListGradient};
            let (selected, base) = match gradient {
                ListGradient::Same(gradient) => (0, *gradient),
                ListGradient::Alternating(a, b) => (1, Gradient::Vertical(*a, *b)),
            };
            let alternating = Rc::new(Cell::new(selected == 1));
            let expander = adw::ExpanderRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .build();
            let kind_field = Field::new(
                "Row Coloring".into(),
                "How list rows are colored".into(),
                Value::Bool(false),
            );
            let kind_changed = changed.clone();
            let kind_alternating = alternating.clone();
            let kind = enum_row(
                &kind_field,
                &["Same", "Alternating"],
                selected,
                kind_changed,
                move |index| {
                    kind_alternating.set(index != 0);
                    Value::ListGradient(if index == 0 {
                        ListGradient::Same(base)
                    } else {
                        let (a, b) = gradient_colors(base);
                        ListGradient::Alternating(a, b)
                    })
                },
            );
            expander.add_row(&kind);
            let colors_field = Field::new(
                "Colors".into(),
                "Colors used for the list rows".into(),
                Value::Gradient(base),
            );
            let colors_changed = changed.clone();
            let colors_alternating = alternating.clone();
            let colors = gradient_row(&colors_field, base, colors_changed, move |gradient| {
                Value::ListGradient(if colors_alternating.get() {
                    let (a, b) = gradient_colors(gradient);
                    ListGradient::Alternating(a, b)
                } else {
                    ListGradient::Same(gradient)
                })
            });
            expander.add_row(&colors);
            expander.upcast()
        }
        Value::Alignment(value) => {
            use livesplit_core::settings::Alignment::*;
            enum_row(
                field,
                &["Automatic", "Left", "Center"],
                match value {
                    Auto => 0,
                    Left => 1,
                    Center => 2,
                },
                changed,
                |index| {
                    Value::Alignment(match index {
                        1 => Left,
                        2 => Center,
                        _ => Auto,
                    })
                },
            )
        }
        Value::ColumnKind(value) => {
            use livesplit_core::settings::ColumnKind::*;
            enum_row(
                field,
                &["Time", "Variable"],
                match value {
                    Time => 0,
                    Variable => 1,
                },
                changed,
                |index| Value::ColumnKind(if index == 0 { Time } else { Variable }),
            )
        }
        Value::ColumnStartWith(value) => {
            use livesplit_core::component::splits::ColumnStartWith::*;
            enum_row(
                field,
                &[
                    "Empty",
                    "Comparison Time",
                    "Comparison Segment Time",
                    "Possible Time Save",
                ],
                match value {
                    Empty => 0,
                    ComparisonTime => 1,
                    ComparisonSegmentTime => 2,
                    PossibleTimeSave => 3,
                },
                changed,
                |index| {
                    Value::ColumnStartWith(match index {
                        1 => ComparisonTime,
                        2 => ComparisonSegmentTime,
                        3 => PossibleTimeSave,
                        _ => Empty,
                    })
                },
            )
        }
        Value::ColumnUpdateWith(value) => {
            use livesplit_core::component::splits::ColumnUpdateWith::*;
            enum_row(
                field,
                &[
                    "Don't Update",
                    "Split Time",
                    "Delta",
                    "Delta with Fallback",
                    "Segment Time",
                    "Segment Delta",
                    "Segment Delta with Fallback",
                ],
                match value {
                    DontUpdate => 0,
                    SplitTime => 1,
                    Delta => 2,
                    DeltaWithFallback => 3,
                    SegmentTime => 4,
                    SegmentDelta => 5,
                    SegmentDeltaWithFallback => 6,
                },
                changed,
                |index| {
                    Value::ColumnUpdateWith(match index {
                        1 => SplitTime,
                        2 => Delta,
                        3 => DeltaWithFallback,
                        4 => SegmentTime,
                        5 => SegmentDelta,
                        6 => SegmentDeltaWithFallback,
                        _ => DontUpdate,
                    })
                },
            )
        }
        Value::ColumnUpdateTrigger(value) => {
            use livesplit_core::component::splits::ColumnUpdateTrigger::*;
            enum_row(
                field,
                &["On Starting Segment", "Contextual", "On Ending Segment"],
                match value {
                    OnStartingSegment => 0,
                    Contextual => 1,
                    OnEndingSegment => 2,
                },
                changed,
                |index| {
                    Value::ColumnUpdateTrigger(match index {
                        0 => OnStartingSegment,
                        2 => OnEndingSegment,
                        _ => Contextual,
                    })
                },
            )
        }
        Value::SubsplitDisplayMode(value) => {
            use livesplit_core::component::splits::SubsplitDisplayMode::*;
            enum_row(
                field,
                &["Flat", "Current Group Expanded", "All Groups Expanded"],
                match value {
                    Flat => 0,
                    CurrentGroupExpanded => 1,
                    AllGroupsExpanded => 2,
                },
                changed,
                |index| {
                    Value::SubsplitDisplayMode(match index {
                        0 => Flat,
                        2 => AllGroupsExpanded,
                        _ => CurrentGroupExpanded,
                    })
                },
            )
        }
        Value::IconDisplayMode(value) => {
            use livesplit_core::component::detailed_timer::IconDisplayMode::*;
            enum_row(
                field,
                &["Hidden", "Both Rows", "First Row"],
                match value {
                    Hidden => 0,
                    BothRows => 1,
                    FirstRow => 2,
                },
                changed,
                |index| {
                    Value::IconDisplayMode(match index {
                        1 => BothRows,
                        2 => FirstRow,
                        _ => Hidden,
                    })
                },
            )
        }
        Value::Hotkey(value) => {
            let row = adw::ActionRow::builder()
                .title(field.text.as_ref())
                .subtitle(
                    value
                        .as_ref()
                        .map_or("Not assigned".into(), |value| format!("{value:?}")),
                )
                .build();
            let clear = gtk::Button::with_label("Clear");
            clear.set_valign(gtk::Align::Center);
            clear.connect_clicked(move |_| changed(Value::Hotkey(None)));
            row.add_suffix(&clear);
            row.upcast()
        }
        Value::LayoutBackground(background) => {
            use livesplit_core::settings::LayoutBackground;
            let expander = adw::ExpanderRow::builder()
                .title(field.text.as_ref())
                .subtitle(field.tooltip.as_ref())
                .build();
            match background {
                LayoutBackground::Gradient(gradient) => {
                    let nested = Field::new(
                        "Gradient".into(),
                        "Background gradient and colors".into(),
                        Value::Gradient(*gradient),
                    );
                    let colors = gradient_row(&nested, *gradient, changed, |gradient| {
                        Value::LayoutBackground(LayoutBackground::Gradient(gradient))
                    });
                    expander.add_row(&colors);
                }
                LayoutBackground::Image(image) => {
                    let current = Rc::new(Cell::new(*image));
                    for (title, value, property) in [
                        ("Brightness", image.brightness, 0_u8),
                        ("Opacity", image.opacity, 1),
                        ("Blur", image.blur, 2),
                    ] {
                        let adjustment =
                            gtk::Adjustment::new(value as f64, 0.0, 1.0, 0.01, 0.1, 0.0);
                        let row = adw::SpinRow::builder()
                            .title(title)
                            .digits(2)
                            .adjustment(&adjustment)
                            .build();
                        let current = current.clone();
                        let image_changed = changed.clone();
                        row.connect_value_notify(move |row| {
                            let mut image = current.get();
                            match property {
                                0 => image.brightness = row.value() as f32,
                                1 => image.opacity = row.value() as f32,
                                _ => image.blur = row.value() as f32,
                            }
                            current.set(image);
                            image_changed(Value::LayoutBackground(LayoutBackground::Image(image)));
                        });
                        expander.add_row(&row);
                    }
                }
            }
            expander.upcast()
        }
        Value::DeltaGradient(value) => {
            use livesplit_core::component::timer::DeltaGradient;
            let selected = match value {
                DeltaGradient::Gradient(_) => 0,
                DeltaGradient::DeltaPlain => 1,
                DeltaGradient::DeltaVertical => 2,
                DeltaGradient::DeltaHorizontal => 3,
            };
            let gradient = match value {
                DeltaGradient::Gradient(value) => *value,
                _ => livesplit_core::settings::Gradient::Transparent,
            };
            enum_row(
                field,
                &[
                    "Custom Gradient",
                    "Delta Plain",
                    "Delta Vertical",
                    "Delta Horizontal",
                ],
                selected,
                changed,
                move |index| {
                    Value::DeltaGradient(match index {
                        1 => DeltaGradient::DeltaPlain,
                        2 => DeltaGradient::DeltaVertical,
                        3 => DeltaGradient::DeltaHorizontal,
                        _ => DeltaGradient::Gradient(gradient),
                    })
                },
            )
        }
    }
}

fn enum_row(
    field: &Field,
    labels: &[&str],
    selected: u32,
    changed: SettingChanged,
    value: impl Fn(u32) -> Value + 'static,
) -> adw::PreferencesRow {
    let model = gtk::StringList::new(labels);
    let row = adw::ComboRow::builder()
        .title(field.text.as_ref())
        .subtitle(field.tooltip.as_ref())
        .model(&model)
        .selected(selected)
        .build();
    row.connect_selected_notify(move |row| changed(value(row.selected())));
    row.upcast()
}

fn gradient_colors(
    gradient: livesplit_core::settings::Gradient,
) -> (
    livesplit_core::settings::Color,
    livesplit_core::settings::Color,
) {
    use livesplit_core::settings::{Color, Gradient};
    match gradient {
        Gradient::Transparent => (Color::transparent(), Color::transparent()),
        Gradient::Plain(color) => (color, color),
        Gradient::Vertical(a, b) | Gradient::Horizontal(a, b) => (a, b),
    }
}

fn gradient_row(
    field: &Field,
    gradient: livesplit_core::settings::Gradient,
    changed: SettingChanged,
    wrap: impl Fn(livesplit_core::settings::Gradient) -> Value + 'static,
) -> adw::PreferencesRow {
    use livesplit_core::settings::Gradient;
    let selected = match gradient {
        Gradient::Transparent => 0,
        Gradient::Plain(_) => 1,
        Gradient::Vertical(_, _) => 2,
        Gradient::Horizontal(_, _) => 3,
    };
    let (first, second) = gradient_colors(gradient);
    let current = Rc::new(Cell::new(gradient));
    let wrap = Rc::new(wrap);
    let expander = adw::ExpanderRow::builder()
        .title(field.text.as_ref())
        .subtitle(field.tooltip.as_ref())
        .build();
    let kind_field = Field::new(
        "Kind".into(),
        "The direction and number of colors".into(),
        Value::Bool(false),
    );
    let kind_wrap = wrap.clone();
    let kind_current = current.clone();
    let kind = enum_row(
        &kind_field,
        &["Transparent", "Plain", "Vertical", "Horizontal"],
        selected,
        changed.clone(),
        move |index| {
            let (first, second) = gradient_colors(kind_current.get());
            let gradient = match index {
                0 => Gradient::Transparent,
                1 => Gradient::Plain(first),
                2 => Gradient::Vertical(first, second),
                _ => Gradient::Horizontal(first, second),
            };
            kind_current.set(gradient);
            kind_wrap(gradient)
        },
    );
    expander.add_row(&kind);

    let first_field = Field::new(
        "First Color".into(),
        "Plain, top, or left color".into(),
        Value::Color(first),
    );
    let first_wrap = wrap.clone();
    let first_current = current.clone();
    let first_changed = changed.clone();
    let first_color = color_row(
        &first_field,
        first,
        Rc::new(move |value| {
            let Value::Color(color) = value else { return };
            let gradient = match first_current.get() {
                Gradient::Transparent => Gradient::Plain(color),
                Gradient::Plain(_) => Gradient::Plain(color),
                Gradient::Vertical(_, b) => Gradient::Vertical(color, b),
                Gradient::Horizontal(_, b) => Gradient::Horizontal(color, b),
            };
            first_current.set(gradient);
            first_changed(first_wrap(gradient));
        }),
    );
    expander.add_row(&first_color);

    let second_field = Field::new(
        "Second Color".into(),
        "Bottom or right color".into(),
        Value::Color(second),
    );
    let second_wrap = wrap.clone();
    let second_current = current.clone();
    let second_color = color_row(
        &second_field,
        second,
        Rc::new(move |value| {
            let Value::Color(color) = value else {
                return;
            };
            let gradient = match second_current.get() {
                Gradient::Transparent => Gradient::Vertical(first, color),
                Gradient::Plain(a) => Gradient::Vertical(a, color),
                Gradient::Vertical(a, _) => Gradient::Vertical(a, color),
                Gradient::Horizontal(a, _) => Gradient::Horizontal(a, color),
            };
            second_current.set(gradient);
            changed(second_wrap(gradient));
        }),
    );
    expander.add_row(&second_color);
    expander.upcast()
}

fn color_row(
    field: &Field,
    color: livesplit_core::settings::Color,
    changed: SettingChanged,
) -> adw::PreferencesRow {
    let row = adw::ActionRow::builder()
        .title(field.text.as_ref())
        .subtitle(field.tooltip.as_ref())
        .activatable(true)
        .build();
    let [red, green, blue, alpha] = color.to_rgba8();
    let button = gtk::Button::new();
    let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(24);
    swatch.set_content_height(18);
    swatch.set_valign(gtk::Align::Center);
    let swatch_color = Rc::new(Cell::new(gtk::gdk::RGBA::new(
        color.red,
        color.green,
        color.blue,
        color.alpha,
    )));
    let draw_color = swatch_color.clone();
    swatch.set_draw_func(move |_, context, width, height| {
        let tile = 4.0;
        for y in 0..=((height as f64 / tile) as usize) {
            for x in 0..=((width as f64 / tile) as usize) {
                let shade = if (x + y) % 2 == 0 { 0.72 } else { 0.92 };
                context.set_source_rgb(shade, shade, shade);
                context.rectangle(x as f64 * tile, y as f64 * tile, tile, tile);
                context.fill().ok();
            }
        }
        let color = draw_color.get();
        context.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            color.alpha() as f64,
        );
        context.rectangle(0.5, 0.5, width as f64 - 1.0, height as f64 - 1.0);
        context.fill_preserve().ok();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        context.set_line_width(1.0);
        context.stroke().ok();
    });
    let hex_label = gtk::Label::new(Some(&format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}")));
    hex_label.add_css_class("monospace");
    button_content.append(&swatch);
    button_content.append(&hex_label);
    button.set_child(Some(&button_content));
    button.set_valign(gtk::Align::Center);
    row.add_suffix(&button);
    let initial = gtk::gdk::RGBA::new(color.red, color.green, color.blue, color.alpha);
    button.connect_clicked(move |_| {
        let dialog = gtk::ColorDialog::builder()
            .title("Choose Color")
            .with_alpha(true)
            .build();
        let hex_label = hex_label.clone();
        let swatch = swatch.clone();
        let swatch_color = swatch_color.clone();
        let changed = changed.clone();
        dialog.choose_rgba(
            None::<&gtk::Window>,
            Some(&initial),
            gio::Cancellable::NONE,
            move |result| {
                if let Ok(color) = result {
                    let value = livesplit_core::settings::Color::rgba(
                        color.red(),
                        color.green(),
                        color.blue(),
                        color.alpha(),
                    );
                    let [r, g, b, a] = value.to_rgba8();
                    hex_label.set_label(&format!("#{r:02X}{g:02X}{b:02X}{a:02X}"));
                    swatch_color.set(color);
                    swatch.queue_draw();
                    changed(Value::Color(value));
                }
            },
        );
    });
    row.upcast()
}

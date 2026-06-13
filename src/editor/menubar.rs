use std::io;

use crossterm::{cursor, queue, style, terminal};

use crate::engine::source::SceneObject;
use crate::menubar::print_menu_item;

use super::properties::{self, PropertyKind};
use super::state::{EditorState, Mode, TableCellSubState};
use super::ui::Layout;

/// Items are listed in a consistent order:
///   motion → resize → property nav → value edit → escape/global
fn mode_items(state: &EditorState) -> Vec<&'static str> {
    match &state.mode {
        Mode::Normal => vec![
            "[←][→] frame",
            "[⇧←][⇧→] ±10",
            "[a]dd",
            "[s]elect",
            "[c]opy",
            "[v] paste",
            "[f]rame",
            "settin[g]s",
            "[Ctrl-s]ave",
            "[S]ave as",
            "[q]uit",
            "[F]ull",
        ],
        Mode::SaveAs { .. } => vec![
            "[type] filename",
            "[Enter] save",
            "[Esc] cancel",
        ],
        Mode::FrameMenu => vec![
            "[a]dd blank",
            "[c]opy",
            "[o]verlay onto",
            "[j]ump",
            "[s]elect",
            "[d]elete",
            "[m]ove",
            "[Esc] back",
            "[F]ull",
        ],
        Mode::FrameJump { .. } => vec![
            "[type] frame #",
            "[Enter] jump",
            "[Esc] cancel",
        ],
        Mode::FrameSelectInput { .. } => vec![
            "[type] 1, 2, 3 or 5-12",
            "[Enter] select",
            "[Esc] cancel",
        ],
        Mode::FrameSelected { .. } => vec![
            "[d]elete selected",
            "[m]ove range",
            "[c]opy range",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::FrameRangePlace { copy, .. } => vec![
            "[←][→] pick target",
            if *copy { "[Enter] copy after" } else { "[Enter] move after" },
            "[b]efore",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::FrameMove { .. } => vec![
            "[←][→] pick slide",
            "[Enter] place here",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::FrameOverlay { .. } => vec![
            "[←][→] pick target",
            "[Enter] paste here",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::FrameMovePlace { .. } => vec![
            "[Enter] after",
            "[b]efore",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::Settings { .. } => vec![
            "[↑↓][Tab] field",
            "[0-9] edit",
            "[Enter] apply",
            "[Esc] cancel",
        ],
        Mode::AddObject { .. } => {
            vec!["[↑][↓] type", "[Enter] add", "[Esc] cancel", "[F]ull"]
        }
        Mode::SelectObject { .. } => vec![
            "[↑][↓] select",
            "[Enter] pick",
            "[d]el",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::SelectedObject { .. } => vec![
            "[←→↑↓] move",
            "[r]esize",
            "[Shift+←→↑↓] grow",
            "[e]dit props",
            "[c]opy",
            "[d]el",
            "[Esc] back",
            "[F]ull",
        ],
        Mode::ResizeObject { .. } => vec![
            "[←→] width",
            "[↑↓] height",
            "[Enter][Esc] done",
            "[F]ull",
        ],
        // Browsing properties — action hint depends on the selected property type
        Mode::EditProperties { editing_value: None, object_index, selected_property, .. } => {
            let props = properties::get_properties(&state.source.objects, *object_index);
            let prop = &props[*selected_property];
            let is_bool = prop.kind == PropertyKind::Bool;
            let is_coord = prop.kind == PropertyKind::Coordinate;
            let is_table = matches!(
                state.source.objects.get(*object_index),
                Some(SceneObject::Table(_))
            );
            let mut items = vec!["[↑][↓] prop"];
            if is_bool {
                items.push("[Enter][Space] toggle");
            } else {
                items.push("[Enter] edit");
                if is_coord {
                    items.push("[a]nimate");
                }
            }
            if is_table {
                items.push("[Alt-c] edit cells");
                items.push("[Alt-a] +col after");
                items.push("[Alt-b] +col before");
                items.push("[Alt-r] -col");
            }
            items.push("[Esc] back");
            items.push("[F]ull");
            items
        }
        // Editing a property value
        Mode::EditProperties { editing_value: Some(_), .. } => vec![
            "[←][→] cursor",
            "[Alt+Enter] newline",
            "[Enter] apply",
            "[Esc] cancel",
        ],
        Mode::AnimateProperty { editing: None, .. } => vec![
            "[↑][↓] field",
            "[Enter] edit",
            "[Space] toggle",
            "[s]ave anim",
            "[x] →fixed",
            "[Esc] cancel",
            "[F]ull",
        ],
        Mode::AnimateProperty { editing: Some(_), .. } => vec![
            "[←][→] cursor",
            "[Enter] apply",
            "[Esc] cancel",
        ],
        Mode::Confirm { .. } => vec![
            "[↑][↓] select",
            "[Enter] confirm",
            "[Esc] cancel",
        ],
        Mode::MultiSelect { purpose, .. } => vec![
            "[↑][↓] navigate",
            "[Space] toggle",
            match purpose {
                super::state::MultiSelectPurpose::Group => "[Enter] create group",
                super::state::MultiSelectPurpose::Copy => "[Enter] copy",
            },
            "[Esc] cancel",
        ],
        Mode::PastePlacing { linked, .. } => vec![
            "[←→↑↓] move",
            "[Enter] stamp",
            if *linked { "[l] linked" } else { "[l] independent" },
            "[Esc] done",
        ],
        Mode::AddArt { .. } => vec![
            "[↑][↓] navigate",
            "[Enter] add",
            "[Esc] back",
        ],
        Mode::LoadArtFile { .. } => vec![
            "[type] file path",
            "[Enter] load",
            "[Esc] back",
        ],
        Mode::TableAddColumn { after, .. } => vec![
            if *after { "[col#] add after" } else { "[col#] add before" },
            "[Enter] confirm",
            "[Esc] cancel",
        ],
        Mode::TableRemoveColumn { .. } => vec![
            "[col#] remove col",
            "[Enter] confirm",
            "[Esc] cancel",
        ],
        Mode::TableEditCellProps { sub_state, .. } => match sub_state {
            TableCellSubState::Selecting => vec![
                "[←→↑↓] navigate",
                "[Space] select/deselect",
                "[l] add list",
                "[s] cell style",
                "[Enter] edit content",
                "[Esc] back",
            ],
            TableCellSubState::EditingContent { .. } => vec![
                "[←→] move cursor",
                "[type] insert after",
                "[Alt+Enter] newline",
                "[Backspace] delete",
                "[Enter] save",
                "[Esc] cancel",
            ],
            TableCellSubState::EditingStyle { selected_prop, .. } => {
                let name = properties::CELL_STYLE_PROPS.get(*selected_prop).copied().unwrap_or("");
                let mut v = vec!["[↑][↓] prop"];
                if name == "bold" || name == "dimmed" {
                    v.push("[Enter][Space] toggle");
                } else {
                    v.push("[Enter] pick color");
                }
                v.push("[Esc] back");
                v
            }
        },
    }
}

pub fn render_menubar(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    if layout.menu_h == 0 {
        return Ok(());
    }

    let items = mode_items(state);

    let mut line: u16 = 0;
    let mut x: u16 = 1; // leading space

    // Initialise first line
    queue!(
        stdout,
        cursor::MoveTo(0, 0),
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::Print(" "),
    )?;

    for (i, item) in items.iter().enumerate() {
        let item_w = item.chars().count() as u16;

        if i > 0 {
            // Does separator (2) + item fit on the current line?
            if x + 2 + item_w > layout.term_width {
                // Wrap if another menu line is available
                if line + 1 < layout.menu_h {
                    line += 1;
                    x = 1;
                    queue!(
                        stdout,
                        cursor::MoveTo(0, line),
                        terminal::Clear(terminal::ClearType::CurrentLine),
                        style::Print(" "),
                    )?;
                } else {
                    break; // No more lines — drop remaining items
                }
            } else {
                queue!(stdout, style::Print("  "))?;
                x += 2;
            }
        }

        if x + item_w > layout.term_width {
            break; // Item wider than remaining space even alone
        }

        print_menu_item(stdout, item)?;
        x += item_w;
    }

    // Clear any remaining allocated menu lines that were not used
    for l in line + 1..layout.menu_h {
        queue!(
            stdout,
            cursor::MoveTo(0, l),
            terminal::Clear(terminal::ClearType::CurrentLine),
        )?;
    }

    Ok(())
}

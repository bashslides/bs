use std::io;

use crossterm::{cursor, queue, style, terminal};

use crate::engine::source::SceneObject;
use crate::menubar::print_menu_item;

use super::properties::{self, PropertyKind};
use super::state::{EditorState, Mode};
use super::ui::Layout;

/// Items are listed in a consistent order:
///   motion → resize → property nav → value edit → escape/global
fn mode_items(state: &EditorState) -> Vec<&'static str> {
    match &state.mode {
        Mode::Normal => vec![
            "[←][→] frame",
            "[a]dd",
            "[s]elect",
            "[+] dup",
            "[-] del",
            "[Ctrl-s]ave",
            "[q]uit",
            "[F11] full",
        ],
        Mode::AddObject { .. } => {
            vec!["[↑][↓] type", "[Enter] add", "[Esc] cancel", "[F11] full"]
        }
        Mode::SelectObject { .. } => vec![
            "[↑][↓] select",
            "[Enter] pick",
            "[d]el",
            "[Esc] cancel",
            "[F11] full",
        ],
        Mode::SelectedObject { .. } => vec![
            "[←→↑↓] move",
            "[Shift+←→↑↓] grow",
            "[Ctrl+Shift+←→↑↓] shrink",
            "[e]dit props",
            "[d]el",
            "[Esc] back",
            "[F11] full",
        ],
        // Browsing properties — action hint depends on the selected property type
        Mode::EditProperties { editing_value: None, object_index, selected_property, .. } => {
            let props = properties::get_properties(&state.source.objects, *object_index);
            let prop = &props[*selected_property];
            let is_bool = prop.value == "true" || prop.value == "false";
            let is_coord = prop.kind == PropertyKind::Coordinate;
            let is_table = matches!(
                state.source.objects.get(*object_index),
                Some(SceneObject::Table(_))
            );
            let mut items = vec!["[↑][↓] prop", "[Enter] edit"];
            if is_bool {
                items.push("[Space] toggle");
            } else if is_coord {
                items.push("[a]nimate");
            }
            if is_table {
                items.push("[Alt-c] edit cells");
                items.push("[Alt-a] +col after");
                items.push("[Alt-b] +col before");
                items.push("[Alt-r] -col");
            }
            items.push("[Esc] back");
            items.push("[F11] full");
            items
        }
        // Editing a property value
        Mode::EditProperties { editing_value: Some(_), .. } => vec![
            "[←][→] cursor",
            "[Alt+Enter] newline",
            "[Enter] apply",
            "[Esc] cancel",
            "[F11] full",
        ],
        Mode::AnimateProperty { editing: None, .. } => vec![
            "[↑][↓] field",
            "[Enter] edit",
            "[s]ave anim",
            "[x] →fixed",
            "[Esc] cancel",
            "[F11] full",
        ],
        Mode::AnimateProperty { editing: Some(_), .. } => vec![
            "[←][→] cursor",
            "[Enter] apply",
            "[Esc] cancel",
            "[F11] full",
        ],
        Mode::Confirm { .. } => vec![
            "[↑][↓] select",
            "[Enter] confirm",
            "[Esc] cancel",
        ],
        Mode::SelectGroupMembers { .. } => vec![
            "[↑][↓] navigate",
            "[Space] toggle",
            "[Enter] create group",
            "[Esc] cancel",
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
        Mode::TableEditCellProps { .. } => vec![
            "[←→↑↓] navigate",
            "[Space] select/deselect",
            "[l] add list",
            "[s] cell style",
            "[Enter] edit content",
            "[Esc] back",
        ],
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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    pub key_bindings: KeyBindings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    pub next_frame: String,
    pub prev_frame: String,
    pub add_object: String,
    pub select_object: String,
    pub edit_object: String,
    pub delete_object: String,
    pub save: String,
    pub quit: String,
    pub confirm: String,
    pub cancel: String,
    pub move_up: String,
    pub move_down: String,
    #[serde(default = "default_fullscreen")]
    pub fullscreen: String,
    pub animate: String,
    pub insert_newline: String,
    // Table-specific bindings (active only when editing a Table object or in table modes)
    #[serde(default = "default_table_add_col_after")]
    pub table_add_col_after: String,
    #[serde(default = "default_table_add_col_before")]
    pub table_add_col_before: String,
    #[serde(default = "default_table_remove_col")]
    pub table_remove_col: String,
    #[serde(default = "default_table_edit_cells")]
    pub table_edit_cells: String,
    // Active inside TableEditCellProps mode
    #[serde(default = "default_table_add_list")]
    pub table_add_list: String,
    #[serde(default = "default_table_edit_cell_style")]
    pub table_edit_cell_style: String,
    /// Open the presentation settings (frame size) from Normal mode.
    #[serde(default = "default_open_settings")]
    pub open_settings: String,
    /// Enter resize mode (arrow-key resize) from the selected-object menu.
    #[serde(default = "default_resize_object")]
    pub resize_object: String,
    /// Open the frame operations sub-menu (add/copy/delete/move) from Normal.
    #[serde(default = "default_frame_menu")]
    pub frame_menu: String,
    /// Within the frame sub-menu: add a blank frame.
    #[serde(default = "default_frame_add")]
    pub frame_add: String,
    /// Within the frame sub-menu: copy (duplicate) the current frame.
    #[serde(default = "default_frame_copy")]
    pub frame_copy: String,
    /// Within the frame sub-menu: delete the current frame.
    #[serde(default = "default_frame_delete")]
    pub frame_delete: String,
    /// Within the frame sub-menu: move (relocate) the current frame.
    #[serde(default = "default_frame_move")]
    pub frame_move: String,
    /// While placing a moved frame: drop it *before* the shown frame
    /// (Enter drops it after).
    #[serde(default = "default_frame_move_before")]
    pub frame_move_before: String,
}

fn default_table_add_col_after() -> String { "Alt-a".into() }
fn default_table_add_col_before() -> String { "Alt-b".into() }
fn default_table_remove_col() -> String { "Alt-r".into() }
fn default_table_edit_cells() -> String { "Alt-c".into() }
fn default_table_add_list() -> String { "l".into() }
fn default_table_edit_cell_style() -> String { "s".into() }
fn default_open_settings() -> String { "g".into() }
fn default_resize_object() -> String { "r".into() }
fn default_fullscreen() -> String { "F".into() }
fn default_frame_menu() -> String { "f".into() }
fn default_frame_add() -> String { "a".into() }
fn default_frame_copy() -> String { "c".into() }
fn default_frame_delete() -> String { "d".into() }
fn default_frame_move() -> String { "m".into() }
fn default_frame_move_before() -> String { "b".into() }

impl Default for EditorConfig {
    fn default() -> Self {
        EditorConfig {
            key_bindings: KeyBindings {
                next_frame: "Right".into(),
                prev_frame: "Left".into(),
                add_object: "a".into(),
                select_object: "s".into(),
                edit_object: "e".into(),
                delete_object: "d".into(),
                save: "Ctrl-s".into(),
                quit: "q".into(),
                confirm: "Enter".into(),
                cancel: "Esc".into(),
                move_up: "Up".into(),
                move_down: "Down".into(),
                fullscreen: default_fullscreen(),
                animate: "a".into(),
                insert_newline: "Alt-Enter".into(),
                table_add_col_after: default_table_add_col_after(),
                table_add_col_before: default_table_add_col_before(),
                table_remove_col: default_table_remove_col(),
                table_edit_cells: default_table_edit_cells(),
                table_add_list: default_table_add_list(),
                table_edit_cell_style: default_table_edit_cell_style(),
                open_settings: default_open_settings(),
                resize_object: default_resize_object(),
                frame_menu: default_frame_menu(),
                frame_add: default_frame_add(),
                frame_copy: default_frame_copy(),
                frame_delete: default_frame_delete(),
                frame_move: default_frame_move(),
                frame_move_before: default_frame_move_before(),
            },
        }
    }
}

impl EditorConfig {
    pub fn load() -> Self {
        let config_path = Self::config_path();
        match std::fs::read_to_string(&config_path) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Warning: invalid editor config ({e}), using defaults");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    fn config_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let mut path = std::path::PathBuf::from(home);
        path.push(".config");
        path.push("bs");
        path.push("editor.json");
        path
    }
}

/// Check whether a crossterm `KeyEvent` matches a binding string from config.
pub fn matches_binding(binding: &str, event: &KeyEvent) -> bool {
    // Handle Alt- prefix
    if let Some(rest) = binding.strip_prefix("Alt-") {
        if !event.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        return match rest {
            "Enter" => event.code == KeyCode::Enter,
            other => {
                if let Some(c) = other.chars().next() {
                    event.code == KeyCode::Char(c)
                } else {
                    false
                }
            }
        };
    }

    // Handle Ctrl- prefix
    if let Some(ch) = binding.strip_prefix("Ctrl-") {
        if !event.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        return match ch {
            "s" => event.code == KeyCode::Char('s'),
            "q" => event.code == KeyCode::Char('q'),
            other => {
                if let Some(c) = other.chars().next() {
                    event.code == KeyCode::Char(c)
                } else {
                    false
                }
            }
        };
    }

    // For non-Ctrl, non-Alt bindings, reject if Ctrl or Alt is held.
    // This prevents plain bindings like "a" from accidentally firing on Alt-a.
    if event.modifiers.contains(KeyModifiers::CONTROL)
        || event.modifiers.contains(KeyModifiers::ALT)
    {
        return false;
    }

    match binding {
        "Right" => event.code == KeyCode::Right,
        "Left" => event.code == KeyCode::Left,
        "Up" => event.code == KeyCode::Up,
        "Down" => event.code == KeyCode::Down,
        "Enter" => event.code == KeyCode::Enter,
        "Esc" => event.code == KeyCode::Esc,
        "Space" => event.code == KeyCode::Char(' '),
        "Tab" => event.code == KeyCode::Tab,
        "Backspace" => event.code == KeyCode::Backspace,
        "Home" => event.code == KeyCode::Home,
        "End" => event.code == KeyCode::End,
        s => {
            // F-key binding: "F1" through "F12" etc.
            if let Some(rest) = s.strip_prefix('F') {
                if let Ok(n) = rest.parse::<u8>() {
                    return event.code == KeyCode::F(n);
                }
            }
            // Single character binding
            if let Some(c) = s.chars().next() {
                event.code == KeyCode::Char(c)
            } else {
                false
            }
        }
    }
}

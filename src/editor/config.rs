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
    pub add_frame: String,
    pub remove_frame: String,
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
}

fn default_table_add_col_after() -> String { "Alt-a".into() }
fn default_table_add_col_before() -> String { "Alt-b".into() }
fn default_table_remove_col() -> String { "Alt-r".into() }
fn default_table_edit_cells() -> String { "Alt-c".into() }
fn default_table_add_list() -> String { "l".into() }
fn default_table_edit_cell_style() -> String { "s".into() }

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
                add_frame: "+".into(),
                remove_frame: "-".into(),
                fullscreen: "F11".into(),
                animate: "a".into(),
                insert_newline: "Alt-Enter".into(),
                table_add_col_after: default_table_add_col_after(),
                table_add_col_before: default_table_add_col_before(),
                table_remove_col: default_table_remove_col(),
                table_edit_cells: default_table_edit_cells(),
                table_add_list: default_table_add_list(),
                table_edit_cell_style: default_table_edit_cell_style(),
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
        path.push("ascii-presenter");
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

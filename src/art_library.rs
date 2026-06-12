//! A small, extendable library of pre-made ASCII-art pieces.
//!
//! Built-ins live here as `&'static str` constants; add a piece by writing the
//! art and listing it in [`BUILTINS`]. Users can also drop their own files into
//! `~/.config/bs/art/` (one piece per file, the file stem is its name), or load
//! an arbitrary file at runtime via [`load_file`].
//!
//! The art is only a *palette*: when a piece is added to a presentation its
//! text is copied into the object, so presentations never depend on the library
//! being present later.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One named piece of ASCII art.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtItem {
    pub name: String,
    pub art: String,
}

// NOTE: continuation lines are flush-left on purpose — in a raw string every
// leading space is part of the art.
const HUMAN: &str = r#"   O
  /|\
   |
  / \"#;

const GHOST: &str = r#"  .-.
 (o o)
 | O \
  \   \
   `~~~'"#;

const TREE: &str = r#"        /\
       /  \
      /____\
      /    \
     /      \
    /________\
    /        \
   /          \
  /____________\
       |  |
       |  |
      _|__|_"#;

// A matched ball/square pair (same 7×5 bounding box) — handy as the two ends of
// a `Morph` (the canonical "ball morphs into a square" demo).
const BALL: &str = r#" .---.
/     \
|     |
\     /
 '---' "#;

const SQUARE: &str = r#"+-----+
|     |
|     |
|     |
+-----+"#;

/// Built-in pieces, in display order. Add new art here.
pub const BUILTINS: &[(&str, &str)] = &[
    ("human", HUMAN),
    ("ghost", GHOST),
    ("tree", TREE),
    ("ball", BALL),
    ("square", SQUARE),
];

/// The built-in pieces as owned [`ArtItem`]s.
pub fn builtins() -> Vec<ArtItem> {
    BUILTINS
        .iter()
        .map(|(name, art)| ArtItem { name: (*name).to_string(), art: (*art).to_string() })
        .collect()
}

/// Directory holding user art files: `~/.config/bs/art/`.
pub fn custom_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("bs").join("art")
}

/// Load every art file in [`custom_dir`], sorted by name. A missing directory
/// or unreadable file yields no items rather than an error.
pub fn load_custom() -> Vec<ArtItem> {
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(custom_dir()) else {
        return items;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(item) = load_file(&path) {
                items.push(item);
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

/// Built-in pieces followed by any user pieces from [`custom_dir`].
pub fn all_items() -> Vec<ArtItem> {
    let mut items = builtins();
    items.extend(load_custom());
    items
}

/// Load a single art file. The item name is the file stem (e.g. `cat.txt` →
/// `cat`). A single trailing newline is trimmed so it doesn't add a blank row.
pub fn load_file(path: &Path) -> Result<ArtItem> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read art file {}", path.display()))?;
    let art = raw.strip_suffix('\n').unwrap_or(&raw).to_string();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("custom")
        .to_string();
    Ok(ArtItem { name, art })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_present_and_named() {
        let items = builtins();
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["human", "ghost", "tree", "ball", "square"]);
        // Art is preserved verbatim, including the leading spaces that align it.
        let human = &items[0].art;
        assert_eq!(human.lines().next().unwrap(), "   O");
        assert_eq!(human.lines().count(), 4);
    }
}

use std::io;

use crossterm::{queue, style};

/// Print a menu item string, bolding any text inside `[...]` brackets.
/// Text outside brackets is printed dim.
pub fn print_menu_item(stdout: &mut io::Stdout, item: &str) -> anyhow::Result<()> {
    let mut rest = item;
    while !rest.is_empty() {
        if let Some(open) = rest.find('[') {
            if open > 0 {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Dim),
                    style::Print(&rest[..open]),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            }
            rest = &rest[open..];
            if let Some(close) = rest.find(']') {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Bold),
                    style::Print(&rest[..=close]),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
                rest = &rest[close + 1..];
            } else {
                queue!(stdout, style::Print(rest))?;
                break;
            }
        } else {
            queue!(
                stdout,
                style::SetAttribute(style::Attribute::Dim),
                style::Print(rest),
                style::SetAttribute(style::Attribute::Reset),
            )?;
            break;
        }
    }
    Ok(())
}

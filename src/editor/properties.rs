use anyhow::{bail, Result};

use crate::engine::source::{Coordinate, SceneObject};
use crate::types::{Color, NamedColor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyKind {
    Text,
    Color,
    Coordinate,
    /// Read-only member entry; `value` holds the member's object index as a string.
    GroupMember,
    /// Computed / read-only display field; cannot be edited.
    ReadOnly,
    /// Dropdown for arrow head character selection.
    HeadChar,
    /// Dropdown for arrow body character selection.
    BodyChar,
    /// Table column width (stored as percentage 0..100).
    TableColWidth,
}

// ---------------------------------------------------------------------------
// Table column-width property name helpers
// ---------------------------------------------------------------------------

/// Static property names for up to 16 table columns.
pub const TABLE_COL_WIDTH_NAMES: &[&str] = &[
    "col_0_width",  "col_1_width",  "col_2_width",  "col_3_width",
    "col_4_width",  "col_5_width",  "col_6_width",  "col_7_width",
    "col_8_width",  "col_9_width",  "col_10_width", "col_11_width",
    "col_12_width", "col_13_width", "col_14_width", "col_15_width",
];

/// Parse a "col_N_width" name and return the column index N.
pub fn parse_col_width_name(name: &str) -> Option<usize> {
    let n = name.strip_prefix("col_")?.strip_suffix("_width")?;
    n.parse().ok()
}

/// Style property names used when editing cell styles in TableEditCellProps.
pub const CELL_STYLE_PROPS: &[&str] = &["fg_color", "bg_color", "bold", "dimmed"];

pub struct Property {
    pub name: &'static str,
    pub value: String,
    pub kind: PropertyKind,
}

pub const COLOR_OPTIONS: &[&str] = &[
    "RGB", "none", "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

pub const HEAD_CHAR_OPTIONS: &[&str] = &[">", "▶", "→", "◆", "●", "★", "custom"];
pub const BODY_CHAR_OPTIONS: &[&str] = &["─", "═", "·", "~", "=", "custom"];

/// Returns the dropdown option list for a property kind, if it uses a dropdown.
pub fn dropdown_options_for(kind: &PropertyKind) -> Option<&'static [&'static str]> {
    match kind {
        PropertyKind::Color    => Some(COLOR_OPTIONS),
        PropertyKind::HeadChar => Some(HEAD_CHAR_OPTIONS),
        PropertyKind::BodyChar => Some(BODY_CHAR_OPTIONS),
        _                      => None,
    }
}

/// Returns the sentinel string that triggers text-input mode inside a dropdown.
pub fn dropdown_custom_sentinel(kind: &PropertyKind) -> &'static str {
    match kind {
        PropertyKind::Color => "RGB",
        _                   => "custom",
    }
}

// ---------------------------------------------------------------------------
// f64 display helper
// ---------------------------------------------------------------------------

fn fmt_f64(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{:.2}", v)
    }
}

// ---------------------------------------------------------------------------
// Properties list
// ---------------------------------------------------------------------------

/// Returns the editable/displayable properties for object at `object_index`.
/// Groups need access to all objects to compute their bounding box.
pub fn get_properties(objects: &[SceneObject], object_index: usize) -> Vec<Property> {
    let obj = &objects[object_index];
    match obj {
        SceneObject::Label(l) => vec![
            Property { name: "text", value: l.text.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&l.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&l.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&l.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&l.height), kind: PropertyKind::Coordinate },
            Property { name: "framed", value: l.framed.to_string(), kind: PropertyKind::Text },
            Property { name: "frame_fg_color", value: format_opt_color(&l.frame_style.as_ref().and_then(|s| s.fg.clone())), kind: PropertyKind::Color },
            Property { name: "frame_bg_color", value: format_opt_color(&l.frame_style.as_ref().and_then(|s| s.bg.clone())), kind: PropertyKind::Color },
            Property { name: "fg_color", value: format_opt_color(&l.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&l.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: l.style.bold.to_string(), kind: PropertyKind::Text },
            Property { name: "dimmed", value: l.style.dim.to_string(), kind: PropertyKind::Text },
            Property { name: "first_frame", value: l.frames.start.to_string(), kind: PropertyKind::Text },
            Property { name: "last_frame", value: l.frames.end.to_string(), kind: PropertyKind::Text },
            Property { name: "z_order", value: l.z_order.to_string(), kind: PropertyKind::Text },
        ],
        SceneObject::HLine(h) => vec![
            Property { name: "y", value: format_coordinate(&h.y), kind: PropertyKind::Coordinate },
            Property { name: "from_x", value: format_coordinate(&h.x_start), kind: PropertyKind::Coordinate },
            Property { name: "to_x", value: format_coordinate(&h.x_end), kind: PropertyKind::Coordinate },
            Property { name: "draw_char", value: h.ch.to_string(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&h.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&h.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: h.style.bold.to_string(), kind: PropertyKind::Text },
            Property { name: "dimmed", value: h.style.dim.to_string(), kind: PropertyKind::Text },
            Property { name: "first_frame", value: h.frames.start.to_string(), kind: PropertyKind::Text },
            Property { name: "last_frame", value: h.frames.end.to_string(), kind: PropertyKind::Text },
            Property { name: "z_order", value: h.z_order.to_string(), kind: PropertyKind::Text },
        ],
        SceneObject::Rect(r) => vec![
            Property { name: "x", value: format_coordinate(&r.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&r.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&r.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&r.height), kind: PropertyKind::Coordinate },
            Property { name: "title", value: r.title.clone().unwrap_or_default(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&r.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&r.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: r.style.bold.to_string(), kind: PropertyKind::Text },
            Property { name: "dimmed", value: r.style.dim.to_string(), kind: PropertyKind::Text },
            Property { name: "first_frame", value: r.frames.start.to_string(), kind: PropertyKind::Text },
            Property { name: "last_frame", value: r.frames.end.to_string(), kind: PropertyKind::Text },
            Property { name: "z_order", value: r.z_order.to_string(), kind: PropertyKind::Text },
        ],
        SceneObject::Header(h) => vec![
            Property { name: "text", value: h.text.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&h.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&h.position.y), kind: PropertyKind::Coordinate },
            Property { name: "draw_char", value: h.ch.to_string(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&h.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&h.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: h.style.bold.to_string(), kind: PropertyKind::Text },
            Property { name: "dimmed", value: h.style.dim.to_string(), kind: PropertyKind::Text },
            Property { name: "first_frame", value: h.frames.start.to_string(), kind: PropertyKind::Text },
            Property { name: "last_frame", value: h.frames.end.to_string(), kind: PropertyKind::Text },
            Property { name: "z_order", value: h.z_order.to_string(), kind: PropertyKind::Text },
        ],
        SceneObject::Arrow(a) => vec![
            Property { name: "x1", value: format_coordinate(&a.x1), kind: PropertyKind::Coordinate },
            Property { name: "y1", value: format_coordinate(&a.y1), kind: PropertyKind::Coordinate },
            Property { name: "x2", value: format_coordinate(&a.x2), kind: PropertyKind::Coordinate },
            Property { name: "y2", value: format_coordinate(&a.y2), kind: PropertyKind::Coordinate },
            Property { name: "head", value: a.head.to_string(), kind: PropertyKind::Text },
            Property { name: "head_char", value: a.head_ch.map(|c| c.to_string()).unwrap_or_else(|| "auto".to_string()), kind: PropertyKind::HeadChar },
            Property { name: "body_char", value: a.body_ch.map(|c| c.to_string()).unwrap_or_else(|| "auto".to_string()), kind: PropertyKind::BodyChar },
            Property { name: "fg_color", value: format_opt_color(&a.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&a.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: a.style.bold.to_string(), kind: PropertyKind::Text },
            Property { name: "dimmed", value: a.style.dim.to_string(), kind: PropertyKind::Text },
            Property { name: "first_frame", value: a.frames.start.to_string(), kind: PropertyKind::Text },
            Property { name: "last_frame", value: a.frames.end.to_string(), kind: PropertyKind::Text },
            Property { name: "z_order", value: a.z_order.to_string(), kind: PropertyKind::Text },
        ],
        SceneObject::Group(g) => {
            let (gx, gy, gw, gh) = group_bounds(objects, object_index);
            let mut props = vec![
                Property { name: "x",      value: fmt_f64(gx), kind: PropertyKind::ReadOnly },
                Property { name: "y",      value: fmt_f64(gy), kind: PropertyKind::ReadOnly },
                Property { name: "width",  value: fmt_f64(gw), kind: PropertyKind::ReadOnly },
                Property { name: "height", value: fmt_f64(gh), kind: PropertyKind::ReadOnly },
                Property { name: "first_frame", value: g.frames.start.to_string(), kind: PropertyKind::Text },
                Property { name: "last_frame",  value: g.frames.end.to_string(),   kind: PropertyKind::Text },
                Property { name: "z_order",     value: g.z_order.to_string(),      kind: PropertyKind::Text },
            ];
            for &member_idx in &g.members {
                props.push(Property {
                    name: "member",
                    value: member_idx.to_string(),
                    kind: PropertyKind::GroupMember,
                });
            }
            props
        }
        SceneObject::Table(t) => {
            let mut props = vec![
                Property { name: "x",           value: format_coordinate(&t.position.x), kind: PropertyKind::Coordinate },
                Property { name: "y",           value: format_coordinate(&t.position.y), kind: PropertyKind::Coordinate },
                Property { name: "width",       value: format_coordinate(&t.width),      kind: PropertyKind::Coordinate },
                Property { name: "height",      value: format_coordinate(&t.height),     kind: PropertyKind::Coordinate },
                Property { name: "rows",        value: t.rows.to_string(),               kind: PropertyKind::Text },
                Property { name: "cols",        value: t.col_widths.len().to_string(),   kind: PropertyKind::ReadOnly },
                Property { name: "header_bold", value: t.header_bold.to_string(),        kind: PropertyKind::Text },
                Property { name: "borders",     value: t.borders.to_string(),            kind: PropertyKind::Text },
                Property { name: "fg_color",    value: format_opt_color(&t.style.fg),    kind: PropertyKind::Color },
                Property { name: "bg_color",    value: format_opt_color(&t.style.bg),    kind: PropertyKind::Color },
                Property { name: "bold",        value: t.style.bold.to_string(),         kind: PropertyKind::Text },
                Property { name: "dimmed",      value: t.style.dim.to_string(),          kind: PropertyKind::Text },
                Property { name: "first_frame", value: t.frames.start.to_string(),       kind: PropertyKind::Text },
                Property { name: "last_frame",  value: t.frames.end.to_string(),         kind: PropertyKind::Text },
                Property { name: "z_order",     value: t.z_order.to_string(),            kind: PropertyKind::Text },
            ];
            // Per-column width properties
            for (col_idx, &frac) in t.col_widths.iter().enumerate() {
                if col_idx < TABLE_COL_WIDTH_NAMES.len() {
                    props.push(Property {
                        name: TABLE_COL_WIDTH_NAMES[col_idx],
                        value: format!("{:.1}", frac * 100.0),
                        kind: PropertyKind::TableColWidth,
                    });
                }
            }
            props
        }
    }
}

pub fn set_property(obj: &mut SceneObject, name: &str, value: &str) -> Result<()> {
    match obj {
        SceneObject::Label(l) => match name {
            "text" => l.text = value.to_string(),
            "x" => l.position.x = parse_coordinate(value)?,
            "y" => l.position.y = parse_coordinate(value)?,
            "width" => l.width = parse_coordinate(value)?,
            "height" => l.height = parse_coordinate(value)?,
            "framed" => l.framed = parse_bool(value)?,
            "frame_fg_color" => {
                let color = parse_opt_color(value)?;
                let fs = l.frame_style.get_or_insert_with(Default::default);
                fs.fg = color;
                if fs.fg.is_none() && fs.bg.is_none() {
                    l.frame_style = None;
                }
            }
            "frame_bg_color" => {
                let color = parse_opt_color(value)?;
                let fs = l.frame_style.get_or_insert_with(Default::default);
                fs.bg = color;
                if fs.fg.is_none() && fs.bg.is_none() {
                    l.frame_style = None;
                }
            }
            "fg_color" => l.style.fg = parse_opt_color(value)?,
            "bg_color" => l.style.bg = parse_opt_color(value)?,
            "bold" => l.style.bold = parse_bool(value)?,
            "dimmed" => l.style.dim = parse_bool(value)?,
            "first_frame" => l.frames.start = value.parse()?,
            "last_frame" => l.frames.end = value.parse()?,
            "z_order" => l.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::HLine(h) => match name {
            "y" => h.y = parse_coordinate(value)?,
            "from_x" => h.x_start = parse_coordinate(value)?,
            "to_x" => h.x_end = parse_coordinate(value)?,
            "draw_char" => h.ch = parse_char(value)?,
            "fg_color" => h.style.fg = parse_opt_color(value)?,
            "bg_color" => h.style.bg = parse_opt_color(value)?,
            "bold" => h.style.bold = parse_bool(value)?,
            "dimmed" => h.style.dim = parse_bool(value)?,
            "first_frame" => h.frames.start = value.parse()?,
            "last_frame" => h.frames.end = value.parse()?,
            "z_order" => h.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::Rect(r) => match name {
            "x" => r.position.x = parse_coordinate(value)?,
            "y" => r.position.y = parse_coordinate(value)?,
            "width" => r.width = parse_coordinate(value)?,
            "height" => r.height = parse_coordinate(value)?,
            "title" => {
                r.title = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }
            "fg_color" => r.style.fg = parse_opt_color(value)?,
            "bg_color" => r.style.bg = parse_opt_color(value)?,
            "bold" => r.style.bold = parse_bool(value)?,
            "dimmed" => r.style.dim = parse_bool(value)?,
            "first_frame" => r.frames.start = value.parse()?,
            "last_frame" => r.frames.end = value.parse()?,
            "z_order" => r.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::Header(h) => match name {
            "text" => h.text = value.to_string(),
            "x" => h.position.x = parse_coordinate(value)?,
            "y" => h.position.y = parse_coordinate(value)?,
            "draw_char" => h.ch = parse_char(value)?,
            "fg_color" => h.style.fg = parse_opt_color(value)?,
            "bg_color" => h.style.bg = parse_opt_color(value)?,
            "bold" => h.style.bold = parse_bool(value)?,
            "dimmed" => h.style.dim = parse_bool(value)?,
            "first_frame" => h.frames.start = value.parse()?,
            "last_frame" => h.frames.end = value.parse()?,
            "z_order" => h.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::Arrow(a) => match name {
            "x1" => a.x1 = parse_coordinate(value)?,
            "y1" => a.y1 = parse_coordinate(value)?,
            "x2" => a.x2 = parse_coordinate(value)?,
            "y2" => a.y2 = parse_coordinate(value)?,
            "head" => a.head = parse_bool(value)?,
            "head_char" => a.head_ch = parse_opt_char(value)?,
            "body_char" => a.body_ch = parse_opt_char(value)?,
            "fg_color" => a.style.fg = parse_opt_color(value)?,
            "bg_color" => a.style.bg = parse_opt_color(value)?,
            "bold" => a.style.bold = parse_bool(value)?,
            "dimmed" => a.style.dim = parse_bool(value)?,
            "first_frame" => a.frames.start = value.parse()?,
            "last_frame" => a.frames.end = value.parse()?,
            "z_order" => a.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::Group(g) => match name {
            "first_frame" => g.frames.start = value.parse()?,
            "last_frame"  => g.frames.end   = value.parse()?,
            "z_order"     => g.z_order      = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        },
        SceneObject::Table(t) => {
            // Column-width properties: "col_N_width"
            if let Some(col_idx) = parse_col_width_name(name) {
                let pct: f64 = value.trim().trim_end_matches('%').parse()
                    .map_err(|_| anyhow::anyhow!("Invalid percentage: {value}"))?;
                if col_idx < t.col_widths.len() {
                    t.col_widths[col_idx] = (pct / 100.0).max(0.01).min(1.0);
                }
                return Ok(());
            }
            match name {
                "x"           => t.position.x = parse_coordinate(value)?,
                "y"           => t.position.y = parse_coordinate(value)?,
                "width"       => t.width  = parse_coordinate(value)?,
                "height"      => t.height = parse_coordinate(value)?,
                "rows"        => {
                    let new_rows: usize = value.parse()?;
                    t.rows = new_rows.max(1);
                    t.normalize_cells();
                }
                "header_bold" => t.header_bold = parse_bool(value)?,
                "borders"     => t.borders     = parse_bool(value)?,
                "fg_color"    => t.style.fg     = parse_opt_color(value)?,
                "bg_color"    => t.style.bg     = parse_opt_color(value)?,
                "bold"        => t.style.bold   = parse_bool(value)?,
                "dimmed"      => t.style.dim    = parse_bool(value)?,
                "first_frame" => t.frames.start = value.parse()?,
                "last_frame"  => t.frames.end   = value.parse()?,
                "z_order"     => t.z_order      = value.parse()?,
                _ => bail!("Unknown property: {name}"),
            }
        }
    }
    Ok(())
}

/// Returns a clone of the named Coordinate field, if the object has one by that name.
pub fn get_coord(obj: &SceneObject, name: &str) -> Option<Coordinate> {
    match obj {
        SceneObject::Label(l) => match name {
            "x" => Some(l.position.x.clone()),
            "y" => Some(l.position.y.clone()),
            "width" => Some(l.width.clone()),
            "height" => Some(l.height.clone()),
            _ => None,
        },
        SceneObject::HLine(h) => match name {
            "y" => Some(h.y.clone()),
            "from_x" => Some(h.x_start.clone()),
            "to_x" => Some(h.x_end.clone()),
            _ => None,
        },
        SceneObject::Rect(r) => match name {
            "x" => Some(r.position.x.clone()),
            "y" => Some(r.position.y.clone()),
            "width" => Some(r.width.clone()),
            "height" => Some(r.height.clone()),
            _ => None,
        },
        SceneObject::Header(h) => match name {
            "x" => Some(h.position.x.clone()),
            "y" => Some(h.position.y.clone()),
            _ => None,
        },
        SceneObject::Arrow(a) => match name {
            "x1" => Some(a.x1.clone()),
            "y1" => Some(a.y1.clone()),
            "x2" => Some(a.x2.clone()),
            "y2" => Some(a.y2.clone()),
            _ => None,
        },
        SceneObject::Group(_) => None,
        SceneObject::Table(t) => match name {
            "x"      => Some(t.position.x.clone()),
            "y"      => Some(t.position.y.clone()),
            "width"  => Some(t.width.clone()),
            "height" => Some(t.height.clone()),
            _ => None,
        },
    }
}

/// Directly sets a Coordinate field by name, bypassing string parsing.
pub fn set_coordinate(obj: &mut SceneObject, name: &str, coord: Coordinate) -> Result<()> {
    match obj {
        SceneObject::Label(l) => match name {
            "x" => l.position.x = coord,
            "y" => l.position.y = coord,
            "width" => l.width = coord,
            "height" => l.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
        SceneObject::HLine(h) => match name {
            "y" => h.y = coord,
            "from_x" => h.x_start = coord,
            "to_x" => h.x_end = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
        SceneObject::Rect(r) => match name {
            "x" => r.position.x = coord,
            "y" => r.position.y = coord,
            "width" => r.width = coord,
            "height" => r.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
        SceneObject::Header(h) => match name {
            "x" => h.position.x = coord,
            "y" => h.position.y = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
        SceneObject::Arrow(a) => match name {
            "x1" => a.x1 = coord,
            "y1" => a.y1 = coord,
            "x2" => a.x2 = coord,
            "y2" => a.y2 = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
        SceneObject::Group(_) => bail!("Groups have no coordinate properties"),
        SceneObject::Table(t) => match name {
            "x"      => t.position.x = coord,
            "y"      => t.position.y = coord,
            "width"  => t.width  = coord,
            "height" => t.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        },
    }
    Ok(())
}

pub fn format_coordinate(coord: &Coordinate) -> String {
    match coord {
        Coordinate::Fixed(v) => fmt_f64(*v),
        Coordinate::Animated { from, to, start_frame, end_frame } =>
            format!("{from}->{to} (f{start_frame}..f{end_frame})"),
    }
}

fn parse_coordinate(s: &str) -> Result<Coordinate> {
    if let Ok(v) = s.parse::<f64>() {
        return Ok(Coordinate::Fixed(v.max(0.0)));
    }
    bail!("Invalid coordinate: {s} (use a number for fixed position)")
}

/// Public wrapper for use in editor modules that need to format/parse colors.
pub fn format_opt_color_pub(color: &Option<Color>) -> String {
    format_opt_color(color)
}

/// Public wrapper for use in editor modules that need to parse colors.
pub fn parse_opt_color_pub(s: &str) -> Result<Option<Color>> {
    parse_opt_color(s)
}

fn format_opt_color(color: &Option<Color>) -> String {
    match color {
        None => "none".into(),
        Some(Color::Named(n)) => format_named_color(n).into(),
        Some(Color::Rgb { r, g, b }) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

fn format_named_color(c: &NamedColor) -> &'static str {
    match c {
        NamedColor::Black => "black",
        NamedColor::Red => "red",
        NamedColor::Green => "green",
        NamedColor::Yellow => "yellow",
        NamedColor::Blue => "blue",
        NamedColor::Magenta => "magenta",
        NamedColor::Cyan => "cyan",
        NamedColor::White => "white",
    }
}

fn parse_opt_color(s: &str) -> Result<Option<Color>> {
    let s = s.trim();
    if s.is_empty() || s == "none" {
        return Ok(None);
    }
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            return Ok(Some(Color::Rgb { r, g, b }));
        }
        bail!("Invalid hex color: {s}");
    }
    let named = match s.to_lowercase().as_str() {
        "black" => NamedColor::Black,
        "red" => NamedColor::Red,
        "green" => NamedColor::Green,
        "yellow" => NamedColor::Yellow,
        "blue" => NamedColor::Blue,
        "magenta" => NamedColor::Magenta,
        "cyan" => NamedColor::Cyan,
        "white" => NamedColor::White,
        _ => bail!("Unknown color: {s}"),
    };
    Ok(Some(Color::Named(named)))
}

fn parse_bool(s: &str) -> Result<bool> {
    match s.trim() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => bail!("Invalid boolean: {s}"),
    }
}

fn parse_char(s: &str) -> Result<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => bail!("Expected a single character, got: {s}"),
    }
}

fn parse_opt_char(s: &str) -> Result<Option<char>> {
    let s = s.trim();
    if s.is_empty() || s == "auto" {
        return Ok(None);
    }
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(Some(c)),
        _ => bail!("Expected a single character or 'auto', got: {s}"),
    }
}

// ---------------------------------------------------------------------------
// Coordinate / geometry helpers
// ---------------------------------------------------------------------------

/// Shift a Fixed coordinate by `delta`; Animated coordinates are left unchanged.
fn adjust_coordinate(coord: &mut Coordinate, delta: i32) {
    if let Coordinate::Fixed(v) = coord {
        *v = (*v + delta as f64).max(0.0);
    }
}

/// Add a positive f64 delta to a Fixed coordinate; Animated left unchanged.
fn adjust_coordinate_add(coord: &mut Coordinate, delta: f64) {
    if let Coordinate::Fixed(v) = coord {
        *v = (*v + delta).max(0.0);
    }
}

/// Read the effective integer value of a coordinate (Fixed → floored; Animated → from).
fn coordinate_val(coord: &Coordinate) -> u16 {
    match coord {
        Coordinate::Fixed(v) => v.max(0.0).floor() as u16,
        Coordinate::Animated { from, .. } => *from,
    }
}

/// Read the raw f64 value of a coordinate (Fixed → f64; Animated → from as f64).
fn coord_val_f(coord: &Coordinate) -> f64 {
    match coord {
        Coordinate::Fixed(v) => *v,
        Coordinate::Animated { from, .. } => *from as f64,
    }
}

/// Set a Fixed coordinate to a specific f64 value (no-op for Animated).
fn set_fixed(coord: &mut Coordinate, v: f64) {
    if let Coordinate::Fixed(f) = coord {
        *f = v.max(0.0);
    }
}

// ---------------------------------------------------------------------------
// Per-object f64 geometry accessors (used by group operations)
// ---------------------------------------------------------------------------

fn object_origin_x_f(obj: &SceneObject) -> f64 {
    match obj {
        SceneObject::Label(l)  => coord_val_f(&l.position.x),
        SceneObject::HLine(h)  => coord_val_f(&h.x_start),
        SceneObject::Rect(r)   => coord_val_f(&r.position.x),
        SceneObject::Header(h) => coord_val_f(&h.position.x),
        SceneObject::Arrow(a)  => coord_val_f(&a.x1).min(coord_val_f(&a.x2)),
        SceneObject::Group(_)  => 0.0,
        SceneObject::Table(t)  => coord_val_f(&t.position.x),
    }
}

fn object_origin_y_f(obj: &SceneObject) -> f64 {
    match obj {
        SceneObject::Label(l)  => coord_val_f(&l.position.y),
        SceneObject::HLine(h)  => coord_val_f(&h.y),
        SceneObject::Rect(r)   => coord_val_f(&r.position.y),
        SceneObject::Header(h) => coord_val_f(&h.position.y),
        SceneObject::Arrow(a)  => coord_val_f(&a.y1).min(coord_val_f(&a.y2)),
        SceneObject::Group(_)  => 0.0,
        SceneObject::Table(t)  => coord_val_f(&t.position.y),
    }
}

/// Width contribution of an object (0 for objects with no explicit width).
fn object_dim_x_f(obj: &SceneObject) -> f64 {
    match obj {
        SceneObject::Label(l)  => coord_val_f(&l.width),
        SceneObject::HLine(h)  => (coord_val_f(&h.x_end) - coord_val_f(&h.x_start)).max(0.0),
        SceneObject::Rect(r)   => coord_val_f(&r.width),
        SceneObject::Header(_) => 0.0,
        SceneObject::Arrow(a)  => (coord_val_f(&a.x2) - coord_val_f(&a.x1)).abs(),
        SceneObject::Group(_)  => 0.0,
        SceneObject::Table(t)  => coord_val_f(&t.width),
    }
}

/// Height contribution of an object.
fn object_dim_y_f(obj: &SceneObject) -> f64 {
    match obj {
        SceneObject::Label(l)  => coord_val_f(&l.height),
        SceneObject::HLine(_)  => 1.0,
        SceneObject::Rect(r)   => coord_val_f(&r.height),
        SceneObject::Header(_) => 1.0,
        SceneObject::Arrow(a)  => (coord_val_f(&a.y2) - coord_val_f(&a.y1)).abs().max(1.0),
        SceneObject::Group(_)  => 0.0,
        SceneObject::Table(t)  => coord_val_f(&t.height).max(1.0),
    }
}

fn set_object_origin_x_f(obj: &mut SceneObject, v: f64) {
    match obj {
        SceneObject::Label(l)  => set_fixed(&mut l.position.x, v),
        SceneObject::HLine(h)  => {
            // Preserve the line width when shifting x_start.
            let w = (coord_val_f(&h.x_end) - coord_val_f(&h.x_start)).max(0.0);
            set_fixed(&mut h.x_start, v);
            set_fixed(&mut h.x_end, v + w);
        }
        SceneObject::Rect(r)   => set_fixed(&mut r.position.x, v),
        SceneObject::Header(h) => set_fixed(&mut h.position.x, v),
        SceneObject::Arrow(a)  => {
            let dx = coord_val_f(&a.x2) - coord_val_f(&a.x1);
            set_fixed(&mut a.x1, v);
            set_fixed(&mut a.x2, v + dx);
        }
        SceneObject::Group(_)  => {}
        SceneObject::Table(t)  => set_fixed(&mut t.position.x, v),
    }
}

fn set_object_origin_y_f(obj: &mut SceneObject, v: f64) {
    match obj {
        SceneObject::Label(l)  => set_fixed(&mut l.position.y, v),
        SceneObject::HLine(h)  => set_fixed(&mut h.y, v),
        SceneObject::Rect(r)   => set_fixed(&mut r.position.y, v),
        SceneObject::Header(h) => set_fixed(&mut h.position.y, v),
        SceneObject::Arrow(a)  => {
            let dy = coord_val_f(&a.y2) - coord_val_f(&a.y1);
            set_fixed(&mut a.y1, v);
            set_fixed(&mut a.y2, v + dy);
        }
        SceneObject::Group(_)  => {}
        SceneObject::Table(t)  => set_fixed(&mut t.position.y, v),
    }
}

fn set_object_dim_x_f(obj: &mut SceneObject, v: f64) {
    match obj {
        SceneObject::Label(l)  => set_fixed(&mut l.width, v),
        SceneObject::HLine(h)  => {
            let start = coord_val_f(&h.x_start);
            set_fixed(&mut h.x_end, start + v.max(0.0));
        }
        SceneObject::Rect(r)   => set_fixed(&mut r.width, v),
        SceneObject::Header(_) => {}
        SceneObject::Arrow(a)  => {
            let x1 = coord_val_f(&a.x1);
            let sign = if coord_val_f(&a.x2) >= x1 { 1.0 } else { -1.0 };
            set_fixed(&mut a.x2, x1 + sign * v.max(0.0));
        }
        SceneObject::Group(_)  => {}
        SceneObject::Table(t)  => set_fixed(&mut t.width, v),
    }
}

fn set_object_dim_y_f(obj: &mut SceneObject, v: f64) {
    match obj {
        SceneObject::Label(l)  => set_fixed(&mut l.height, v),
        SceneObject::HLine(_)  => {}  // height is always 1
        SceneObject::Rect(r)   => set_fixed(&mut r.height, v),
        SceneObject::Header(_) => {}  // height is always 1
        SceneObject::Arrow(a)  => {
            let y1 = coord_val_f(&a.y1);
            let sign = if coord_val_f(&a.y2) >= y1 { 1.0 } else { -1.0 };
            set_fixed(&mut a.y2, y1 + sign * v.max(0.0));
        }
        SceneObject::Group(_)  => {}
        SceneObject::Table(t)  => set_fixed(&mut t.height, v),
    }
}

// ---------------------------------------------------------------------------
// Object movement (individual)
// ---------------------------------------------------------------------------

/// Move an object by (dx, dy) steps. Only Fixed coordinates are adjusted;
/// Animated coordinates are left unchanged.
pub fn move_object(obj: &mut SceneObject, dx: i32, dy: i32) {
    match obj {
        SceneObject::Label(l) => {
            adjust_coordinate(&mut l.position.x, dx);
            adjust_coordinate(&mut l.position.y, dy);
        }
        SceneObject::HLine(h) => {
            adjust_coordinate(&mut h.y, dy);
            adjust_coordinate(&mut h.x_start, dx);
            adjust_coordinate(&mut h.x_end, dx);
        }
        SceneObject::Rect(r) => {
            adjust_coordinate(&mut r.position.x, dx);
            adjust_coordinate(&mut r.position.y, dy);
        }
        SceneObject::Header(h) => {
            adjust_coordinate(&mut h.position.x, dx);
            adjust_coordinate(&mut h.position.y, dy);
        }
        SceneObject::Arrow(a) => {
            adjust_coordinate(&mut a.x1, dx);
            adjust_coordinate(&mut a.y1, dy);
            adjust_coordinate(&mut a.x2, dx);
            adjust_coordinate(&mut a.y2, dy);
        }
        SceneObject::Group(_) => {} // groups have no position of their own
        SceneObject::Table(t) => {
            adjust_coordinate(&mut t.position.x, dx);
            adjust_coordinate(&mut t.position.y, dy);
        }
    }
}

/// Resize an object's width/height by growing the specified edge.
pub fn resize_object(obj: &mut SceneObject, dw: i32, dh: i32) {
    match obj {
        SceneObject::Rect(r) => {
            if dw > 0 {
                adjust_coordinate_add(&mut r.width, dw as f64);
            } else if dw < 0 {
                let delta = (-dw) as f64;
                adjust_coordinate_add(&mut r.width, delta);
                adjust_coordinate(&mut r.position.x, dw);
            }
            if dh > 0 {
                adjust_coordinate_add(&mut r.height, dh as f64);
            } else if dh < 0 {
                let delta = (-dh) as f64;
                adjust_coordinate_add(&mut r.height, delta);
                adjust_coordinate(&mut r.position.y, dh);
            }
        }
        SceneObject::HLine(h) => {
            if dw > 0 {
                adjust_coordinate_add(&mut h.x_end, dw as f64);
            } else if dw < 0 {
                adjust_coordinate(&mut h.x_start, dw);
            }
        }
        SceneObject::Label(l) => {
            if dw > 0 {
                adjust_coordinate_add(&mut l.width, dw as f64);
            } else if dw < 0 {
                let delta = (-dw) as f64;
                adjust_coordinate_add(&mut l.width, delta);
                adjust_coordinate(&mut l.position.x, dw);
            }
            if dh > 0 {
                adjust_coordinate_add(&mut l.height, dh as f64);
            } else if dh < 0 {
                let delta = (-dh) as f64;
                adjust_coordinate_add(&mut l.height, delta);
                adjust_coordinate(&mut l.position.y, dh);
            }
        }
        SceneObject::Arrow(a) => {
            adjust_coordinate(&mut a.x2, dw);
            adjust_coordinate(&mut a.y2, dh);
        }
        SceneObject::Table(t) => {
            if dw > 0 {
                adjust_coordinate_add(&mut t.width, dw as f64);
            } else if dw < 0 {
                let delta = (-dw) as f64;
                adjust_coordinate_add(&mut t.width, delta);
                adjust_coordinate(&mut t.position.x, dw);
            }
            if dh > 0 {
                adjust_coordinate_add(&mut t.height, dh as f64);
            } else if dh < 0 {
                let delta = (-dh) as f64;
                adjust_coordinate_add(&mut t.height, delta);
                adjust_coordinate(&mut t.position.y, dh);
            }
        }
        _ => {}
    }
}

/// Shrink an object's width/height by pulling the specified edge inward.
pub fn shrink_object(obj: &mut SceneObject, dw: i32, dh: i32) {
    match obj {
        SceneObject::Rect(r) => {
            if dw > 0 {
                if let Coordinate::Fixed(v) = &mut r.width {
                    *v = (*v - dw as f64).max(1.0);
                }
            } else if dw < 0 {
                let delta = (-dw) as f64;
                if coordinate_val(&r.width) > 1 {
                    if let Coordinate::Fixed(v) = &mut r.width {
                        *v = (*v - delta).max(1.0);
                    }
                    adjust_coordinate(&mut r.position.x, -dw);
                }
            }
            if dh > 0 {
                if let Coordinate::Fixed(v) = &mut r.height {
                    *v = (*v - dh as f64).max(1.0);
                }
            } else if dh < 0 {
                let delta = (-dh) as f64;
                if coordinate_val(&r.height) > 1 {
                    if let Coordinate::Fixed(v) = &mut r.height {
                        *v = (*v - delta).max(1.0);
                    }
                    adjust_coordinate(&mut r.position.y, -dh);
                }
            }
        }
        SceneObject::HLine(h) => {
            if dw > 0 {
                let xe = coordinate_val(&h.x_end);
                let xs = coordinate_val(&h.x_start);
                if xe > xs {
                    if let Coordinate::Fixed(v) = &mut h.x_end {
                        *v = (*v - dw as f64).max(xs as f64 + 1.0);
                    }
                }
            } else if dw < 0 {
                let delta = (-dw) as u16;
                let xe = coordinate_val(&h.x_end);
                let xs = coordinate_val(&h.x_start);
                if xs + delta < xe {
                    adjust_coordinate_add(&mut h.x_start, delta as f64);
                }
            }
        }
        SceneObject::Label(l) => {
            if dw > 0 {
                if let Coordinate::Fixed(v) = &mut l.width {
                    *v = (*v - dw as f64).max(0.0);
                }
            } else if dw < 0 {
                let delta = (-dw) as f64;
                if coordinate_val(&l.width) > 0 {
                    if let Coordinate::Fixed(v) = &mut l.width {
                        *v = (*v - delta).max(0.0);
                    }
                    adjust_coordinate(&mut l.position.x, -dw);
                }
            }
            if dh > 0 {
                if let Coordinate::Fixed(v) = &mut l.height {
                    *v = (*v - dh as f64).max(0.0);
                }
            } else if dh < 0 {
                let delta = (-dh) as f64;
                if coordinate_val(&l.height) > 0 {
                    if let Coordinate::Fixed(v) = &mut l.height {
                        *v = (*v - delta).max(0.0);
                    }
                    adjust_coordinate(&mut l.position.y, -dh);
                }
            }
        }
        SceneObject::Arrow(a) => {
            adjust_coordinate(&mut a.x2, -dw);
            adjust_coordinate(&mut a.y2, -dh);
        }
        SceneObject::Table(t) => {
            if dw > 0 {
                if let Coordinate::Fixed(v) = &mut t.width {
                    *v = (*v - dw as f64).max(3.0);
                }
            } else if dw < 0 {
                let delta = (-dw) as f64;
                if coordinate_val(&t.width) > 3 {
                    if let Coordinate::Fixed(v) = &mut t.width {
                        *v = (*v - delta).max(3.0);
                    }
                    adjust_coordinate(&mut t.position.x, -dw);
                }
            }
            if dh > 0 {
                if let Coordinate::Fixed(v) = &mut t.height {
                    *v = (*v - dh as f64).max(0.0);
                }
            } else if dh < 0 {
                let delta = (-dh) as f64;
                if coordinate_val(&t.height) > 0 {
                    if let Coordinate::Fixed(v) = &mut t.height {
                        *v = (*v - delta).max(0.0);
                    }
                    adjust_coordinate(&mut t.position.y, -dh);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Group operations
// ---------------------------------------------------------------------------

/// Compute the bounding box of a group's members: (min_x, min_y, width, height).
/// Width = rightmost edge − min_x; height = bottommost edge − min_y.
pub fn group_bounds(objects: &[SceneObject], group_idx: usize) -> (f64, f64, f64, f64) {
    let members = match &objects[group_idx] {
        SceneObject::Group(g) => g.members.clone(),
        _ => return (0.0, 0.0, 0.0, 0.0),
    };
    group_bounds_from_members(objects, &members)
}

fn group_bounds_from_members(objects: &[SceneObject], members: &[usize]) -> (f64, f64, f64, f64) {
    let valid: Vec<usize> = members.iter().copied().filter(|&m| m < objects.len()).collect();
    if valid.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let min_x = valid.iter().map(|&m| object_origin_x_f(&objects[m])).fold(f64::INFINITY, |a, b| a.min(b));
    let min_y = valid.iter().map(|&m| object_origin_y_f(&objects[m])).fold(f64::INFINITY, |a, b| a.min(b));
    let max_rx = valid.iter().map(|&m| object_origin_x_f(&objects[m]) + object_dim_x_f(&objects[m])).fold(f64::NEG_INFINITY, |a, b| a.max(b));
    let max_by = valid.iter().map(|&m| object_origin_y_f(&objects[m]) + object_dim_y_f(&objects[m])).fold(f64::NEG_INFINITY, |a, b| a.max(b));
    let width  = (max_rx - min_x).max(0.0);
    let height = (max_by - min_y).max(0.0);
    (min_x, min_y, width, height)
}

/// Move all members of a group by (dx, dy).
pub fn move_group(objects: &mut Vec<SceneObject>, group_idx: usize, dx: i32, dy: i32) {
    let members = match &objects[group_idx] {
        SceneObject::Group(g) => g.members.clone(),
        _ => return,
    };
    for &m in &members {
        if m < objects.len() {
            move_object(&mut objects[m], dx, dy);
        }
    }
}

/// Scale all members of a group so the bounding box changes by (dw, dh).
/// Both member positions (relative to group origin) and member dimensions are scaled.
///
/// `anchor_left` / `anchor_top` control which edge is held fixed:
///   - `anchor_left=true`  → left edge fixed,  right edge moves  (grow/shrink from right)
///   - `anchor_left=false` → right edge fixed,  left edge moves   (grow/shrink from left)
///   - `anchor_top=true`   → top edge fixed,    bottom edge moves (grow/shrink from bottom)
///   - `anchor_top=false`  → bottom edge fixed, top edge moves    (grow/shrink from top)
pub fn resize_group(
    objects: &mut Vec<SceneObject>,
    group_idx: usize,
    dw: i32,
    dh: i32,
    anchor_left: bool,
    anchor_top: bool,
) {
    let members = match &objects[group_idx] {
        SceneObject::Group(g) => g.members.clone(),
        _ => return,
    };
    if members.is_empty() {
        return;
    }

    let (gx, gy, old_w, old_h) = group_bounds_from_members(objects, &members);
    let new_w = (old_w + dw as f64).max(1.0);
    let new_h = (old_h + dh as f64).max(1.0);
    let scale_x = if old_w > 0.0 { new_w / old_w } else { 1.0 };
    let scale_y = if old_h > 0.0 { new_h / old_h } else { 1.0 };

    // Anchor point: the edge that stays fixed when scaling.
    let ax = if anchor_left { gx } else { gx + old_w };
    let ay = if anchor_top  { gy } else { gy + old_h };

    // Collect new values before applying (avoids aliasing / borrow issues).
    let updates: Vec<(f64, f64, f64, f64)> = members.iter().map(|&m| {
        if m < objects.len() {
            let ox = object_origin_x_f(&objects[m]);
            let oy = object_origin_y_f(&objects[m]);
            let dxf = object_dim_x_f(&objects[m]);
            let dyf = object_dim_y_f(&objects[m]);
            (
                ax + (ox - ax) * scale_x,
                ay + (oy - ay) * scale_y,
                dxf * scale_x,
                dyf * scale_y,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    }).collect();

    for (&m, (nx, ny, ndx, ndy)) in members.iter().zip(updates.iter()) {
        if m < objects.len() {
            set_object_origin_x_f(&mut objects[m], *nx);
            set_object_origin_y_f(&mut objects[m], *ny);
            if *ndx > 0.0 { set_object_dim_x_f(&mut objects[m], *ndx); }
            if *ndy > 0.0 { set_object_dim_y_f(&mut objects[m], *ndy); }
        }
    }
}

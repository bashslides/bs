use anyhow::{bail, Result};

use crate::engine::source::{
    Animation, Arrow, Art, AutoAdvance, Circle, Command, Coordinate, FrameRange, Group, HLine,
    Header, Label, List, Loop, Morph, MorphMode, Rect, SceneObject, Table, TextAlign,
    VerticalAlign,
};
use crate::types::{Color, NamedColor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyKind {
    Text,
    /// Plain numeric field (frame indices, z-order, counts, spacing, timeouts).
    /// Edited in place in the narrow panel like a coordinate — never in the
    /// centred multi-line overlay, which is reserved for free-form [`Text`].
    Number,
    /// Boolean flag, rendered as a checkbox and flipped in place (no text entry).
    Bool,
    Color,
    Coordinate,
    /// Read-only member entry; `value` holds the member's object index as a string.
    GroupMember,
    /// Computed / read-only display field; cannot be edited.
    ReadOnly,
    /// Free-form informational/warning line (the `value` is the whole message;
    /// `name` is ignored in rendering). Non-editable, like [`ReadOnly`], but
    /// drawn as a standalone note rather than a `name: value` field.
    Note,
    /// Dropdown for arrow head character selection.
    HeadChar,
    /// Dropdown for arrow body character selection.
    BodyChar,
    /// Dropdown for a morph's transition mode.
    MorphMode,
    /// Dropdown for a label's horizontal text alignment.
    TextAlign,
    /// Dropdown for a label's vertical alignment.
    VerticalAlign,
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
pub const MORPH_MODE_OPTIONS: &[&str] =
    &["dissolve", "wipe-right", "wipe-left", "wipe-down", "wipe-up"];
pub const TEXT_ALIGN_OPTIONS: &[&str] = &["left", "center", "right"];
pub const VERTICAL_ALIGN_OPTIONS: &[&str] = &["top", "center", "bottom"];

/// Returns the dropdown option list for a property kind, if it uses a dropdown.
pub fn dropdown_options_for(kind: &PropertyKind) -> Option<&'static [&'static str]> {
    match kind {
        PropertyKind::Color         => Some(COLOR_OPTIONS),
        PropertyKind::HeadChar      => Some(HEAD_CHAR_OPTIONS),
        PropertyKind::BodyChar      => Some(BODY_CHAR_OPTIONS),
        PropertyKind::MorphMode     => Some(MORPH_MODE_OPTIONS),
        PropertyKind::TextAlign     => Some(TEXT_ALIGN_OPTIONS),
        PropertyKind::VerticalAlign => Some(VERTICAL_ALIGN_OPTIONS),
        _                           => None,
    }
}

/// Flip a boolean property's string value. Anything that isn't `"true"` becomes
/// `"true"`, so an unexpected value resolves to the safe on-state on first toggle.
pub fn toggled_bool_value(value: &str) -> &'static str {
    if value.trim() == "true" { "false" } else { "true" }
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
// Editable — one impl per object type
// ---------------------------------------------------------------------------
//
// All per-type property/geometry editing lives in that type's `impl Editable`.
// Adding or changing a property touches exactly one impl block; the generic
// dispatch below (`get_properties`, `set_property`, geometry accessors, …)
// never needs to change. This replaces what used to be ~12 functions that each
// matched on every object type and property name.

/// Cross-object context passed to [`Editable::properties`]. Only `Group` reads
/// it (to compute its members' bounding box); every other type ignores it.
pub struct PropContext<'a> {
    pub objects: &'a [SceneObject],
    pub index: usize,
}

trait Editable {
    /// Editable/displayable properties, in display order.
    fn properties(&self, ctx: &PropContext) -> Vec<Property>;
    /// Apply a property edit from its string form.
    fn set(&mut self, name: &str, value: &str) -> Result<()>;

    /// Clone the named `Coordinate` field, if any.
    fn get_coord(&self, name: &str) -> Option<Coordinate>;
    /// Set the named `Coordinate` field directly (bypassing string parsing).
    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()>;

    // --- f64 geometry (used by group scaling) ---
    fn origin_x(&self) -> f64;
    fn origin_y(&self) -> f64;
    fn dim_x(&self) -> f64;
    fn dim_y(&self) -> f64;
    fn set_origin_x(&mut self, v: f64);
    fn set_origin_y(&mut self, v: f64);
    fn set_dim_x(&mut self, v: f64);
    fn set_dim_y(&mut self, v: f64);

    // --- integer-step move/resize/shrink (Fixed coords only) ---
    fn move_by(&mut self, dx: i32, dy: i32);
    /// Grow the object's far edge. Default: no-op (objects without a size).
    fn resize_by(&mut self, _dw: i32, _dh: i32) {}
    /// Pull the object's far edge inward. Default: no-op.
    fn shrink_by(&mut self, _dw: i32, _dh: i32) {}
}

/// View a `SceneObject` as `&dyn Editable` — the single place that maps the
/// enum to its per-type impl.
fn as_editable(obj: &SceneObject) -> &dyn Editable {
    match obj {
        SceneObject::Label(o) => o,
        SceneObject::HLine(o) => o,
        SceneObject::Rect(o) => o,
        SceneObject::Header(o) => o,
        SceneObject::Arrow(o) => o,
        SceneObject::Group(o) => o,
        SceneObject::Table(o) => o,
        SceneObject::Art(o) => o,
        SceneObject::Command(o) => o,
        SceneObject::List(o) => o,
        SceneObject::Loop(o) => o,
        SceneObject::Morph(o) => o,
        SceneObject::Animation(o) => o,
        SceneObject::AutoAdvance(o) => o,
        SceneObject::Circle(o) => o,
    }
}

fn as_editable_mut(obj: &mut SceneObject) -> &mut dyn Editable {
    match obj {
        SceneObject::Label(o) => o,
        SceneObject::HLine(o) => o,
        SceneObject::Rect(o) => o,
        SceneObject::Header(o) => o,
        SceneObject::Arrow(o) => o,
        SceneObject::Group(o) => o,
        SceneObject::Table(o) => o,
        SceneObject::Art(o) => o,
        SceneObject::Command(o) => o,
        SceneObject::List(o) => o,
        SceneObject::Loop(o) => o,
        SceneObject::Morph(o) => o,
        SceneObject::Animation(o) => o,
        SceneObject::AutoAdvance(o) => o,
        SceneObject::Circle(o) => o,
    }
}

impl Editable for Circle {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "diameter", value: self.diameter.to_string(), kind: PropertyKind::Number },
            Property { name: "fill_char", value: self.ch.to_string(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "diameter" => self.diameter = value.trim().parse::<u16>()?.max(1),
            "fill_char" => self.ch = parse_char(value)?,
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    // The bounding box: `diameter` rows tall, derived columns wide (kept round).
    fn dim_x(&self) -> f64 { Circle::columns(self.diameter) as f64 }
    fn dim_y(&self) -> f64 { self.diameter as f64 }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { self.diameter = Circle::rows_for_width(v).round().max(1.0) as u16; }
    fn set_dim_y(&mut self, v: f64) { self.diameter = v.round().max(1.0) as u16; }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    // A circle has a single size knob, so either arrow grows/shrinks its diameter
    // (anchored at the top-left corner), keeping it round.
    fn resize_by(&mut self, dw: i32, dh: i32) {
        self.diameter = (self.diameter as i32 + dw + dh).max(1) as u16;
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        self.diameter = (self.diameter as i32 - dw.abs() - dh.abs()).max(1) as u16;
    }
}

impl Editable for AutoAdvance {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "delay_ms", value: self.delay_ms.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "delay_ms" => self.delay_ms = value.trim().parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    // Auto-advance has no geometry: it draws nothing and has no position or size.
    fn get_coord(&self, _name: &str) -> Option<Coordinate> { None }
    fn set_coord(&mut self, _name: &str, _coord: Coordinate) -> Result<()> {
        bail!("Auto-advance has no coordinate properties")
    }
    fn origin_x(&self) -> f64 { 0.0 }
    fn origin_y(&self) -> f64 { 0.0 }
    fn dim_x(&self) -> f64 { 0.0 }
    fn dim_y(&self) -> f64 { 0.0 }
    fn set_origin_x(&mut self, _v: f64) {}
    fn set_origin_y(&mut self, _v: f64) {}
    fn set_dim_x(&mut self, _v: f64) {}
    fn set_dim_y(&mut self, _v: f64) {}
    fn move_by(&mut self, _dx: i32, _dy: i32) {}
}

impl Editable for Loop {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "delay_ms", value: self.delay_ms.to_string(), kind: PropertyKind::Number },
            Property { name: "count", value: self.count.to_string(), kind: PropertyKind::Number },
            Property { name: "bounce", value: self.bounce.to_string(), kind: PropertyKind::Bool },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "delay_ms" => self.delay_ms = value.trim().parse()?,
            "count" => self.count = value.trim().parse()?,
            "bounce" => self.bounce = parse_bool(value)?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    // A loop has no geometry: it draws nothing and has no position or size.
    fn get_coord(&self, _name: &str) -> Option<Coordinate> { None }
    fn set_coord(&mut self, _name: &str, _coord: Coordinate) -> Result<()> {
        bail!("Loops have no coordinate properties")
    }
    fn origin_x(&self) -> f64 { 0.0 }
    fn origin_y(&self) -> f64 { 0.0 }
    fn dim_x(&self) -> f64 { 0.0 }
    fn dim_y(&self) -> f64 { 0.0 }
    fn set_origin_x(&mut self, _v: f64) {}
    fn set_origin_y(&mut self, _v: f64) {}
    fn set_dim_x(&mut self, _v: f64) {}
    fn set_dim_y(&mut self, _v: f64) {}
    fn move_by(&mut self, _dx: i32, _dy: i32) {}
}

impl Editable for Animation {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "auto_play", value: self.auto_play.to_string(), kind: PropertyKind::Bool },
            Property { name: "delay_ms", value: self.delay_ms.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "auto_play" => self.auto_play = parse_bool(value)?,
            "delay_ms" => self.delay_ms = value.trim().parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    // An animation span has no geometry: it draws nothing and has no position.
    fn get_coord(&self, _name: &str) -> Option<Coordinate> { None }
    fn set_coord(&mut self, _name: &str, _coord: Coordinate) -> Result<()> {
        bail!("Animations have no coordinate properties")
    }
    fn origin_x(&self) -> f64 { 0.0 }
    fn origin_y(&self) -> f64 { 0.0 }
    fn dim_x(&self) -> f64 { 0.0 }
    fn dim_y(&self) -> f64 { 0.0 }
    fn set_origin_x(&mut self, _v: f64) {}
    fn set_origin_y(&mut self, _v: f64) {}
    fn set_dim_x(&mut self, _v: f64) {}
    fn set_dim_y(&mut self, _v: f64) {}
    fn move_by(&mut self, _dx: i32, _dy: i32) {}
}

impl Editable for Command {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "command", value: self.command.clone(), kind: PropertyKind::Text },
            Property { name: "args", value: self.args.join(" "), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&self.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&self.height), kind: PropertyKind::Coordinate },
            Property { name: "border", value: self.border.to_string(), kind: PropertyKind::Bool },
            Property { name: "cwd", value: self.cwd.clone().unwrap_or_default(), kind: PropertyKind::Text },
            Property { name: "timeout_secs", value: self.timeout_secs.map(|t| t.to_string()).unwrap_or_default(), kind: PropertyKind::Number },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "command" => self.command = value.to_string(),
            "args" => {
                self.args = value.split_whitespace().map(|s| s.to_string()).collect();
            }
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "width" => self.width = parse_coordinate(value)?,
            "height" => self.height = parse_coordinate(value)?,
            "border" => self.border = parse_bool(value)?,
            "cwd" => {
                self.cwd = if value.is_empty() { None } else { Some(value.to_string()) };
            }
            "timeout_secs" => {
                self.timeout_secs = if value.trim().is_empty() { None } else { Some(value.trim().parse()?) };
            }
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            "width" => Some(self.width.clone()),
            "height" => Some(self.height.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            "width" => self.width = coord,
            "height" => self.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { coord_val_f(&self.width) }
    fn dim_y(&self) -> f64 { coord_val_f(&self.height) }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { set_fixed(&mut self.width, v); }
    fn set_dim_y(&mut self, v: f64) { set_fixed(&mut self.height, v); }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.width, dw as f64);
        } else if dw < 0 {
            adjust_coordinate_add(&mut self.width, (-dw) as f64);
            adjust_coordinate(&mut self.position.x, dw);
        }
        if dh > 0 {
            adjust_coordinate_add(&mut self.height, dh as f64);
        } else if dh < 0 {
            adjust_coordinate_add(&mut self.height, (-dh) as f64);
            adjust_coordinate(&mut self.position.y, dh);
        }
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        if dw != 0 {
            if let Coordinate::Fixed(v) = &mut self.width {
                *v = (*v - dw.abs() as f64).max(1.0);
            }
            if dw < 0 {
                adjust_coordinate(&mut self.position.x, -dw);
            }
        }
        if dh != 0 {
            if let Coordinate::Fixed(v) = &mut self.height {
                *v = (*v - dh.abs() as f64).max(1.0);
            }
            if dh < 0 {
                adjust_coordinate(&mut self.position.y, -dh);
            }
        }
    }
}

impl Editable for Label {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "text", value: self.text.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&self.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&self.height), kind: PropertyKind::Coordinate },
            Property { name: "align", value: self.align.as_str().to_string(), kind: PropertyKind::TextAlign },
            Property { name: "valign", value: self.valign.as_str().to_string(), kind: PropertyKind::VerticalAlign },
            Property { name: "framed", value: self.framed.to_string(), kind: PropertyKind::Bool },
            Property { name: "frame_fg_color", value: format_opt_color(&self.frame_style.as_ref().and_then(|s| s.fg.clone())), kind: PropertyKind::Color },
            Property { name: "frame_bg_color", value: format_opt_color(&self.frame_style.as_ref().and_then(|s| s.bg.clone())), kind: PropertyKind::Color },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "text" => self.text = value.to_string(),
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "width" => self.width = parse_coordinate(value)?,
            "height" => self.height = parse_coordinate(value)?,
            "align" => {
                self.align = TextAlign::from_str_opt(value)
                    .ok_or_else(|| anyhow::anyhow!("Unknown alignment: {value}"))?
            }
            "valign" => {
                self.valign = VerticalAlign::from_str_opt(value)
                    .ok_or_else(|| anyhow::anyhow!("Unknown vertical alignment: {value}"))?
            }
            "framed" => self.framed = parse_bool(value)?,
            "frame_fg_color" => {
                let color = parse_opt_color(value)?;
                let fs = self.frame_style.get_or_insert_with(Default::default);
                fs.fg = color;
                if fs.fg.is_none() && fs.bg.is_none() {
                    self.frame_style = None;
                }
            }
            "frame_bg_color" => {
                let color = parse_opt_color(value)?;
                let fs = self.frame_style.get_or_insert_with(Default::default);
                fs.bg = color;
                if fs.fg.is_none() && fs.bg.is_none() {
                    self.frame_style = None;
                }
            }
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            "width" => Some(self.width.clone()),
            "height" => Some(self.height.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            "width" => self.width = coord,
            "height" => self.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { coord_val_f(&self.width) }
    fn dim_y(&self) -> f64 { coord_val_f(&self.height) }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { set_fixed(&mut self.width, v); }
    fn set_dim_y(&mut self, v: f64) { set_fixed(&mut self.height, v); }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.width, dw as f64);
        } else if dw < 0 {
            let delta = (-dw) as f64;
            adjust_coordinate_add(&mut self.width, delta);
            adjust_coordinate(&mut self.position.x, dw);
        }
        if dh > 0 {
            adjust_coordinate_add(&mut self.height, dh as f64);
        } else if dh < 0 {
            let delta = (-dh) as f64;
            adjust_coordinate_add(&mut self.height, delta);
            adjust_coordinate(&mut self.position.y, dh);
        }
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            if let Coordinate::Fixed(v) = &mut self.width {
                *v = (*v - dw as f64).max(0.0);
            }
        } else if dw < 0 {
            let delta = (-dw) as f64;
            if coordinate_val(&self.width) > 0 {
                if let Coordinate::Fixed(v) = &mut self.width {
                    *v = (*v - delta).max(0.0);
                }
                adjust_coordinate(&mut self.position.x, -dw);
            }
        }
        if dh > 0 {
            if let Coordinate::Fixed(v) = &mut self.height {
                *v = (*v - dh as f64).max(0.0);
            }
        } else if dh < 0 {
            let delta = (-dh) as f64;
            if coordinate_val(&self.height) > 0 {
                if let Coordinate::Fixed(v) = &mut self.height {
                    *v = (*v - delta).max(0.0);
                }
                adjust_coordinate(&mut self.position.y, -dh);
            }
        }
    }
}

impl Editable for List {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "text", value: self.text.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&self.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&self.height), kind: PropertyKind::Coordinate },
            Property { name: "ordered", value: self.ordered.to_string(), kind: PropertyKind::Bool },
            Property { name: "bullet", value: self.bullet.clone(), kind: PropertyKind::Text },
            Property { name: "spacing", value: self.spacing.to_string(), kind: PropertyKind::Number },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "text" => self.text = value.to_string(),
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "width" => self.width = parse_coordinate(value)?,
            "height" => self.height = parse_coordinate(value)?,
            "ordered" => self.ordered = parse_bool(value)?,
            "bullet" => {
                // Keep a sane non-empty marker; fall back to "-" if cleared.
                self.bullet = if value.is_empty() { "-".to_string() } else { value.to_string() };
            }
            "spacing" => self.spacing = value.trim().parse()?,
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            "width" => Some(self.width.clone()),
            "height" => Some(self.height.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            "width" => self.width = coord,
            "height" => self.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { coord_val_f(&self.width) }
    fn dim_y(&self) -> f64 { coord_val_f(&self.height) }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { set_fixed(&mut self.width, v); }
    fn set_dim_y(&mut self, v: f64) { set_fixed(&mut self.height, v); }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.width, dw as f64);
        } else if dw < 0 {
            let delta = (-dw) as f64;
            adjust_coordinate_add(&mut self.width, delta);
            adjust_coordinate(&mut self.position.x, dw);
        }
        if dh > 0 {
            adjust_coordinate_add(&mut self.height, dh as f64);
        } else if dh < 0 {
            let delta = (-dh) as f64;
            adjust_coordinate_add(&mut self.height, delta);
            adjust_coordinate(&mut self.position.y, dh);
        }
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            if let Coordinate::Fixed(v) = &mut self.width {
                *v = (*v - dw as f64).max(0.0);
            }
        } else if dw < 0 {
            let delta = (-dw) as f64;
            if coordinate_val(&self.width) > 0 {
                if let Coordinate::Fixed(v) = &mut self.width {
                    *v = (*v - delta).max(0.0);
                }
                adjust_coordinate(&mut self.position.x, -dw);
            }
        }
        if dh > 0 {
            if let Coordinate::Fixed(v) = &mut self.height {
                *v = (*v - dh as f64).max(0.0);
            }
        } else if dh < 0 {
            let delta = (-dh) as f64;
            if coordinate_val(&self.height) > 0 {
                if let Coordinate::Fixed(v) = &mut self.height {
                    *v = (*v - delta).max(0.0);
                }
                adjust_coordinate(&mut self.position.y, -dh);
            }
        }
    }
}

impl Editable for HLine {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "y", value: format_coordinate(&self.y), kind: PropertyKind::Coordinate },
            Property { name: "from_x", value: format_coordinate(&self.x_start), kind: PropertyKind::Coordinate },
            Property { name: "to_x", value: format_coordinate(&self.x_end), kind: PropertyKind::Coordinate },
            Property { name: "draw_char", value: self.ch.to_string(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "y" => self.y = parse_coordinate(value)?,
            "from_x" => self.x_start = parse_coordinate(value)?,
            "to_x" => self.x_end = parse_coordinate(value)?,
            "draw_char" => self.ch = parse_char(value)?,
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "y" => Some(self.y.clone()),
            "from_x" => Some(self.x_start.clone()),
            "to_x" => Some(self.x_end.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "y" => self.y = coord,
            "from_x" => self.x_start = coord,
            "to_x" => self.x_end = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.x_start) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.y) }
    fn dim_x(&self) -> f64 { (coord_val_f(&self.x_end) - coord_val_f(&self.x_start)).max(0.0) }
    fn dim_y(&self) -> f64 { 1.0 }
    fn set_origin_x(&mut self, v: f64) {
        // Preserve the line width when shifting x_start.
        let w = (coord_val_f(&self.x_end) - coord_val_f(&self.x_start)).max(0.0);
        set_fixed(&mut self.x_start, v);
        set_fixed(&mut self.x_end, v + w);
    }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.y, v); }
    fn set_dim_x(&mut self, v: f64) {
        let start = coord_val_f(&self.x_start);
        set_fixed(&mut self.x_end, start + v.max(0.0));
    }
    fn set_dim_y(&mut self, _v: f64) {} // height is always 1

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.y, dy);
        adjust_coordinate(&mut self.x_start, dx);
        adjust_coordinate(&mut self.x_end, dx);
    }

    fn resize_by(&mut self, dw: i32, _dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.x_end, dw as f64);
        } else if dw < 0 {
            adjust_coordinate(&mut self.x_start, dw);
        }
    }

    fn shrink_by(&mut self, dw: i32, _dh: i32) {
        if dw > 0 {
            let xe = coordinate_val(&self.x_end);
            let xs = coordinate_val(&self.x_start);
            if xe > xs {
                if let Coordinate::Fixed(v) = &mut self.x_end {
                    *v = (*v - dw as f64).max(xs as f64 + 1.0);
                }
            }
        } else if dw < 0 {
            let delta = (-dw) as u16;
            let xe = coordinate_val(&self.x_end);
            let xs = coordinate_val(&self.x_start);
            if xs + delta < xe {
                adjust_coordinate_add(&mut self.x_start, delta as f64);
            }
        }
    }
}

impl Editable for Rect {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width", value: format_coordinate(&self.width), kind: PropertyKind::Coordinate },
            Property { name: "height", value: format_coordinate(&self.height), kind: PropertyKind::Coordinate },
            Property { name: "title", value: self.title.clone().unwrap_or_default(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "width" => self.width = parse_coordinate(value)?,
            "height" => self.height = parse_coordinate(value)?,
            "title" => {
                self.title = if value.is_empty() { None } else { Some(value.to_string()) };
            }
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            "width" => Some(self.width.clone()),
            "height" => Some(self.height.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            "width" => self.width = coord,
            "height" => self.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { coord_val_f(&self.width) }
    fn dim_y(&self) -> f64 { coord_val_f(&self.height) }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { set_fixed(&mut self.width, v); }
    fn set_dim_y(&mut self, v: f64) { set_fixed(&mut self.height, v); }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.width, dw as f64);
        } else if dw < 0 {
            let delta = (-dw) as f64;
            adjust_coordinate_add(&mut self.width, delta);
            adjust_coordinate(&mut self.position.x, dw);
        }
        if dh > 0 {
            adjust_coordinate_add(&mut self.height, dh as f64);
        } else if dh < 0 {
            let delta = (-dh) as f64;
            adjust_coordinate_add(&mut self.height, delta);
            adjust_coordinate(&mut self.position.y, dh);
        }
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            if let Coordinate::Fixed(v) = &mut self.width {
                *v = (*v - dw as f64).max(1.0);
            }
        } else if dw < 0 {
            let delta = (-dw) as f64;
            if coordinate_val(&self.width) > 1 {
                if let Coordinate::Fixed(v) = &mut self.width {
                    *v = (*v - delta).max(1.0);
                }
                adjust_coordinate(&mut self.position.x, -dw);
            }
        }
        if dh > 0 {
            if let Coordinate::Fixed(v) = &mut self.height {
                *v = (*v - dh as f64).max(1.0);
            }
        } else if dh < 0 {
            let delta = (-dh) as f64;
            if coordinate_val(&self.height) > 1 {
                if let Coordinate::Fixed(v) = &mut self.height {
                    *v = (*v - delta).max(1.0);
                }
                adjust_coordinate(&mut self.position.y, -dh);
            }
        }
    }
}

impl Editable for Header {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "text", value: self.text.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "draw_char", value: self.ch.to_string(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "text" => self.text = value.to_string(),
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "draw_char" => self.ch = parse_char(value)?,
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { 0.0 }
    fn dim_y(&self) -> f64 { 1.0 }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, _v: f64) {}
    fn set_dim_y(&mut self, _v: f64) {}

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }
    // resize_by / shrink_by: default no-op (a header has no resizable box).
}

impl Editable for Arrow {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "x1", value: format_coordinate(&self.x1), kind: PropertyKind::Coordinate },
            Property { name: "y1", value: format_coordinate(&self.y1), kind: PropertyKind::Coordinate },
            Property { name: "x2", value: format_coordinate(&self.x2), kind: PropertyKind::Coordinate },
            Property { name: "y2", value: format_coordinate(&self.y2), kind: PropertyKind::Coordinate },
            Property { name: "head", value: self.head.to_string(), kind: PropertyKind::Bool },
            Property { name: "head_start", value: self.head_start.to_string(), kind: PropertyKind::Bool },
            Property { name: "head_char", value: self.head_ch.map(|c| c.to_string()).unwrap_or_else(|| "auto".to_string()), kind: PropertyKind::HeadChar },
            Property { name: "body_char", value: self.body_ch.map(|c| c.to_string()).unwrap_or_else(|| "auto".to_string()), kind: PropertyKind::BodyChar },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "x1" => self.x1 = parse_coordinate(value)?,
            "y1" => self.y1 = parse_coordinate(value)?,
            "x2" => self.x2 = parse_coordinate(value)?,
            "y2" => self.y2 = parse_coordinate(value)?,
            "head" => self.head = parse_bool(value)?,
            "head_start" => self.head_start = parse_bool(value)?,
            "head_char" => self.head_ch = parse_opt_char(value)?,
            "body_char" => self.body_ch = parse_opt_char(value)?,
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x1" => Some(self.x1.clone()),
            "y1" => Some(self.y1.clone()),
            "x2" => Some(self.x2.clone()),
            "y2" => Some(self.y2.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x1" => self.x1 = coord,
            "y1" => self.y1 = coord,
            "x2" => self.x2 = coord,
            "y2" => self.y2 = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.x1).min(coord_val_f(&self.x2)) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.y1).min(coord_val_f(&self.y2)) }
    fn dim_x(&self) -> f64 { (coord_val_f(&self.x2) - coord_val_f(&self.x1)).abs() }
    fn dim_y(&self) -> f64 { (coord_val_f(&self.y2) - coord_val_f(&self.y1)).abs().max(1.0) }

    fn set_origin_x(&mut self, v: f64) {
        let dx = coord_val_f(&self.x2) - coord_val_f(&self.x1);
        set_fixed(&mut self.x1, v);
        set_fixed(&mut self.x2, v + dx);
    }
    fn set_origin_y(&mut self, v: f64) {
        let dy = coord_val_f(&self.y2) - coord_val_f(&self.y1);
        set_fixed(&mut self.y1, v);
        set_fixed(&mut self.y2, v + dy);
    }
    fn set_dim_x(&mut self, v: f64) {
        let x1 = coord_val_f(&self.x1);
        let sign = if coord_val_f(&self.x2) >= x1 { 1.0 } else { -1.0 };
        set_fixed(&mut self.x2, x1 + sign * v.max(0.0));
    }
    fn set_dim_y(&mut self, v: f64) {
        let y1 = coord_val_f(&self.y1);
        let sign = if coord_val_f(&self.y2) >= y1 { 1.0 } else { -1.0 };
        set_fixed(&mut self.y2, y1 + sign * v.max(0.0));
    }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.x1, dx);
        adjust_coordinate(&mut self.y1, dy);
        adjust_coordinate(&mut self.x2, dx);
        adjust_coordinate(&mut self.y2, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        adjust_coordinate(&mut self.x2, dw);
        adjust_coordinate(&mut self.y2, dh);
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        adjust_coordinate(&mut self.x2, -dw);
        adjust_coordinate(&mut self.y2, -dh);
    }
}

impl Editable for Group {
    fn properties(&self, ctx: &PropContext) -> Vec<Property> {
        let (gx, gy, gw, gh) = group_bounds(ctx.objects, ctx.index);
        // Auto range (`None`) shows blank frame fields; an explicit range shows
        // its values and a warning that it overrides the members' own ranges.
        let (first_frame, last_frame) = match &self.frames {
            Some(fr) => (fr.start.to_string(), fr.end.to_string()),
            None => (String::new(), String::new()),
        };
        let mut props = vec![
            Property { name: "x",      value: fmt_f64(gx), kind: PropertyKind::ReadOnly },
            Property { name: "y",      value: fmt_f64(gy), kind: PropertyKind::ReadOnly },
            Property { name: "width",  value: fmt_f64(gw), kind: PropertyKind::ReadOnly },
            Property { name: "height", value: fmt_f64(gh), kind: PropertyKind::ReadOnly },
            Property { name: "first_frame", value: first_frame, kind: PropertyKind::Number },
            Property { name: "last_frame",  value: last_frame,  kind: PropertyKind::Number },
            Property { name: "z_order",     value: self.z_order.to_string(), kind: PropertyKind::Number },
        ];
        if self.frames.is_some() {
            props.push(Property {
                name: "note",
                value: "! explicit range overrides member frames (blank = auto)".into(),
                kind: PropertyKind::Note,
            });
        }
        for &member_idx in &self.members {
            props.push(Property {
                name: "member",
                value: member_idx.to_string(),
                kind: PropertyKind::GroupMember,
            });
        }
        props
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        // Materialising an explicit range from "auto" is normally seeded with the
        // derived union by the caller (`apply_property`); the `None` arms here are
        // a defensive fallback that produces a single-slide range.
        match name {
            "first_frame" => {
                let v: usize = value.parse()?;
                match &mut self.frames {
                    Some(fr) => fr.start = v,
                    None => self.frames = Some(FrameRange { start: v, end: v + 1 }),
                }
            }
            "last_frame" => {
                let v: usize = value.parse()?;
                match &mut self.frames {
                    Some(fr) => fr.end = v,
                    None => self.frames = Some(FrameRange { start: v.saturating_sub(1), end: v }),
                }
            }
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, _name: &str) -> Option<Coordinate> { None }
    fn set_coord(&mut self, _name: &str, _coord: Coordinate) -> Result<()> {
        bail!("Groups have no coordinate properties")
    }

    // A group has no position/size of its own — its bounds are derived from
    // members, which are moved/scaled via `move_group` / `resize_group`.
    fn origin_x(&self) -> f64 { 0.0 }
    fn origin_y(&self) -> f64 { 0.0 }
    fn dim_x(&self) -> f64 { 0.0 }
    fn dim_y(&self) -> f64 { 0.0 }
    fn set_origin_x(&mut self, _v: f64) {}
    fn set_origin_y(&mut self, _v: f64) {}
    fn set_dim_x(&mut self, _v: f64) {}
    fn set_dim_y(&mut self, _v: f64) {}
    fn move_by(&mut self, _dx: i32, _dy: i32) {}
}

impl Editable for Table {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        let mut props = vec![
            Property { name: "x",           value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y",           value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "width",       value: format_coordinate(&self.width),      kind: PropertyKind::Coordinate },
            Property { name: "height",      value: format_coordinate(&self.height),     kind: PropertyKind::Coordinate },
            Property { name: "rows",        value: self.rows.to_string(),               kind: PropertyKind::Number },
            Property { name: "cols",        value: self.col_widths.len().to_string(),   kind: PropertyKind::ReadOnly },
            Property { name: "header_bold", value: self.header_bold.to_string(),        kind: PropertyKind::Bool },
            Property { name: "borders",     value: self.borders.to_string(),            kind: PropertyKind::Bool },
            Property { name: "fg_color",    value: format_opt_color(&self.style.fg),    kind: PropertyKind::Color },
            Property { name: "bg_color",    value: format_opt_color(&self.style.bg),    kind: PropertyKind::Color },
            Property { name: "bold",        value: self.style.bold.to_string(),         kind: PropertyKind::Bool },
            Property { name: "dimmed",      value: self.style.dim.to_string(),          kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(),       kind: PropertyKind::Number },
            Property { name: "last_frame",  value: self.frames.end.to_string(),         kind: PropertyKind::Number },
            Property { name: "z_order",     value: self.z_order.to_string(),            kind: PropertyKind::Number },
        ];
        // Per-column width properties.
        for (col_idx, &frac) in self.col_widths.iter().enumerate() {
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

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        // Column-width properties: "col_N_width"
        if let Some(col_idx) = parse_col_width_name(name) {
            let pct: f64 = value.trim().trim_end_matches('%').parse()
                .map_err(|_| anyhow::anyhow!("Invalid percentage: {value}"))?;
            if col_idx < self.col_widths.len() {
                self.col_widths[col_idx] = (pct / 100.0).max(0.01).min(1.0);
            }
            return Ok(());
        }
        match name {
            "x"           => self.position.x = parse_coordinate(value)?,
            "y"           => self.position.y = parse_coordinate(value)?,
            "width"       => self.width  = parse_coordinate(value)?,
            "height"      => self.height = parse_coordinate(value)?,
            "rows"        => {
                let new_rows: usize = value.parse()?;
                self.rows = new_rows.max(1);
                self.normalize_cells();
            }
            "header_bold" => self.header_bold = parse_bool(value)?,
            "borders"     => self.borders     = parse_bool(value)?,
            "fg_color"    => self.style.fg     = parse_opt_color(value)?,
            "bg_color"    => self.style.bg     = parse_opt_color(value)?,
            "bold"        => self.style.bold   = parse_bool(value)?,
            "dimmed"      => self.style.dim    = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame"  => self.frames.end   = value.parse()?,
            "z_order"     => self.z_order      = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x"      => Some(self.position.x.clone()),
            "y"      => Some(self.position.y.clone()),
            "width"  => Some(self.width.clone()),
            "height" => Some(self.height.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x"      => self.position.x = coord,
            "y"      => self.position.y = coord,
            "width"  => self.width  = coord,
            "height" => self.height = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { coord_val_f(&self.width) }
    fn dim_y(&self) -> f64 { coord_val_f(&self.height).max(1.0) }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, v: f64) { set_fixed(&mut self.width, v); }
    fn set_dim_y(&mut self, v: f64) { set_fixed(&mut self.height, v); }

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }

    fn resize_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            adjust_coordinate_add(&mut self.width, dw as f64);
        } else if dw < 0 {
            let delta = (-dw) as f64;
            adjust_coordinate_add(&mut self.width, delta);
            adjust_coordinate(&mut self.position.x, dw);
        }
        if dh > 0 {
            adjust_coordinate_add(&mut self.height, dh as f64);
        } else if dh < 0 {
            let delta = (-dh) as f64;
            adjust_coordinate_add(&mut self.height, delta);
            adjust_coordinate(&mut self.position.y, dh);
        }
    }

    fn shrink_by(&mut self, dw: i32, dh: i32) {
        if dw > 0 {
            if let Coordinate::Fixed(v) = &mut self.width {
                *v = (*v - dw as f64).max(3.0);
            }
        } else if dw < 0 {
            let delta = (-dw) as f64;
            if coordinate_val(&self.width) > 3 {
                if let Coordinate::Fixed(v) = &mut self.width {
                    *v = (*v - delta).max(3.0);
                }
                adjust_coordinate(&mut self.position.x, -dw);
            }
        }
        if dh > 0 {
            if let Coordinate::Fixed(v) = &mut self.height {
                *v = (*v - dh as f64).max(0.0);
            }
        } else if dh < 0 {
            let delta = (-dh) as f64;
            if coordinate_val(&self.height) > 0 {
                if let Coordinate::Fixed(v) = &mut self.height {
                    *v = (*v - delta).max(0.0);
                }
                adjust_coordinate(&mut self.position.y, -dh);
            }
        }
    }
}

impl Editable for Art {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "name", value: self.name.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "art", value: self.art.clone(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "name" => self.name = value.to_string(),
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "art" => self.art = value.to_string(),
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { self.art.split('\n').map(|l| l.chars().count()).max().unwrap_or(0) as f64 }
    fn dim_y(&self) -> f64 { self.art.split('\n').count() as f64 }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, _v: f64) {} // art size is fixed by its content
    fn set_dim_y(&mut self, _v: f64) {}

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }
    // resize_by / shrink_by: default no-op (art is sized by its content).
}

impl Editable for Morph {
    fn properties(&self, _ctx: &PropContext) -> Vec<Property> {
        vec![
            Property { name: "name", value: self.name.clone(), kind: PropertyKind::Text },
            Property { name: "x", value: format_coordinate(&self.position.x), kind: PropertyKind::Coordinate },
            Property { name: "y", value: format_coordinate(&self.position.y), kind: PropertyKind::Coordinate },
            Property { name: "mode", value: self.mode.as_str().to_string(), kind: PropertyKind::MorphMode },
            Property { name: "from", value: self.from.clone(), kind: PropertyKind::Text },
            Property { name: "to", value: self.to.clone(), kind: PropertyKind::Text },
            Property { name: "fg_color", value: format_opt_color(&self.style.fg), kind: PropertyKind::Color },
            Property { name: "bg_color", value: format_opt_color(&self.style.bg), kind: PropertyKind::Color },
            Property { name: "bold", value: self.style.bold.to_string(), kind: PropertyKind::Bool },
            Property { name: "dimmed", value: self.style.dim.to_string(), kind: PropertyKind::Bool },
            Property { name: "first_frame", value: self.frames.start.to_string(), kind: PropertyKind::Number },
            Property { name: "last_frame", value: self.frames.end.to_string(), kind: PropertyKind::Number },
            Property { name: "z_order", value: self.z_order.to_string(), kind: PropertyKind::Number },
        ]
    }

    fn set(&mut self, name: &str, value: &str) -> Result<()> {
        match name {
            "name" => self.name = value.to_string(),
            "x" => self.position.x = parse_coordinate(value)?,
            "y" => self.position.y = parse_coordinate(value)?,
            "mode" => {
                self.mode = MorphMode::from_str_opt(value)
                    .ok_or_else(|| anyhow::anyhow!("Unknown morph mode: {value}"))?
            }
            "from" => self.from = value.to_string(),
            "to" => self.to = value.to_string(),
            "fg_color" => self.style.fg = parse_opt_color(value)?,
            "bg_color" => self.style.bg = parse_opt_color(value)?,
            "bold" => self.style.bold = parse_bool(value)?,
            "dimmed" => self.style.dim = parse_bool(value)?,
            "first_frame" => self.frames.start = value.parse()?,
            "last_frame" => self.frames.end = value.parse()?,
            "z_order" => self.z_order = value.parse()?,
            _ => bail!("Unknown property: {name}"),
        }
        Ok(())
    }

    fn get_coord(&self, name: &str) -> Option<Coordinate> {
        match name {
            "x" => Some(self.position.x.clone()),
            "y" => Some(self.position.y.clone()),
            _ => None,
        }
    }

    fn set_coord(&mut self, name: &str, coord: Coordinate) -> Result<()> {
        match name {
            "x" => self.position.x = coord,
            "y" => self.position.y = coord,
            _ => bail!("Unknown coordinate property: {name}"),
        }
        Ok(())
    }

    fn origin_x(&self) -> f64 { coord_val_f(&self.position.x) }
    fn origin_y(&self) -> f64 { coord_val_f(&self.position.y) }
    fn dim_x(&self) -> f64 { morph_span_cols(self) as f64 }
    fn dim_y(&self) -> f64 { morph_span_rows(self) as f64 }
    fn set_origin_x(&mut self, v: f64) { set_fixed(&mut self.position.x, v); }
    fn set_origin_y(&mut self, v: f64) { set_fixed(&mut self.position.y, v); }
    fn set_dim_x(&mut self, _v: f64) {} // sized by its art content
    fn set_dim_y(&mut self, _v: f64) {}

    fn move_by(&mut self, dx: i32, dy: i32) {
        adjust_coordinate(&mut self.position.x, dx);
        adjust_coordinate(&mut self.position.y, dy);
    }
    // resize_by / shrink_by: default no-op (sized by its art content).
}

/// Width (in cells) of a morph's bounding box: the wider of its two art grids.
fn morph_span_cols(m: &Morph) -> usize {
    let w = |art: &str| art.split('\n').map(|l| l.chars().count()).max().unwrap_or(0);
    w(&m.from).max(w(&m.to))
}

/// Height (in rows) of a morph's bounding box: the taller of its two art grids.
fn morph_span_rows(m: &Morph) -> usize {
    m.from.split('\n').count().max(m.to.split('\n').count())
}

// ---------------------------------------------------------------------------
// Generic dispatch over the per-type `Editable` impls
// ---------------------------------------------------------------------------

/// Returns the editable/displayable properties for object at `object_index`.
/// Groups need access to all objects to compute their bounding box.
pub fn get_properties(objects: &[SceneObject], object_index: usize) -> Vec<Property> {
    let ctx = PropContext { objects, index: object_index };
    let mut props = as_editable(&objects[object_index]).properties(&ctx);
    // `first_frame` is stored 0-based (the inclusive start index) but shown to
    // the user as a 1-based slide number, so an object only on slide 1 reads
    // first_frame=1 / last_frame=1 (last_frame is the exclusive end, which
    // already equals the 1-based inclusive last slide).
    for p in &mut props {
        if p.name == "first_frame" {
            if let Ok(start) = p.value.parse::<usize>() {
                p.value = (start + 1).to_string();
            }
        }
    }
    props
}

/// The properties common to **every** object in `members` — same `name` *and*
/// `kind` across all of them — restricted to the kinds that make sense to
/// bulk-edit. The value shown for each is taken from the **first** member as the
/// representative seed; editing it in the multi-edit panel writes to all members.
/// Empty when `members` is empty or the objects share no bulk-editable property.
pub fn common_properties(objects: &[SceneObject], members: &[usize]) -> Vec<Property> {
    let Some((&first, rest)) = members.split_first() else {
        return Vec::new();
    };
    get_properties(objects, first)
        .into_iter()
        .filter(|p| is_bulk_editable_kind(&p.kind))
        .filter(|p| {
            rest.iter().all(|&m| {
                get_properties(objects, m)
                    .iter()
                    .any(|q| q.name == p.name && q.kind == p.kind)
            })
        })
        .collect()
}

/// Whether a property kind can be edited across a multi-object selection. The
/// overlay-edited `Text`, the structural `GroupMember`/`TableColWidth`, and the
/// non-editable `ReadOnly`/`Note` are excluded; everything else (coordinates,
/// colours, bools, numbers, and the simple dropdowns) is fair game.
fn is_bulk_editable_kind(kind: &PropertyKind) -> bool {
    !matches!(
        kind,
        PropertyKind::Text
            | PropertyKind::GroupMember
            | PropertyKind::ReadOnly
            | PropertyKind::Note
            | PropertyKind::TableColWidth
    )
}

pub fn set_property(obj: &mut SceneObject, name: &str, value: &str) -> Result<()> {
    if name == "first_frame" {
        // Translate the user's 1-based slide number back to the 0-based start.
        let one_based: usize = match value.trim().parse() {
            Ok(v) => v,
            Err(_) => bail!("first_frame must be a whole number"),
        };
        let start = one_based.saturating_sub(1);
        return as_editable_mut(obj).set(name, &start.to_string());
    }
    as_editable_mut(obj).set(name, value)
}

/// Returns a clone of the named Coordinate field, if the object has one by that name.
pub fn get_coord(obj: &SceneObject, name: &str) -> Option<Coordinate> {
    as_editable(obj).get_coord(name)
}

/// Directly sets a Coordinate field by name, bypassing string parsing.
pub fn set_coordinate(obj: &mut SceneObject, name: &str, coord: Coordinate) -> Result<()> {
    as_editable_mut(obj).set_coord(name, coord)
}

pub fn format_coordinate(coord: &Coordinate) -> String {
    match coord {
        Coordinate::Fixed(v) => fmt_f64(*v),
        // The motion (`from->to`) plus which animation owns the span — the span
        // itself lives on that `Animation` object (its first/last-frame fields),
        // the single source of truth, not duplicated here.
        Coordinate::Animated { from, to, anim } => format!("{from}->{to} (anim {anim})"),
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
// Per-object f64 geometry accessors (thin wrappers used by group operations)
// ---------------------------------------------------------------------------

fn object_origin_x_f(obj: &SceneObject) -> f64 { as_editable(obj).origin_x() }
fn object_origin_y_f(obj: &SceneObject) -> f64 { as_editable(obj).origin_y() }
fn object_dim_x_f(obj: &SceneObject) -> f64 { as_editable(obj).dim_x() }
fn object_dim_y_f(obj: &SceneObject) -> f64 { as_editable(obj).dim_y() }
fn set_object_origin_x_f(obj: &mut SceneObject, v: f64) { as_editable_mut(obj).set_origin_x(v); }
fn set_object_origin_y_f(obj: &mut SceneObject, v: f64) { as_editable_mut(obj).set_origin_y(v); }
fn set_object_dim_x_f(obj: &mut SceneObject, v: f64) { as_editable_mut(obj).set_dim_x(v); }
fn set_object_dim_y_f(obj: &mut SceneObject, v: f64) { as_editable_mut(obj).set_dim_y(v); }

// ---------------------------------------------------------------------------
// Object movement (individual)
// ---------------------------------------------------------------------------

/// Move an object by (dx, dy) steps. Only Fixed coordinates are adjusted;
/// Animated coordinates are left unchanged.
pub fn move_object(obj: &mut SceneObject, dx: i32, dy: i32) {
    as_editable_mut(obj).move_by(dx, dy);
}

/// Resize an object's width/height by growing the specified edge.
pub fn resize_object(obj: &mut SceneObject, dw: i32, dh: i32) {
    as_editable_mut(obj).resize_by(dw, dh);
}

/// Shrink an object's width/height by pulling the specified edge inward.
pub fn shrink_object(obj: &mut SceneObject, dw: i32, dh: i32) {
    as_editable_mut(obj).shrink_by(dw, dh);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: &str) -> SceneObject {
        serde_json::from_str(json).expect("test object JSON should parse")
    }

    /// Every property `get_properties` reports for an object must be accepted
    /// by `set_property` (round-tripping its own displayed value) — this is the
    /// invariant the old per-type/per-name duplication kept breaking. Read-only
    /// and group-member rows are display-only and skipped.
    fn assert_props_roundtrip(objects: &mut [SceneObject], idx: usize) {
        let props = get_properties(objects, idx);
        assert!(!props.is_empty());
        for p in props {
            if matches!(
                p.kind,
                PropertyKind::ReadOnly | PropertyKind::GroupMember | PropertyKind::Note
            ) {
                continue;
            }
            let r = set_property(&mut objects[idx], p.name, &p.value);
            assert!(r.is_ok(), "set_property({:?}, {:?}) failed: {:?}", p.name, p.value, r.err());
        }
    }

    #[test]
    fn label_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"label","text":"Hi","position":{"x":{"fixed":3},"y":{"fixed":1}},
                "frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn label_exposes_align_and_valign_dropdowns() {
        let o = vec![obj(
            r#"{"type":"label","text":"Hi","position":{"x":{"fixed":0},"y":{"fixed":0}},
                "frames":{"start":0,"end":1}}"#,
        )];
        let props = get_properties(&o, 0);
        let align = props.iter().find(|p| p.name == "align").expect("align property is listed");
        assert_eq!(align.kind, PropertyKind::TextAlign);
        assert_eq!(align.value, "left");
        let valign = props.iter().find(|p| p.name == "valign").expect("valign property is listed");
        assert_eq!(valign.kind, PropertyKind::VerticalAlign);
        assert_eq!(valign.value, "top");
        // Both are dropdowns with the expected options.
        assert_eq!(dropdown_options_for(&PropertyKind::TextAlign), Some(TEXT_ALIGN_OPTIONS));
        assert_eq!(dropdown_options_for(&PropertyKind::VerticalAlign), Some(VERTICAL_ALIGN_OPTIONS));
    }

    #[test]
    fn circle_properties_roundtrip() {
        let mut o = vec![obj(
            r##"{"type":"circle","position":{"x":{"fixed":3},"y":{"fixed":2}},
                "diameter":8,"ch":"#","frames":{"start":0,"end":2}}"##,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn common_properties_intersects_shared_editable_props() {
        let objects = vec![
            obj(r#"{"type":"label","text":"A","position":{"x":{"fixed":0},"y":{"fixed":0}},"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"rect","position":{"x":{"fixed":0},"y":{"fixed":0}},"width":4,"height":3,"frames":{"start":0,"end":2}}"#),
        ];
        let names: Vec<&str> =
            common_properties(&objects, &[0, 1]).iter().map(|p| p.name).collect();
        // Shared geometry / colour / flags / frames survive the intersection.
        for n in ["x", "y", "width", "height", "fg_color", "bg_color", "bold", "dimmed",
                  "first_frame", "last_frame", "z_order"] {
            assert!(names.contains(&n), "expected common prop {n}, got {names:?}");
        }
        // Label-only properties do not (and `text` is Text-kind, excluded anyway).
        assert!(!names.contains(&"text"));
        assert!(!names.contains(&"align"));
    }

    #[test]
    fn common_properties_shrinks_for_heterogeneous_types() {
        // A Label and a Loop share only their frame-range numbers.
        let objects = vec![
            obj(r#"{"type":"label","text":"A","position":{"x":{"fixed":0},"y":{"fixed":0}},"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"loop","frames":{"start":0,"end":2}}"#),
        ];
        let names: Vec<&str> =
            common_properties(&objects, &[0, 1]).iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["first_frame", "last_frame"]);
        assert!(!names.contains(&"x"));
        assert!(!names.contains(&"fg_color"));
    }

    #[test]
    fn common_properties_value_is_the_first_members() {
        // The representative value shown (and seeded for editing) is member 0's.
        let objects = vec![
            obj(r#"{"type":"label","text":"A","position":{"x":{"fixed":7},"y":{"fixed":0}},"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"label","text":"B","position":{"x":{"fixed":3},"y":{"fixed":0}},"frames":{"start":0,"end":2}}"#),
        ];
        let x = common_properties(&objects, &[0, 1])
            .into_iter()
            .find(|p| p.name == "x")
            .unwrap();
        assert_eq!(x.value, "7");
    }

    #[test]
    fn hline_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"h_line","y":2,"x_start":1,"x_end":5,"frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn rect_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"rect","position":{"x":{"fixed":1},"y":{"fixed":1}},
                "width":5,"height":3,"frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn header_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"header","text":"Hi","position":{"x":{"fixed":1},"y":{"fixed":1}},
                "frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn arrow_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"arrow","x1":1,"y1":1,"x2":5,"y2":3,"frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn art_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"art","position":{"x":{"fixed":2},"y":{"fixed":1}},
                "art":"AB\nCD","name":"human","frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn table_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"table","position":{"x":{"fixed":0},"y":{"fixed":0}},
                "col_widths":[0.5,0.5],"rows":2,"frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn group_properties_roundtrip_and_bounds() {
        // Group at index 1 wraps the label at index 0.
        let mut o = vec![
            obj(r#"{"type":"label","text":"Hi","position":{"x":{"fixed":4},"y":{"fixed":2}},
                    "width":6,"height":1,"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"group","members":[0],"frames":{"start":0,"end":2}}"#),
        ];
        // Editable rows (frames / z_order) round-trip; x/y/width/height are ReadOnly.
        assert_props_roundtrip(&mut o, 1);

        let props = get_properties(&o, 1);
        let x = props.iter().find(|p| p.name == "x").unwrap();
        assert_eq!(x.kind, PropertyKind::ReadOnly);
        assert_eq!(x.value, "4");
        let w = props.iter().find(|p| p.name == "width").unwrap();
        assert_eq!(w.value, "6");
        // An explicit range shows its values and the override warning note.
        let ff = props.iter().find(|p| p.name == "first_frame").unwrap();
        assert_eq!(ff.value, "1", "0-based start 0 shown 1-based");
        assert!(props.iter().any(|p| p.kind == PropertyKind::Note));
    }

    #[test]
    fn auto_group_shows_blank_frames_and_no_note() {
        // A group with no `frames` field is auto: blank first/last frame, no note.
        let o = vec![
            obj(r#"{"type":"label","text":"Hi","position":{"x":{"fixed":0},"y":{"fixed":0}},
                    "frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"group","members":[0]}"#),
        ];
        let props = get_properties(&o, 1);
        let ff = props.iter().find(|p| p.name == "first_frame").unwrap();
        let lf = props.iter().find(|p| p.name == "last_frame").unwrap();
        assert_eq!(ff.value, "");
        assert_eq!(lf.value, "");
        assert!(!props.iter().any(|p| p.kind == PropertyKind::Note));
    }

    #[test]
    fn unknown_property_is_rejected() {
        let mut label = obj(
            r#"{"type":"label","text":"Hi","position":{"x":{"fixed":0},"y":{"fixed":0}},
                "frames":{"start":0,"end":1}}"#,
        );
        assert!(set_property(&mut label, "nonexistent", "x").is_err());
    }

    #[test]
    fn coordinate_get_set_roundtrips() {
        let mut label = obj(
            r#"{"type":"label","text":"Hi","position":{"x":{"fixed":3},"y":{"fixed":7}},
                "frames":{"start":0,"end":1}}"#,
        );
        let y = get_coord(&label, "y").unwrap();
        set_coordinate(&mut label, "x", y).unwrap();
        assert_eq!(get_coord(&label, "x").unwrap().start_value(), 7);
    }

    #[test]
    fn command_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"command","position":{"x":{"fixed":0},"y":{"fixed":0}},
                "width":10,"height":4,"command":"echo","args":["hi"],
                "frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn list_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"list","text":"a\nb","position":{"x":{"fixed":1},"y":{"fixed":1}},
                "ordered":true,"bullet":"*","spacing":2,"frames":{"start":0,"end":2}}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
    }

    #[test]
    fn loop_properties_roundtrip() {
        let mut o = vec![obj(
            r#"{"type":"loop","frames":{"start":2,"end":6},
                "delay_ms":250,"count":3,"bounce":false}"#,
        )];
        assert_props_roundtrip(&mut o, 0);
        // Editing the loop's own fields sticks.
        set_property(&mut o[0], "delay_ms", "750").unwrap();
        set_property(&mut o[0], "bounce", "true").unwrap();
        let props = get_properties(&o, 0);
        let delay = props.iter().find(|p| p.name == "delay_ms").unwrap();
        assert_eq!(delay.value, "750");
        let bounce = props.iter().find(|p| p.name == "bounce").unwrap();
        assert_eq!(bounce.value, "true");
    }

    #[test]
    fn resize_group_scales_members_with_fractional_precision() {
        // Members span x = 0..4 (bbox width 4). Anchored on the left, growing
        // the width to 6 scales by 1.5, so the second member's origin (x=1) and
        // size (w=3) become the fractional 1.5 and 4.5.
        let mut o = vec![
            obj(r#"{"type":"label","text":"A","position":{"x":{"fixed":0},"y":{"fixed":0}},
                    "width":1,"height":1,"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"label","text":"B","position":{"x":{"fixed":1},"y":{"fixed":0}},
                    "width":3,"height":1,"frames":{"start":0,"end":2}}"#),
            obj(r#"{"type":"group","members":[0,1],"frames":{"start":0,"end":2}}"#),
        ];
        let (_, _, gw, _) = group_bounds(&o, 2);
        assert_eq!(gw, 4.0);

        resize_group(&mut o, 2, 2, 0, true, true);

        assert_eq!(object_origin_x_f(&o[0]), 0.0, "left-anchored member stays put");
        assert_eq!(object_origin_x_f(&o[1]), 1.5);
        assert_eq!(object_dim_x_f(&o[1]), 4.5);
    }
}

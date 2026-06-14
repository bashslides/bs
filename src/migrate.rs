//! One-shot migration of old-format presentation JSON to the current format.
//!
//! The animation model changed to a single source of truth: an animation's span
//! lives only on the `Animation` object, and animated coordinates reference it by
//! a stable `id`. Old files instead stored the span *on each coordinate*
//! (`{"animated":{"from","to","start_frame","end_frame"}}`) and their `animation`
//! objects had no `id`. Those files no longer parse — hence this migrator.
//!
//! It works on the raw `serde_json::Value` tree (the current structs can't
//! deserialize the old shape, which is the whole problem), reconstructing the
//! once-implicit coordinate→animation link that the old engine resolved by
//! *span matching* (`upsert_animation` reused the `Animation` whose `frames`
//! equalled a coordinate's `[start_frame, end_frame + 1)`):
//!
//! 1. Assign an `id` to every `animation` object and map its span → id.
//! 2. Rewrite each old-style animated coordinate to `{from, to, anim: <id>}`,
//!    looking the id up by span (synthesizing an `Animation` for any span with no
//!    matching object — covers files predating the `Animation` sidecar).
//!
//! It is idempotent (already-migrated nodes are left alone) and self-verifying
//! (the result is parsed through the real `SourcePresentation` before any write).

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::engine::source::SourcePresentation;

/// Outcome of a migration, for reporting.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Animated coordinates rewritten to the id-referenced form.
    pub coords_linked: usize,
    /// `animation` objects that were missing an `id` and got one.
    pub ids_assigned: usize,
    /// `Animation` objects created for animated spans that had no sidecar.
    pub synthesized: usize,
}

impl Report {
    /// True when nothing needed changing (the file is already current).
    pub fn unchanged(&self) -> bool {
        self.coords_linked == 0 && self.ids_assigned == 0 && self.synthesized == 0
    }
}

/// Migrate `path` **in place**, saving the original to `<path>.bak` first. A
/// no-op (no write, no backup) when the file is already in the current format.
pub fn migrate_file(path: &str) -> Result<()> {
    let original =
        fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
    let mut json: Value = serde_json::from_str(&original)
        .with_context(|| format!("Failed to parse {path} as JSON"))?;

    let report = migrate_value(&mut json)
        .with_context(|| format!("Failed to migrate {path}"))?;

    if report.unchanged() {
        eprintln!("{path} is already in the current format — nothing to do.");
        return Ok(());
    }

    let migrated = serde_json::to_string_pretty(&json)?;

    // Self-verify: the result must parse through the real model. If it doesn't,
    // leave the file untouched rather than write something still broken.
    serde_json::from_str::<SourcePresentation>(&migrated).with_context(|| {
        "migration produced JSON the engine still can't parse — left the file unchanged"
    })?;

    let backup = format!("{path}.bak");
    fs::write(&backup, &original)
        .with_context(|| format!("Failed to write backup {backup}"))?;
    fs::write(path, &migrated).with_context(|| format!("Failed to write {path}"))?;

    eprintln!(
        "Migrated {path} (backup: {backup}) — linked {} coordinate{}, assigned {} id{}, synthesized {} animation{}.",
        report.coords_linked, plural(report.coords_linked),
        report.ids_assigned, plural(report.ids_assigned),
        report.synthesized, plural(report.synthesized),
    );
    Ok(())
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Transform a parsed presentation `Value` in place from the old animation format
/// to the current one. Pure (no I/O) so it can be unit-tested.
pub fn migrate_value(root: &mut Value) -> Result<Report> {
    let objects = root
        .get_mut("objects")
        .and_then(Value::as_array_mut)
        .context("not a presentation (no `objects` array)")?;

    // Pass 1: give every animation object an id and record span → id.
    let mut span_to_id: BTreeMap<(i64, i64), u64> = BTreeMap::new();
    let mut next_id: u64 = 1;
    // Seed the counter past any ids already present (so re-running is stable).
    for obj in objects.iter() {
        if is_animation(obj) {
            if let Some(id) = obj.get("id").and_then(Value::as_u64) {
                next_id = next_id.max(id + 1);
            }
        }
    }
    let mut report = Report::default();
    for obj in objects.iter_mut() {
        if !is_animation(obj) {
            continue;
        }
        let id = match obj.get("id").and_then(Value::as_u64) {
            Some(id) => id,
            None => {
                let id = next_id;
                next_id += 1;
                report.ids_assigned += 1;
                obj.as_object_mut().unwrap().insert("id".into(), json!(id));
                id
            }
        };
        if let Some((s, e)) = frame_span(obj.get("frames")) {
            span_to_id.insert((s, e), id);
        }
    }

    // Pass 2: rewrite every old-style animated coordinate, anywhere in the tree.
    let mut synthesized: BTreeMap<(i64, i64), u64> = BTreeMap::new();
    transform_coords(root, &span_to_id, &mut synthesized, &mut next_id, &mut report.coords_linked);

    // Pass 3: append a sidecar for every span we had to synthesize. Appending
    // keeps existing object indices (group members, `links`) valid.
    if !synthesized.is_empty() {
        report.synthesized = synthesized.len();
        let objects = root.get_mut("objects").and_then(Value::as_array_mut).unwrap();
        for ((start, end), id) in &synthesized {
            objects.push(json!({
                "type": "animation",
                "id": id,
                "frames": { "start": start, "end": end },
                "auto_play": true,
                "delay_ms": 500,
            }));
        }
    }

    Ok(report)
}

fn is_animation(obj: &Value) -> bool {
    obj.get("type").and_then(Value::as_str) == Some("animation")
}

/// Read a `{ "start", "end" }` frame range as `(start, end)`.
fn frame_span(frames: Option<&Value>) -> Option<(i64, i64)> {
    let f = frames?;
    Some((f.get("start")?.as_i64()?, f.get("end")?.as_i64()?))
}

/// Recursively rewrite old-style animated coordinates (`{"animated":{…,
/// "start_frame","end_frame"}}`) to `{"animated":{from,to,anim}}`. A coordinate
/// already in the new form (no `start_frame`/`end_frame`) is left untouched.
fn transform_coords(
    v: &mut Value,
    span_to_id: &BTreeMap<(i64, i64), u64>,
    synthesized: &mut BTreeMap<(i64, i64), u64>,
    next_id: &mut u64,
    linked: &mut usize,
) {
    match v {
        Value::Object(map) => {
            // An old-style animated coordinate: the `Coordinate::Animated` enum
            // serialized as `{"animated": { from, to, start_frame, end_frame }}`.
            let old = map.get("animated").and_then(Value::as_object).is_some_and(|a| {
                a.contains_key("start_frame") || a.contains_key("end_frame")
            });
            if old {
                let a = map.get("animated").and_then(Value::as_object).unwrap();
                let from = a.get("from").cloned().unwrap_or_else(|| json!(0));
                let to = a.get("to").cloned().unwrap_or_else(|| json!(0));
                let sf = a.get("start_frame").and_then(Value::as_i64).unwrap_or(0);
                let ef = a.get("end_frame").and_then(Value::as_i64).unwrap_or(sf);
                // Coordinate end is inclusive; the Animation span end is exclusive.
                let span = (sf, ef + 1);
                let id = span_to_id
                    .get(&span)
                    .copied()
                    .or_else(|| synthesized.get(&span).copied())
                    .unwrap_or_else(|| {
                        let id = *next_id;
                        *next_id += 1;
                        synthesized.insert(span, id);
                        id
                    });
                map.insert("animated".into(), json!({ "from": from, "to": to, "anim": id }));
                *linked += 1;
                return; // the rewritten node holds only scalars; no need to recurse
            }
            for child in map.values_mut() {
                transform_coords(child, span_to_id, synthesized, next_id, linked);
            }
        }
        Value::Array(arr) => {
            for child in arr.iter_mut() {
                transform_coords(child, span_to_id, synthesized, next_id, linked);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn links_a_coord_to_its_existing_animation_by_span() {
        // Old format: animated y over frames 0..=4, plus an idless animation
        // whose [0,5) span matches.
        let mut v = val(
            r#"{ "width": 4, "height": 4, "frame_count": 5, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "fixed": 0 },
                                "y": { "animated": { "from": 0, "to": 3, "start_frame": 0, "end_frame": 4 } } },
                  "frames": { "start": 0, "end": 5 } },
                { "type": "animation", "frames": { "start": 0, "end": 5 }, "auto_play": true, "delay_ms": 500 }
            ] }"#,
        );
        let r = migrate_value(&mut v).unwrap();
        assert_eq!(r.coords_linked, 1);
        assert_eq!(r.ids_assigned, 1);
        assert_eq!(r.synthesized, 0);

        let objs = v["objects"].as_array().unwrap();
        let anim_id = objs[1]["id"].as_u64().unwrap();
        let coord = &objs[0]["position"]["y"]["animated"];
        assert_eq!(coord["anim"].as_u64().unwrap(), anim_id, "coord references the matched animation");
        assert_eq!(coord["from"].as_i64(), Some(0));
        assert_eq!(coord["to"].as_i64(), Some(3));
        assert!(coord.get("start_frame").is_none(), "old span fields dropped");

        // The migrated value parses through the real model.
        serde_json::from_str::<SourcePresentation>(&serde_json::to_string(&v).unwrap()).unwrap();
    }

    #[test]
    fn synthesizes_an_animation_for_an_orphan_span() {
        // A pre-sidecar file: animated coord, no animation object at all.
        let mut v = val(
            r#"{ "width": 4, "height": 4, "frame_count": 5, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "animated": { "from": 0, "to": 3, "start_frame": 1, "end_frame": 3 } },
                                "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 5 } }
            ] }"#,
        );
        let r = migrate_value(&mut v).unwrap();
        assert_eq!(r.coords_linked, 1);
        assert_eq!(r.synthesized, 1);

        let objs = v["objects"].as_array().unwrap();
        let anim = objs.iter().find(|o| is_animation(o)).expect("a synthesized animation");
        // end_frame 3 (inclusive) → exclusive 4.
        assert_eq!(frame_span(anim.get("frames")), Some((1, 4)));
        let id = anim["id"].as_u64().unwrap();
        assert_eq!(objs[0]["position"]["x"]["animated"]["anim"].as_u64(), Some(id));
    }

    #[test]
    fn two_coords_sharing_a_span_share_one_animation() {
        // x and y over the same span → one animation, both reference it.
        let mut v = val(
            r#"{ "width": 4, "height": 4, "frame_count": 5, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "animated": { "from": 0, "to": 3, "start_frame": 0, "end_frame": 4 } },
                                "y": { "animated": { "from": 1, "to": 2, "start_frame": 0, "end_frame": 4 } } },
                  "frames": { "start": 0, "end": 5 } },
                { "type": "animation", "frames": { "start": 0, "end": 5 } }
            ] }"#,
        );
        let r = migrate_value(&mut v).unwrap();
        assert_eq!(r.coords_linked, 2);
        assert_eq!(r.synthesized, 0);
        let objs = v["objects"].as_array().unwrap();
        let xa = objs[0]["position"]["x"]["animated"]["anim"].as_u64();
        let ya = objs[0]["position"]["y"]["animated"]["anim"].as_u64();
        assert_eq!(xa, ya, "shared span → shared animation id");
    }

    #[test]
    fn already_migrated_is_a_noop() {
        let mut v = val(
            r#"{ "width": 4, "height": 4, "frame_count": 5, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "fixed": 0 },
                                "y": { "animated": { "from": 0, "to": 3, "anim": 1 } } },
                  "frames": { "start": 0, "end": 5 } },
                { "type": "animation", "id": 1, "frames": { "start": 0, "end": 5 } }
            ] }"#,
        );
        let before = v.clone();
        let r = migrate_value(&mut v).unwrap();
        assert!(r.unchanged(), "an already-current file changes nothing");
        assert_eq!(v, before, "value untouched");
    }

    #[test]
    fn synthesized_ids_avoid_existing_ones() {
        // An existing animation has id 7; a synthesized one must not collide.
        let mut v = val(
            r#"{ "width": 4, "height": 4, "frame_count": 6, "objects": [
                { "type": "label", "text": "A",
                  "position": { "x": { "animated": { "from": 0, "to": 3, "start_frame": 0, "end_frame": 2 } },
                                "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 3 } },
                { "type": "animation", "id": 7, "frames": { "start": 9, "end": 10 } }
            ] }"#,
        );
        migrate_value(&mut v).unwrap();
        let objs = v["objects"].as_array().unwrap();
        let synth = objs.iter().find(|o| is_animation(o) && o["id"].as_u64() != Some(7)).unwrap();
        assert_eq!(synth["id"].as_u64(), Some(8), "next id is past the existing max");
        assert_eq!(objs[0]["position"]["x"]["animated"]["anim"].as_u64(), Some(8));
    }
}

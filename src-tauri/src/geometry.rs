//! Where the pet goes and how big it is. Pure functions, unit-tested.
//!
//! - Every pet is sized by its *visible body* (not its canvas), as a share of the height of the
//!   screen it's on, times the Size setting. Pixel pets snap to whole-number scales.
//! - Its spot is remembered per screen as a fraction of that screen ("feet at 92% across").
//! - The session list is a separate window that opens above or below the pet, wherever there's
//!   more room, and is clamped to the screen.
use serde::Serialize;
use serde_json::{json, Value};

pub const SIZES: [(&str, f64); 4] = [("small", 0.75), ("medium", 1.0), ("large", 1.3), ("huge", 1.7)];
const CONTENT_SHARE: f64 = 0.073;
const REF_HEIGHT: f64 = 840.0;
const PANEL_W: f64 = 312.0;
const PANEL_MAX_H: f64 = 560.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct Display {
    pub id: String,
    pub bounds: Rect,
    pub work_area: Rect,
}

/// What the geometry needs to know about a pet.
#[derive(Debug, Clone, Copy)]
pub struct PetShape {
    pub frame_w: f64,
    pub frame_h: f64,
    pub pixel_art: bool,
    /// Rows that actually have pixels (top, bottom); None = the whole frame.
    pub content: Option<(f64, f64)>,
}

impl PetShape {
    pub fn from_pet(pet: &Value) -> PetShape {
        let fw = pet["frameWidth"].as_f64().unwrap_or(32.0);
        let fh = pet["frameHeight"].as_f64().unwrap_or(32.0);
        let content = pet.get("content").and_then(|c| Some((c.get("top")?.as_f64()?, c.get("bottom")?.as_f64()?)));
        PetShape { frame_w: fw, frame_h: fh, pixel_art: pet["pixelArt"].as_bool() != Some(false), content }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pad {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub zoom: f64,
    pub scale: f64,
    pub pet_w: f64,
    pub pet_h: f64,
    pub body_h: f64,
    pub clip_bottom: f64,
    pub pad: Pad,
    pub box_w: f64,
    pub box_h: f64,
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

fn size_factor(size: &str) -> f64 {
    SIZES.iter().find(|(k, _)| *k == size).map(|(_, f)| *f).unwrap_or(1.0)
}

pub fn zoom_for(work_area: &Rect) -> f64 {
    clamp(work_area.height / REF_HEIGHT, 0.85, 1.7)
}

/// Whole-number scale for pixel art, making sure every Size step is visibly different.
fn pixel_scale(medium_scale: f64, size: &str) -> f64 {
    let r = |k: &str| (medium_scale * size_factor(k)).round().max(1.0);
    let medium = r("medium");
    let small = r("small").min(medium - 1.0).max(1.0);
    let large = r("large").max(medium + 1.0);
    let huge = r("huge").max(large + 1.0);
    match size {
        "small" => small,
        "large" => large,
        "huge" => huge,
        _ => medium,
    }
}

pub fn pet_metrics(work_area: &Rect, pet: &PetShape, size: &str) -> Metrics {
    let zoom = zoom_for(work_area);
    let (top_row, bottom_row) = pet.content.filter(|(t, b)| b >= t).unwrap_or((0.0, pet.frame_h - 1.0));
    let content_h = bottom_row - top_row + 1.0;
    let scale = if pet.pixel_art {
        pixel_scale(work_area.height * CONTENT_SHARE / content_h, size)
    } else {
        work_area.height * CONTENT_SHARE * size_factor(size) / content_h
    };
    let pet_w = (pet.frame_w * scale).round();
    let pet_h = (pet.frame_h * scale).round();
    let headroom = (top_row * scale).round();
    let clip_bottom = ((pet.frame_h - 1.0 - bottom_row) * scale).round();
    let pad_x = (16.0 * zoom).round();
    let bottom = (3.0 * zoom).round();
    let top = ((34.0 * zoom).round() - headroom).max(0.0);
    Metrics {
        zoom,
        scale,
        pet_w,
        pet_h,
        body_h: (content_h * scale).round(),
        clip_bottom,
        pad: Pad { x: pad_x, top, bottom },
        box_w: (pet_w + 2.0 * pad_x).max((84.0 * zoom).round()),
        box_h: pet_h - clip_bottom + top + bottom,
    }
}

/// Window rect whose sprite feet (bottom-center) land on `feet`.
pub fn rect_for_feet(feet: (f64, f64), m: &Metrics) -> Rect {
    Rect { x: (feet.0 - m.box_w / 2.0).round(), y: (feet.1 - (m.box_h - m.pad.bottom)).round(), width: m.box_w, height: m.box_h }
}

pub fn feet_of(r: &Rect, m: &Metrics) -> (f64, f64) {
    (r.x + r.width / 2.0, r.y + r.height - m.pad.bottom)
}

/// Keep the pet fully on its screen: it may sit over the Dock, not under the menu bar.
pub fn clamp_rect(r: Rect, d: &Display) -> Rect {
    let (b, wa) = (&d.bounds, &d.work_area);
    Rect {
        x: clamp(r.x, b.x, b.x + b.width - r.width).round(),
        y: clamp(r.y, wa.y, b.y + b.height - r.height).round(),
        ..r
    }
}

pub fn feet_from_place(fx: Option<f64>, fy: Option<f64>, d: &Display) -> (f64, f64) {
    let wa = &d.work_area;
    match (fx, fy) {
        (Some(fx), Some(fy)) => (wa.x + fx * wa.width, wa.y + fy * wa.height),
        _ => (wa.x + wa.width - (90.0 * zoom_for(wa)).round(), wa.y + wa.height - 6.0),
    }
}

pub fn place_from_feet(feet: (f64, f64), d: &Display) -> (f64, f64) {
    let wa = &d.work_area;
    (((feet.0 - wa.x) / wa.width * 10000.0).round() / 10000.0, ((feet.1 - wa.y) / wa.height * 10000.0).round() / 10000.0)
}

pub struct PanelPlace {
    pub rect: Rect,
    pub above: bool,
    pub right: bool,
    pub pet_offset_x: f64,
}

impl PanelPlace {
    pub fn layout(&self, zoom: f64) -> Value {
        json!({ "v": if self.above { "above" } else { "below" }, "h": if self.right { "right" } else { "left" }, "zoom": zoom, "petOffsetX": self.pet_offset_x })
    }
}

/// Where the session list goes for a pet at `pet`.
pub fn panel_rect(pet: &Rect, d: &Display, zoom: f64) -> PanelPlace {
    let wa = &d.work_area;
    let w = (PANEL_W * zoom).round();
    let max_h = (PANEL_MAX_H * zoom).round();
    let cx = pet.x + pet.width / 2.0;
    let right = cx > wa.x + wa.width / 2.0;
    let x = clamp(if right { pet.x + pet.width - w } else { pet.x }, wa.x, wa.x + wa.width - w);
    let above_room = pet.y - wa.y;
    let below_room = wa.y + wa.height - (pet.y + pet.height);
    let above = above_room >= below_room;
    let h = (140.0 * zoom).round().max(max_h.min(if above { above_room } else { below_room }));
    let y = clamp(if above { pet.y - h } else { pet.y + pet.height }, wa.y, wa.y + wa.height - h);
    PanelPlace { rect: Rect { x: x.round(), y: y.round(), width: w, height: h }, above, right, pet_offset_x: (cx - x).round() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn laptop() -> Display {
        Display { id: "laptop".into(), bounds: Rect { x: 0.0, y: 0.0, width: 1470.0, height: 956.0 }, work_area: Rect { x: 0.0, y: 33.0, width: 1470.0, height: 856.0 } }
    }
    fn monitor() -> Rect { Rect { x: 1470.0, y: -275.0, width: 2560.0, height: 1415.0 } }
    const PIP: PetShape = PetShape { frame_w: 32.0, frame_h: 28.0, pixel_art: true, content: Some((9.0, 23.0)) };

    #[test]
    fn pet_scales_with_the_screen() {
        let a = pet_metrics(&laptop().work_area, &PIP, "medium");
        let b = pet_metrics(&monitor(), &PIP, "medium");
        assert!(a.scale.fract() == 0.0 && b.scale.fract() == 0.0);
        assert!(b.pet_h > a.pet_h && b.zoom > a.zoom);
        let big = PetShape { content: None, ..PIP };
        let s: Vec<f64> = ["small", "medium", "large", "huge"].iter().map(|k| pet_metrics(&laptop().work_area, &big, k).scale).collect();
        assert!(s.windows(2).all(|w| w[0] < w[1]), "every size differs: {s:?}");
    }

    #[test]
    fn pets_come_out_about_the_same_size() {
        let mochi = PetShape { frame_w: 16.0, frame_h: 14.0, pixel_art: true, content: Some((3.0, 13.0)) };
        let a = pet_metrics(&laptop().work_area, &PIP, "medium").body_h;
        let b = pet_metrics(&laptop().work_area, &mochi, "medium").body_h;
        assert!((a.max(b) / a.min(b)) < 1.25, "{a} vs {b}");
    }

    #[test]
    fn pet_can_sit_anywhere_on_screen() {
        let d = laptop();
        let m = pet_metrics(&d.work_area, &PIP, "medium");
        for (fx, fy) in [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0), (0.0, 0.5), (0.5, 1.2)] {
            let r = clamp_rect(rect_for_feet(feet_from_place(Some(fx), Some(fy), &d), &m), &d);
            assert!(r.x >= 0.0 && r.x + r.width <= 1470.0);
            assert!(r.y >= 33.0 && r.y + r.height <= 956.0);
        }
        let feet = (300.0, 500.0);
        let (fx, fy) = place_from_feet(feet, &d);
        let back = feet_from_place(Some(fx), Some(fy), &d);
        assert!((back.0 - 300.0).abs() < 1.0 && (back.1 - 500.0).abs() < 1.0);
    }

    #[test]
    fn list_opens_toward_the_room() {
        let d = laptop();
        let m = pet_metrics(&d.work_area, &PIP, "medium");
        let bottom_right = clamp_rect(rect_for_feet((1410.0, 889.0), &m), &d);
        let p = panel_rect(&bottom_right, &d, m.zoom);
        assert!(p.above && p.right);
        assert_eq!(p.rect.y + p.rect.height, bottom_right.y);
        let top_left = clamp_rect(rect_for_feet((40.0, 113.0), &m), &d);
        let p = panel_rect(&top_left, &d, m.zoom);
        assert!(!p.above && !p.right);
        assert_eq!(p.rect.y, top_left.y + top_left.height);
    }
}


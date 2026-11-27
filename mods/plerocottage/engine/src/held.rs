//! The items GeckoLib draws while they are held (the mod's `GeoItemRenderer`s: the display foods and the blocks
//! drawn as their block entity), drawn here with `gecko`'s player where the game asks who draws a held item
//! (`native_item_submit`).
//!
//! `data/gecko_items.json` says which model and texture each item has, the display transforms of its model file,
//! and in which places the mod's own model for it lets GeckoLib draw (`FoodDisplayItemModelEvents`: its flat model
//! in the GUI, on the ground and in frames), as pleromod's extraction found them (`gecko_items.py`).
use crate::display::{self, Layer};
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Index {
    /// Each place an item is drawn in, by its name in model files: whether GeckoLib draws a flat-modelled item there.
    places: HashMap<String, bool>,
    items: HashMap<String, Item>,
}

#[derive(Deserialize)]
struct Item {
    model: String,
    texture: String,
    #[serde(default)]
    layers: Vec<Layer>,
    /// `display` of the item's model file: the transform of each place.
    #[serde(default)]
    display: HashMap<String, Transform>,
    /// Whether the mod gives it a flat model for the places GeckoLib does not draw it in.
    flat: bool,
}

/// An `ItemTransform` as a model file writes it: degrees, sixteenths of a block, and a scale.
#[derive(Deserialize)]
struct Transform {
    #[serde(default)]
    rotation: [f32; 3],
    #[serde(default)]
    translation: [f32; 3],
    #[serde(default = "whole")]
    scale: [f32; 3],
}

fn whole() -> [f32; 3] {
    [1.0; 3]
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/gecko_items.json")))
            .expect("data/gecko_items.json is what gecko_items.py writes")
    })
}

/// A place's name in model files, and whether it is a left hand (`ItemTransform.apply` mirrors it).
fn place(display: DisplayContext) -> Option<(&'static str, bool)> {
    Some(match display {
        DisplayContext::FirstPersonLeftHand => ("firstperson_lefthand", true),
        DisplayContext::FirstPersonRightHand => ("firstperson_righthand", false),
        DisplayContext::ThirdPersonLeftHand => ("thirdperson_lefthand", true),
        DisplayContext::ThirdPersonRightHand => ("thirdperson_righthand", false),
        DisplayContext::Head => ("head", false),
        DisplayContext::Ground => ("ground", false),
        DisplayContext::Fixed => ("fixed", false),
        DisplayContext::Gui => ("gui", false),
        _ => return None,
    })
}

/// Whether GeckoLib draws `item` in the place named: everywhere, unless the mod's model for it is flat there.
fn drawn_in(index: &Index, item: &Item, name: &str) -> bool {
    !item.flat || index.places.get(name).copied().unwrap_or(false)
}

/// `GeoItemRenderer.renderByItem`: the item's model under its display transform. True when the mod draws it here.
pub fn draw(host: &dyn Host, event: &mut ItemSubmitEvent<'_>) -> bool {
    let index = index();
    let Some(item) = index.items.get(&*event.stack.item) else {
        return false;
    };
    let Some((name, left)) = place(event.display).filter(|(name, _)| drawn_in(index, item, name)) else {
        return false;
    };
    let Some(geometry) = display::geometry(host, &item.model) else {
        return false;
    };
    let draw = &mut *event.draw;
    draw.push();
    if let Some(transform) = item.display.get(name) {
        // `ItemTransform.apply`: moved (at most five blocks), turned about x, y and z in turn, scaled (at most four
        // times); a left hand is the right one mirrored.
        let side = if left { -1.0 } else { 1.0 };
        let moved = transform.translation.map(|v| (v / 16.0).clamp(-5.0, 5.0));
        draw.translate([side * moved[0], moved[1], moved[2]]);
        draw.rotate_about([1.0, 0.0, 0.0], transform.rotation[0], [0.0; 3]);
        draw.rotate_about([0.0, 1.0, 0.0], side * transform.rotation[1], [0.0; 3]);
        draw.rotate_about([0.0, 0.0, 1.0], side * transform.rotation[2], [0.0; 3]);
        draw.scale(transform.scale.map(|v| v.clamp(-4.0, 4.0)));
    }
    // `ItemRenderer.render` puts the model's corner half a block back; `GeoItemRenderer.preRender` stands the
    // GeckoLib model in the middle of that block, a hundredth of a block up.
    draw.translate([-0.5, -0.5, -0.5]);
    draw.translate([0.5, 0.51, 0.5]);
    display::submit(host, draw, &geometry, None, &item.texture, 0xFF_FFFF);
    for layer in &item.layers {
        if let Some(part) = display::geometry(host, &layer.model) {
            display::submit(host, draw, &part, None, &layer.texture, display::tint(layer.tint));
        }
    }
    draw.pop();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_items_are_geckolibs_in_the_hands_and_the_others_everywhere() {
        let index = index();
        assert!(!index.items.is_empty());
        let hands = ["firstperson_lefthand", "firstperson_righthand", "thirdperson_lefthand", "thirdperson_righthand"];
        for (name, item) in &index.items {
            for hand in hands {
                assert!(drawn_in(index, item, hand), "{name} in {hand}");
            }
            for flat in ["gui", "ground", "fixed"] {
                assert_eq!(drawn_in(index, item, flat), !item.flat, "{name} in {flat}");
            }
        }
        // The mod's own model says where: every place has its answer.
        for display in [DisplayContext::FirstPersonLeftHand, DisplayContext::Head, DisplayContext::Gui, DisplayContext::Ground] {
            let (name, _) = place(display).unwrap();
            assert!(index.places.contains_key(name), "{name}");
        }
    }
}

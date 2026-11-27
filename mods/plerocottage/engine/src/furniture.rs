//! `FurnitureBlock`: the cabinets and drawers. Each keeps a container (`FurnitureBlockEntity`)
//! that `use` opens in a menu (`FurnitureMenu`, drawn by `FurnitureScreen`); the block stands open
//! while someone looks in, with a sound each way (`setOpen`), and the screen's buttons sort the
//! container's slots (`FurnitureMenu.clickMenuButton` and `sortRange`).
//!
//! Which block is which, its menu, its screen, the slots a button sorts and the sounds are what
//! pleromod's extraction found by running the mod's own classes (`tools/mod-content/furniture.py`,
//! `data/furniture.json`); `sort` is the port of `sortRange`.
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Default, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub blocks: HashMap<String, Block>,
    #[serde(default)]
    pub kinds: HashMap<String, Kind>,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub kind: String,
    pub open: Opening,
    pub close: Opening,
}

/// What `setOpen` does one way: the properties it sets and the sound it plays.
#[derive(Debug, Deserialize)]
pub struct Opening {
    pub set: HashMap<String, bool>,
    pub flags: i32,
    pub sound: String,
    pub source: String,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kind {
    pub size: usize,
    pub faces: Vec<Vec<usize>>,
    pub inputs: Vec<usize>,
    pub outputs: Vec<usize>,
    pub menu_type: String,
    pub menu: Vec<Slot>,
    pub inventory: Inventory,
    pub screen: Screen,
    /// What `clickMenuButton` does with each button number: the slots it sorts.
    pub buttons: Vec<Press>,
}

#[derive(Debug, Deserialize)]
pub struct Slot {
    pub index: usize,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Deserialize)]
pub struct Inventory {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Deserialize)]
pub struct Press {
    pub id: i32,
    pub sorts: [usize; 2],
}

#[derive(Debug, Deserialize)]
pub struct Screen {
    pub width: f32,
    pub height: f32,
    pub background: Vec<Vec<serde_json::Value>>,
    pub labels: Vec<Vec<serde_json::Value>>,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub draw: Vec<serde_json::Value>,
    /// Where the picture is taken from while the pointer is over it (`AbstractWidget.renderTexture`).
    pub hovered_v: f32,
    pub button: i32,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/furniture.json")))
            .expect("furniture.json is the extraction's")
    })
}

/// The cabinet or drawer `block` is, with what its kind keeps.
pub fn of(block: &str) -> Option<(&'static Block, &'static Kind)> {
    let data = data();
    let found = data.blocks.get(block)?;
    Some((found, data.kinds.get(&found.kind)?))
}

/// `FurnitureBlock.newBlockEntity`: the block entity keeps nothing but its slots.
pub fn container(kind: &Kind) -> ModContainer {
    let mut faces: [Vec<usize>; 6] = Default::default();
    for (face, slots) in faces.iter_mut().zip(&kind.faces) {
        face.clone_from(slots);
    }
    ModContainer {
        slots: kind.size,
        faces,
        inputs: kind.inputs.clone(),
        outputs: kind.outputs.clone(),
        menu: Some(kind.menu_type.clone()),
    }
}

pub fn screen(kind: &Kind) -> NativeScreen {
    let mut slots = vec![None; kind.size];
    for slot in &kind.menu {
        if let Some(place) = slots.get_mut(slot.index) {
            *place = Some([slot.x, slot.y]);
        }
    }
    NativeScreen {
        width: kind.screen.width,
        height: kind.screen.height,
        slots,
        inventory_x: kind.inventory.x,
        inventory_y: kind.inventory.y,
        // FurnitureScreen.renderLabels draws its own title, and renderBg its picture.
        own_labels: true,
        own_background: true,
    }
}

fn hit(button: &Button, at: [f32; 2]) -> bool {
    at[0] >= button.x && at[1] >= button.y && at[0] < button.x + button.width && at[1] < button.y + button.height
}

/// A blit of the screen's: its picture is a GUI texture to Horizon, where the content mod carries
/// it (`plerocottage:textures/screens/x.png` as `plerocottage:textures/gui/screens/x.png`).
fn blit(host: &dyn Host, batch: &mut HudBatch, origin: [f32; 2], call: &[serde_json::Value]) {
    let mut call = call.to_vec();
    if let Some(serde_json::Value::String(texture)) = call.get_mut(1)
        && let Some((namespace, path)) = texture.split_once(':')
        && !path.starts_with("textures/gui/")
    {
        *texture = format!("{namespace}:textures/gui/{}", path.trim_start_matches("textures/"));
    }
    crate::draw(host, batch, origin, &call);
}

/// `FurnitureScreen.render`: the background, the buttons, the labels.
pub fn draw(host: &dyn Host, batch: &mut HudBatch, origin: [f32; 2], kind: &Kind) {
    let screen = &kind.screen;
    for call in &screen.background {
        blit(host, batch, origin, call);
    }
    let pointer = host.cursor();
    let at = [pointer[0] - origin[0], pointer[1] - origin[1]];
    for button in &screen.buttons {
        let mut call = button.draw.clone();
        if hit(button, at)
            && let Some(v) = call.get_mut(5)
        {
            *v = button.hovered_v.into();
        }
        blit(host, batch, origin, &call);
    }
    for label in &screen.labels {
        // drawString(font, text, x, y, colour, shadow)
        let text = match label.get(1) {
            Some(serde_json::Value::Object(said)) => match (said.get("translate"), said.get("literal")) {
                (Some(serde_json::Value::String(key)), _) => Text::translate(key, &[]).plain(),
                (_, Some(serde_json::Value::String(text))) => text.clone(),
                _ => continue,
            },
            _ => continue,
        };
        let n = |i: usize| label.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32;
        let colour = label.get(4).and_then(serde_json::Value::as_i64).unwrap_or(0) as u32 & 0xFF_FFFF;
        let shadow = label.get(5).is_some_and(|s| s.as_bool() == Some(true) || s.as_i64().is_some_and(|v| v != 0));
        host.text(batch, &text, [origin[0] + n(2), origin[1] + n(3)], colour, shadow);
    }
}

/// A click at `at` on the screen: the menu button the button under it clicks
/// (`MultiPlayerGameMode.handleInventoryButtonClick`).
pub fn click(kind: &Kind, at: [f32; 2]) -> Option<i32> {
    kind.screen.buttons.iter().find(|b| hit(b, at)).map(|b| b.button)
}

/// `FurnitureMenu.clickMenuButton`: a button the menu knows sorts its slots; any other does nothing.
pub fn pressed(host: &dyn Host, kind: &Kind, items: &mut Vec<Option<ItemStack>>, id: i32) -> bool {
    let Some(press) = kind.buttons.iter().find(|p| p.id == id) else {
        return false;
    };
    items.resize(kind.size.max(items.len()), None);
    let order = registry_order(host);
    let [from, to] = press.sorts;
    sort(&mut items[from.min(to)..to.min(kind.size)], |item| order.get(item).copied(), |item| {
        host.max_stack_size(item).unwrap_or(64)
    });
    true
}

/// `BuiltInRegistries.ITEM`'s order: each item's number in the registry.
fn registry_order(host: &dyn Host) -> HashMap<String, usize> {
    let mut order = HashMap::new();
    for id in 0.. {
        let Some(name) = host.registry_name("minecraft:item", id) else {
            break;
        };
        order.insert(name, id as usize);
    }
    order
}

/// `FurnitureMenu.sortRange`: the stacks of `slots` in the registry's order of their items (one
/// not in it last), stable; each joined into the first earlier one of the same item and components
/// that has room, then laid from the first slot on, the rest of the slots emptied.
pub fn sort(
    slots: &mut [Option<ItemStack>],
    order: impl Fn(&str) -> Option<usize>,
    max_stack: impl Fn(&str) -> u32,
) {
    let mut stacks: Vec<ItemStack> = slots.iter().flatten().filter(|s| s.count > 0).cloned().collect();
    stacks.sort_by_key(|s| order(&s.item).unwrap_or(i32::MAX as usize));
    let mut merged: Vec<ItemStack> = Vec::new();
    for mut stack in stacks {
        for into in &mut merged {
            let max = max_stack(&into.item);
            if into.same_item_same_components(&stack) && into.count < max {
                let moved = stack.count.min(max - into.count);
                into.count += moved;
                stack.count -= moved;
                if stack.count == 0 {
                    break;
                }
            }
        }
        if stack.count > 0 {
            merged.push(stack);
        }
    }
    for slot in slots.iter_mut() {
        *slot = None;
    }
    let mut at = 0;
    for mut stack in merged {
        while stack.count > 0 && at < slots.len() {
            let mut laid = stack.clone();
            laid.count = stack.count.min(max_stack(&stack.item));
            stack.count -= laid.count;
            slots[at] = Some(laid);
            at += 1;
        }
    }
}

/// `SoundSource`'s constants in order: the number a sound's source is played with.
const SOURCES: [&str; 10] = [
    "MASTER", "MUSIC", "RECORDS", "WEATHER", "BLOCKS", "HOSTILE", "NEUTRAL", "PLAYERS", "AMBIENT", "VOICE",
];

/// `FurnitureBlockEntity.setOpen`, which `openFor` calls for the first player to look in and
/// `closeFor` after the last: the block's state follows, and the sound is heard by everyone.
pub fn opened(host: &mut dyn Host, pos: [i32; 3], block: &Block, open: bool) {
    let way = if open { &block.open } else { &block.close };
    // Only when the block does not stand that way already.
    let already = way.set.iter().all(|(key, value)| {
        host.block_property(pos, key).as_deref() == Some(if *value { "true" } else { "false" })
    });
    if already {
        return;
    }
    for (key, value) in &way.set {
        host.set_block_property_with(pos, key, if *value { "true" } else { "false" }, way.flags);
    }
    let source = SOURCES.iter().position(|s| *s == way.source).unwrap_or(crate::BLOCKS);
    host.play_sound(&way.sound, crate::at(pos), source, way.volume, way.pitch);
}

#[cfg(test)]
mod tests;

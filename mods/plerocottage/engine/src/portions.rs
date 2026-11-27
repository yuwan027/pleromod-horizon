//! The foods that stand as a block and are eaten from it: the cakes, pizzas and cheese wheels
//! (`PortionFoodBlock.use`: a knife in the main hand cuts a portion off into the inventory, an
//! empty or other hand eats one; the last portion takes the block) and the pies
//! (`MultiBlockPieBlock.use`: each part is eaten whole).
//!
//! What a click does is what pleromod's extraction saw the block's own `use` do, run in every
//! state it asks about (`tools/mod-content/portions.py`, `data/portions.json`): the item given,
//! the item eaten, the block set and the sound played. Here the state is read from the world and
//! those steps done. Eating is `Item.finishUsingItem` as the mod's food items leave it to
//! Minecraft (`LivingEntity.eat` and `Player.eat`), with the food the extraction found.
use horizon_client::mod_api::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Default, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub blocks: HashMap<String, Block>,
    #[serde(default)]
    pub uses: Vec<Uses>,
    #[serde(default)]
    pub foods: HashMap<String, Food>,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub item: String,
    pub uses: usize,
}

#[derive(Debug, Deserialize)]
pub struct Uses {
    /// The tag of the items the use cuts with (`KNIVES`).
    #[serde(default)]
    pub knives: Option<String>,
    pub runs: Vec<Run>,
}

/// One run of `use`: the block's state and the player's, what it answered and what it did, in
/// order. `$block` and `$item` stand for the block and the item it holds.
#[derive(Debug, Deserialize)]
pub struct Run {
    pub block: HashMap<String, Value>,
    pub player: Player,
    pub answer: String,
    pub does: Vec<Vec<Value>>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct Player {
    pub main: bool,
    pub knife: bool,
    pub sneaking: bool,
    pub creative: bool,
    /// The food level: 20, full, or 19 for any less.
    pub food: i32,
    /// Whether `Inventory.add` takes what it is given.
    pub room: bool,
}

#[derive(Debug, Deserialize)]
pub struct Food {
    pub nutrition: i32,
    /// 26.3's saturation: 1.20.1's modifier times the nutrition times two, as `FoodData.eat` adds it.
    pub saturation: f32,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

#[derive(Debug, Deserialize)]
pub struct Effect {
    pub id: String,
    pub ticks: i32,
    pub amplifier: u8,
    pub chance: f32,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/portions.json")))
            .expect("portions.json is the extraction's")
    })
}

/// `SoundSource`'s constants in order.
const SOURCES: [&str; 10] = [
    "MASTER", "MUSIC", "RECORDS", "WEATHER", "BLOCKS", "HOSTILE", "NEUTRAL", "PLAYERS", "AMBIENT", "VOICE",
];
/// `SoundSource.NEUTRAL` and `PLAYERS`, which eating plays in.
const NEUTRAL: usize = 6;
const PLAYERS: usize = 7;
/// `GameType.CREATIVE`: the player whose abilities `instabuild`.
const CREATIVE: i32 = 1;

/// The player who clicks, as the use reads them.
pub struct Who {
    pub uuid: u128,
    pub sneaking: bool,
    /// `getAbilities().instabuild`.
    pub creative: bool,
    /// At their feet: where what they eat is heard.
    pub position: [f64; 3],
}

/// What the world around a click is, as the use reads it, and what it does to it.
pub trait World {
    fn player(&self) -> Option<Who>;
    fn property(&self, key: &str) -> Option<String>;
    fn held(&self, uuid: u128, main: bool) -> Option<ItemStack>;
    fn in_tag(&self, stack: &ItemStack, tag: &str) -> bool;
    fn food(&self, uuid: u128) -> Option<(i32, f32, f32)>;
    /// `Inventory.add`: what did not fit.
    fn give(&mut self, uuid: u128, stack: ItemStack) -> Option<ItemStack>;
    fn set_property(&mut self, key: &str, value: &str, flags: i32);
    fn set_block(&mut self, block: &str, flags: i32);
    /// A sound at `at`, or at the block's middle.
    fn sound(&mut self, sound: &str, at: Option<[f64; 3]>, source: usize, volume: f32, pitch: f32);
    fn set_food(&mut self, uuid: u128, food: (i32, f32, f32));
    fn add_effect(&mut self, uuid: u128, effect: NativeEffect);
    /// The level's random, `nextFloat`.
    fn float(&mut self) -> f32;
}

/// A click with one hand on the block at `pos`. False when the click passes on (to the other hand).
pub fn used(host: &mut dyn Host, pos: [i32; 3], main: bool) -> bool {
    let block = host.block(pos);
    let Some(found) = data().blocks.get(&block) else {
        return false;
    };
    used_in(&mut Level { host, pos }, found, main)
}

pub fn used_in(world: &mut dyn World, block: &Block, main: bool) -> bool {
    let data = data();
    let Some(uses) = data.uses.get(block.uses) else {
        return false;
    };
    let Some(player) = world.player() else {
        return false;
    };
    let (held, off) = (world.held(player.uuid, true), world.held(player.uuid, false));
    // `isSecondaryUseActive` with something in either hand: the game does not ask the block.
    if player.sneaking && (held.is_some() || off.is_some()) {
        return false;
    }
    let knife = match (&uses.knives, &held) {
        (Some(tag), Some(stack)) => world.in_tag(stack, tag),
        _ => false,
    };
    let creative = player.creative;
    let food = world.food(player.uuid).map_or(20, |f| f.0);
    let mut state = Player {
        main,
        knife,
        sneaking: player.sneaking,
        creative,
        food: if food >= 20 { 20 } else { 19 },
        room: true,
    };
    // The block's state, as the runs name it.
    let mut stands: HashMap<&str, Option<String>> = HashMap::new();
    for key in uses.runs.iter().flat_map(|r| r.block.keys()) {
        stands.entry(key).or_insert_with(|| world.property(key));
    }
    let matches = |run: &&Run, state: &Player| {
        run.player == *state
            && run.block.iter().all(|(key, value)| stands.get(key.as_str()).and_then(Option::as_deref) == Some(&*shown(value)))
    };
    let Some(mut run) = uses.runs.iter().find(|r| matches(r, &state)) else {
        return false;
    };
    let mut step = 0;
    while let Some(done) = run.does.get(step) {
        step += 1;
        let name = |at: usize| done.get(at).and_then(Value::as_str).map(|s| if s == "$item" { block.item.as_str() } else { s });
        let number = |at: usize| done.get(at).and_then(Value::as_i64).unwrap_or(0);
        match name(0) {
            Some("give") => {
                let Some(item) = name(1) else { continue };
                let left = world.give(player.uuid, ItemStack::new(item, number(2) as u32));
                // `Inventory.add`: what does not fit is still taken from a player in creative.
                if left.is_some_and(|l| l.count > 0) && !creative {
                    // The run in which it did not fit, from the same point on.
                    state.room = false;
                    let Some(other) = uses.runs.iter().find(|r| matches(r, &state)) else {
                        return false;
                    };
                    if other.does.get(..step) != run.does.get(..step) {
                        return false;
                    }
                    run = other;
                }
            }
            Some("eat") => {
                if let Some(food) = name(1).and_then(|item| data.foods.get(item)) {
                    eat(world, &player, food);
                }
            }
            Some("set") => {
                let Some(Value::Object(set)) = done.get(1) else { continue };
                let flags = number(2) as i32;
                match set.get("block").and_then(Value::as_str) {
                    Some("$block") => {
                        for (key, value) in set.iter().filter(|(key, _)| *key != "block") {
                            world.set_property(key, &shown(value), flags);
                        }
                    }
                    Some(other) => world.set_block(other, flags),
                    None => {}
                }
            }
            Some("sound") => {
                let Some(sound) = done.get(1) else { continue };
                let source = sound["source"].as_str().and_then(|s| SOURCES.iter().position(|n| *n == s));
                if let (Some(name), Some(source)) = (sound["sound"].as_str(), source) {
                    let level = |key: &str| sound[key].as_f64().unwrap_or(1.0) as f32;
                    world.sound(name, None, source, level("volume"), level("pitch"));
                }
            }
            _ => {}
        }
    }
    run.answer != "PASS"
}

/// A property's value as the world names it: `1` for 1, `true` for true.
fn shown(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

/// `Item.finishUsingItem` of a food: `Player.eat`, then `LivingEntity.eat`, on the server.
fn eat(world: &mut dyn World, player: &Who, food: &Food) {
    // FoodData.eat(nutrition, saturationModifier)
    if let Some((level, saturation, exhaustion)) = world.food(player.uuid) {
        let level = (level + food.nutrition).min(20);
        let saturation = (saturation + food.saturation).min(level as f32);
        world.set_food(player.uuid, (level, saturation, exhaustion));
    }
    // The burp, at the player's feet.
    let pitch = world.float() * 0.1 + 0.9;
    world.sound("minecraft:entity.player.burp", Some(player.position), PLAYERS, 0.5, pitch);
    // LivingEntity.eat: the eating sound (`Item.getEatingSound`), then the food's effects, each by its chance.
    let (a, b) = (world.float(), world.float());
    world.sound("minecraft:entity.generic.eat", Some(player.position), NEUTRAL, 1.0, 1.0 + (a - b) * 0.4);
    for effect in &food.effects {
        if world.float() < effect.chance {
            world.add_effect(
                player.uuid,
                NativeEffect {
                    id: effect.id.clone(),
                    amplifier: effect.amplifier,
                    duration: effect.ticks,
                    ambient: false,
                    particles: true,
                },
            );
        }
    }
}

struct Level<'a> {
    host: &'a mut dyn Host,
    pos: [i32; 3],
}

impl World for Level<'_> {
    fn player(&self) -> Option<Who> {
        self.host.acting_player().map(|p| Who {
            uuid: p.uuid,
            sneaking: p.sneaking,
            creative: p.game_mode == CREATIVE,
            position: p.position,
        })
    }
    fn property(&self, key: &str) -> Option<String> {
        self.host.block_property(self.pos, key)
    }
    fn held(&self, uuid: u128, main: bool) -> Option<ItemStack> {
        let stack = if main {
            self.host.held_stack(uuid)
        } else {
            self.host.player_equipment(uuid, EquipmentSlot::OffHand)
        };
        stack.filter(|s| s.count > 0)
    }
    fn in_tag(&self, stack: &ItemStack, tag: &str) -> bool {
        self.host.server_item_in_tag(stack, tag).unwrap_or(false)
    }
    fn food(&self, uuid: u128) -> Option<(i32, f32, f32)> {
        self.host.player_food(uuid)
    }
    fn give(&mut self, uuid: u128, stack: ItemStack) -> Option<ItemStack> {
        self.host.give_stack(uuid, stack)
    }
    fn set_property(&mut self, key: &str, value: &str, flags: i32) {
        self.host.set_block_property_with(self.pos, key, value, flags);
    }
    fn set_block(&mut self, block: &str, flags: i32) {
        if let Some(state) = self.host.default_state(block) {
            let dimension = self.host.dimension();
            self.host.set_block(&dimension, self.pos, state, flags);
        }
    }
    fn sound(&mut self, sound: &str, at: Option<[f64; 3]>, source: usize, volume: f32, pitch: f32) {
        self.host.play_sound(sound, at.unwrap_or_else(|| crate::at(self.pos)), source, volume, pitch);
    }
    fn set_food(&mut self, uuid: u128, food: (i32, f32, f32)) {
        self.host.set_player_status(uuid, None, Some(food), None);
    }
    fn add_effect(&mut self, uuid: u128, effect: NativeEffect) {
        self.host.add_effect(uuid, &effect);
    }
    fn float(&mut self) -> f32 {
        let dimension = self.host.dimension();
        self.host.level_random(&dimension).map_or(0.5, |random| random.next_float())
    }
}

#[cfg(test)]
mod tests;

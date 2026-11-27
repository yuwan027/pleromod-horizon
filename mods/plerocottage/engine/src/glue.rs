//! Between the game and the machines: a block entity's saved compound read into a `Unit` and
//! written back, and the world a machine reaches (`Env`) answered by the host.
use crate::machines::Kind;
use crate::shared::{Env, Unit};
use crate::spec::{self, Machine, Recipe2};
use crate::{JavaString, int};
use horizon_client::mod_api::*;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// `SoundSource.BLOCKS`.
const BLOCKS: usize = 4;

fn name(key: &str) -> JavaString {
    JavaString::from(key)
}

pub fn stack_tag(stack: &ItemStack) -> Tag {
    let mut map = Compound::new();
    map.insert(name("id"), Tag::String(JavaString::from(&*stack.item)));
    map.insert(name("count"), Tag::Int(stack.count as i32));
    if stack.damage > 0 {
        map.insert(name("damage"), Tag::Int(stack.damage as i32));
    }
    Tag::Compound(map)
}

fn tag_stack(tag: &Tag) -> Option<ItemStack> {
    let Tag::Compound(map) = tag else {
        return None;
    };
    let Some(Tag::String(id)) = map.get(&name("id")) else {
        return None;
    };
    let mut stack = ItemStack::new(&id.to_string_lossy(), int(map, "count").max(0) as u32);
    stack.damage = int(map, "damage").max(0) as u32;
    (stack.count > 0).then_some(stack)
}

/// The first tank of a mod block entity, as Horizon saves tanks (`fluids`).
pub fn tank_of(nbt: &Compound) -> Option<(String, i32)> {
    let Some(Tag::List(list)) = nbt.get(&name("fluids")) else {
        return None;
    };
    let Some(Tag::Compound(map)) = list.first() else {
        return None;
    };
    let Some(Tag::String(fluid)) = map.get(&name("Fluid")) else {
        return None;
    };
    let (fluid, amount) = (fluid.to_string_lossy(), int(map, "Amount"));
    (fluid != "minecraft:empty" && amount > 0).then_some((fluid, amount))
}

/// A block entity's fields, saved under the names the mod saves them by (`saveAdditional`); what
/// the mod keeps only in memory goes under names of its fields too.
pub fn load(kind: &dyn Kind, entity: &ModBlockEntity) -> Unit {
    let (spec, nbt) = (kind.spec(), &entity.nbt);
    let mut unit = kind.new_unit();
    unit.items = entity.items.clone();
    unit.items.resize(spec.size, None);
    if nbt.contains_key(&name("count")) {
        unit.running = int(nbt, "isRunning") != 0;
        unit.cooking = int(nbt, "cookingTime");
        unit.max = int(nbt, "maxCookingTime");
        unit.count = int(nbt, "count");
        unit.seen = match nbt.get(&name("seen")) {
            Some(Tag::Long(v)) => *v as u64,
            _ => 0,
        };
    }
    unit.pending = nbt.get(&name("output")).and_then(tag_stack);
    if let Some(Tag::List(list)) = nbt.get(&name("fryerCraftContainer")) {
        unit.originals = list.iter().map(tag_stack).collect();
    }
    if let Some(Tag::String(light)) = nbt.get(&name("recipeLightState")) {
        unit.light = Some(light.to_string_lossy());
    }
    unit.tank = tank_of(nbt);
    unit
}

pub fn store(kind: &dyn Kind, entity: &mut ModBlockEntity, unit: &Unit) {
    let spec = kind.spec();
    entity.items = unit.items.clone();
    let nbt = &mut entity.nbt;
    nbt.insert(name("isRunning"), Tag::Byte(unit.running.into()));
    nbt.insert(name("cookingTime"), Tag::Int(unit.cooking));
    nbt.insert(name("maxCookingTime"), Tag::Int(unit.max));
    nbt.insert(name("count"), Tag::Int(unit.count));
    nbt.insert(name("seen"), Tag::Long(unit.seen as i64));
    match &unit.pending {
        Some(stack) => nbt.insert(name("output"), stack_tag(stack)),
        None => nbt.remove(&name("output")),
    };
    if !unit.originals.is_empty() {
        let list = unit
            .originals
            .iter()
            .map(|s| s.as_ref().map_or_else(|| Tag::Compound(Compound::new()), stack_tag))
            .collect();
        nbt.insert(name("fryerCraftContainer"), Tag::List(list));
    }
    if let Some(light) = &unit.light {
        nbt.insert(name("recipeLightState"), Tag::String(JavaString::from(light.as_str())));
    }
    if spec.tank.is_some() {
        let mut tank = Compound::new();
        let (fluid, amount) = unit.tank.clone().unwrap_or(("minecraft:empty".to_owned(), 0));
        tank.insert(name("Fluid"), Tag::String(JavaString::from(fluid.as_str())));
        tank.insert(name("Amount"), Tag::Int(amount));
        nbt.insert(name("fluids"), Tag::List(vec![Tag::Compound(tank)]));
    }
    // The menu's data slots, by the field each says.
    let mut data = vec![0; spec.data_count()];
    for (field, &slot) in &spec.data_slots {
        data[slot] = match field.as_str() {
            "cookingTime" => unit.cooking,
            "maxCookingTime" => unit.max,
            _ => unit.count,
        };
    }
    entity.data = data;
}

/// Whether the block below `pos` heats the machine, as the mod's own test of it answered.
pub fn heated(host: &dyn Host, spec: &Machine, pos: [i32; 3]) -> bool {
    let below = [pos[0], pos[1] - 1, pos[2]];
    match spec.heat.get(&host.block(below)) {
        Some(Value::Bool(heats)) => *heats,
        Some(Value::Object(by)) => by
            .iter()
            .filter(|(property, _)| *property != "none")
            .find_map(|(property, answers)| {
                host.block_property(below, property)
                    .and_then(|value| answers[&value].as_bool())
            })
            .or_else(|| by.get("none").and_then(Value::as_bool))
            .unwrap_or(false),
        _ => false,
    }
}

/// The jar writes 1.20's ingredients ({"item"}, {"tag"}, or a list of them, any of which will
/// do); the host reads 26.3's: an id, a #tag, or a list of those.
fn modern(json: &Value) -> Value {
    match json {
        Value::Array(list) => list
            .iter()
            .flat_map(|j| match modern(j) {
                Value::Array(inner) => inner,
                one => vec![one],
            })
            .collect(),
        Value::Object(o) => match (o.get("item"), o.get("tag")) {
            (Some(item), _) => item.clone(),
            (_, Some(Value::String(tag))) => format!("#{tag}").into(),
            _ => Value::Null,
        },
        other => other.clone(),
    }
}

/// A recipe file of the mod's as its serializer reads it (`BaseRecipeSerializer`): `place` gives
/// the items an ingredient takes.
pub fn recipe(kind: &str, json: &Value, place: &dyn Fn(&Value) -> Vec<String>) -> Option<Recipe2> {
    let places = |key: &str| -> Vec<Vec<String>> {
        match json.get(key) {
            Some(Value::Array(list)) => list.iter().map(place).collect(),
            Some(one @ Value::Object(_)) => vec![place(one)],
            _ => Vec::new(),
        }
    };
    let stack = |value: &Value| -> Option<(String, u32)> {
        Some((
            value.get("item")?.as_str()?.to_owned(),
            value.get("count").and_then(Value::as_u64).unwrap_or(1) as u32,
        ))
    };
    let mut input = places("input");
    // Places that are told apart by where they are keep their empty ones.
    let padded = match kind {
        "cutting_board_multiple" => spec::cutting_board().size,
        "ice_cream" => 2,
        _ => 0,
    };
    while input.len() < padded {
        input.push(Vec::new());
    }
    Some(Recipe2 {
        file: String::new(),
        output: Some(stack(json.get("output")?)?),
        container: json.get("container").map(place).unwrap_or_default(),
        main_input: places("mainInput"),
        secondary_input: places("secondaryInput"),
        outgrowth: json.get("outgrowth").and_then(stack),
        outgrowth_chance: json.get("outgrowthChance").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        outgrowths: json
            .get("outgrowths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|o| Some((stack(o)?, o.get("chance")?.as_f64()? as f32)))
            .collect(),
        max_cooking_time: json.get("cookingTime").and_then(Value::as_i64).unwrap_or(0) as i32,
        heat_state: json.get("heatState").and_then(Value::as_str).map(str::to_uppercase),
        light_state: kind.strip_suffix("_fermentation").map(str::to_uppercase),
        // IceCreamRecipe: two flavours when it names a second one.
        is_tow_flavor: i32::from(kind == "ice_cream" && input.get(1).is_some_and(|p| !p.is_empty())),
        fluid_stack: json.get("fluid").and_then(|f| {
            Some(spec::Fluid {
                fluid: f.get("FluidName")?.as_str()?.to_owned(),
                amount: f.get("Amount")?.as_i64()? as i32,
            })
        }),
        tool: json.get("tool").map(place).unwrap_or_default(),
        is_shaped: i32::from(json.get("isShaped").and_then(Value::as_bool).unwrap_or(false)),
        input,
    })
}

/// The world around the machine at `pos`.
pub struct HostEnv<'a> {
    pub host: &'a mut dyn Host,
    pub pos: [i32; 3],
    /// What heats it and the smoking recipes it takes over.
    pub spec: Option<&'static Machine>,
    /// How the mod plays its sounds.
    levels: Option<&'static HashMap<String, spec::Level>>,
    recipes: HashMap<String, Rc<[Recipe2]>>,
}

impl<'a> HostEnv<'a> {
    pub fn new(host: &'a mut dyn Host, pos: [i32; 3], spec: Option<&'static Machine>) -> Self {
        Self {
            host,
            pos,
            spec,
            levels: Some(spec.map_or(&spec::cutting_board().sound_levels, |s| &s.sound_levels)),
            recipes: HashMap::new(),
        }
    }
}

impl Env for HostEnv<'_> {
    fn recipes(&mut self, kind: &str) -> Rc<[Recipe2]> {
        if let Some(found) = self.recipes.get(kind) {
            return found.clone();
        }
        let found: Rc<[Recipe2]> = if kind == "smoking" {
            self.spec.map(|s| s.smoking.clone()).unwrap_or_default().into()
        } else {
            let host = &*self.host;
            let place = |json: &Value| -> Vec<String> {
                host.recipe_ingredient(&modern(json))
                    .map(|i| i.order)
                    .unwrap_or_default()
            };
            host.recipes_of_type(&format!("plerocottage:{kind}"))
                .iter()
                .filter_map(|r| recipe(kind, &r.json, &place))
                .collect()
        };
        self.recipes.insert(kind.to_owned(), found.clone());
        found
    }

    fn heat(&mut self) -> bool {
        self.spec.is_some_and(|spec| heated(self.host, spec, self.pos))
    }

    fn light(&mut self) -> u8 {
        let (block, sky) = self.host.light(self.pos);
        block.max(sky)
    }

    fn next_float(&mut self) -> f32 {
        let dimension = self.host.dimension();
        self.host
            .level_random(dimension)
            .map_or(1.0, |random| random.next_float())
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        let dimension = self.host.dimension();
        self.host
            .level_random(dimension)
            .and_then(|random| random.next_int_bound(bound).ok())
            .unwrap_or(bound - 1)
    }

    fn sound(&mut self, sound: &str) {
        let sound = if sound.contains(':') {
            sound.to_owned()
        } else {
            format!("plerocottage:{sound}")
        };
        let at = self.pos.map(|v| f64::from(v) + 0.5);
        // As loud and as high as the mod's code plays it; a `playSound` without the two numbers plays at 1 and 1.
        let level = self.levels.and_then(|levels| levels.get(&sound)).copied().unwrap_or(spec::Level { volume: 1.0, pitch: 1.0 });
        self.host.play_sound(&sound, at, BLOCKS, level.volume, level.pitch);
    }

    fn pop(&mut self, stack: ItemStack) {
        // Block.popResourceFromFace(.., Direction.UP, ..): an item entity just above the block.
        let mut nbt = Compound::new();
        nbt.insert(name("Item"), stack_tag(&stack));
        let at = [
            f64::from(self.pos[0]) + 0.5,
            f64::from(self.pos[1]) + 1.1,
            f64::from(self.pos[2]) + 0.5,
        ];
        let dimension = self.host.dimension();
        self.host.summon(dimension, "minecraft:item", at, nbt);
    }

    fn property(&mut self, key: &str, value: bool) {
        self.host
            .set_block_property(self.pos, key, if value { "true" } else { "false" });
    }

    fn max_stack(&mut self, item: &str) -> u32 {
        self.host
            .max_stack_size(item)
            .unwrap_or_else(|| crate::shared::fact(item).map_or(64, |f| f.max_stack))
    }

    fn max_damage(&mut self, item: &str) -> u32 {
        let stack = ItemStack::new(item, 1);
        match self.host.component(&stack, "minecraft:max_damage") {
            Ok(Some(Tag::Int(max))) => (*max).max(0) as u32,
            Ok(None) => 0,
            _ => crate::shared::fact(item).map_or(0, |f| f.max_damage),
        }
    }
}

/// The cutting board's block entity: its six places, `craftIndex` and `inCraft`.
pub fn load_board(entity: &ModBlockEntity) -> Unit {
    let spec = spec::cutting_board();
    let mut unit = crate::machines::cutting_board::new_unit(spec);
    unit.items = entity.items.clone();
    unit.items.resize(spec.size, None);
    if entity.nbt.contains_key(&name("craftIndex")) {
        unit.count = int(&entity.nbt, "craftIndex");
        unit.flag = int(&entity.nbt, "inCraft") != 0;
    }
    unit
}

pub fn store_board(entity: &mut ModBlockEntity, unit: &Unit) {
    entity.items = unit.items.clone();
    entity.nbt.insert(name("craftIndex"), Tag::Int(unit.count));
    entity.nbt.insert(name("inCraft"), Tag::Byte(unit.flag.into()));
}

/// `CuttingBoardBlock.use`: a click on the board with one hand. False when the click passes to the other hand.
pub fn use_board(host: &mut dyn Host, pos: [i32; 3], main: bool) -> bool {
    use crate::machines::cutting_board::{self, Answer};
    let (Some(player), Some(entity)) = (host.acting_player(), host.mod_block_entity(pos)) else {
        return false;
    };
    let main_hand = host.held_stack(player.uuid);
    let off_hand = host.player_equipment(player.uuid, EquipmentSlot::OffHand);
    let (mut held, other) = if main {
        (main_hand.clone(), off_hand.clone())
    } else {
        (off_hand.clone(), main_hand.clone())
    };
    let before = held.clone();
    let was = load_board(&entity);
    let mut unit = was.clone();
    let answer = cutting_board::used(
        &mut unit,
        &mut HostEnv::new(host, pos, None),
        spec::cutting_board(),
        &mut held,
        &other,
        main,
        &main_hand,
        player.sneaking,
    );
    if answer == Answer::Pass {
        return false;
    }
    if unit != was {
        host.change_mod_block_entity(pos, &mut |entity| store_board(entity, &unit));
        host.block_entity_sync(pos);
    }
    if held != before {
        // The hand's stack: one laid on the board, or the tool worn.
        let slot = if main {
            usize::from(player.selected_slot)
        } else {
            40
        };
        let set = host.set_inventory_slot(player.uuid, NativeInventoryKind::Player, slot, before.as_ref(), held.clone());
        if let (Err(_), Some(was)) = (set, &before) {
            // The player's own state is in place while their click is handled: taken from their hand instead.
            let gone = was.count.saturating_sub(held.as_ref().map_or(0, |h| h.count));
            host.take_items(&was.item, gone);
        }
    }
    true
}

/// What clients get of a cutting board: its fields, and what lies in its places (`laid`), which its renderer draws.
pub fn board_tag(entity: &ModBlockEntity) -> Compound {
    let mut tag = entity.nbt.clone();
    let laid = entity
        .items
        .iter()
        .map(|slot| slot.as_ref().map_or_else(|| Tag::Compound(Compound::new()), stack_tag))
        .collect();
    tag.insert(name("laid"), Tag::List(laid));
    tag
}

/// `CuttingBoardBlockEntityRenderer.render`: the calls its pose stack got for the way the board faces, as the
/// extraction recorded them, with each thing drawn where the renderer draws it.
pub fn draw_board(host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
    let Some(Tag::List(laid)) = event.entity.nbt.get(&name("laid")) else {
        return;
    };
    let facing = host.block_state_property(event.state, "facing").unwrap_or("north").to_owned();
    let Some(calls) = spec::cutting_board().drawn.get(&facing) else {
        return;
    };
    let number = |call: &spec::Draw, at: usize| call.get(at).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let seed = event.pos[0].wrapping_mul(31).wrapping_add(event.pos[1]).wrapping_mul(31).wrapping_add(event.pos[2]);
    let draw = &mut *event.draw;
    let mut depth = 0;
    for call in calls {
        match call.first().and_then(Value::as_str) {
            Some("push") => {
                draw.push();
                depth += 1;
            }
            Some("pop") if depth > 0 => {
                draw.pop();
                depth -= 1;
            }
            Some("translate") => draw.translate([number(call, 1), number(call, 2), number(call, 3)]),
            Some("scale") => draw.scale([number(call, 1), number(call, 2), number(call, 3)]),
            Some("rotate") => {
                let axis = match call.get(1).and_then(Value::as_str) {
                    Some("x") => [1.0, 0.0, 0.0],
                    Some("y") => [0.0, 1.0, 0.0],
                    _ => [0.0, 0.0, 1.0],
                };
                draw.rotate_about(axis, number(call, 2), [0.0; 3]);
            }
            Some("item") => {
                let place = call.get(1).and_then(Value::as_u64).unwrap_or(0) as usize;
                let display = match call.get(2).and_then(Value::as_str) {
                    Some("ground") => DisplayContext::Ground,
                    _ => DisplayContext::Fixed,
                };
                if let Some(stack) = laid.get(place).and_then(tag_stack) {
                    host.draw_item(draw, &stack, display, None, seed.wrapping_add(place as i32));
                }
            }
            _ => {}
        }
    }
    for _ in 0..depth {
        draw.pop();
    }
}

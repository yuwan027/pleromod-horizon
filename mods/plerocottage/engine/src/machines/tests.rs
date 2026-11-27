//! Every run pleromod's extraction measured of the mod's own machines (`output/machines.json`: each
//! recipe once and in a batch of four, and what happens `otherwise`; native.py's runs beyond
//! those), made again here and compared.
use super::*;
use serde_json::{Value, json};
use std::rc::Rc;

pub fn measured() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/machines.json"
    )))
    .unwrap()
}

/// machines.py's world: no heat below unless a run asks, full light, every roll 0.
pub struct World {
    pub spec: &'static Spec,
    pub heat: bool,
    pub light: u8,
    pub roll: f32,
    pub sounds: Vec<String>,
    pub dropped: Vec<ItemStack>,
    pub properties: Vec<(String, bool)>,
    /// The cutting board's recipes rather than `spec`'s.
    pub board: bool,
}

impl World {
    pub fn new(spec: &'static Spec) -> Self {
        Self {
            spec,
            heat: false,
            light: 15,
            roll: 0.0,
            sounds: Vec::new(),
            dropped: Vec::new(),
            properties: Vec::new(),
            board: false,
        }
    }
}

impl Env for World {
    fn recipes(&mut self, kind: &str) -> Rc<[Recipe]> {
        if kind == "smoking" {
            return self.spec.smoking.clone().into();
        }
        if self.board {
            return spec::cutting_board().recipes[kind].clone().into();
        }
        self.spec.recipes[kind].clone().into()
    }
    fn heat(&mut self) -> bool {
        self.heat
    }
    fn light(&mut self) -> u8 {
        self.light
    }
    fn next_float(&mut self) -> f32 {
        self.roll
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        (self.roll * bound as f32) as i32
    }
    fn sound(&mut self, name: &str) {
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("plerocottage:{name}")
        };
        if !self.sounds.contains(&name) {
            self.sounds.push(name);
        }
    }
    fn pop(&mut self, stack: ItemStack) {
        self.dropped.push(stack);
    }
    fn property(&mut self, key: &str, value: bool) {
        self.properties.push((key.to_owned(), value));
    }
}

/// machines.py's `put`: the first item of each place into the slots in order, `count` of it
/// where it stacks so far.
pub fn put(unit: &mut Unit, slots: &[usize], places: &[Vec<String>], count: u32) {
    for (&slot, place) in slots.iter().zip(places) {
        if let Some(item) = place.first() {
            unit.items[slot] = Some(ItemStack::new(item, count.min(shared::fact(item).map_or(64, |f| f.max_stack))));
        }
    }
}

pub fn run_until_still(kind: &dyn Kind, unit: &mut Unit, world: &mut World) -> u32 {
    let mut ticks = 0;
    while unit.running {
        kind.tick(unit, world);
        ticks += 1;
        assert!(ticks < 200_000, "never done");
    }
    ticks
}

fn pairs(value: Option<&Value>) -> Vec<(String, u64)> {
    let mut out: Vec<_> = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].as_u64().unwrap()))
        .collect();
    out.sort();
    out
}

/// What machines.py's `left` reports: the results, what else lies there, what is worn, what
/// popped out, the sounds.
pub fn left(unit: &Unit, world: &World, outputs: &[usize]) -> Value {
    let show = |s: &ItemStack| json!([&*s.item, s.count]);
    let slots = || unit.items.iter().enumerate();
    json!({
        "made": outputs.iter().filter_map(|&o| unit.items[o].as_ref().map(show)).collect::<Vec<_>>(),
        "kept": slots().filter(|(i, _)| !outputs.contains(i)).filter_map(|(_, s)| s.as_ref().map(show)).collect::<Vec<_>>(),
        "damaged": slots().filter_map(|(_, s)| s.as_ref().filter(|s| s.damage > 0).map(|s| json!([&*s.item, s.damage]))).collect::<Vec<_>>(),
        "dropped": world.dropped.iter().map(show).collect::<Vec<_>>(),
        "sounds": world.sounds,
    })
}

pub fn compare(name: &str, expected: &Value, got: &Value, ticks: Option<u32>) -> Vec<String> {
    let mut wrong = Vec::new();
    if expected.get("ticks").and_then(Value::as_u64) != ticks.map(u64::from) {
        wrong.push(format!(
            "{name}: ticks {ticks:?}, measured {}",
            expected["ticks"]
        ));
    }
    for key in ["made", "kept", "damaged", "dropped"] {
        if pairs(got.get(key)) != pairs(expected.get(key)) {
            wrong.push(format!(
                "{name}: {key} {}, measured {}",
                got[key],
                expected.get(key).unwrap_or(&Value::Null)
            ));
        }
    }
    let sounds = |v: &Value| {
        let mut s: Vec<String> = v
            .get("sounds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect();
        s.sort();
        s
    };
    if sounds(got) != sounds(expected) {
        wrong.push(format!(
            "{name}: sounds {:?}, measured {:?}",
            sounds(got),
            sounds(expected)
        ));
    }
    wrong
}

/// One run of a recipe as machines.py makes it: the unit and world after it, the ticks it took
/// when the machine has a clock, and the slots its results lie in.
pub type Cook = fn(&Recipe, u32) -> (Unit, World, Option<u32>, Vec<usize>);

/// Every recipe of `kind` in `machines.json`, once and in a batch of four.
pub fn every_recipe(kind: &str, recipes: &[Recipe], cook: Cook, runs: usize, labels: &[(&str, u32)]) {
    let measured = measured();
    let list = measured["recipes"][kind].as_object().unwrap();
    let (mut wrong, mut made) = (Vec::new(), 0);
    for (file, run) in list {
        let recipe = recipes
            .iter()
            .find(|r| r.file == *file)
            .unwrap_or_else(|| panic!("{file} is not among the machine's recipes"));
        for &(label, count) in labels {
            let (unit, world, ticks, outputs) = cook(recipe, count);
            let got = left(&unit, &world, &outputs);
            wrong.extend(compare(&format!("{kind} {file} {label}"), &run[label], &got, ticks));
            made += 1;
        }
    }
    assert_eq!(made, runs, "machines.json's runs of {kind}");
    assert!(
        wrong.is_empty(),
        "{} of {made} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

pub const BOTH: &[(&str, u32)] = &[("one", 1), ("batch", 4)];

/// machines.py's `two_inputs`.
fn two(kind: &dyn Kind, recipe: &Recipe, count: u32) -> Unit {
    let spec = kind.spec();
    let mut unit = kind.new_unit();
    put(&mut unit, spec.slots("MAIN_INPUT_SLOTS"), &recipe.main_input, count);
    put(
        &mut unit,
        spec.slots("SECONDARY_INPUT_SLOTS"),
        &recipe.secondary_input,
        count,
    );
    put(&mut unit, &[spec.slot("CONTAINER")], &[recipe.container.clone()], 64);
    unit
}

fn blend(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&blender::Blender, spec::blender());
    let (mut unit, mut world) = (two(kind, recipe, count), World::new(spec));
    kind.packet(&mut unit, &mut world, "BlenderStartPacket");
    let ticks = run_until_still(kind, &mut unit, &mut world);
    let outputs = vec![spec.slot("MAIN_OUTPUT"), spec.slot("SECONDARY_OUTPUT")];
    (unit, world, Some(ticks), outputs)
}

#[test]
fn the_blender_makes_every_recipe_as_the_mod_does() {
    every_recipe("blender", &spec::blender().recipes["blender"], blend, 62, BOTH);
}

fn stock(recipe: &Recipe, count: u32, heat: bool) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&stock_pot::StockPot, spec::stock_pot());
    let (mut unit, mut world) = (two(kind, recipe, count), World::new(spec));
    world.heat = heat;
    kind.packet(&mut unit, &mut world, "StockPotStartPacket");
    let ticks = run_until_still(kind, &mut unit, &mut world);
    (unit, world, Some(ticks), vec![spec.slot("RESULT")])
}

#[test]
fn the_stock_pot_makes_every_recipe_as_the_mod_does() {
    every_recipe(
        "stock_pot",
        &spec::stock_pot().recipes["stock_pot"],
        |r, c| stock(r, c, true),
        18,
        BOTH,
    );
}

fn brew(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&teapot::Teapot, spec::teapot());
    let (mut unit, mut world) = (kind.new_unit(), World::new(spec));
    let slots = ["DRINK", "MATERIAL", "TOP_LEFT", "TOP_RIGHT", "BOTTOM_LEFT", "BOTTOM_RIGHT"]
        .map(|name| spec.slot(name));
    put(&mut unit, &slots, &recipe.input, count);
    put(&mut unit, &[spec.slot("CONTAINER")], &[recipe.container.clone()], 64);
    kind.packet(&mut unit, &mut world, "BrewingStartPacket");
    let ticks = run_until_still(kind, &mut unit, &mut world);
    (unit, world, Some(ticks), vec![spec.slot("RESULT")])
}

#[test]
fn the_teapot_makes_every_recipe_as_the_mod_does() {
    every_recipe("teapot", &spec::teapot().recipes["teapot"], brew, 78, BOTH);
}

fn whisk(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&saucepan::Saucepan, spec::saucepan_and_whisk());
    let (mut unit, mut world) = (two(kind, recipe, count), World::new(spec));
    world.heat = recipe.heat_state.as_deref() == Some("NEED");
    saucepan::craft(&mut unit, &mut world, spec);
    (unit, world, None, vec![spec.slot("RESULT")])
}

#[test]
fn the_saucepan_makes_every_recipe_as_the_mod_does() {
    every_recipe(
        "saucepan_and_whisk",
        &spec::saucepan_and_whisk().recipes["saucepan_and_whisk"],
        whisk,
        130,
        BOTH,
    );
}

#[test]
fn without_heat_the_stock_pot_stands_and_the_teapot_brews() {
    let measured = measured();
    let without = &measured["otherwise"]["without heat"];
    let mut wrong = Vec::new();
    let (unit, world, ticks, outputs) = stock(&spec::stock_pot().recipes["stock_pot"][0], 1, false);
    wrong.extend(compare(
        "stock pot",
        &without["stock_pot"],
        &left(&unit, &world, &outputs),
        ticks,
    ));
    let (unit, world, ticks, outputs) = brew(&spec::teapot().recipes["teapot"][0], 1);
    wrong.extend(compare(
        "teapot",
        &without["teapot"],
        &left(&unit, &world, &outputs),
        ticks,
    ));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

const OIL: &str = "plerocottage:olive_oil";

/// machines.py's `start_fryer`: `count` of the recipe's ingredient in the first basket, so much oil.
fn fryer_with(recipe: &Recipe, count: u32, oil: i32, roll: f32) -> (Unit, World) {
    let (kind, spec) = (&fryer::Fryer, spec::fryer());
    let (mut unit, mut world) = (kind.new_unit(), World::new(spec));
    world.roll = roll;
    unit.tank = (oil > 0).then(|| (OIL.to_owned(), oil));
    put(&mut unit, spec.slots("INPUT_SLOTS"), &recipe.input, count);
    kind.packet(&mut unit, &mut world, "FryingStartPacket");
    (unit, world)
}

/// It goes on frying until everything is burnt: looked at as soon as the result is there.
fn fry(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&fryer::Fryer, spec::fryer());
    let (mut unit, mut world) = fryer_with(recipe, count, 1000, 0.0);
    let (slot, wanted) = (spec.slots("INPUT_SLOTS")[0], &output(recipe).0);
    let mut ticks = 0;
    while unit.running && unit.items[slot].as_ref().is_none_or(|s| *s.item != **wanted) {
        kind.tick(&mut unit, &mut world);
        ticks += 1;
    }
    (unit, world, Some(ticks), vec![slot])
}

#[test]
fn the_fryer_fries_every_recipe_as_the_mod_does() {
    let spec = spec::fryer();
    every_recipe("fast_fry", &spec.recipes["fast_fry"], fry, 38, BOTH);
    every_recipe("slow_fry", &spec.recipes["slow_fry"], fry, 36, BOTH);
    every_recipe("smoking in fast_fry", &spec.smoking, fry, 98, BOTH);
}

#[test]
fn the_fryer_burns_what_stays_in_it_and_needs_oil() {
    let (measured, spec) = (measured(), spec::fryer());
    let otherwise = &measured["otherwise"];
    let first = &spec.recipes["fast_fry"][0];
    let (mut unit, mut world) = fryer_with(first, 1, 1000, 0.999);
    let ticks = run_until_still(&fryer::Fryer, &mut unit, &mut world);
    let wrong = compare(
        "until it stops",
        &otherwise["fryer until it stops"],
        &left(&unit, &world, spec.slots("INPUT_SLOTS")),
        Some(ticks),
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let little = otherwise["fryer with little oil"].as_object().unwrap();
    assert_eq!(little.len(), 4);
    for (oil, runs) in little {
        let (unit, _) = fryer_with(first, 1, oil.parse().unwrap(), 0.999);
        assert_eq!(Some(unit.running), runs.as_bool(), "{oil} of oil");
    }
    // By how much lies in it: the share of the rolls of ten thousand that cost oil, and how much.
    let chances = otherwise["chance that frying uses oil"].as_object().unwrap();
    assert_eq!(chances.len(), 4);
    for (count, run) in chances {
        let edge = (run["chance"].as_f64().unwrap() * 10000.0).round() as i32;
        let used = |roll: i32| {
            let (unit, _) = fryer_with(first, count.parse().unwrap(), 1000, (roll as f32 + 0.5) / 10000.0);
            1000 - unit.tank_amount()
        };
        assert_eq!(i64::from(used(edge - 1)), run["used"].as_i64().unwrap(), "{count} in it");
        if edge < 10000 {
            assert_eq!(used(edge), 0, "{count} in it, past its chance");
        }
    }
}

fn scoop(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&ice_cream::IceCream, spec::ice_cream_machine());
    let (mut unit, mut world) = (kind.new_unit(), World::new(spec));
    unit.tank = recipe.fluid_stack.as_ref().map(|f| (f.fluid.clone(), 3000));
    put(&mut unit, spec.slots("MATERIAL_SLOTS"), &recipe.input, count);
    put(&mut unit, &[spec.slot("CONTAINER_SLOT")], &[recipe.container.clone()], count);
    kind.packet(&mut unit, &mut world, "IceCreamCraftPacket");
    (unit, world, None, vec![spec.slot("OUTPUT_SLOT")])
}

#[test]
fn the_ice_cream_machine_makes_every_recipe_as_the_mod_does() {
    every_recipe("ice_cream", &spec::ice_cream_machine().recipes["ice_cream"], scoop, 18, BOTH);
}

fn light_of(recipe: &Recipe) -> u8 {
    match recipe.light_state.as_deref() {
        Some("DIM") => 0,
        Some("NORMAL") => 8,
        _ => 15,
    }
}

/// machines.py's `fermenter`, with a catalyst in the top slot and the light it ends in.
fn ferment_with(recipe: &Recipe, count: u32, catalyst: Option<&str>, ends_in: Option<u8>) -> (Unit, World, Option<u32>, Vec<usize>) {
    let (kind, spec) = (&fermenter::Fermenter, spec::fermenter());
    let (mut unit, mut world) = (kind.new_unit(), World::new(spec));
    world.light = light_of(recipe);
    put(&mut unit, spec.slots("MATERIAL_SLOTS"), &recipe.input, count);
    put(&mut unit, &[spec.slot("CONTAINER")], &[recipe.container.clone()], 64);
    if let Some(catalyst) = catalyst {
        unit.items[spec.slot("CATALYSTS_TOP")] = Some(ItemStack::new(catalyst, 1));
    }
    kind.packet(&mut unit, &mut world, "FermentationStartPacket");
    world.light = ends_in.unwrap_or(world.light);
    let ticks = run_until_still(kind, &mut unit, &mut world);
    (unit, world, Some(ticks), vec![spec.slot("OUTPUT")])
}

fn ferment(recipe: &Recipe, count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    ferment_with(recipe, count, None, None)
}

#[test]
fn the_fermenter_makes_every_recipe_as_the_mod_does() {
    let spec = spec::fermenter();
    every_recipe("dim_fermentation", &spec.recipes["dim_fermentation"], ferment, 20, BOTH);
    every_recipe("normal_fermentation", &spec.recipes["normal_fermentation"], ferment, 36, BOTH);
    every_recipe("bright_fermentation", &spec.recipes["bright_fermentation"], ferment, 18, BOTH);
}

#[test]
fn a_catalyst_shortens_fermenting_and_another_light_spoils_it() {
    let (measured, spec) = (measured(), spec::fermenter());
    let normal = &spec.recipes["normal_fermentation"];
    let catalysts = measured["otherwise"]["fermenter with a catalyst"].as_object().unwrap();
    assert_eq!(catalysts.len(), 4);
    for (item, ticks) in catalysts {
        let (_, _, took, _) = ferment_with(&normal[0], 1, (item != "none").then_some(item), None);
        assert_eq!(took.map(u64::from), ticks.as_u64(), "{item}");
    }
    // machines.py's `other_light`: the first recipe of each length.
    let lights = measured["otherwise"]["fermenter in another light"].as_object().unwrap();
    let mut wrong = Vec::new();
    for (seconds, run) in lights {
        let recipe = normal
            .iter()
            .find(|r| r.max_cooking_time.to_string() == *seconds)
            .unwrap();
        let (unit, world, ticks, outputs) = ferment_with(recipe, 1, None, Some(15));
        wrong.extend(compare(seconds, run, &left(&unit, &world, &outputs), ticks));
    }
    assert_eq!(lights.len(), 3);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// machines.py's `cutting_board`: one of each place's first item on the board, the recipe's first tool in hand.
fn cut(recipe: &Recipe, _count: u32) -> (Unit, World, Option<u32>, Vec<usize>) {
    let spec = spec::cutting_board();
    let mut unit = cutting_board::new_unit(spec);
    let mut world = World::new(spec::blender());
    world.board = true;
    let every: Vec<usize> = (0..spec.size).collect();
    put(&mut unit, &every, &recipe.input, 1);
    let mut tool = recipe.tool.first().map(|t| ItemStack::new(t, 1));
    if recipe.file.contains("cutting_board_single") {
        cutting_board::craft_single(&mut unit, &mut world, spec, &mut tool);
    } else {
        cutting_board::craft_multiple(&mut unit, &mut world, spec, &mut tool);
    }
    // machines.py's `left` is asked with no result slot: everything lying there would be `kept`.
    (unit, world, None, Vec::new())
}

#[test]
fn the_cutting_board_makes_every_recipe_as_the_mod_does() {
    let spec = spec::cutting_board();
    const ONE: &[(&str, u32)] = &[("one", 1)];
    every_recipe("cutting_board_single", &spec.recipes["cutting_board_single"], cut, 103, ONE);
    every_recipe("cutting_board_multiple", &spec.recipes["cutting_board_multiple"], cut, 27, ONE);
}

#[test]
fn the_shaker_plans_every_recipe_as_the_mod_does() {
    // machines.py's `shaker`: `count` of each place's first item in the shaker's slots, a stack of the cup in the other hand.
    let (measured, spec) = (measured(), spec::shaker());
    let list = measured["recipes"]["shaker"].as_object().unwrap();
    let (mut wrong, mut runs) = (Vec::new(), 0);
    for (file, run) in list {
        let recipe = spec.recipes["shaker"].iter().find(|r| r.file == *file).unwrap();
        for (label, count) in BOTH {
            let contents: Vec<Option<ItemStack>> = recipe
                .input
                .iter()
                .map(|place| place.first().map(|item| ItemStack::new(item, (*count).min(shared::fact(item).map_or(64, |f| f.max_stack)))))
                .collect();
            let offhand = recipe.container.first().map(|item| ItemStack::new(item, 64));
            let plan = shaker::plan(recipe, &contents, &offhand, spec, &mut World::new(spec::blender()));
            let got = plan.map(|p| json!({"batches": p.batches, "used": p.used}));
            if got.as_ref() != Some(&run[*label]) {
                wrong.push(format!("{file} {label}: {got:?}, measured {}", run[*label]));
            }
            runs += 1;
        }
    }
    assert_eq!(runs, 28);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_shake_uses_what_the_mods_plan_uses_and_fills_the_other_hand() {
    // The same runs of machines.json, shaken (`ShakerItem.craft`): every place gives up what the plan uses of it,
    // the result takes the cups' place, the cups not used come back, and a recipe without cups gives its result.
    let (measured, spec) = (measured(), spec::shaker());
    let recipes = &spec.recipes["shaker"];
    let list = measured["recipes"]["shaker"].as_object().unwrap();
    let (mut wrong, mut runs) = (Vec::new(), 0);
    for (file, run) in list {
        let recipe = recipes.iter().find(|r| r.file == *file).unwrap();
        for (label, count) in BOTH {
            let world = &mut World::new(spec::blender());
            let contents: Vec<Option<ItemStack>> = recipe
                .input
                .iter()
                .map(|place| place.first().map(|item| ItemStack::new(item, (*count).min(shared::fact(item).map_or(64, |f| f.max_stack)))))
                .collect();
            let offhand = recipe.container.first().map(|item| ItemStack::new(item, 64));
            // Only this recipe is known: several take the same things, and the game's order is the world's.
            let Some(shaken) = shaker::craft(std::slice::from_ref(recipe), &contents, &offhand, spec, world) else {
                wrong.push(format!("{file} {label}: nothing made"));
                continue;
            };
            let batches = run[*label]["batches"].as_u64().unwrap() as u32;
            let (item, each) = recipe.output.clone().unwrap();
            let made = ItemStack::new(&item, each * batches);
            for (slot, used) in run[*label]["used"].as_array().unwrap().iter().enumerate() {
                let used = used.as_u64().unwrap() as u32;
                let Some(before) = &contents[slot] else {
                    continue;
                };
                let worn = shared::is(&before.item, "DamageableMaterial");
                let left = shaken.contents[slot].as_ref();
                // What a used-up thing leaves lies in its place.
                let after = match left {
                    Some(l) if l.item == before.item => if worn { l.damage } else { l.count },
                    _ => 0,
                };
                let expected = if worn { used % world.max_damage(&before.item).max(1) } else { before.count - used };
                if after != expected {
                    wrong.push(format!("{file} {label}: place {slot} holds {left:?}, {used} used of {before:?}"));
                }
            }
            let got_made = if recipe.container.is_empty() { shaken.given.first() } else { shaken.offhand.as_ref() };
            if got_made != Some(&made) {
                wrong.push(format!("{file} {label}: made {got_made:?}, the plan makes {made:?}"));
            }
            if let Some(cups) = &offhand {
                let unused = shaken.given.iter().find(|s| s.item == cups.item).map_or(0, |s| s.count);
                if unused != cups.count - batches {
                    wrong.push(format!("{file} {label}: {unused} cups back of {}", cups.count));
                }
            }
            runs += 1;
        }
    }
    assert_eq!(runs, 28);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_shaker_fills_and_empties_by_its_places_limit() {
    // `overrideStackedOnOther` and `overrideOtherStackedOnMe`, with the item's own numbers.
    let spec = spec::shaker();
    let (size, limit) = (spec.constants["ITEMS_SIZE"] as usize, shaker::limit(spec));
    let world = &mut World::new(spec::blender());
    let item = &spec.recipes["shaker"][0].input[0][0];
    let other = spec.recipes["shaker"].iter().flat_map(|r| r.input.iter().flatten()).find(|i| *i != item).unwrap();
    let mut held = vec![None; size];
    // A right click with the shaker on a stack takes a place's worth of it, then tops nothing up.
    let mut slot = Some(ItemStack::new(item, limit + 3));
    assert!(shaker::onto_slot(&mut held, &mut slot, limit, world));
    assert_eq!((held[0].as_ref().map(|s| s.count), slot.as_ref().map(|s| s.count)), (Some(limit), Some(3)));
    // The next click starts the next place; another thing goes into the place after.
    assert!(shaker::onto_slot(&mut held, &mut slot, limit, world));
    assert_eq!((held[1].as_ref().map(|s| s.count), slot), (Some(3), None));
    let mut carried = Some(ItemStack::new(other, 1));
    assert!(shaker::onto_shaker(&mut held, &mut carried, limit, world));
    assert_eq!((held[2].as_ref().map(|s| &*s.item), carried.clone()), (Some(other.as_str()), None));
    // More of the first thing tops up its place that has room.
    carried = Some(ItemStack::new(item, 5));
    assert!(shaker::onto_shaker(&mut held, &mut carried, limit, world));
    assert_eq!((held[1].as_ref().map(|s| s.count), carried.as_ref().map(|s| s.count)), (Some(limit), Some(4)));
    // An empty cursor takes the last thing out, place by place, until nothing is left.
    carried = None;
    assert!(shaker::onto_shaker(&mut held, &mut carried, limit, world));
    assert_eq!(carried.as_ref().map(|s| &*s.item), Some(other.as_str()));
    assert_eq!(shaker::extraction_slot(&held), Some(1));
    // A full shaker takes nothing more.
    let mut full: Vec<Option<ItemStack>> = (0..size).map(|_| Some(ItemStack::new(item, limit))).collect();
    let mut more = Some(ItemStack::new(item, 1));
    assert!(!shaker::onto_slot(&mut full, &mut more, limit, world));
    assert_eq!(more.map(|s| s.count), Some(1));
    held.iter_mut().for_each(|place| *place = None);
    assert!(!shaker::onto_shaker(&mut held, &mut None, limit, world));
}

#[test]
fn the_runs_beyond_the_recipes_end_as_the_mods_do() {
    // native.py's `machine_runs`: an empty machine, a taken result, a missing ingredient, uneven and full stacks, too
    // few containers, no heat.
    let (mut wrong, mut runs) = (Vec::new(), 0);
    for kind in all() {
        let spec = kind.spec();
        for (name, run) in &spec.runs {
            let name = format!("{} {name}", spec.block);
            let (mut unit, mut world) = (kind.new_unit(), World::new(spec));
            for (slot, stack) in run["items"].as_object().unwrap() {
                let mut laid = ItemStack::new(stack[0].as_str().unwrap(), stack[1].as_u64().unwrap() as u32);
                laid.damage = stack.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
                unit.items[slot.parse::<usize>().unwrap()] = Some(laid);
            }
            world.heat = run["heat"].as_bool().unwrap();
            world.light = run["light"].as_u64().unwrap() as u8;
            if let Some(tank) = run.get("tank") {
                unit.tank = tank[0].as_str().map(|fluid| (fluid.to_owned(), tank[1].as_i64().unwrap() as i32));
            }
            match run["packet"].as_str().unwrap() {
                // The saucepan's whisk makes the recipe at its last stir: the run asks for that alone.
                "craftItem" => saucepan::craft(&mut unit, &mut world, spec),
                packet => kind.packet(&mut unit, &mut world, packet),
            }
            let started = unit.running;
            let ticks = run_until_still(kind, &mut unit, &mut world);
            if Some(started) != run["started"].as_bool() || Some(u64::from(ticks)) != run["ticks"].as_u64() {
                wrong.push(format!("{name}: started {started}, {ticks} ticks, measured {} and {}", run["started"], run["ticks"]));
            }
            let after: serde_json::Map<String, Value> = unit
                .items
                .iter()
                .enumerate()
                .filter_map(|(slot, stack)| {
                    let stack = stack.as_ref()?;
                    let mut shown = vec![json!(&*stack.item), json!(stack.count)];
                    if stack.damage > 0 {
                        shown.push(json!(stack.damage));
                    }
                    Some((slot.to_string(), Value::Array(shown)))
                })
                .collect();
            if Value::Object(after.clone()) != run["after"] {
                wrong.push(format!("{name}: slots {}\n  measured {}", Value::Object(after), run["after"]));
            }
            let got = left(&unit, &world, &[]);
            let expected = json!({"dropped": run["dropped"], "sounds": run["sounds"], "kept": got["kept"], "damaged": got["damaged"]});
            wrong.extend(compare(&name, &expected, &got, None));
            if let Some(tank) = run.get("tankAfter") {
                if tank.as_i64() != Some(i64::from(unit.tank_amount())) {
                    wrong.push(format!("{name}: {} in the tank, measured {tank}", unit.tank_amount()));
                }
            }
            runs += 1;
        }
    }
    assert!(runs >= 56, "native.py's runs: {runs}");
    assert!(wrong.is_empty(), "{} of {runs} differ:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn clicks_on_the_cutting_board_answer_as_the_mods_block_does() {
    // native.py's `board_uses`: the block's own `use`, the main hand first and the other when that passes.
    use cutting_board::Answer;
    let spec = spec::cutting_board();
    let stack = |value: &Value| -> Option<ItemStack> {
        let value = value.as_array()?;
        let mut stack = ItemStack::new(value[0].as_str()?, value[1].as_u64()? as u32);
        stack.damage = value.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
        Some(stack)
    };
    let show = |stack: &Option<ItemStack>| match stack {
        Some(s) if s.damage > 0 => json!([&*s.item, s.count, s.damage]),
        Some(s) => json!([&*s.item, s.count]),
        None => Value::Null,
    };
    let (mut wrong, mut clicks) = (Vec::new(), 0);
    for (name, case) in &spec.uses {
        let mut unit = cutting_board::new_unit(spec);
        for (slot, item) in case["board"].as_array().unwrap().iter().enumerate() {
            unit.items[slot] = Some(ItemStack::new(item.as_str().unwrap(), 1));
        }
        let (mut main, mut other) = (stack(&case["main"]), stack(&case["other"]));
        let sneaking = case["sneaking"].as_bool().unwrap();
        for (number, click) in case["clicks"].as_array().unwrap().iter().enumerate() {
            let mut world = World::new(spec::blender());
            world.board = true;
            let mut answers = Vec::new();
            let before = main.clone();
            let answer = cutting_board::used(&mut unit, &mut world, spec, &mut main, &other, true, &before, sneaking);
            answers.push(answer);
            if answer == Answer::Pass {
                let held = main.clone();
                answers.push(cutting_board::used(&mut unit, &mut world, spec, &mut other, &held, false, &held, sneaking));
            }
            let named: Vec<&str> = answers
                .iter()
                .map(|a| match a {
                    Answer::Pass => "PASS",
                    Answer::Success => "SUCCESS",
                    Answer::ConsumePartial => "CONSUME_PARTIAL",
                })
                .collect();
            let got = json!({
                "answers": named,
                "board": unit.items.iter().map(show).collect::<Vec<_>>(),
                "main": show(&main),
                "other": show(&other),
                "dropped": world.dropped.iter().map(|s| show(&Some(s.clone()))).collect::<Vec<_>>(),
                "sounds": world.sounds,
                "craftIndex": unit.count,
                "inCraft": unit.flag,
            });
            if got != *click {
                wrong.push(format!("{name} click {number}:\n   {got}\n   measured {click}"));
            }
            clicks += 1;
        }
    }
    assert!(clicks >= 28, "native.py's clicks: {clicks}");
    assert!(wrong.is_empty(), "{} of {clicks} differ:\n{}", wrong.len(), wrong.join("\n"));
}

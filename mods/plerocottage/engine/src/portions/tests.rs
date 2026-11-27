use super::*;

const UUID: u128 = 7;

#[derive(Default)]
struct Table {
    state: HashMap<String, String>,
    block: Option<String>,
    main: Option<ItemStack>,
    off: Option<ItemStack>,
    sneaking: bool,
    creative: bool,
    food: (i32, f32, f32),
    full: bool,
    given: Vec<String>,
    sounds: Vec<(String, Option<[f64; 3]>, usize)>,
    effects: Vec<NativeEffect>,
    flags: Vec<i32>,
}

impl World for Table {
    fn player(&self) -> Option<Who> {
        Some(Who { uuid: UUID, sneaking: self.sneaking, creative: self.creative, position: [1.0, 2.0, 3.0] })
    }
    fn property(&self, key: &str) -> Option<String> {
        self.state.get(key).cloned()
    }
    fn held(&self, _uuid: u128, main: bool) -> Option<ItemStack> {
        if main { self.main.clone() } else { self.off.clone() }
    }
    fn in_tag(&self, stack: &ItemStack, tag: &str) -> bool {
        tag == "c:tools/knives" && stack.item.ends_with("_knife")
    }
    fn food(&self, _uuid: u128) -> Option<(i32, f32, f32)> {
        Some(self.food)
    }
    fn give(&mut self, _uuid: u128, stack: ItemStack) -> Option<ItemStack> {
        if self.full {
            return Some(stack);
        }
        self.given.push(stack.item.to_string());
        None
    }
    fn set_property(&mut self, key: &str, value: &str, flags: i32) {
        self.state.insert(key.to_owned(), value.to_owned());
        self.flags.push(flags);
    }
    fn set_block(&mut self, block: &str, flags: i32) {
        self.block = Some(block.to_owned());
        self.flags.push(flags);
    }
    fn sound(&mut self, sound: &str, at: Option<[f64; 3]>, source: usize, _volume: f32, _pitch: f32) {
        self.sounds.push((sound.to_owned(), at, source));
    }
    fn set_food(&mut self, _uuid: u128, food: (i32, f32, f32)) {
        self.food = food;
    }
    fn add_effect(&mut self, _uuid: u128, effect: NativeEffect) {
        self.effects.push(effect);
    }
    fn float(&mut self) -> f32 {
        0.5
    }
}

fn block(name: &str) -> &'static Block {
    &data().blocks[name]
}

fn table(bites: &str) -> Table {
    Table {
        state: HashMap::from([("blockstate".to_owned(), bites.to_owned())]),
        food: (20, 5.0, 0.0),
        ..Table::default()
    }
}

#[test]
fn every_cake_pizza_cheese_wheel_and_pie_holds_its_food() {
    let data = data();
    assert_eq!(data.blocks.len(), 18);
    for (name, found) in &data.blocks {
        let food = &data.foods[&found.item];
        assert!(food.nutrition > 0, "{name}");
        assert!(!data.uses[found.uses].runs.is_empty());
    }
}

#[test]
fn a_knife_cuts_a_portion_into_the_inventory_and_the_last_takes_the_block() {
    let cake = block("plerocottage:caramel_chocolate_cake");
    let mut world = table("0");
    world.main = Some(ItemStack::new("plerocottage:iron_knife", 1));
    for bites in ["1", "2", "3"] {
        assert!(used_in(&mut world, cake, true));
        assert_eq!(world.state["blockstate"], bites);
    }
    assert!(used_in(&mut world, cake, true));
    assert_eq!(world.block.as_deref(), Some("minecraft:air"));
    assert_eq!(world.given, vec![cake.item.clone(); 4]);
    assert_eq!(world.flags, [3; 4]);
    // Heard at the block, in NEUTRAL; nothing eaten.
    assert!(world.sounds.iter().all(|(_, at, source)| at.is_none() && *source == NEUTRAL));
    assert_eq!(world.food, (20, 5.0, 0.0));
}

#[test]
fn with_no_room_the_knife_cuts_nothing_and_the_other_hand_eats() {
    let pizza = block("plerocottage:vanilla_pizza");
    let mut world = table("1");
    world.main = Some(ItemStack::new("plerocottage:iron_knife", 1));
    world.full = true;
    world.food = (10, 0.0, 0.0);
    assert!(!used_in(&mut world, pizza, true));
    assert_eq!(world.state["blockstate"], "1");
    assert!(world.sounds.is_empty());
    assert!(used_in(&mut world, pizza, false));
    assert_eq!(world.state["blockstate"], "2");
    let food = &data().foods[&pizza.item];
    assert_eq!(world.food.0, 10 + food.nutrition);
}

#[test]
fn a_hungry_player_eats_a_portion_with_its_effects() {
    let (name, cake) = data()
        .blocks
        .iter()
        .find(|(name, b)| name.ends_with("_cake") && !data().foods[&b.item].effects.is_empty())
        .unwrap();
    let mut world = table("2");
    world.food = (15, 1.0, 0.5);
    assert!(used_in(&mut world, cake, true), "{name}");
    assert_eq!(world.state["blockstate"], "3");
    let food = &data().foods[&cake.item];
    assert_eq!(world.food, ((15 + food.nutrition).min(20), (1.0 + food.saturation).min((15 + food.nutrition).min(20) as f32), 0.5));
    let heard: Vec<_> = world.sounds.iter().map(|(s, at, source)| (s.as_str(), at.is_some(), *source)).collect();
    assert_eq!(heard, [("minecraft:entity.player.burp", true, PLAYERS), ("minecraft:entity.generic.eat", true, NEUTRAL)]);
    let given: Vec<_> = food.effects.iter().filter(|e| e.chance > 0.5).map(|e| e.id.as_str()).collect();
    assert_eq!(world.effects.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), given);
}

#[test]
fn a_full_player_eats_only_in_creative_and_sneaking_passes() {
    let cheese = block("plerocottage:cheddar_cheese_wheel");
    let mut world = table("0");
    assert!(!used_in(&mut world, cheese, true));
    world.sneaking = true;
    world.creative = true;
    assert!(!used_in(&mut world, cheese, true));
    world.sneaking = false;
    assert!(used_in(&mut world, cheese, true));
    assert_eq!(world.state["blockstate"], "1");
    // Sneaking with something in a hand: the block is not asked.
    world.sneaking = true;
    world.off = Some(ItemStack::new("minecraft:dirt", 1));
    world.food = (1, 0.0, 0.0);
    assert!(!used_in(&mut world, cheese, true));
}

#[test]
fn a_part_of_a_pie_is_eaten_whole() {
    let pie = block("plerocottage:cherries_pie");
    let mut world = table("3");
    world.food = (4, 0.0, 0.0);
    world.main = Some(ItemStack::new("plerocottage:iron_knife", 1));
    assert!(used_in(&mut world, pie, true));
    assert_eq!(world.block.as_deref(), Some("minecraft:air"));
    assert!(world.given.is_empty());
    assert_eq!(world.food.0, 4 + data().foods[&pie.item].nutrition);
}

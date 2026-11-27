use super::*;

fn stack(item: &str, count: u32) -> Option<ItemStack> {
    Some(ItemStack::new(item, count))
}

fn order(item: &str) -> Option<usize> {
    ["minecraft:stone", "minecraft:dirt", "minecraft:egg", "minecraft:diamond_sword"]
        .iter()
        .position(|i| *i == item)
}

fn max_stack(item: &str) -> u32 {
    match item {
        "minecraft:egg" => 16,
        "minecraft:diamond_sword" => 1,
        _ => 64,
    }
}

fn shown(slots: &[Option<ItemStack>]) -> Vec<Option<(String, u32)>> {
    slots.iter().map(|s| s.as_ref().map(|s| (s.item.to_string(), s.count))).collect()
}

#[test]
fn every_block_is_a_cabinet_or_a_drawer_with_its_menu_and_screen() {
    let data = data();
    assert_eq!(data.blocks.len(), 64);
    for (name, block) in &data.blocks {
        let (_, kind) = of(name).unwrap();
        assert!(block.open.set.values().all(|v| *v) && block.close.set.values().all(|v| !*v));
        assert_eq!(kind.menu.len(), kind.size);
        assert!(!kind.buttons.is_empty());
        assert!(kind.screen.buttons.iter().all(|b| kind.buttons.iter().any(|p| p.id == b.button)));
        assert_eq!(data.kinds.values().filter(|k| k.menu_type == kind.menu_type).count(), 1);
        let shown = screen(kind);
        assert!(shown.slots.iter().all(Option::is_some));
    }
    // Every slot of a drawer is sorted by one of its buttons, and only by one.
    for kind in data.kinds.values() {
        let mut covered = vec![0; kind.size];
        for press in &kind.buttons {
            for slot in press.sorts[0]..press.sorts[1] {
                covered[slot] += 1;
            }
        }
        assert!(covered.iter().all(|c| *c == 1), "{covered:?}");
    }
}

#[test]
fn a_sort_lays_the_items_in_the_registrys_order_and_joins_what_stacks() {
    let mut slots = vec![
        stack("minecraft:egg", 10),
        None,
        stack("minecraft:dirt", 40),
        stack("minecraft:egg", 10),
        stack("minecraft:diamond_sword", 1),
        stack("minecraft:dirt", 40),
        stack("mymod:unknown", 3),
        stack("minecraft:stone", 1),
        stack("minecraft:diamond_sword", 1),
    ];
    sort(&mut slots, order, max_stack);
    let expected: Vec<Option<(String, u32)>> = [
        ("minecraft:stone", 1),
        ("minecraft:dirt", 64),
        ("minecraft:dirt", 16),
        ("minecraft:egg", 16),
        ("minecraft:egg", 4),
        ("minecraft:diamond_sword", 1),
        ("minecraft:diamond_sword", 1),
        ("mymod:unknown", 3),
    ]
    .iter()
    .map(|(i, c)| Some(((*i).to_owned(), *c)))
    .chain([None])
    .collect();
    assert_eq!(shown(&slots), expected);
}

#[test]
fn stacks_of_other_components_stay_apart() {
    let mut damaged = ItemStack::new("minecraft:dirt", 5);
    damaged.damage = 1;
    let mut slots = vec![stack("minecraft:dirt", 5), Some(damaged), stack("minecraft:dirt", 5)];
    sort(&mut slots, order, max_stack);
    assert_eq!(slots[0].as_ref().map(|s| (s.count, s.damage)), Some((10, 0)));
    assert_eq!(slots[1].as_ref().map(|s| (s.count, s.damage)), Some((5, 1)));
    assert!(slots[2].is_none());
}

#[derive(Default)]
struct Level {
    open: String,
    sounds: Vec<(String, usize, f32, f32)>,
    flags: Vec<i32>,
}

impl Host for Level {
    fn block_property(&self, _pos: [i32; 3], key: &str) -> Option<String> {
        (key == "open").then(|| self.open.clone())
    }
    fn set_block_property_with(&mut self, _pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {
        assert_eq!(key, "open");
        self.open = value.to_owned();
        self.flags.push(flags);
        true
    }
    fn play_sound(&mut self, sound: &str, _at: [f64; 3], source: usize, volume: f32, pitch: f32) -> bool {
        self.sounds.push((sound.to_owned(), source, volume, pitch));
        true
    }
}

#[test]
fn the_first_look_opens_it_and_the_last_shuts_it_with_a_sound_each_way() {
    let (name, block) = data().blocks.iter().min_by_key(|(name, _)| name.as_str()).unwrap();
    let mut level = Level { open: "false".to_owned(), ..Level::default() };
    opened(&mut level, [0; 3], block, true);
    assert_eq!(level.open, "true", "{name}");
    // Open already: nothing more.
    opened(&mut level, [0; 3], block, true);
    opened(&mut level, [0; 3], block, false);
    assert_eq!(level.open, "false");
    assert_eq!(level.flags, [block.open.flags, block.close.flags]);
    let heard: Vec<_> = level.sounds.iter().map(|(s, ..)| s.as_str()).collect();
    assert_eq!(heard, [block.open.sound.as_str(), block.close.sound.as_str()]);
    assert!(level.sounds.iter().all(|(_, source, ..)| *source == crate::BLOCKS));
}

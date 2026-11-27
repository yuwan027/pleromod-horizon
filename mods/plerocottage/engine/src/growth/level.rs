//! The rules in the game: what the hooks of a block's random tick, bone meal and use hand over,
//! as the world a rule changes.
use super::*;
use horizon_client::mod_api::*;
use std::sync::Mutex;

/// `Level.setBlock`'s flags: a crop grows with `UPDATE_CLIENTS`, a new block with `UPDATE_ALL`.
const UPDATE_CLIENTS: i32 = 2;
const UPDATE_ALL: i32 = 3;
const AROUND: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// What random ticks dropped, until a hook that can put items into the world comes round.
static DROPS: Mutex<Vec<([i32; 3], String, u32)>> = Mutex::new(Vec::new());

fn at(pos: BlockPos) -> [i32; 3] {
    [pos.x, pos.y, pos.z]
}

fn plus(pos: [i32; 3], by: [i32; 3]) -> [i32; 3] {
    [pos[0] + by[0], pos[1] + by[1], pos[2] + by[2]]
}

/// The plant a state is a state of, with the rules of that state.
fn rules_of(level: &dyn LevelHost, state: BlockStateId) -> Option<(&'static str, &'static Rules)> {
    let name = level.state_block(state)?;
    let (name, plant) = data().blocks.get_key_value(&name)?;
    Some((name, plant.rules(|key| level.state_property(state, key))?))
}

/// The tree of a rule where the plant stands.
fn tree_at<'a>(rule: &'a Rule, level: &dyn LevelHost, pos: [i32; 3]) -> Option<&'a Node> {
    let light = rule.light_at.map_or(15, |by| level.raw_brightness(plus(pos, by)));
    let enclosed = rule.enclosed.is_some()
        && AROUND.iter().all(|&by| !level.replaceable(level.block_state(plus(pos, by))));
    rule.tree(light, enclosed)
}

struct Drawn<'a>(&'a mut dyn LevelRandomHost);

impl Random for Drawn<'_> {
    fn int(&mut self, bound: i32) -> i32 {
        self.0.random().and_then(|r| r.next_int_bound(bound).ok()).unwrap_or(bound - 1)
    }
    fn float(&mut self) -> f32 {
        self.0.random().map_or(1.0, RandomSource::next_float)
    }
    fn double(&mut self) -> f64 {
        self.0.random().map_or(1.0, RandomSource::next_double)
    }
    fn boolean(&mut self) -> bool {
        self.0.random().is_some_and(RandomSource::next_boolean)
    }
}

struct Level<'a>(&'a mut dyn LevelWriteHost);

impl Random for Level<'_> {
    fn int(&mut self, bound: i32) -> i32 {
        self.0.random().and_then(|r| r.next_int_bound(bound).ok()).unwrap_or(bound - 1)
    }
    fn float(&mut self) -> f32 {
        self.0.random().map_or(1.0, RandomSource::next_float)
    }
    fn double(&mut self) -> f64 {
        self.0.random().map_or(1.0, RandomSource::next_double)
    }
    fn boolean(&mut self) -> bool {
        self.0.random().is_some_and(RandomSource::next_boolean)
    }
}

impl Level<'_> {
    fn with(&self, mut state: BlockStateId, properties: &BTreeMap<String, String>) -> BlockStateId {
        for (key, value) in properties {
            // A property the block here doesn't have (a state of 1.20.1 its template lacks) is passed over
            state = self.0.state_with_property(state, key, value).unwrap_or(state);
        }
        state
    }
}

impl Plot for Level<'_> {
    fn block(&self, at: [i32; 3]) -> Option<String> {
        self.0.state_block(self.0.block_state(at))
    }
    fn replaceable(&self, at: [i32; 3]) -> bool {
        self.0.replaceable(self.0.block_state(at))
    }
    fn set_properties(&mut self, at: [i32; 3], properties: &BTreeMap<String, String>) {
        let state = self.with(self.0.block_state(at), properties);
        self.0.set_block(at, state, UPDATE_CLIENTS);
    }
    fn set_block(&mut self, at: [i32; 3], block: Option<(&str, &BTreeMap<String, String>)>) {
        let state = match block {
            None => self.0.default_state("minecraft:air"),
            Some((block, properties)) => self.0.default_state(block).map(|state| self.with(state, properties)),
        };
        if let Some(state) = state {
            self.0.set_block(at, state, UPDATE_ALL);
        }
    }
    fn drop(&mut self, at: [i32; 3], item: &str, count: u32) {
        if let Ok(mut drops) = horizon_os::sync::lock(&DROPS) {
            drops.push((at, item.to_owned(), count));
        }
    }
    fn give(&mut self, at: [i32; 3], item: &str, count: u32) {
        self.drop(at, item, count);
    }
    fn settle(&mut self, at: [i32; 3], block: &str) {
        // LeavesBlock.tick works the distance out; a tick of any other block of a tree does nothing
        if self.block(at).as_deref() == Some(block) {
            self.0.schedule_block_tick(at, block, 1);
        }
    }
}

/// A random tick of the block `state` at `pos`. Whether the plant's own rule took it: the
/// template block's random tick must not follow then.
pub fn random_tick(level: &mut dyn LevelWriteHost, pos: BlockPos, state: BlockStateId) -> bool {
    let pos = at(pos);
    let Some((name, rules)) = rules_of(level, state) else {
        return false;
    };
    let Some(tree) = rules.random_tick.as_ref().and_then(|rule| tree_at(rule, level, pos)) else {
        return false;
    };
    let mut world = Level(level);
    let end = tree.end(&mut world);
    if end.gone() {
        // Leaves far from a trunk: the template's own decay, with its drops
        return false;
    }
    apply(end, pos, data().saplings.get(name), &mut world);
    true
}

/// `mayPlaceOn`: whether the plant `state` may be planted on the block `ground`.
pub fn may_place_on(level: &dyn LevelHost, state: BlockStateId, ground: BlockStateId) -> Option<bool> {
    data().grounds.stands(&level.state_block(state)?, &level.state_block(ground)?)
}

/// `canSurvive`, as far as the ground under the plant decides it.
pub fn can_survive(level: &dyn LevelHost, pos: BlockPos, state: BlockStateId) -> Option<bool> {
    may_place_on(level, state, level.block_state(plus(at(pos), [0, -1, 0])))
}

/// `isValidBonemealTarget`: the plant's own answer, or none where it has none.
pub fn bonemeal_target(level: &dyn LevelHost, state: BlockStateId) -> Option<bool> {
    let (_, rules) = rules_of(level, state)?;
    if rules.no_bonemeal {
        return Some(false);
    }
    rules.perform_bonemeal.is_some().then_some(true)
}

/// `isBonemealSuccess`.
pub fn bonemeal_success(level: &mut dyn LevelRandomHost, pos: BlockPos, state: BlockStateId) -> Option<bool> {
    let (_, rules) = rules_of(level, state)?;
    let tree = tree_at(rules.is_bonemeal_success.as_ref()?, level, at(pos))?;
    tree.end(&mut Drawn(level)).answer
}

/// `performBonemeal`.
pub fn bonemeal(level: &mut dyn LevelWriteHost, pos: BlockPos, state: BlockStateId) {
    let pos = at(pos);
    let Some((name, rules)) = rules_of(level, state) else {
        return;
    };
    let Some(tree) = rules.perform_bonemeal.as_ref().and_then(|rule| tree_at(rule, level, pos)) else {
        return;
    };
    let mut world = Level(level);
    let end = tree.end(&mut world);
    apply(end, pos, data().saplings.get(name), &mut world);
}

fn item_entity(host: &mut dyn Host, pos: [i32; 3], item: &str, count: u32) {
    // Block.popResource: an item entity in the block's place
    let mut stack = Compound::new();
    stack.insert(crate::JavaString::from("id"), Tag::String(crate::JavaString::from(item)));
    stack.insert(crate::JavaString::from("count"), Tag::Int(count as i32));
    let mut nbt = Compound::new();
    nbt.insert(crate::JavaString::from("Item"), Tag::Compound(stack));
    let dimension = host.dimension();
    host.summon(dimension, "minecraft:item", pos.map(|v| f64::from(v) + 0.5), nbt);
}

/// Puts what random ticks dropped into the world.
pub fn flush_drops(host: &mut dyn Host) {
    let drops = horizon_os::sync::lock(&DROPS).map(|mut drops| std::mem::take(&mut *drops)).unwrap_or_default();
    for (pos, item, count) in drops {
        item_entity(host, pos, &item, count);
    }
}

struct Used<'a> {
    host: &'a mut dyn Host,
    player: Option<u128>,
}

impl Random for Used<'_> {
    fn int(&mut self, bound: i32) -> i32 {
        let dimension = self.host.dimension();
        self.host.level_random(dimension).and_then(|r| r.next_int_bound(bound).ok()).unwrap_or(0)
    }
    fn float(&mut self) -> f32 {
        let dimension = self.host.dimension();
        self.host.level_random(dimension).map_or(1.0, RandomSource::next_float)
    }
    fn double(&mut self) -> f64 {
        let dimension = self.host.dimension();
        self.host.level_random(dimension).map_or(1.0, RandomSource::next_double)
    }
    fn boolean(&mut self) -> bool {
        let dimension = self.host.dimension();
        self.host.level_random(dimension).is_some_and(RandomSource::next_boolean)
    }
}

impl Plot for Used<'_> {
    fn block(&self, at: [i32; 3]) -> Option<String> {
        Some(self.host.block(at))
    }
    fn replaceable(&self, _at: [i32; 3]) -> bool {
        false
    }
    fn set_properties(&mut self, at: [i32; 3], properties: &BTreeMap<String, String>) {
        for (key, value) in properties {
            self.host.set_block_property(at, key, value);
        }
    }
    fn set_block(&mut self, at: [i32; 3], block: Option<(&str, &BTreeMap<String, String>)>) {
        let (block, properties) = block.unzip();
        let Some(state) = self.host.default_state(block.unwrap_or("minecraft:air")) else {
            return;
        };
        let dimension = self.host.dimension();
        self.host.set_block(dimension, at, state, UPDATE_ALL);
        self.set_properties(at, properties.unwrap_or(&BTreeMap::new()));
    }
    fn drop(&mut self, at: [i32; 3], item: &str, count: u32) {
        item_entity(self.host, at, item, count);
    }
    fn give(&mut self, at: [i32; 3], item: &str, count: u32) {
        // ItemHandlerHelper.giveItemToPlayer: into the inventory, the rest at the player's feet
        let left = match self.player {
            Some(player) => self.host.give_stack(player, ItemStack::new(item, count)).map_or(0, |s| s.count),
            None => count,
        };
        if left > 0 {
            item_entity(self.host, at, item, left);
        }
    }
}

/// A use by hand of the block at `pos`: a ripe plant picked. Whether the plant's rule did anything.
pub fn used(host: &mut dyn Host, pos: [i32; 3]) -> bool {
    let name = host.block(pos);
    let Some(plant) = data().blocks.get(&name) else {
        return false;
    };
    let Some(rule) = plant.rules(|key| host.block_property(pos, key)).and_then(|rules| rules.used.as_ref()) else {
        return false;
    };
    let light = rule.light_at.map_or(15, |by| {
        let (block, sky) = host.light(plus(pos, by));
        i32::from(block.max(sky))
    });
    let Some(tree) = rule.tree(light, false) else {
        return false;
    };
    let player = host.acting_player().map(|player| player.uuid);
    let mut world = Used { host, player };
    let end = tree.end(&mut world);
    apply(end, pos, None, &mut world)
}

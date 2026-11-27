//! The mod's mobs: the mallard and the turkey (`PoultryEntity`), the eggs thrown at things
//! (`PoultryEggProjectile`, `PoultryEggItem`), the hay nest they lay into (`HayNestBlock`), and
//! what the mod does to animals of Minecraft's: alfalfa in the hand (`AlfalfaFeeding`) and the
//! truffles pigs dig (`TruffleDigGoal`). The types are the content mod's (an animal's template
//! with their own size, sounds, food and spawn placement); what their classes do beyond an
//! animal's is here, by `data/mobs.json`, which pleromod's extraction asked of the classes
//! themselves by running them: the goals `registerGoals` adds with their numbers, the nest goal
//! tried against levels with and without a nest, the egg `aiStep` lays and where, what `hurt`
//! refuses, what a thrown egg hatches, the nest's `use` under every answer to the tags it asks.
use horizon_client::mobs::goals::{
    self, Breed, Cx, Float, FollowParent, Goal, GoalSelector, Panic, RandomLookAround, Stroll, Tempt,
};
use horizon_client::mobs::{Mob, MobEvent, MobWorld};
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Key = <Compound as crate::MapKey>::Key;

#[derive(Deserialize)]
pub struct Data {
    pub entities: HashMap<String, Spec>,
    /// The blocks a mob lays into, and what a hand does with them.
    #[serde(default)]
    pub blocks: HashMap<String, Held>,
    /// The items whose use throws something.
    #[serde(default)]
    pub items: HashMap<String, Thrown>,
    /// The items that feed any animal (`PlayerInteractEvent.EntityInteract`).
    #[serde(default)]
    pub feeding: HashMap<String, Feeding>,
    /// The goals the mod gives mobs of Minecraft's (`EntityJoinLevelEvent`).
    #[serde(default)]
    pub joining: Vec<Joining>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Spec {
    /// `registerGoals`, in its order.
    pub goals: Vec<GoalSpec>,
    /// `isFood`'s tag, which the content mod's type declares; here for the tests.
    #[allow(dead_code)]
    pub food: Option<TagRef>,
    /// Damage types `hurt` refuses outright.
    pub immune: Vec<String>,
    pub egg: Option<Egg>,
    /// A thrown egg's: what `onHit` may spawn.
    pub hatches: Option<Hatch>,
    /// What the mod registered with `SpawnPlacements`, which the content mod's type declares; here
    /// for the tests, as are the modifiers that add its spawns and its loot table in a few facts.
    #[allow(dead_code)]
    pub placement: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub spawns: Vec<serde_json::Value>,
    #[allow(dead_code)]
    pub drops: Vec<Dropped>,
}

#[derive(Deserialize)]
pub struct TagRef {
    #[allow(dead_code)]
    pub tag: String,
}

/// One entry of the jar's loot table of a mob.
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct Dropped {
    pub item: String,
    /// `set_count`.
    pub count: Option<[f64; 2]>,
    /// `looting_enchant`.
    pub looting: Option<[f64; 2]>,
    /// `furnace_smelt`'s condition: the flags of the mob it asks for.
    pub cooked: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct GoalSpec {
    pub priority: i32,
    /// The goal's class (`PanicGoal`), or a class of the mod's (`own`).
    pub goal: String,
    /// What its constructor was given, the mob left out.
    pub args: Vec<serde_json::Value>,
    #[serde(default)]
    pub own: bool,
    /// A goal of the mod's that walks the mob to its nest.
    pub nest: Option<Nest>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Egg {
    /// `MAX_EGG_LAY_TIME`: the timer a new mob starts with.
    pub time: i32,
    pub item: String,
    pub sound: String,
    pub volume: f32,
    /// The pitch at the lowest and the highest roll.
    pub pitch: [f32; 2],
    /// The timer after an egg.
    pub back_to: i32,
    /// The key the timer is saved under (`addAdditionalSaveData`).
    pub saved_as: Option<String>,
    /// The nest an egg is laid into when the mob stands on one.
    pub into: Option<Into>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Into {
    pub block: String,
    /// How far below its feet the block is looked for.
    pub below: f64,
    /// The places tried, in their order.
    pub slots: Vec<usize>,
}

/// `FindNearestNestGoal`, as the extraction found it to behave.
#[derive(Deserialize, Clone, Debug)]
pub struct Nest {
    pub block: String,
    /// The egg timer it starts at: above `above`, up to `to`.
    pub timer: Span,
    /// The goal-ticks it waits after a search that found none, and one that found one.
    pub cooldown: Cooldown,
    /// How far it searches along x, y and z.
    pub range: [i32; 3],
    /// The block it must not stand on to start, from its own.
    pub standing_on: [i32; 3],
    /// The point of a nest its distance is measured to, from the block's corner.
    pub nearest_from: [f64; 3],
    /// The places of which one must be empty.
    pub egg_slots: Vec<usize>,
    pub needs_room: bool,
    pub not_from_one: bool,
    pub adults: bool,
    pub walk: Walk,
    pub arrive: Arrive,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Span {
    pub above: i32,
    pub to: i32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Cooldown {
    pub none: i32,
    pub found: i32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Walk {
    pub to: [f64; 3],
    pub speed: f64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Arrive {
    /// The squared distance below which it is there.
    pub within_sqr: f64,
    pub from: [f64; 3],
    /// Where it is put, from the block's corner.
    pub moves_to: [f64; 3],
    /// The egg timer it is left with.
    pub timer: i32,
    pub leaves: Leaves,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Leaves {
    pub item: String,
    pub slots: Vec<usize>,
    pub only_empty: bool,
}

#[derive(Deserialize)]
pub struct Hatch {
    pub chance: f32,
    pub entity: String,
    /// `setAge`: a baby's ticks to grow up, negative.
    pub age: i32,
}

/// A block whose block entity holds what is laid on it by hand (`HayNestBlock.use`).
#[derive(Deserialize)]
pub struct Held {
    pub size: usize,
    /// What a stack in the hand is put into, by the tags it is in: the first case that holds.
    pub put: Vec<Put>,
    pub take: Take,
    /// How its block entity renderer draws what it holds: its places in runs drawn alike.
    #[serde(default)]
    pub drawn: Vec<Drawn>,
}

/// The places of a block drawn alike (`HayNestBlockEntityRenderer.renderGroup`): the stacks in
/// them are laid out by how many there are.
#[derive(Deserialize)]
pub struct Drawn {
    pub slots: Vec<usize>,
    /// `block` (`renderSingleBlock`) or `item` (`ItemRenderer.renderStatic`).
    #[serde(rename = "as")]
    pub kind: String,
    /// An item's display context.
    pub display: Option<String>,
    /// The block an item that places none is drawn as; one that places a block is drawn as it.
    #[serde(rename = "else")]
    pub other: Option<String>,
    /// For one stack, two, and so on: each one's pose, step by step (`translate`, `rotate` by a
    /// quaternion, `scale`).
    pub places: Vec<Vec<Vec<Pose>>>,
}

/// One step of a pose: its name, then its numbers.
#[derive(Deserialize)]
#[serde(from = "Vec<serde_json::Value>")]
pub struct Pose {
    pub step: String,
    pub by: Vec<f32>,
}

impl From<Vec<serde_json::Value>> for Pose {
    fn from(written: Vec<serde_json::Value>) -> Self {
        Self {
            step: written.first().and_then(|name| name.as_str()).unwrap_or_default().to_owned(),
            by: written.iter().skip(1).filter_map(|number| Some(number.as_f64()? as f32)).collect(),
        }
    }
}

#[derive(Deserialize)]
pub struct Put {
    /// Tag by tag, whether the stack must be in it.
    pub is: HashMap<String, bool>,
    pub slots: Vec<usize>,
    pub count: u32,
    pub takes: u32,
    pub sound: Sound,
}

#[derive(Deserialize)]
pub struct Take {
    /// The places an empty hand empties, the first that holds something.
    pub order: Vec<usize>,
    pub sound: Sound,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Sound {
    pub id: String,
    pub volume: f32,
    pub pitch: f32,
}

/// An item thrown by its use (`PoultryEggItem`).
#[derive(Deserialize)]
pub struct Thrown {
    /// The block it places (`BlockItem.getBlock`).
    pub block: Option<String>,
    pub throws: Throw,
    pub standing: Used,
    pub sneaking: Used,
}

#[derive(Deserialize)]
pub struct Throw {
    pub entity: String,
    /// `shootFromRotation`'s roll, speed and inaccuracy.
    pub roll: f32,
    pub speed: f32,
    /// None in the mod's: its eggs fly straight, and [`shot`] spreads nothing (the tests hold it to that).
    #[allow(dead_code)]
    pub inaccuracy: f32,
    pub sound: Sound,
    pub takes: u32,
}

/// What `use` and `useOn` do with the shift key up or down.
#[derive(Deserialize, Clone, Copy)]
pub struct Used {
    /// `use` starts using the item, whose first tick throws it.
    pub starts: bool,
    /// `useOn` places it as a block.
    pub places: bool,
}

/// What an item in the hand does to an animal (`AlfalfaFeeding`).
#[derive(Deserialize)]
pub struct Feeding {
    pub baby: Option<FedBaby>,
    pub adult: Option<FedAdult>,
}

#[derive(Deserialize)]
pub struct FedBaby {
    /// How often one baby takes it.
    pub times: i32,
    /// The key the count is kept under in the animal's own data.
    pub kept_as: String,
    pub takes: u32,
    /// Some ages with what the item leaves of them, which [`fed_age`] is tested against.
    #[allow(dead_code)]
    pub ages: Vec<[i32; 2]>,
    pub particles: Particles,
}

#[derive(Deserialize)]
pub struct Particles {
    pub id: String,
    /// How far up the animal's box they come, 1 its top.
    pub above: f64,
    pub count: i32,
    pub spread: [f64; 3],
    pub speed: f64,
}

#[derive(Deserialize)]
pub struct FedAdult {
    pub love: bool,
    pub takes: u32,
    pub when_in_love: bool,
    pub when_bred: bool,
}

/// A goal the mod adds to mobs of a class of Minecraft's (`TruffleDigGoal` to pigs).
#[derive(Deserialize, Debug)]
pub struct Joining {
    /// The class, from `net.minecraft.world.entity` (`animal/Pig`).
    pub class: String,
    pub priority: i32,
    pub constants: HashMap<String, f64>,
    pub flags: Vec<String>,
    pub block: String,
    pub range: [i32; 3],
    /// `least + nextInt(spread)` goal-ticks between searches.
    pub cooldown: Between,
    pub walk: Walk,
    pub gives_up: GivesUp,
    /// The squared distance within which it goes on.
    pub keeps_within_sqr: f64,
    pub dig: Dig,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Between {
    pub least: i32,
    pub spread: i32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct GivesUp {
    /// The goal-ticks of walking after which it gives up.
    pub after: i32,
    pub cooldown: i32,
}

#[derive(Deserialize, Debug)]
pub struct Dig {
    /// The goal-ticks of digging.
    pub ticks: i32,
    pub grunts: Grunts,
    pub becomes: String,
    /// The level event of the dug block.
    pub event: i32,
    pub drops: DigDrops,
    /// The two sounds of the block dug: at the block, at the mob.
    pub sounds: Vec<SoundAt>,
}

#[derive(Deserialize, Debug)]
pub struct Grunts {
    pub every: i32,
    #[serde(flatten)]
    pub sound: Sound,
}

#[derive(Deserialize, Debug)]
pub struct DigDrops {
    pub item: String,
    pub count: [i32; 2],
    pub pickup_delay: u32,
    /// How far ahead of the mob's box it falls.
    pub ahead: f64,
}

#[derive(Deserialize, Debug)]
pub struct SoundAt {
    pub at: String,
    #[serde(flatten)]
    pub sound: Sound,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/mobs.json")))
            .unwrap_or_else(|e| panic!("data/mobs.json is what mobs.py writes: {e}"))
    })
}

pub fn spec(kind: &str) -> Option<&'static Spec> {
    data().entities.get(kind)
}

// ---- The egg timer and the nest -------------------------------------------------------------

/// `PoultryEntity.eggTimer`, which its `aiStep` and its nest goal both read and set: one for the
/// two goals of a mob.
#[derive(Clone, Debug, Default)]
pub struct Timer(Arc<AtomicI32>);

impl Timer {
    pub fn new(ticks: i32) -> Self {
        Self(Arc::new(AtomicI32::new(ticks)))
    }
    pub fn get(&self) -> i32 {
        self.0.load(Ordering::Relaxed)
    }
    pub fn set(&self, ticks: i32) {
        self.0.store(ticks, Ordering::Relaxed);
    }
}

/// `PoultryEntity.aiStep`: an adult's timer runs down, an egg comes with the chicken's plop, and
/// the timer starts again. The egg goes into the nest the mob stands on, where one of its places
/// is empty, and drops where it stands otherwise. The timer is saved as the mod saves it.
#[derive(Clone, Debug)]
pub struct LayEgg {
    pub egg: Egg,
    pub timer: Timer,
}

impl LayEgg {
    /// One tick of an adult: the pitch of the egg's sound when one is laid now. `roll` is the
    /// pitch's draw, 0 to 1.
    pub fn tick(&mut self, roll: impl FnOnce() -> f32) -> Option<f32> {
        let timer = self.timer.get();
        if timer > 0 {
            self.timer.set(timer - 1);
            return None;
        }
        self.timer.set(self.egg.back_to);
        let [low, high] = self.egg.pitch;
        Some(low + roll() * (high - low))
    }

    /// The block an egg would be laid into from `at`: `BlockPos.containing(x, y - below, z)`.
    pub fn below(&self, at: [f64; 3]) -> Option<[i32; 3]> {
        let into = self.egg.into.as_ref()?;
        Some(block_of([at[0], at[1] - into.below, at[2]]))
    }
}

fn block_of(at: [f64; 3]) -> [i32; 3] {
    at.map(|v| v.floor() as i32)
}

/// The middle of a block, where `Level.playSound(null, BlockPos, ..)` plays.
fn middle(pos: [i32; 3]) -> [f64; 3] {
    pos.map(|v| f64::from(v) + 0.5)
}

fn offset(pos: [i32; 3], by: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| f64::from(pos[i]) + by[i])
}

fn distance_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

impl Goal for LayEgg {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        m.alive()
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        if m.is_baby() {
            return;
        }
        let random = &mut m.random;
        let Some(pitch) = self.tick(|| random.next_float()) else {
            return;
        };
        let at = [m.position.x, m.position.y, m.position.z];
        let nest = self.below(at).filter(|&[x, y, z]| {
            self.egg.into.as_ref().is_some_and(|into| cx.w.block_name(x, y, z) == into.block)
        });
        let sound = |at| MobEvent::SoundId(self.egg.sound.clone(), at, self.egg.volume, pitch);
        match (nest, &self.egg.into) {
            // The nest's block entity is the level's: the egg is put there once the mobs have ticked.
            (Some(nest), Some(into)) => queue(Op::Lay {
                level: cx.w.level.id(),
                nest,
                slots: into.slots.clone(),
                item: self.egg.item.clone(),
                from: at,
                sound: Sound { id: self.egg.sound.clone(), volume: self.egg.volume, pitch },
            }),
            _ => {
                cx.events.push(MobEvent::DropItem { mob: m.id, item: self.egg.item.as_str().into(), count: 1, at });
                cx.events.push(sound(middle(block_of(at))));
            }
        }
    }
    fn save(&self, tag: &mut Compound) {
        if let Some(key) = &self.egg.saved_as {
            tag.insert(Key::from(key.as_str()), Tag::Int(self.timer.get()));
        }
    }
    fn load(&mut self, tag: &Compound) {
        if let Some(Tag::Int(ticks)) = self.egg.saved_as.as_ref().and_then(|key| tag.get(&Key::from(key.as_str()))) {
            self.timer.set(*ticks);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "LayEgg"
    }
}

/// What the nest goal asks of the level.
pub trait NestLevel {
    fn is(&self, at: [i32; 3], block: &str) -> bool;
    /// Whether one of these places of the block's entity is empty.
    fn room(&self, at: [i32; 3], slots: &[usize]) -> bool;
}

struct InWorld<'a, 'w>(&'a MobWorld<'w>);

impl NestLevel for InWorld<'_, '_> {
    fn is(&self, [x, y, z]: [i32; 3], block: &str) -> bool {
        self.0.block_name(x, y, z) == block
    }
    fn room(&self, [x, y, z]: [i32; 3], slots: &[usize]) -> bool {
        match self.0.world.block_entities.get(&(x, y, z)) {
            Some(horizon_client::containers::BlockEntity::Mod(entity)) => {
                slots.iter().any(|&slot| entity.items.get(slot).is_none_or(Option::is_none))
            }
            _ => false,
        }
    }
}

/// What a tick of the nest goal does.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// `getNavigation().moveTo(..)`.
    Walk([f64; 3], f64),
    /// It is there: it is put on the nest, which gets its feathers, and the egg comes next.
    Arrive { nest: [i32; 3], to: [f64; 3] },
}

/// `PoultryEntity.FindNearestNestGoal`: with an egg soon to come, the mob walks to the nearest
/// nest that has room for one, leaves its feathers there, and lays.
#[derive(Clone, Debug)]
pub struct ToNest {
    pub nest: Nest,
    pub timer: Timer,
    pub target: Option<[i32; 3]>,
    pub cooldown: i32,
}

impl ToNest {
    pub fn new(nest: Nest, timer: Timer) -> Self {
        Self { nest, timer, target: None, cooldown: 0 }
    }

    /// `canUse`.
    pub fn search(&mut self, baby: bool, at: [f64; 3], level: &impl NestLevel) -> bool {
        let (nest, timer) = (&self.nest, self.timer.get());
        if nest.adults && baby || timer <= nest.timer.above || timer > nest.timer.to {
            return false;
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return false;
        }
        let origin = block_of(at);
        let under = [0, 1, 2].map(|i| origin[i] + nest.standing_on[i]);
        if nest.not_from_one && level.is(under, &nest.block) {
            return false;
        }
        self.target = self.nearest(at, origin, level);
        self.cooldown = if self.target.is_some() { nest.cooldown.found } else { nest.cooldown.none };
        self.target.is_some()
    }

    /// `findNearestNest`: the nearest in the box about the mob, the first of those as near.
    fn nearest(&self, at: [f64; 3], origin: [i32; 3], level: &impl NestLevel) -> Option<[i32; 3]> {
        let (nest, [rx, ry, rz]) = (&self.nest, self.nest.range);
        let mut closest: Option<([i32; 3], f64)> = None;
        for dx in -rx..=rx {
            for dz in -rz..=rz {
                for dy in -ry..=ry {
                    let pos = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                    if !level.is(pos, &nest.block) || nest.needs_room && !level.room(pos, &nest.egg_slots) {
                        continue;
                    }
                    let distance = distance_sqr(at, offset(pos, nest.nearest_from));
                    if closest.is_none_or(|(_, least)| distance < least) {
                        closest = Some((pos, distance));
                    }
                }
            }
        }
        closest.map(|(pos, _)| pos)
    }

    /// `canContinueToUse`.
    pub fn goes_on(&self, baby: bool, level: &impl NestLevel) -> bool {
        self.target.is_some_and(|target| {
            !(self.nest.adults && baby) && self.timer.get() > self.nest.timer.above && level.is(target, &self.nest.block)
        })
    }

    /// Where it walks (`moveToNest`).
    pub fn walk(&self) -> Option<Step> {
        self.target.map(|target| Step::Walk(offset(target, self.nest.walk.to), self.nest.walk.speed))
    }

    /// `tick`.
    pub fn step(&mut self, at: [f64; 3]) -> Option<Step> {
        let target = self.target?;
        let arrive = &self.nest.arrive;
        if distance_sqr(at, offset(target, arrive.from)) < arrive.within_sqr {
            self.timer.set(arrive.timer);
            self.target = None;
            return Some(Step::Arrive { nest: target, to: offset(target, arrive.moves_to) });
        }
        self.walk()
    }
}

impl Goal for ToNest {
    // The class sets no flags: it runs beside whatever else moves the mob.
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.search(m.is_baby(), [m.position.x, m.position.y, m.position.z], &InWorld(cx.w))
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.goes_on(m.is_baby(), &InWorld(cx.w))
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        if let Some(Step::Walk(to, speed)) = self.walk() {
            m.nav_move_to(cx.w, to, speed);
        }
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        match self.step([m.position.x, m.position.y, m.position.z]) {
            Some(Step::Walk(to, speed)) => {
                m.nav_move_to(cx.w, to, speed);
            }
            Some(Step::Arrive { nest, to }) => {
                // `teleportTo`, `fillFeatherSlots`, `stop`.
                (m.position.x, m.position.y, m.position.z) = (to[0], to[1], to[2]);
                let leaves = &self.nest.arrive.leaves;
                queue(Op::Leave {
                    level: cx.w.level.id(),
                    nest,
                    slots: leaves.slots.clone(),
                    item: leaves.item.clone(),
                    only_empty: leaves.only_empty,
                });
                m.nav_stop();
            }
            None => {}
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
        self.target = None;
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "ToNest"
    }
}

/// What a mob does to a nest's block entity, kept until a hook with the level's host comes
/// (`flush`, after the level's tick): goals have the level to read, not to change block entities.
#[derive(Debug)]
enum Op {
    Lay { level: &'static str, nest: [i32; 3], slots: Vec<usize>, item: String, from: [f64; 3], sound: Sound },
    Leave { level: &'static str, nest: [i32; 3], slots: Vec<usize>, item: String, only_empty: bool },
}

static OPS: Mutex<Vec<Op>> = Mutex::new(Vec::new());

fn queue(op: Op) {
    if let Ok(mut ops) = horizon_os::sync::lock(&OPS) {
        ops.push(op);
    }
}

/// `SoundSource`'s ordinals.
const NEUTRAL: usize = 6;
const PLAYERS: usize = 7;

/// Puts `stack` into the first empty of `slots` of the mod block entity at `pos`. False with none.
fn put_in(host: &mut dyn Host, pos: [i32; 3], slots: &[usize], stack: &ItemStack) -> bool {
    let Some(entity) = host.mod_block_entity(pos) else {
        return false;
    };
    let Some(&slot) = slots.iter().find(|&&slot| entity.items.get(slot).is_none_or(Option::is_none)) else {
        return false;
    };
    host.change_mod_block_entity(pos, &mut |entity| {
        if entity.items.len() <= slot {
            entity.items.resize(slot + 1, None);
        }
        entity.items[slot] = Some(stack.clone());
    });
    host.block_entity_sync(pos);
    true
}

fn item_entity(host: &mut dyn Host, at: [f64; 3], stack: &ItemStack) {
    let mut item = Compound::new();
    item.insert(Key::from("id"), Tag::String(Key::from(&*stack.item)));
    item.insert(Key::from("count"), Tag::Int(stack.count as i32));
    let mut nbt = Compound::new();
    nbt.insert(Key::from("Item"), Tag::Compound(item));
    // `Entity.spawnAtLocation`: `setDefaultPickUpDelay`.
    nbt.insert(Key::from("PickupDelay"), Tag::Short(10));
    let dimension = host.dimension();
    host.summon(dimension, "minecraft:item", at, nbt);
}

/// What the mobs of the level that just ticked did to nests.
pub fn flush(host: &mut dyn Host, dimension: &str) {
    let ops: Vec<Op> = match horizon_os::sync::lock(&OPS) {
        Ok(mut ops) => {
            let (mine, rest) = std::mem::take(&mut *ops).into_iter().partition(|op| match op {
                Op::Lay { level, .. } | Op::Leave { level, .. } => *level == dimension,
            });
            *ops = rest;
            mine
        }
        Err(_) => return,
    };
    for op in ops {
        match op {
            Op::Lay { nest, slots, item, from, sound, .. } => {
                let stack = ItemStack::new(&item, 1);
                // In the nest, or beside it when it is full: the sound comes from the nest, or the mob.
                let at = if put_in(host, nest, &slots, &stack) {
                    middle(nest)
                } else {
                    item_entity(host, from, &stack);
                    middle(block_of(from))
                };
                host.play_sound(&sound.id, at, NEUTRAL, sound.volume, sound.pitch);
            }
            Op::Leave { nest, slots, item, only_empty, .. } => {
                let stack = ItemStack::new(&item, 1);
                if only_empty {
                    // Each of its places that is empty gets one.
                    for slot in slots {
                        put_in(host, nest, &[slot], &stack);
                    }
                } else if host.mod_block_entity(nest).is_some() {
                    host.change_mod_block_entity(nest, &mut |entity| {
                        for &slot in &slots {
                            if entity.items.len() <= slot {
                                entity.items.resize(slot + 1, None);
                            }
                            entity.items[slot] = Some(stack.clone());
                        }
                    });
                    host.block_entity_sync(nest);
                }
            }
        }
    }
}

// ---- The nest by hand -----------------------------------------------------------------------

pub fn holding(block: &str) -> Option<&'static Held> {
    data().blocks.get(block)
}

impl Held {
    /// The case a stack in these tags is put by: every tag a case names answered as it says.
    pub fn case(&self, in_tag: impl Fn(&str) -> bool) -> Option<&Put> {
        self.put.iter().find(|put| put.is.iter().all(|(tag, wanted)| in_tag(tag) == *wanted))
    }
}

/// `HayNestBlock.use`: a click with one hand on a block that holds what is laid on it. False when
/// the click passes on.
pub fn use_held(host: &mut dyn Host, pos: [i32; 3], main: bool) -> bool {
    let block = host.block(pos);
    let (Some(spec), Some(player), Some(entity)) = (holding(&block), host.acting_player(), host.mod_block_entity(pos)) else {
        return false;
    };
    let hand = if main {
        host.held_stack(player.uuid)
    } else {
        host.player_equipment(player.uuid, EquipmentSlot::OffHand)
    };
    let at = middle(pos);
    let Some(hand) = hand.filter(|stack| stack.count > 0) else {
        // An empty hand takes the first of its order that holds something.
        let Some(&slot) = spec.take.order.iter().find(|&&slot| entity.items.get(slot).is_some_and(Option::is_some)) else {
            return false;
        };
        let mut taken = None;
        host.change_mod_block_entity(pos, &mut |entity| taken = entity.items[slot].take());
        host.block_entity_sync(pos);
        if let Some(stack) = taken
            && let Some(left) = host.give_stack(player.uuid, stack)
        {
            // `placeItemBackInInventory`: what does not fit is dropped.
            item_entity(host, player.position, &left);
        }
        let sound = &spec.take.sound;
        host.play_sound(&sound.id, at, crate::BLOCKS, sound.volume, sound.pitch);
        return true;
    };
    let Some(put) = spec.case(|tag| host.server_item_in_tag(&hand, tag).unwrap_or(false)) else {
        return false;
    };
    let mut one = hand.clone();
    one.count = put.count;
    if !put_in(host, pos, &put.slots, &one) {
        return false;
    }
    // `instabuild`: a creative hand keeps its stack.
    if player.game_mode != 1 {
        take_from_hand(host, &player, main, &hand, put.takes);
    }
    host.play_sound(&put.sound.id, at, crate::BLOCKS, put.sound.volume, put.sound.pitch);
    true
}

/// `ItemStack.shrink` of the stack in a hand.
fn take_from_hand(host: &mut dyn Host, player: &NativeServerPlayer, main: bool, hand: &ItemStack, count: u32) {
    let mut left = hand.clone();
    left.count = hand.count.saturating_sub(count);
    let slot = if main { usize::from(player.selected_slot) } else { 40 };
    let new = (left.count > 0).then_some(left);
    if host.set_inventory_slot(player.uuid, NativeInventoryKind::Player, slot, Some(hand), new).is_err() {
        // The player's own state is in place while their click is handled: taken from their hand instead.
        host.take_items(&hand.item, count);
    }
}

/// What clients get of a block that holds stacks: its fields, and the item in each of its places
/// (`laid`, an empty compound for none), which its renderer draws; a client's copy has no slots.
pub fn held_tag(entity: &ModBlockEntity) -> Compound {
    let mut tag = entity.nbt.clone();
    let laid = entity.items.iter().map(|slot| {
        let mut one = Compound::new();
        if let Some(stack) = slot {
            one.insert(Key::from("id"), Tag::String(Key::from(&*stack.item)));
        }
        Tag::Compound(one)
    });
    tag.insert(Key::from("laid"), Tag::List(laid.collect()));
    tag
}

/// A rotation by a quaternion `[x, y, z, w]` as the axis and the degrees `rotate_about` takes.
pub fn axis_angle([x, y, z, w]: [f32; 4]) -> ([f32; 3], f32) {
    let length = (x * x + y * y + z * z + w * w).sqrt();
    if length == 0.0 {
        return ([1.0, 0.0, 0.0], 0.0);
    }
    let (x, y, z, w) = (x / length, y / length, z / length, w / length);
    let sine = (1.0 - w * w).max(0.0).sqrt();
    if sine < 1e-6 {
        return ([1.0, 0.0, 0.0], 0.0);
    }
    ([x / sine, y / sine, z / sine], (2.0 * w.clamp(-1.0, 1.0).acos()).to_degrees())
}

/// `HayNestBlockEntityRenderer.render`: what lies in the block, each run of places laid out by
/// how many of them are taken.
pub fn draw_held(host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
    let Some(spec) = holding(event.block) else {
        return;
    };
    let Some(Tag::List(laid)) = event.entity.nbt.get(&Key::from("laid")) else {
        return;
    };
    let item = |slot: usize| match laid.get(slot) {
        Some(Tag::Compound(one)) => match one.get(&Key::from("id")) {
            Some(Tag::String(id)) => Some(id.to_string_lossy()),
            _ => None,
        },
        _ => None,
    };
    let seed = event.pos[0].wrapping_mul(31).wrapping_add(event.pos[1]).wrapping_mul(31).wrapping_add(event.pos[2]);
    let draw = &mut *event.draw;
    for group in &spec.drawn {
        let items: Vec<String> = group.slots.iter().filter_map(|&slot| item(slot)).collect();
        let Some(places) = items.len().checked_sub(1).and_then(|more| group.places.get(more)) else {
            continue;
        };
        for (id, steps) in items.iter().zip(places) {
            draw.push();
            for pose in steps {
                match (pose.step.as_str(), pose.by.as_slice()) {
                    ("translate", &[x, y, z]) => draw.translate([x, y, z]),
                    ("scale", &[x, y, z]) => draw.scale([x, y, z]),
                    ("rotate", &[x, y, z, w]) => {
                        let (axis, degrees) = axis_angle([x, y, z, w]);
                        draw.rotate_about(axis, degrees, [0.0; 3]);
                    }
                    _ => {}
                }
            }
            if group.kind == "block" {
                // The block the item places, or the stand-in of one that places none.
                let own = data().items.get(id).and_then(|thrown| thrown.block.as_deref());
                if let Some(block) = own.or(group.other.as_deref()) {
                    host.draw_block(draw, block, &[]);
                }
            } else {
                let display = match group.display.as_deref() {
                    Some("ground") => DisplayContext::Ground,
                    _ => DisplayContext::Fixed,
                };
                host.draw_item(draw, &ItemStack::new(id, 1), display, None, seed);
            }
            draw.pop();
        }
    }
}

// ---- The egg in the hand --------------------------------------------------------------------

/// `Entity.calculateViewVector` turned as `Projectile.shootFromRotation` turns it, at `speed`.
pub fn shot(pitch: f32, yaw: f32, roll: f32, speed: f32) -> [f64; 3] {
    let radians = std::f32::consts::PI / 180.0;
    let x = -(yaw * radians).sin() * (pitch * radians).cos();
    let y = -((pitch + roll) * radians).sin();
    let z = (yaw * radians).cos() * (pitch * radians).cos();
    let length = f64::from(x * x + y * y + z * z).sqrt();
    [x, y, z].map(|v| f64::from(v) / length * f64::from(speed))
}

/// What a click with a thrown item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Throw,
    Place,
    Nothing,
}

impl Thrown {
    /// `useOn` on a block first, then `use`.
    pub fn click(&self, sneaking: bool, on_block: bool) -> Click {
        let used = if sneaking { self.sneaking } else { self.standing };
        if on_block && used.places {
            Click::Place
        } else if used.starts {
            Click::Throw
        } else {
            Click::Nothing
        }
    }
}

/// `PoultryEggItem.use` and `useOn`: with the shift key up the egg is thrown (`onUseTick`, on the
/// first tick it is used), on a block or in the air; with it down it is placed as a block, on one.
/// True when the click is the mod's: the item's own use (a snowball's throw, the block placed)
/// does not follow.
pub fn use_thrown(host: &mut dyn Host, stack: &ItemStack, on_block: bool) -> bool {
    let Some(spec) = data().items.get(&*stack.item) else {
        return false;
    };
    match spec.click(host.sneaking(), on_block) {
        // Placed by the item's own use, as any block's item.
        Click::Place => return false,
        // Neither thrown nor placed (`Item.use` passes): on a block the click goes on to the air's.
        Click::Nothing => return !on_block,
        Click::Throw => {}
    }
    let Some(player) = host.acting_player() else {
        return false;
    };
    let throw = &spec.throws;
    // `ThrowableProjectile(type, owner, level)`: a little below the eyes.
    let from = [player.position[0], player.position[1] + f64::from(player.eye_height) - 0.1, player.position[2]];
    let motion = shot(player.pitch, player.yaw, throw.roll, throw.speed);
    let mut nbt = Compound::new();
    nbt.insert(Key::from("Motion"), Tag::List(motion.iter().map(|v| Tag::Double(v.to_bits())).collect()));
    let uuid = player.uuid;
    let owner = [(uuid >> 96) as i32, (uuid >> 64) as i32, (uuid >> 32) as i32, uuid as i32];
    nbt.insert(Key::from("Owner"), Tag::IntArray(owner.to_vec()));
    let dimension = host.dimension();
    host.summon(dimension, &throw.entity, from, nbt);
    host.play_sound(&throw.sound.id, player.position, PLAYERS, throw.sound.volume, throw.sound.pitch);
    if player.game_mode != 1 {
        host.take_items(&stack.item, throw.takes);
    }
    true
}

// ---- Alfalfa --------------------------------------------------------------------------------

/// `animal.getAge() - animal.getAge() / 2`: what the item leaves of a baby's ticks to grow up.
pub fn fed_age(age: i32) -> i32 {
    age - age / 2
}

fn fed_key() -> Key {
    let key = data().feeding.values().find_map(|f| Some(f.baby.as_ref()?.kept_as.as_str()));
    Key::from(key.unwrap_or("fed"))
}

/// How often a baby has been fed (`getPersistentData`), saved with it.
static FED: AttachmentType<i32> = AttachmentType {
    namespace: "plerocottage",
    name: "fed",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |times| Ok([(fed_key(), Tag::Int(*times))].into()),
        decode: |data: &Compound| match data.get(&fed_key()) {
            Some(Tag::Int(times)) => Ok(*times),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// `AlfalfaFeeding.feed`: the item in the main hand, on any animal. A baby loses half of what it
/// has left to grow, a few times; a grown one that could breed falls in love. The click goes on
/// to the animal as before: the mod cancels nothing.
pub fn feed(host: &mut dyn Host, entity: u32, stack: Option<&ItemStack>) {
    let Some((stack, spec)) = stack.and_then(|s| Some((s, data().feeding.get(&*s.item)?))) else {
        return;
    };
    let dimension = host.dimension();
    let (Some(player), Some(nbt)) = (host.acting_player(), host.entity_nbt_in(dimension, entity)) else {
        return;
    };
    // An `Animal` saves `InLove`; nothing else does.
    let Some(in_love) = nbt.get(&Key::from("InLove")).map(|_| crate::int(&nbt, "InLove")) else {
        return;
    };
    let age = crate::int(&nbt, "Age");
    let mut change = Compound::new();
    let takes = if age < 0 {
        let Some(baby) = &spec.baby else {
            return;
        };
        let holder = Holder::Entity(entity);
        let times = FED.get_existing(host, holder).ok().flatten().unwrap_or(0);
        if times >= baby.times {
            return;
        }
        change.insert(Key::from("Age"), Tag::Int(fed_age(age)));
        if !host.merge_entity_nbt(dimension, entity, change) {
            return;
        }
        let _ = FED.set(host, holder, times + 1);
        let particles = &baby.particles;
        if let Some(kind) = ParticleType::from_name(&particles.id)
            && let Some([low, high]) = host.entity(entity).and_then(|e| e.bounding_box(host))
        {
            let at = [(low[0] + high[0]) / 2.0, low[1] + (high[1] - low[1]) * particles.above, (low[2] + high[2]) / 2.0];
            host.send_particles(&ParticleOptions::simple(kind), None, at, particles.spread, particles.speed, particles.count);
        }
        baby.takes
    } else {
        let Some(adult) = spec.adult.as_ref().filter(|adult| adult.love) else {
            return;
        };
        // `getLoveCause() == null && !isInLove() && getAge() == 0`
        if in_love > 0 && !adult.when_in_love || age > 0 && !adult.when_bred || nbt.contains_key(&Key::from("LoveCause")) {
            return;
        }
        // `setInLove(player)`: 600 ticks, and who caused it.
        let uuid = player.uuid;
        let cause = [(uuid >> 96) as i32, (uuid >> 64) as i32, (uuid >> 32) as i32, uuid as i32];
        change.insert(Key::from("InLove"), Tag::Int(600));
        change.insert(Key::from("LoveCause"), Tag::IntArray(cause.to_vec()));
        if !host.merge_entity_nbt(dimension, entity, change) {
            return;
        }
        adult.takes
    };
    if player.game_mode != 1 {
        host.take_items(&stack.item, takes);
    }
}

// ---- Truffles -------------------------------------------------------------------------------

/// The entity type of a class of Minecraft's, as `EntityJoinLevelEvent`'s listener names it
/// (`animal/Pig` is `minecraft:pig`).
fn type_of(class: &str) -> String {
    let name = class.rsplit('/').next().unwrap_or(class);
    let mut id = String::from("minecraft:");
    for (index, letter) in name.chars().enumerate() {
        if letter.is_ascii_uppercase() && index > 0 {
            id.push('_');
        }
        id.push(letter.to_ascii_lowercase());
    }
    id
}

/// `TruffleDigGoal`: a grown pig that finds the block near walks to it, digs for a while and
/// turns it into another, with what it held dropped before its snout.
#[derive(Clone, Debug)]
pub struct DigUp {
    pub spec: &'static Joining,
    pub target: Option<[i32; 3]>,
    pub dig_timer: i32,
    pub cooldown: i32,
    pub approach: i32,
}

impl DigUp {
    pub fn new(spec: &'static Joining) -> Self {
        Self { spec, target: None, dig_timer: 0, cooldown: 0, approach: 0 }
    }

    fn constant(&self, name: &str) -> f64 {
        self.spec.constants.get(name).copied().unwrap_or(0.0)
    }

    /// `REACH_DISTANCE_SQ`: how near, level with the ground, it digs from.
    pub fn reach_sqr(&self) -> f64 {
        self.constant("REACH_DISTANCE_SQ")
    }

    /// `canUse`, once the mob itself may: `roll` is `nextInt(spread)`.
    pub fn search(&mut self, at: [f64; 3], roll: impl FnOnce(i32) -> i32, is: impl Fn([i32; 3]) -> bool) -> bool {
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return false;
        }
        let between = self.spec.cooldown;
        self.cooldown = between.least + roll(between.spread);
        self.target = self.nearest(at, is);
        self.target.is_some()
    }

    /// `findNearestTruffle`: column by column, skipping those farther than the nearest so far,
    /// and done with the first within reach.
    fn nearest(&self, at: [f64; 3], is: impl Fn([i32; 3]) -> bool) -> Option<[i32; 3]> {
        let (origin, [rx, ry, rz]) = (block_of(at), self.spec.range);
        let mut closest: Option<([i32; 3], f64)> = None;
        for dx in -rx..=rx {
            for dz in -rz..=rz {
                if closest.is_some_and(|(_, least)| f64::from(dx * dx + dz * dz) > least) {
                    continue;
                }
                for dy in -ry..=ry {
                    let pos = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                    if !is(pos) {
                        continue;
                    }
                    let distance = distance_sqr(at, offset(pos, [0.5, 0.0, 0.5]));
                    if closest.is_none_or(|(_, least)| distance < least) {
                        closest = Some((pos, distance));
                        if distance <= self.reach_sqr() {
                            return Some(pos);
                        }
                    }
                }
            }
        }
        closest.map(|(pos, _)| pos)
    }

    /// `canContinueToUse`.
    pub fn goes_on(&self, at: [f64; 3], is: impl Fn([i32; 3]) -> bool) -> bool {
        self.target.is_some_and(|target| {
            is(target) && distance_sqr(at, offset(target, [0.5, 0.0, 0.5])) < self.spec.keeps_within_sqr
        })
    }
}

/// What a tick of the digging goal does.
#[derive(Debug, PartialEq)]
pub enum Digging {
    /// Too far: it walks on (again when its path is done).
    Approach,
    /// It has walked too long: the block is given up for a while.
    GiveUp,
    /// It digs, grunting now and then.
    Dig { grunt: bool },
    /// The block is dug, with a last grunt when it is due.
    Dug { grunt: bool },
}

impl DigUp {
    /// `tick`, from where the mob stands.
    pub fn step(&mut self, at: [f64; 3]) -> Option<Digging> {
        let target = self.target?;
        let (dx, dz) = (at[0] - (f64::from(target[0]) + 0.5), at[2] - (f64::from(target[2]) + 0.5));
        if dx * dx + dz * dz > self.reach_sqr() {
            self.dig_timer = 0;
            self.approach += 1;
            if self.approach >= self.spec.gives_up.after {
                self.target = None;
                self.cooldown = self.spec.gives_up.cooldown;
                return Some(Digging::GiveUp);
            }
            return Some(Digging::Approach);
        }
        self.dig_timer += 1;
        let grunt = self.dig_timer % self.spec.dig.grunts.every == 0;
        if self.dig_timer >= self.spec.dig.ticks {
            return Some(Digging::Dug { grunt });
        }
        Some(Digging::Dig { grunt })
    }
}

impl Goal for DigUp {
    fn flags(&self) -> u8 {
        self.spec.flags.iter().fold(0, |flags, flag| {
            flags
                | match flag.as_str() {
                    "move" => goals::MOVE,
                    "look" => goals::LOOK,
                    "jump" => goals::JUMP,
                    "target" => goals::TARGET,
                    _ => 0,
                }
        })
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        // `isBaby() || isLeashed() || isVehicle()`
        if m.is_baby() || m.extra.leash.is_some() || m.extra.rider.is_some() {
            return false;
        }
        let (random, block) = (&mut m.random, &self.spec.block);
        let roll = |bound| random.next_int_bound(bound).unwrap_or(0);
        let at = [m.position.x, m.position.y, m.position.z];
        self.search(at, roll, |[x, y, z]| cx.w.block_name(x, y, z) == block)
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let block = &self.spec.block;
        self.goes_on([m.position.x, m.position.y, m.position.z], |[x, y, z]| cx.w.block_name(x, y, z) == block)
    }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.dig_timer = 0;
        self.approach = 0;
        if let Some(target) = self.target {
            m.nav_move_to(cx.w, offset(target, self.spec.walk.to), self.spec.walk.speed);
        }
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(target) = self.target else {
            return;
        };
        let at = [m.position.x, m.position.y, m.position.z];
        let here = middle(block_of(at));
        let (spec, dig) = (self.spec, &self.spec.dig);
        let step = self.step(at);
        if let Some(Digging::Dig { grunt } | Digging::Dug { grunt }) = step {
            m.ai.look.set_look_at(offset(target, [0.5, 0.0, 0.5]), m.kind.head_rot_speed, m.kind.max_head_x_rot);
            m.nav_stop();
            if grunt {
                let sound = &dig.grunts.sound;
                cx.events.push(MobEvent::SoundId(sound.id.clone(), here, sound.volume, sound.pitch));
            }
        }
        match step {
            Some(Digging::Approach) if m.ai.nav.is_done() => {
                m.nav_move_to(cx.w, offset(target, spec.walk.to), spec.walk.speed);
            }
            Some(Digging::Dug { .. }) => {
                let [x, y, z] = target;
                if cx.w.block_name(x, y, z) == spec.block
                    && let Some(state) = cx.w.registry.default_state(&dig.becomes)
                {
                    let dug = cx.w.registry.default_state(&spec.block).map_or(0, |state| state.0 as i32);
                    cx.events.push(MobEvent::SetBlock { pos: target, state });
                    cx.events.push(MobEvent::LevelEvent { id: dig.event, pos: target, data: dug });
                    // Before its snout: half its box and a little more along where it faces.
                    let ahead = m.kind.width / 2.0 + dig.drops.ahead;
                    let yaw = f64::from(m.yaw).to_radians();
                    let [least, most] = dig.drops.count;
                    let count = least + cx.lr.next_int_bound(most - least + 1).unwrap_or(0);
                    // `new ItemEntity(level, x, y, z, stack)`: a small hop any way.
                    let mut hop = || f64::from(m.random.next_float()) * 0.2 - 0.1;
                    let motion = [hop(), 0.2, hop()];
                    cx.events.push(MobEvent::ThrowItem {
                        mob: m.id,
                        stack: ItemStack::new(&dig.drops.item, count.max(0) as u32),
                        at: [at[0] - yaw.sin() * ahead, at[1], at[2] + yaw.cos() * ahead],
                        motion,
                        pickup_delay: dig.drops.pickup_delay,
                    });
                    for sound in &dig.sounds {
                        let at = if sound.at == "block" { middle(target) } else { here };
                        cx.events.push(MobEvent::SoundId(sound.sound.id.clone(), at, sound.sound.volume, sound.sound.pitch));
                    }
                }
                self.stop(m, cx);
            }
            _ => {}
        }
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        self.target = None;
        self.dig_timer = 0;
        m.nav_stop();
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
    fn name(&self) -> &'static str {
        "DigUp"
    }
}

// ---- Goals ----------------------------------------------------------------------------------

/// Adds a goal of Minecraft's by its class and what the mod's constructor call gave it, or the
/// mod's own nest goal. False for a class this doesn't know.
fn add(selector: &mut GoalSelector, spec: &GoalSpec, tempt: &std::sync::Arc<[String]>, timer: &Timer) -> bool {
    let number = |i: usize| spec.args.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let priority = spec.priority;
    if spec.own {
        let Some(nest) = &spec.nest else {
            return false;
        };
        selector.add(priority, ToNest::new(nest.clone(), timer.clone()));
        return true;
    }
    match spec.goal.as_str() {
        "FloatGoal" => selector.add(priority, Float),
        "PanicGoal" => selector.add(priority, Panic::new(number(0))),
        "BreedGoal" => selector.add(priority, Breed::new(number(0))),
        // The ingredient is the type's food tag, whose items the type holds.
        "TemptGoal" => selector.add(priority, Tempt::new(number(0), tempt.clone(), number(2) != 0.0)),
        "FollowParentGoal" => selector.add(priority, FollowParent::new(number(0))),
        "RandomStrollGoal" => selector.add(priority, Stroll::new(number(0))),
        "RandomSwimmingGoal" => selector.add(priority, Stroll::swimming(number(0), number(1) as i32)),
        "RandomLookAroundGoal" => selector.add(priority, RandomLookAround::default()),
        _ => return false,
    }
    true
}

/// `registerGoals`: a type of the mod's gets its own goals in place of its template's; a mob of
/// Minecraft's keeps its own and gets what the mod adds when it joins a level.
pub fn register_goals(event: &mut RegisterGoalsEvent<'_>) {
    let kind = event.mob.kind.id.as_str();
    for joining in data().joining.iter().filter(|joining| type_of(&joining.class) == kind) {
        event.mob.ai.goals.add(joining.priority, DigUp::new(joining));
    }
    let Some(spec) = spec(kind) else {
        return;
    };
    if spec.goals.is_empty() {
        return;
    }
    let tempt = event.mob.kind.tempt.clone();
    let ai = &mut event.mob.ai;
    ai.goals.clear();
    ai.targets.clear();
    // `eggTimer`, which the class's `aiStep` and its nest goal share.
    let timer = Timer::new(spec.egg.as_ref().map_or(0, |egg| egg.time));
    for goal in &spec.goals {
        add(&mut ai.goals, goal, &tempt, &timer);
    }
    if let Some(egg) = &spec.egg {
        // Not a goal of the mod's: its `aiStep`, which runs beside whatever the mob is doing.
        ai.goals.add(i32::MAX, LayEgg { egg: egg.clone(), timer });
    }
}

/// `PoultryEntity.hurt`: nothing of what it refuses.
pub fn refuses(kind: &str, damage_type: &str) -> bool {
    spec(kind).is_some_and(|s| s.immune.iter().any(|t| t == damage_type))
}

/// The mob's template is a cow, which gives milk to a bucket; the mod's class is no cow.
pub fn refuses_bucket(kind: &str, stack: Option<&ItemStack>) -> bool {
    spec(kind).is_some_and(|s| !s.goals.is_empty()) && stack.is_some_and(|s| &*s.item == "minecraft:bucket")
}

/// `PoultryEggProjectile.finishHit`: on a roll below the chance, a baby of the egg's kind where
/// it broke, turned any way.
pub fn egg_hit(host: &mut dyn Host, event: &ProjectileImpactEvent<'_>) {
    let Some(hatch) = spec(event.kind).and_then(|s| s.hatches.as_ref()) else {
        return;
    };
    let Some(random) = host.level_random(event.dimension) else {
        return;
    };
    let (roll, turn) = (random.next_float(), random.next_float());
    if roll >= hatch.chance {
        return;
    }
    let mut nbt = Compound::new();
    nbt.insert(Key::from("Age"), Tag::Int(hatch.age));
    nbt.insert(
        Key::from("Rotation"),
        Tag::List(vec![Tag::Float((turn * 360.0).to_bits()), Tag::Float(0f32.to_bits())]),
    );
    // `blockPosition()`: the block it broke in.
    let at = [event.at[0].floor() + 0.5, event.at[1].floor(), event.at[2].floor() + 0.5];
    host.summon(event.dimension, &hatch.entity, at, nbt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashSet};

    const POULTRY: [&str; 2] = ["plerocottage:mallard_duck", "plerocottage:turkey"];

    fn empty() -> std::sync::Arc<[String]> {
        std::sync::Arc::from(Vec::new())
    }

    #[test]
    fn every_goal_the_class_registers_is_added() {
        for kind in POULTRY {
            let spec = spec(kind).unwrap();
            let mut selector = GoalSelector::default();
            let timer = Timer::new(0);
            let added: Vec<&str> =
                spec.goals.iter().filter(|g| add(&mut selector, g, &empty(), &timer)).map(|g| g.goal.as_str()).collect();
            assert_eq!(added.len(), spec.goals.len(), "{kind}: {added:?}");
            // The mod's own goal among them is the one to its nest.
            let own: Vec<&GoalSpec> = spec.goals.iter().filter(|g| g.own).collect();
            assert_eq!(own.len(), 1);
            assert!(own[0].nest.is_some() && selector.has("ToNest"));
        }
    }

    fn content() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../web/mods/plerocottage"))
    }

    fn json(path: std::path::PathBuf) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
    }

    #[test]
    fn both_poultry_have_the_goals_the_class_registers() {
        for kind in POULTRY {
            let spec = spec(kind).expect(kind);
            // PoultryEntity.registerGoals: nine goals, priorities 0 to 8.
            let goals: Vec<(i32, &str)> = spec.goals.iter().map(|g| (g.priority, g.goal.as_str())).collect();
            assert_eq!(
                goals,
                [
                    (0, "FloatGoal"),
                    (1, "PanicGoal"),
                    (2, "BreedGoal"),
                    (3, "TemptGoal"),
                    (4, "FollowParentGoal"),
                    (5, "PoultryEntity$FindNearestNestGoal"),
                    (6, "RandomStrollGoal"),
                    (7, "RandomSwimmingGoal"),
                    (8, "RandomLookAroundGoal"),
                ]
            );
            let speed = |name: &str| spec.goals.iter().find(|g| g.goal == name).unwrap().args[0].as_f64().unwrap();
            assert_eq!((speed("PanicGoal"), speed("FollowParentGoal"), speed("RandomStrollGoal")), (1.2, 0.8, 0.8));
            // The tempting ingredient is the food `isFood` takes, which the declared type names.
            let tempt = &spec.goals[3].args[1]["tag"];
            assert_eq!(tempt.as_str(), spec.food.as_ref().map(|f| f.tag.as_str()));
            let declared = json(content().join(format!("data/plerocottage/horizon/entity_type/{}.json", &kind[13..])));
            assert_eq!(declared["food"], *tempt);
            assert_eq!(declared["template"], "minecraft:cow");
            assert!(refuses(kind, "minecraft:fall") && !refuses(kind, "minecraft:player_attack"));
        }
        assert!(!refuses("minecraft:cow", "minecraft:fall"));
    }

    #[test]
    fn an_adult_lays_its_own_egg_when_its_timer_runs_out() {
        for kind in POULTRY {
            let egg = spec(kind).unwrap().egg.clone().expect("an egg");
            assert_eq!(egg.item, format!("{kind}_egg"));
            let (time, back_to, [low, high]) = (egg.time, egg.back_to, egg.pitch);
            let mut laying = LayEgg { timer: Timer::new(time), egg };
            // The timer counts down for as many ticks as it holds; the tick after, the egg comes.
            let quiet = (0..time).filter(|_| laying.tick(|| 0.5).is_some()).count();
            assert_eq!((quiet, laying.timer.get()), (0, 0));
            let pitch = laying.tick(|| 0.5).expect("an egg on the tick after");
            assert!((pitch - (low + high) / 2.0).abs() < 1e-6);
            assert_eq!(laying.timer.get(), back_to);
            assert!(laying.tick(|| 0.0).is_none());
            assert_eq!(laying.timer.get(), back_to - 1);
        }
    }

    #[test]
    fn the_egg_timer_is_saved_under_the_mods_own_key_and_read_back() {
        for kind in POULTRY {
            let egg = spec(kind).unwrap().egg.clone().unwrap();
            let key = egg.saved_as.clone().expect("the key `addAdditionalSaveData` writes");
            let laying = LayEgg { timer: Timer::new(1234), egg: egg.clone() };
            let mut saved = Compound::new();
            laying.save(&mut saved);
            assert_eq!(saved.get(&Key::from(key.as_str())), Some(&Tag::Int(1234)));
            // A mob loaded again starts with the class's timer, then takes the saved one; the nest
            // goal reads the same timer.
            let timer = Timer::new(egg.time);
            let mut loaded = LayEgg { timer: timer.clone(), egg: egg.clone() };
            loaded.load(&saved);
            assert_eq!(timer.get(), 1234);
            // A compound without the key leaves the timer as it is.
            loaded.load(&Compound::new());
            assert_eq!(timer.get(), 1234);
            // Through the selector, as the engine saves and loads a mob's goals.
            let mut selector = GoalSelector::default();
            selector.add(0, LayEgg { timer: Timer::new(77), egg: egg.clone() });
            let mut tag = Compound::new();
            selector.save(&mut tag);
            assert_eq!(tag.get(&Key::from(key.as_str())), Some(&Tag::Int(77)));
        }
    }

    #[test]
    fn a_thrown_egg_hatches_its_own_kind_one_time_in_sixteen() {
        for (thrown, kind, item) in [
            ("plerocottage:thrown_mallard_duck_egg", "plerocottage:mallard_duck", "plerocottage:mallard_duck_egg"),
            ("plerocottage:thrown_turkey_egg", "plerocottage:turkey", "plerocottage:turkey_egg"),
        ] {
            let hatch = spec(thrown).and_then(|s| s.hatches.as_ref()).expect(thrown);
            // `random.nextFloat() < 0.0625F`, `setAge(-24000)`
            assert_eq!((hatch.chance, hatch.entity.as_str(), hatch.age), (0.0625, kind, -24000));
            let declared = json(content().join(format!("data/plerocottage/horizon/entity_type/{}.json", &thrown[13..])));
            assert_eq!((declared["template"].as_str(), declared["item"].as_str()), (Some("minecraft:snowball"), Some(item)));
        }
    }

    /// An object with its keys in order, to compare two written in different ones.
    fn sorted(value: &serde_json::Value) -> String {
        let keys: std::collections::BTreeMap<&String, &serde_json::Value> = value.as_object().unwrap().iter().collect();
        serde_json::to_string(&keys).unwrap()
    }

    /// The spawners of the biome modifiers the content mod carries, each with its biomes.
    fn carried_spawners() -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        for file in std::fs::read_dir(content().join("data/plerocottage/forge/biome_modifier")).unwrap() {
            let modifier = json(file.unwrap().path());
            assert_eq!(modifier["type"], "forge:add_spawns");
            let spawners = match &modifier["spawners"] {
                serde_json::Value::Array(list) => list.clone(),
                one => vec![one.clone()],
            };
            for mut spawner in spawners {
                spawner["biomes"] = modifier["biomes"].clone();
                found.insert(sorted(&spawner));
            }
        }
        found
    }

    #[test]
    fn the_mallard_spawns_in_rivers_and_on_beaches_and_the_turkey_in_meadows() {
        // What the extraction read of the jar's spawn modifiers, type by type, is what the content mod
        // carries: no other modifier (the mod's others add features, which world generation would place).
        let mut extracted = BTreeSet::new();
        for kind in POULTRY {
            let spec = spec(kind).unwrap();
            assert!(!spec.spawns.is_empty(), "{kind} spawns somewhere");
            for spawn in &spec.spawns {
                let mut spawner = spawn.clone();
                spawner["type"] = kind.into();
                assert!(spawner["weight"].as_i64().is_some_and(|weight| weight > 0));
                assert!(spawner["minCount"].as_i64() <= spawner["maxCount"].as_i64());
                extracted.insert(sorted(&spawner));
            }
        }
        assert_eq!(carried_spawners(), extracted);
    }

    #[test]
    fn each_type_is_placed_as_the_mod_registered_it() {
        let mut rules = HashSet::new();
        for kind in POULTRY {
            let placement = spec(kind).unwrap().placement.clone().expect("a placement");
            let declared = json(content().join(format!("data/plerocottage/horizon/entity_type/{}.json", &kind[13..])));
            assert_eq!(declared["spawn_placement"], placement, "{kind}");
            // What the engine reads of the declaration is what the extraction wrote.
            let own = horizon_client::mobs::spawn_rules::OwnPlacement::of_row(&placement).expect("a known placement");
            let named = format!("{:?}", own.placement).to_lowercase();
            assert_eq!(named, placement["type"].as_str().unwrap().replace('_', ""));
            rules.insert(placement["rules"].as_str().unwrap().to_owned());
        }
        // The two differ in the jar: one is registered with an animal's rules, the other not at all.
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn they_drop_feathers_and_their_meat_cooked_when_they_burn() {
        let span = |value: &serde_json::Value| Some([value["min"].as_f64()?, value["max"].as_f64()?]);
        for kind in POULTRY {
            let (spec, name) = (spec(kind).unwrap(), &kind[13..]);
            assert!(!spec.drops.is_empty(), "{kind} drops something");
            let table = json(content().join(format!("data/plerocottage/loot_table/entities/{name}.json")));
            let pools = table["pools"].as_array().unwrap();
            // The jar's table, fact by fact as the extraction read it, in what the content mod carries.
            assert_eq!(pools.len(), spec.drops.len());
            for drop in &spec.drops {
                let entry = pools
                    .iter()
                    .map(|pool| &pool["entries"][0])
                    .find(|entry| entry["name"] == drop.item.as_str())
                    .unwrap_or_else(|| panic!("{name} drops {}", drop.item));
                let modifiers = match &entry["modifier"] {
                    serde_json::Value::Array(list) => list.clone(),
                    serde_json::Value::Null => Vec::new(),
                    one => vec![one.clone()],
                };
                let modifier = |kind: &str| modifiers.iter().find(|m| m["type"] == kind);
                // How many, and the looting enchantment's more a level.
                assert_eq!(modifier("minecraft:set_count").and_then(|m| span(&m["count"])), drop.count, "{}", drop.item);
                let more = modifier("minecraft:enchanted_count_increase");
                assert_eq!(more.and_then(|m| span(&m["count"])), drop.looting, "{}", drop.item);
                assert!(more.is_none_or(|m| m["enchantment"] == "minecraft:looting"));
                // Smelted when the mob has the flags the jar's condition asks for.
                let smelt = modifier("minecraft:furnace_smelt");
                assert_eq!(smelt.map(|m| m["condition"]["predicate"]["minecraft:flags"].clone()), drop.cooked, "{}", drop.item);
            }
            // Both kinds of fact are in the jar's tables: a count and a cooked drop.
            assert!(spec.drops.iter().any(|d| d.count.is_some()) && spec.drops.iter().any(|d| d.cooked.is_some()));
        }
    }

    // ---- The nest goal ----

    /// A level of nests by position, each with the places that are taken.
    #[derive(Default)]
    struct Nests {
        block: String,
        at: HashMap<[i32; 3], Vec<usize>>,
    }

    impl NestLevel for Nests {
        fn is(&self, at: [i32; 3], block: &str) -> bool {
            block == self.block && self.at.contains_key(&at)
        }
        fn room(&self, at: [i32; 3], slots: &[usize]) -> bool {
            self.at.get(&at).is_some_and(|taken| slots.iter().any(|slot| !taken.contains(slot)))
        }
    }

    fn nest_of(kind: &str) -> Nest {
        spec(kind).unwrap().goals.iter().find_map(|g| g.nest.clone()).expect("a nest goal")
    }

    fn level(nest: &Nest, at: &[[i32; 3]]) -> Nests {
        Nests { block: nest.block.clone(), at: at.iter().map(|&at| (at, Vec::new())).collect() }
    }

    const HERE: [f64; 3] = [0.5, 64.0, 0.5];

    #[test]
    fn the_nest_goal_starts_with_an_egg_soon_to_come_and_a_nest_with_room_in_reach() {
        for kind in POULTRY {
            let nest = nest_of(kind);
            let beside = level(&nest, &[[3, 64, 1]]);
            let starts = |timer: i32, baby: bool, level: &Nests| {
                let mut goal = ToNest::new(nest.clone(), Timer::new(timer));
                let found = goal.search(baby, HERE, level);
                (found, goal)
            };
            // The timer's window, as the class was found to take it.
            let Span { above, to } = nest.timer;
            for (timer, expected) in [(above, false), (above + 1, true), (to, true), (to + 1, false)] {
                assert_eq!(starts(timer, false, &beside).0, expected, "{kind} at {timer}");
            }
            assert!(to < spec(kind).unwrap().egg.as_ref().unwrap().time, "it looks only late in the timer");
            assert_eq!(starts(to, true, &beside).0, !nest.adults);
            // The box it searches: a nest at its edge is found, one past it is not.
            let [rx, ry, rz] = nest.range;
            for (at, expected) in [
                ([rx, 64, 0], true),
                ([rx + 1, 64, 0], false),
                ([0, 64 + ry, 2], true),
                ([0, 64 + ry + 1, 2], false),
                ([0, 64, -rz], true),
                ([0, 64, -rz - 1], false),
            ] {
                let (found, goal) = starts(to, false, &level(&nest, &[at]));
                assert_eq!((found, goal.target), (expected, expected.then_some(at)), "{kind} with a nest at {at:?}");
                // Found or not, it waits before it looks again.
                let wait = if expected { nest.cooldown.found } else { nest.cooldown.none };
                assert_eq!(goal.cooldown, wait);
            }
            // The nearer of two, by the point of a nest the class measures to.
            let two = level(&nest, &[[5, 64, 0], [-2, 64, 1]]);
            assert_eq!(starts(to, false, &two).1.target, Some([-2, 64, 1]));
            // A nest whose places for eggs are all taken is none; with one of them free it is.
            let mut full = level(&nest, &[[3, 64, 1]]);
            full.at.insert([3, 64, 1], nest.egg_slots.clone());
            assert_eq!(starts(to, false, &full).0, !nest.needs_room);
            full.at.insert([3, 64, 1], nest.egg_slots[1..].to_vec());
            assert!(starts(to, false, &full).0);
            // Standing on a nest already, it does not go to another.
            let under = [0, 1, 2].map(|i| block_of(HERE)[i] + nest.standing_on[i]);
            assert_eq!(starts(to, false, &level(&nest, &[under, [3, 64, 1]])).0, !nest.not_from_one);
        }
    }

    #[test]
    fn it_waits_its_cooldown_between_searches() {
        let nest = nest_of(POULTRY[0]);
        let mut goal = ToNest::new(nest.clone(), Timer::new(nest.timer.to));
        let (none, one) = (level(&nest, &[]), level(&nest, &[[2, 64, 0]]));
        assert!(!goal.search(false, HERE, &none));
        // For that many asks it does not look, though a nest is there now.
        let waited = (0..nest.cooldown.none).filter(|_| goal.search(false, HERE, &one)).count();
        assert_eq!((waited, goal.cooldown), (0, 0));
        assert!(goal.search(false, HERE, &one));
    }

    #[test]
    fn it_walks_to_the_nest_and_once_there_sits_on_it_and_lays() {
        for kind in POULTRY {
            let (nest, egg) = (nest_of(kind), spec(kind).unwrap().egg.clone().unwrap());
            let at = [4, 64, -3];
            let timer = Timer::new(nest.timer.to);
            let mut goal = ToNest::new(nest.clone(), timer.clone());
            let nests = level(&nest, &[at]);
            assert!(goal.search(false, HERE, &nests) && goal.goes_on(false, &nests));
            let walk = Step::Walk(offset(at, nest.walk.to), nest.walk.speed);
            assert_eq!(goal.walk(), Some(walk));
            // The speed is what the constructor was given.
            let given = spec(kind).unwrap().goals.iter().find(|g| g.own).unwrap().args[0].as_f64();
            assert_eq!(given, Some(nest.walk.speed));
            // Far, it walks on; at the distance the class takes for there, not yet; nearer, it is.
            assert_eq!(goal.step(HERE), Some(Step::Walk(offset(at, nest.walk.to), nest.walk.speed)));
            let from = offset(at, nest.arrive.from);
            let edge = [from[0] + nest.arrive.within_sqr.sqrt(), from[1], from[2]];
            assert!(matches!(goal.step(edge), Some(Step::Walk(..))));
            let near = [from[0] + nest.arrive.within_sqr.sqrt() * 0.99, from[1], from[2]];
            assert_eq!(goal.step(near), Some(Step::Arrive { nest: at, to: offset(at, nest.arrive.moves_to) }));
            // It is done, and the egg comes as the timer it is left with runs out.
            assert_eq!((goal.target, timer.get()), (None, nest.arrive.timer));
            assert!(!goal.goes_on(false, &nests));
            let mut laying = LayEgg { timer, egg: egg.clone() };
            let ticks = (0..).find(|_| laying.tick(|| 0.0).is_some()).unwrap();
            assert_eq!(ticks, nest.arrive.timer);
            // Sat where the goal put it, the block its egg goes into is the nest.
            let into = egg.into.clone().expect("where an egg is laid into");
            assert_eq!(into.block, nest.block);
            assert_eq!(laying.below(offset(at, nest.arrive.moves_to)), Some(at));
            assert_eq!(into.slots, nest.egg_slots);
            // The feathers it leaves are its own drop, in the places eggs do not go.
            let leaves = &nest.arrive.leaves;
            assert!(spec(kind).unwrap().drops.iter().any(|d| d.item == leaves.item));
            assert!(leaves.slots.iter().all(|slot| !nest.egg_slots.contains(slot)));
        }
    }

    #[test]
    fn it_gives_up_a_nest_that_is_gone_or_an_egg_already_laid() {
        let nest = nest_of(POULTRY[0]);
        let at = [2, 64, 0];
        let timer = Timer::new(nest.timer.to);
        let mut goal = ToNest::new(nest.clone(), timer.clone());
        assert!(goal.search(false, HERE, &level(&nest, &[at])));
        assert!(!goal.goes_on(false, &level(&nest, &[])));
        timer.set(nest.timer.above);
        assert!(!goal.goes_on(false, &level(&nest, &[at])));
    }

    // ---- The nest by hand ----

    #[test]
    fn a_nest_takes_what_may_lie_on_it_by_its_tags_and_gives_it_back_in_its_order() {
        for kind in POULTRY {
            let nest = nest_of(kind);
            let held = holding(&nest.block).expect("the nest's block");
            assert!(!held.put.is_empty());
            let mut placed = BTreeSet::new();
            for put in &held.put {
                // The case found for a stack in exactly the tags it names is itself.
                let found = held.case(|tag| put.is.get(tag).copied().unwrap_or(false)).expect("a case");
                assert_eq!(found.slots, put.slots);
                assert!(put.slots.iter().all(|&slot| slot < held.size));
                placed.extend(put.slots.iter().copied());
                assert!(put.count > 0 && put.takes == put.count);
            }
            // A stack in none of the tags the block asks about passes.
            assert!(held.case(|_| false).is_none());
            // Every place is filled by some case and emptied by an empty hand, each once.
            let order: BTreeSet<usize> = held.take.order.iter().copied().collect();
            assert_eq!((placed.len(), order.len(), held.take.order.len()), (held.size, held.size, held.size));
            // The places a mob lays into and leaves feathers in are places a hand reaches.
            assert!(nest.egg_slots.iter().chain(&nest.arrive.leaves.slots).all(|slot| order.contains(slot)));
        }
    }

    #[test]
    fn what_lies_in_a_nest_is_drawn_place_by_place() {
        for kind in POULTRY {
            let (nest, egg) = (nest_of(kind), spec(kind).unwrap().egg.clone().unwrap());
            let held = holding(&nest.block).unwrap();
            // The renderer's two runs of places: the eggs as blocks, the feathers as items.
            let run = |slots: &[usize]| held.drawn.iter().find(|group| group.slots == slots).expect("a run of places drawn alike");
            let (eggs, feathers) = (run(&nest.egg_slots), run(&nest.arrive.leaves.slots));
            assert_eq!((eggs.kind.as_str(), feathers.kind.as_str()), ("block", "item"));
            assert!(feathers.display.is_some() && eggs.other.is_some());
            for group in [eggs, feathers] {
                // A layout for one stack, for two, up to all of the run's places.
                let counts: Vec<usize> = group.places.iter().map(Vec::len).collect();
                assert_eq!(counts, (1..=group.slots.len()).collect::<Vec<_>>());
                for steps in group.places.iter().flatten() {
                    assert!(steps.iter().any(|pose| pose.step == "translate"));
                    for pose in steps {
                        assert_eq!(pose.by.len(), if pose.step == "rotate" { 4 } else { 3 }, "{}", pose.step);
                    }
                }
            }
            // An egg is drawn as the block it places, which the content mod has.
            let block = data().items.get(&egg.item).and_then(|thrown| thrown.block.clone()).expect("the egg's block");
            let declared = content().join(format!("data/plerocottage/horizon/block/{}.json", &block[13..]));
            assert!(declared.exists(), "{}", declared.display());
            let other = eggs.other.as_ref().unwrap();
            assert!(content().join(format!("data/plerocottage/horizon/block/{}.json", &other[13..])).exists());
        }
        // A quaternion turns as its axis and angle do.
        let (axis, degrees) = axis_angle([0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2]);
        assert!((degrees - 90.0).abs() < 1e-3 && (axis[1] - 1.0).abs() < 1e-5 && axis[0] == 0.0);
        assert_eq!(axis_angle([0.0, 0.0, 0.0, 1.0]).1, 0.0);
        // What clients are told lies in it: an id for each place taken, in the places' order.
        let mut entity = ModBlockEntity::default();
        entity.items = vec![None, Some(ItemStack::new("plerocottage:turkey_egg", 1))];
        let Some(Tag::List(laid)) = held_tag(&entity).get(&Key::from("laid")).cloned() else {
            panic!("no list of what is laid");
        };
        let ids: Vec<Option<&Tag>> = laid
            .iter()
            .map(|one| match one {
                Tag::Compound(one) => one.get(&Key::from("id")),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [None, Some(&Tag::String(Key::from("plerocottage:turkey_egg")))]);
    }

    // ---- The egg in the hand ----

    #[test]
    fn an_egg_is_thrown_standing_and_placed_only_sneaking() {
        for kind in POULTRY {
            let item = &spec(kind).unwrap().egg.as_ref().unwrap().item;
            let thrown = data().items.get(item).unwrap_or_else(|| panic!("{item} is thrown"));
            // What the item's own `use` and `useOn` did under each key, as the extraction ran them.
            for (sneaking, used) in [(false, thrown.standing), (true, thrown.sneaking)] {
                let on_block = thrown.click(sneaking, true);
                let in_air = thrown.click(sneaking, false);
                assert_eq!(on_block == Click::Place, used.places, "{item} on a block, sneaking {sneaking}");
                assert_eq!(in_air == Click::Throw, used.starts, "{item} in the air, sneaking {sneaking}");
                assert_ne!(in_air, Click::Place);
            }
            assert_eq!(thrown.click(false, true), Click::Throw);
            assert_eq!((thrown.click(true, true), thrown.click(true, false)), (Click::Place, Click::Nothing));
            // The entity it throws is the declared one of this item.
            let throws = &thrown.throws;
            let declared = json(content().join(format!("data/plerocottage/horizon/entity_type/{}.json", &throws.entity[13..])));
            assert_eq!(declared["item"].as_str(), Some(item.as_str()));
            // At the speed the item gives `shootFromRotation`, whichever way the thrower looks.
            for (pitch, yaw) in [(0.0, 0.0), (-30.0, 45.0), (60.0, -170.0), (-90.0, 12.0)] {
                let motion = shot(pitch, yaw, throws.roll, throws.speed);
                let speed = motion.iter().map(|v| v * v).sum::<f64>().sqrt();
                assert!((speed - f64::from(throws.speed)).abs() < 1e-6, "{motion:?}");
            }
            // Straight ahead is south, up is a negative pitch.
            let ahead = shot(0.0, 0.0, throws.roll, throws.speed);
            assert!(ahead[2] > 0.0 && ahead[0].abs() < 1e-6 && shot(-45.0, 0.0, throws.roll, throws.speed)[1] > 0.0);
            assert!(throws.speed > 1.5, "faster than a snowball, which the template throws at 1.5");
            assert_eq!(throws.inaccuracy, 0.0, "thrown straight: `shot` spreads nothing");
        }
    }

    #[test]
    fn a_mob_is_moving_by_its_speed_over_the_ground_and_its_walk() {
        use crate::display::is_moving;
        // Half the sum of the speeds along x and z, at GeckoLib's threshold or above.
        assert!(is_moving([0.03, 0.0, 0.0], 0.5) && is_moving([-0.016, 0.0, 0.016], 0.5));
        assert!(!is_moving([0.029, 0.0, 0.0], 0.5) && !is_moving([0.014, 0.0, -0.014], 0.5));
        // Falling is not moving, nor is sliding with still legs.
        assert!(!is_moving([0.0, -0.5, 0.0], 0.5) && !is_moving([0.2, 0.0, 0.2], 0.0));
    }

    // ---- Alfalfa ----

    #[test]
    fn alfalfa_halves_what_a_baby_has_left_to_grow() {
        assert!(!data().feeding.is_empty());
        for (item, feeding) in &data().feeding {
            let baby = feeding.baby.as_ref().unwrap_or_else(|| panic!("{item} feeds babies"));
            assert!(!baby.ages.is_empty() && baby.times > 0 && baby.takes > 0);
            for [age, left] in &baby.ages {
                assert_eq!(fed_age(*age), *left, "{item} at {age}");
            }
            assert_eq!(fed_key(), Key::from(baby.kept_as.as_str()));
            assert!(ParticleType::from_name(&baby.particles.id).is_some(), "{}", baby.particles.id);
            // A grown animal falls in love, unless it is already or has just bred.
            let adult = feeding.adult.as_ref().unwrap_or_else(|| panic!("{item} feeds grown animals"));
            assert!(adult.love && !adult.when_in_love && !adult.when_bred);
        }
    }

    // ---- Truffles ----

    fn digging() -> &'static Joining {
        data().joining.first().expect("a goal the mod gives a mob of Minecraft's")
    }

    #[test]
    fn pigs_get_the_digging_goal_at_the_priority_the_mod_gives_it() {
        let spec = digging();
        assert_eq!(type_of(&spec.class), "minecraft:pig");
        let goal = DigUp::new(spec);
        let flags: HashSet<&str> = spec.flags.iter().map(String::as_str).collect();
        assert_eq!(goal.flags() & goals::MOVE != 0, flags.contains("move"));
        assert_eq!(goal.flags() & goals::LOOK != 0, flags.contains("look"));
        assert_eq!(goal.flags() & (goals::JUMP | goals::TARGET), 0);
        // The numbers the class names are those it was found to act by.
        assert_eq!(spec.constants["SEARCH_RANGE"], f64::from(spec.range[0]));
        assert_eq!(spec.constants["DIG_TIME"], f64::from(spec.dig.ticks));
        assert_eq!(spec.constants["MAX_APPROACH_TICKS"] + 1.0, f64::from(spec.gives_up.after));
    }

    #[test]
    fn a_pig_finds_a_truffle_in_reach_walks_to_it_and_digs_it_up() {
        let spec = digging();
        let [rx, ry, rz] = spec.range;
        for (at, expected) in [
            ([rx, 64, 0], true),
            ([rx + 1, 64, 0], false),
            ([1, 64 + ry, 0], true),
            ([1, 64 + ry + 1, 0], false),
            ([0, 64, -rz], true),
            ([0, 64, -rz - 1], false),
        ] {
            let mut goal = DigUp::new(spec);
            // The wait before the next search: the least and what is rolled on top.
            assert_eq!(goal.search(HERE, |spread| spread - 1, |pos| pos == at), expected, "a truffle at {at:?}");
            assert_eq!(goal.cooldown, spec.cooldown.least + spec.cooldown.spread - 1);
            assert_eq!(goal.target, expected.then_some(at));
        }
        // The nearest of two.
        let mut goal = DigUp::new(spec);
        assert!(goal.search(HERE, |_| 0, |pos| pos == [6, 64, 0] || pos == [-3, 64, 2]));
        assert_eq!(goal.target, Some([-3, 64, 2]));
        // While it waits it does not look.
        let waited = (0..spec.cooldown.least).filter(|_| goal.search(HERE, |_| 0, |_| true)).count();
        assert_eq!(waited, 0);
        // Out of reach it walks, and gives the block up after a while, for a while.
        let far = [HERE[0] + 3.0 + spec.constants["REACH_DISTANCE_SQ"].sqrt(), 64.0, 2.5];
        let walked = (0..).take_while(|_| goal.step(far) == Some(Digging::Approach)).count() as i32;
        assert_eq!((walked + 1, goal.target, goal.cooldown), (spec.gives_up.after, None, spec.gives_up.cooldown));
        // In reach it digs for as long as the class says, grunting as often.
        let mut goal = DigUp::new(spec);
        let at = [2, 63, 0];
        assert!(goal.search(HERE, |_| 0, |pos| pos == at));
        let beside = [2.5 + spec.constants["REACH_DISTANCE_SQ"].sqrt() - 0.01, 64.0, 0.5];
        assert!(goal.goes_on(beside, |pos| pos == at) && !goal.goes_on(beside, |_| false));
        let mut grunts = Vec::new();
        let dug = (1..)
            .find(|&tick| match goal.step(beside) {
                Some(step @ (Digging::Dig { grunt } | Digging::Dug { grunt })) => {
                    if grunt {
                        grunts.push(tick);
                    }
                    matches!(step, Digging::Dug { .. })
                }
                other => panic!("{other:?}"),
            })
            .unwrap();
        assert_eq!(dug, spec.dig.ticks);
        let every = spec.dig.grunts.every;
        assert_eq!(grunts, (1..=spec.dig.ticks / every).map(|n| n * every).collect::<Vec<_>>());
        // Too far away it is given up.
        let away = [2.5 + spec.keeps_within_sqr.sqrt() + 0.01, 63.0, 0.5];
        assert!(!goal.goes_on(away, |pos| pos == at));
        // What digging leaves: another block of the mod's, its truffles, and the two sounds.
        assert_ne!(spec.dig.becomes, spec.block);
        let [least, most] = spec.dig.drops.count;
        assert!(0 < least && least <= most);
        let sounds: Vec<&str> = spec.dig.sounds.iter().map(|sound| sound.at.as_str()).collect();
        assert_eq!(sounds, ["block", "mob"]);
    }
}

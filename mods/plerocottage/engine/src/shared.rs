//! What the machines have in common, the oven included: how the mod's recipes are matched to the
//! slots (`RecipeMatcher`), how many times a recipe is made at once, and how its ingredients are
//! used up, worn or left behind (the loop every `craftItem` of the mod's block entities runs).
use crate::spec::{self, Recipe2};
use horizon_client::mod_api::ItemStack;
use std::rc::Rc;

pub type Slots = [Option<ItemStack>];

/// A block entity of the mod's: its slots and the fields its class saves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unit {
    pub items: Vec<Option<ItemStack>>,
    pub running: bool,
    pub cooking: i32,
    pub max: i32,
    /// The fermenter's `tickCount`, the saucepan's `stirsCount`, the fryer's `craftState`, the
    /// teapot's `playSoundCooldown`, the cutting board's `craftIndex`.
    pub count: i32,
    /// The teapot's `output` while it brews.
    pub pending: Option<ItemStack>,
    /// The fryer's `originalItem` of each basket.
    pub originals: Vec<Option<ItemStack>>,
    /// The fermenter's `recipeLightState`.
    pub light: Option<String>,
    /// The tank's fluid and millibuckets.
    pub tank: Option<(String, i32)>,
    /// The cutting board's `inCraft`.
    pub flag: bool,
    /// What lay in the saucepan when it was last looked at: its stirring starts over when that changes.
    pub seen: u64,
}

impl Unit {
    pub fn new(size: usize) -> Self {
        Self {
            items: vec![None; size],
            ..Self::default()
        }
    }

    pub fn tank_amount(&self) -> i32 {
        self.tank.as_ref().map_or(0, |t| t.1)
    }

    /// `FluidTank.drain`: what is taken out.
    pub fn drain(&mut self, amount: i32) -> i32 {
        let Some(tank) = self.tank.as_mut() else {
            return 0;
        };
        let taken = amount.min(tank.1);
        tank.1 -= taken;
        if tank.1 == 0 {
            self.tank = None;
        }
        taken
    }
}

/// What a machine reaches of the world around it.
pub trait Env {
    /// The recipes of a type of the mod's (`blender`), or the smoking recipes as a machine takes
    /// them over (`smoking`), in the order the game looks through them.
    fn recipes(&mut self, kind: &str) -> Rc<[Recipe2]>;
    /// Whether the block below heats the machine.
    fn heat(&mut self) -> bool;
    /// `Level.getRawBrightness(pos, 0)`.
    fn light(&mut self) -> u8;
    /// `level.random.nextFloat()`.
    fn next_float(&mut self) -> f32;
    /// `level.random.nextInt(bound)`.
    fn next_int(&mut self, bound: i32) -> i32;
    /// A sound at the block: the mod's own by its name, Minecraft's with its namespace.
    fn sound(&mut self, name: &str);
    /// `Block.popResourceFromFace` upwards, or `Containers.dropItemStack`.
    fn pop(&mut self, stack: ItemStack);
    /// A property of the machine's block state.
    fn property(&mut self, key: &str, value: bool);
    /// How many of an item stack, and how worn it may get (0: it does not wear). The game knows
    /// its own items; what the extraction found of the recipes' items stands in elsewhere.
    fn max_stack(&mut self, item: &str) -> u32 {
        fact(item).map_or(64, |f| f.max_stack)
    }
    fn max_damage(&mut self, item: &str) -> u32 {
        fact(item).map_or(0, |f| f.max_damage)
    }
}

pub fn fact(item: &str) -> Option<&'static spec::Fact> {
    spec::facts().get(item)
}

/// One of `PleroCottageCompat`'s tests (`DamageableMaterial`).
pub fn is(item: &str, test: &str) -> bool {
    fact(item).is_some_and(|f| f.is.iter().any(|t| t == test))
}

pub fn holds(slot: &Option<ItemStack>, items: &[String]) -> bool {
    slot.as_ref().is_some_and(|s| items.iter().any(|i| **i == *s.item))
}

/// `Ingredient.test`: an empty ingredient takes only an empty slot.
pub fn test(slot: &Option<ItemStack>, items: &[String]) -> bool {
    if items.is_empty() {
        slot.is_none()
    } else {
        holds(slot, items)
    }
}

pub fn take(slot: &mut Option<ItemStack>, count: u32) {
    if let Some(s) = slot {
        s.count = s.count.saturating_sub(count);
        if s.count == 0 {
            *slot = None;
        }
    }
}

/// `RecipeMatcher.findMatches` over the filled ones of `slots`: the slot each place takes, when
/// every filled slot meets a place of its own. Empty places stand for the empty slots.
pub fn assign(items: &Slots, slots: &[usize], places: &[Vec<String>]) -> Option<Vec<usize>> {
    let filled: Vec<usize> = slots
        .iter()
        .copied()
        .filter(|&s| items[s].is_some())
        .collect();
    let places: Vec<&Vec<String>> = places.iter().filter(|p| !p.is_empty()).collect();
    if filled.len() != places.len() {
        return None;
    }
    // Augmenting paths, as machines.py's stand-in for RecipeMatcher does.
    let mut taken: Vec<Option<usize>> = vec![None; places.len()];
    fn place(
        slot: usize,
        seen: &mut [bool],
        taken: &mut [Option<usize>],
        items: &Slots,
        places: &[&Vec<String>],
    ) -> bool {
        let item = &*items[slot].as_ref().expect("filled").item;
        for (target, members) in places.iter().enumerate() {
            if seen[target] || !members.iter().any(|m| m == item) {
                continue;
            }
            seen[target] = true;
            if taken[target].is_none_or(|other| place(other, seen, taken, items, places)) {
                taken[target] = Some(slot);
                return true;
            }
        }
        false
    }
    for &slot in &filled {
        let mut seen = vec![false; places.len()];
        if !place(slot, &mut seen, &mut taken, items, &places) {
            return None;
        }
    }
    taken.into_iter().collect()
}

pub fn matches(items: &Slots, slots: &[usize], places: &[Vec<String>]) -> bool {
    assign(items, slots, places).is_some()
}

/// How the container's slot counts when a recipe is made several times at once.
#[derive(Clone, Copy, PartialEq)]
pub enum Container {
    /// Like any other slot.
    Plain,
    /// Not at all: the recipe has no container.
    Skip,
    /// By the results it has room for: its count over the count of one result.
    Results,
}

/// `assemble`'s `multiple`: how many times the recipe is made, by what each filled slot of
/// `slots` has left (a worn material by its uses, anything else by its count), at most a stack of
/// the result.
pub fn multiple(
    items: &Slots,
    slots: &[usize],
    container: Option<(usize, Container)>,
    output: &(String, u32),
    count: u32,
    env: &mut dyn Env,
) -> u32 {
    let mut multiple = 64.min(env.max_stack(&output.0)) / count;
    for &slot in slots {
        let Some(stack) = &items[slot] else {
            continue;
        };
        match container {
            Some((at, Container::Skip)) if at == slot => continue,
            Some((at, Container::Results)) if at == slot => {
                multiple = multiple.min(stack.count / count);
                continue;
            }
            _ => {}
        }
        multiple = multiple.min(if is(&stack.item, "DamageableMaterial") {
            env.max_damage(&stack.item).saturating_sub(stack.damage)
        } else {
            stack.count
        });
    }
    multiple
}

/// `MachineUtil.getCraftRemaining`: what `count` of the stack leave, nothing for what wears.
pub fn remaining(stack: &ItemStack, count: u32, env: &mut dyn Env) -> Option<ItemStack> {
    if env.max_damage(&stack.item) > 0 || count == 0 {
        return None;
    }
    let left = fact(&stack.item)?.remainder.as_deref()?;
    Some(ItemStack::new(left, count))
}

/// `PleroCottageCompat.damageItem`: wears a stack that can wear, and breaks it at its last use.
pub fn wear(slot: &mut Option<ItemStack>, amount: u32, env: &mut dyn Env) {
    let Some(stack) = slot else {
        return;
    };
    let max = env.max_damage(&stack.item);
    if max == 0 {
        return;
    }
    stack.damage += amount;
    if stack.damage >= max {
        stack.damage = 0;
        take(slot, 1);
    }
}

/// The loop of the mod's `craftItem`: each filled slot of `slots`, in order, is worn when it is a
/// worn material and used up `times` over otherwise (the container by `container_takes` when
/// given), and leaves its remainder in its own slot when that emptied. The remainders that found
/// no place there are returned, for `insert`.
pub fn consume(
    items: &mut Slots,
    slots: &[usize],
    times: u32,
    container: Option<(usize, Option<u32>)>,
    env: &mut dyn Env,
) -> Vec<ItemStack> {
    let mut left = Vec::new();
    for &slot in slots {
        let Some(stack) = &items[slot] else {
            continue;
        };
        let takes = match container {
            Some((at, None)) if at == slot => continue,
            Some((at, Some(takes))) if at == slot => takes,
            _ => times,
        };
        let mut rest = remaining(stack, times, env);
        if is(&stack.item, "DamageableMaterial") {
            wear(&mut items[slot], times, env);
            if items[slot].is_some() {
                rest = None;
            }
        } else {
            take(&mut items[slot], takes);
        }
        if let Some(rest) = rest {
            if items[slot].is_none() {
                items[slot] = Some(rest);
            } else {
                left.push(rest);
            }
        }
    }
    left
}

/// `MachineUtil.insertCraftRemaining`, then `popCraftRemaining`: remainders go into the empty
/// ones of `slots` in order, and what is left over pops out of the block.
pub fn insert(items: &mut Slots, slots: &[usize], mut left: Vec<ItemStack>, env: &mut dyn Env) {
    for &slot in slots {
        if !left.is_empty() && items[slot].is_none() {
            items[slot] = Some(left.remove(0));
        }
    }
    for stack in left {
        env.pop(stack);
    }
}

/// The first of `recipes` that `matches`.
pub fn first<'a>(recipes: &'a [Recipe2], matches: impl Fn(&Recipe2) -> bool) -> Option<&'a Recipe2> {
    recipes.iter().find(|r| matches(r))
}

/// What lies in the slots, as one number: to tell whether a player changed it.
pub fn contents(items: &Slots) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for slot in items {
        match slot {
            Some(stack) => (&*stack.item, stack.count, stack.damage).hash(&mut hasher),
            None => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish() | 1
}

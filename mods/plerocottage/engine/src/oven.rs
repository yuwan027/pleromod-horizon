//! The oven (`OvenBlockEntity`): set its temperature and time with the two knobs, press start, and
//! when the time is up it bakes what the recipe in it makes, or cinders when the setting is not
//! the recipe's. The rules below are those `machines.json` and native.py's runs measured of the
//! mod's own code (tests.rs holds every one of them).
use crate::shared::{self, take};
use crate::spec::{self, Oven as Spec};
use horizon_client::mod_api::ItemStack;

pub const BLOCK: &str = "plerocottage:oven";

/// A recipe the oven can bake: each place's items, the container the result goes into, the
/// result, and the setting it wants. A smoking recipe the oven takes over bakes the whole stack.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub inputs: Vec<Vec<String>>,
    pub container: Vec<String>,
    pub output: (String, u32),
    pub temperature: i32,
    pub time: i32,
    pub smoking: bool,
}

impl Recipe {
    pub fn from_spec(recipe: &spec::Recipe, smoking: bool) -> Self {
        Self {
            inputs: recipe.input.clone(),
            container: recipe.container.clone(),
            output: recipe.output.clone(),
            temperature: recipe.temperature,
            time: recipe.cooking_time,
            smoking,
        }
    }
}

/// The knobs and the bake under way: `temperature` and `max_time` are the settings' numbers
/// (degrees, ticks), as the menu's data slots carry them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub temperature: i32,
    pub max_time: i32,
    pub cooking: i32,
    pub running: bool,
}

pub fn output_slot(spec: &Spec) -> usize {
    spec.slots["OUTPUT"]
}

pub fn container_slot(spec: &Spec) -> usize {
    spec.slots["CONTAINER"]
}

fn max_damage(spec: &Spec, item: &str) -> u32 {
    spec.items.get(item).map_or(0, |i| i.max_damage)
}

fn remainder<'a>(spec: &'a Spec, item: &str) -> Option<&'a str> {
    spec.items.get(item).and_then(|i| i.remainder.as_deref())
}

/// The slot each place of the recipe takes, when every filled input slot meets a place of its own.
fn assign(items: &[Option<ItemStack>], recipe: &Recipe, spec: &Spec) -> Option<Vec<usize>> {
    shared::assign(items, &spec.input_slots, &recipe.inputs)
}

/// The container slot holds the recipe's container, enough for one result.
fn container_ok(items: &[Option<ItemStack>], recipe: &Recipe, spec: &Spec) -> bool {
    if recipe.container.is_empty() {
        return true;
    }
    items[container_slot(spec)].as_ref().is_some_and(|s| {
        recipe.container.iter().any(|c| **c == *s.item) && s.count >= recipe.output.1
    })
}

/// The first recipe the oven's contents make, whatever the knobs say, and the slot of each place.
pub fn find<'a>(
    items: &[Option<ItemStack>],
    recipes: &'a [Recipe],
    spec: &Spec,
) -> Option<(&'a Recipe, Vec<usize>)> {
    recipes.iter().find_map(|recipe| {
        let places = assign(items, recipe, spec)?;
        container_ok(items, recipe, spec).then_some((recipe, places))
    })
}

/// The start button: the oven runs when both knobs are off zero, the result slot is empty and its
/// contents make a recipe.
pub fn start(
    state: &mut State,
    items: &[Option<ItemStack>],
    recipes: &[Recipe],
    spec: &Spec,
) -> bool {
    if state.running
        || state.temperature <= 0
        || state.max_time <= 0
        || items[output_slot(spec)].is_some()
        || find(items, recipes, spec).is_none()
    {
        return false;
    }
    state.running = true;
    state.cooking = 0;
    true
}

/// One server tick; true on the tick the bake ends (the oven dings).
pub fn tick(
    state: &mut State,
    items: &mut [Option<ItemStack>],
    recipes: &[Recipe],
    spec: &Spec,
) -> bool {
    if !state.running {
        return false;
    }
    state.cooking += 1;
    if state.cooking < state.max_time {
        return false;
    }
    state.running = false;
    state.cooking = 0;
    finish(state, items, recipes, spec);
    true
}

fn finish(state: &State, items: &mut [Option<ItemStack>], recipes: &[Recipe], spec: &Spec) {
    // What is in it now: the slots stay open to players while it bakes.
    let Some((recipe, places)) = find(items, recipes, spec) else {
        return;
    };
    let right = recipe.temperature == state.temperature && recipe.time == state.max_time;
    let stack = |slot: usize| items[slot].as_ref().expect("a place of the recipe");
    let times = if recipe.smoking {
        // A smoking recipe takes the whole stack, rightly set or not.
        places.iter().map(|&s| stack(s).count).min().unwrap_or(0)
    } else if right {
        // As many as every place has: a tool by the uses it has left, the container by results.
        let mut times = places
            .iter()
            .map(|&s| {
                let s = stack(s);
                match max_damage(spec, &s.item) {
                    0 => s.count,
                    max => max.saturating_sub(s.damage),
                }
            })
            .min()
            .unwrap_or(0);
        if !recipe.container.is_empty() {
            let held = items[container_slot(spec)].as_ref().map_or(0, |s| s.count);
            times = times.min(held / recipe.output.1);
        }
        times
    } else {
        1
    };
    consume(items, &places, times, spec);
    let made = if right {
        if !recipe.container.is_empty() {
            take(&mut items[container_slot(spec)], recipe.output.1 * times);
        }
        ItemStack::new(&recipe.output.0, recipe.output.1 * times)
    } else {
        ItemStack::new(&spec.failure, times)
    };
    items[output_slot(spec)] = Some(made);
}

/// Uses each place `times` over: a tool wears (and breaks at its last use), anything else is used
/// up and leaves its remainder (a bucket, a bottle) in a place of the recipe that emptied.
fn consume(items: &mut [Option<ItemStack>], places: &[usize], times: u32, spec: &Spec) {
    let mut left = Vec::new();
    let order: Vec<usize> = spec
        .input_slots
        .iter()
        .copied()
        .filter(|s| places.contains(s))
        .collect();
    for &slot in &order {
        let Some(s) = items[slot].as_mut() else {
            continue;
        };
        match max_damage(spec, &s.item) {
            0 => {
                if let Some(r) = remainder(spec, &s.item) {
                    left.push(ItemStack::new(r, times));
                }
                take(&mut items[slot], times);
            }
            max => {
                s.damage += times;
                if s.damage >= max {
                    items[slot] = None;
                }
            }
        }
    }
    // Into a place of the recipe that emptied, else another empty input slot; past that the mod
    // pops it out (popCraftRemaining), which no run reached.
    for stack in left {
        let free = |s: &usize| items[*s].is_none();
        if let Some(slot) = order
            .iter()
            .copied()
            .find(free)
            .or_else(|| spec.input_slots.iter().copied().find(free))
        {
            items[slot] = Some(stack);
        }
    }
}

/// The setting a knob turns to from `value`: the mod's knob sends the next state's number, and the
/// block entity's setter makes a setting of it.
pub fn turned(spec: &Spec, temperature_knob: bool, value: i32) -> i32 {
    let (settings, values) = if temperature_knob {
        (&spec.settings.temperature, &spec.knobs.temperature)
    } else {
        (
            &spec.settings.max_cooking_time,
            &spec.knobs.max_cooking_time,
        )
    };
    let state = settings
        .iter()
        .find(|s| s.value == value)
        .map_or(0, |s| s.state);
    let sent = (state + 1) % settings.len() as i32;
    values.get(&sent.to_string()).copied().unwrap_or(value)
}

#[cfg(test)]
mod tests;

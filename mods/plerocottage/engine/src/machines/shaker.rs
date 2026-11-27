//! `ShakerItem` and `ShakerRecipe`: the shaker is an item that holds up to eight ingredients, four to a place. A
//! right click in the inventory fills and empties it as a bundle's does; shaking it makes its recipe as often as
//! the ingredients, the cups in the other hand and a stack of the result allow, at most its places' limit, and
//! the result takes the cups' place in the other hand. `crate::shaker` is its half with the game.
use super::*;

/// How often the recipe is made at once, and how many of each slot that uses.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub batches: u32,
    pub used: Vec<u32>,
}

pub fn plan(
    recipe: &Recipe,
    contents: &[Option<ItemStack>],
    offhand: &Option<ItemStack>,
    spec: &spec::Shaker,
    env: &mut dyn Env,
) -> Option<Plan> {
    plan_within(recipe, contents, offhand, limit(spec), env)
}

/// `SLOT_LIMIT`: how many of a thing a place of the shaker holds, and how often a shake makes its recipe at most.
pub fn limit(spec: &spec::Shaker) -> u32 {
    spec.constants["SLOT_LIMIT"] as u32
}

/// `ShakerRecipe.matches`: a plan for one.
pub fn matches(recipe: &Recipe, contents: &[Option<ItemStack>], offhand: &Option<ItemStack>, env: &mut dyn Env) -> bool {
    plan_within(recipe, contents, offhand, 1, env).is_some()
}

fn plan_within(
    recipe: &Recipe,
    contents: &[Option<ItemStack>],
    offhand: &Option<ItemStack>,
    limit: u32,
    env: &mut dyn Env,
) -> Option<Plan> {
    let out = output(recipe);
    let mut most = limit.min(env.max_stack(&out.0) / out.1);
    if !recipe.container.is_empty() {
        if !shared::holds(offhand, &recipe.container) {
            return None;
        }
        most = most.min(offhand.as_ref().map_or(0, |o| o.count));
    }
    (1..=most).rev().find_map(|batches| {
        allocate(recipe, contents, batches, env).map(|used| Plan { batches, used })
    })
}

/// `allocate`: every place of the recipe, `batches` times over, gets a unit of a slot that will do for it.
fn allocate(recipe: &Recipe, contents: &[Option<ItemStack>], batches: u32, env: &mut dyn Env) -> Option<Vec<u32>> {
    let demands: Vec<&Vec<String>> = recipe
        .input
        .iter()
        .filter(|place| !place.is_empty())
        .flat_map(|place| std::iter::repeat_n(place, batches as usize))
        .collect();
    let mut units = Vec::new();
    for (slot, stack) in contents.iter().enumerate() {
        let Some(stack) = stack else {
            continue;
        };
        let available = if shared::is(&stack.item, "DamageableMaterial") {
            env.max_damage(&stack.item).saturating_sub(stack.damage)
        } else {
            stack.count
        };
        units.extend(std::iter::repeat_n(slot, (available as usize).min(demands.len())));
    }
    if units.len() < demands.len() {
        return None;
    }
    let fits = |demand: usize, slot: usize| shared::holds(&contents[slot], demands[demand]);
    fn assign(
        demand: usize,
        units: &[usize],
        owners: &mut [Option<usize>],
        visited: &mut [bool],
        fits: &dyn Fn(usize, usize) -> bool,
    ) -> bool {
        for unit in 0..owners.len() {
            if visited[unit] || !fits(demand, units[unit]) {
                continue;
            }
            visited[unit] = true;
            if owners[unit].is_none_or(|other| assign(other, units, owners, visited, fits)) {
                owners[unit] = Some(demand);
                return true;
            }
        }
        false
    }
    let mut owners = vec![None; units.len()];
    for demand in 0..demands.len() {
        if !assign(demand, &units, &mut owners, &mut vec![false; units.len()], &fits) {
            return None;
        }
    }
    let mut used = vec![0; contents.len()];
    for (unit, owner) in owners.iter().enumerate() {
        if owner.is_some() {
            used[units[unit]] += 1;
        }
    }
    Some(used)
}

/// What a shake leaves (`ShakerItem.craft`).
#[derive(Debug, PartialEq)]
pub struct Shaken {
    pub contents: Vec<Option<ItemStack>>,
    /// What the other hand holds afterwards: the result, where the recipe takes cups.
    pub offhand: Option<ItemStack>,
    /// What goes into the inventory, in the mod's order: the cups not used and what they leave, or the result
    /// of a recipe without cups; then what the ingredients leave that found no place in the shaker.
    pub given: Vec<ItemStack>,
}

/// `ShakerItem.craft`: the first recipe that matches is made as often as its plan says. None when nothing matches.
pub fn craft(
    recipes: &[Recipe],
    contents: &[Option<ItemStack>],
    offhand: &Option<ItemStack>,
    spec: &spec::Shaker,
    env: &mut dyn Env,
) -> Option<Shaken> {
    let mut found = None;
    for recipe in recipes {
        if matches(recipe, contents, offhand, env) {
            found = Some(recipe);
            break;
        }
    }
    let recipe = found?;
    let plan = plan(recipe, contents, offhand, spec, env)?;
    let out = output(recipe);
    let made = ItemStack::new(&out.0, out.1 * plan.batches);
    let (mut contents, mut left) = (contents.to_vec(), Vec::new());
    for slot in 0..contents.len() {
        let used = plan.used[slot];
        let Some(material) = contents[slot].clone().filter(|_| used > 0) else {
            continue;
        };
        let mut rest = shared::remaining(&material, used, env);
        if shared::is(&material.item, "DamageableMaterial") {
            shared::wear(&mut contents[slot], used, env);
            if contents[slot].is_some() {
                rest = None;
            }
        } else {
            shared::take(&mut contents[slot], used);
        }
        if let Some(mut rest) = rest {
            if contents[slot].is_none() {
                // `contents.insertItem(slot, remainder)`: as many as the place holds.
                let put = rest.count.min(limit(spec)).min(env.max_stack(&rest.item));
                contents[slot] = Some(ItemStack::new(&rest.item, put));
                rest.count -= put;
            }
            if rest.count > 0 {
                left.push(rest);
            }
        }
    }
    let mut given = Vec::new();
    let offhand = match offhand {
        Some(cups) if !recipe.container.is_empty() => {
            if cups.count > plan.batches {
                let mut unused = cups.clone();
                unused.count -= plan.batches;
                given.push(unused);
            }
            given.extend(shared::remaining(cups, plan.batches, env));
            Some(made)
        }
        other => {
            given.push(made);
            other.clone()
        }
    };
    given.extend(left);
    Some(Shaken { contents, offhand, given })
}

/// `ItemHandlerHelper.canItemStacksStack`: the same item with the same wear.
fn stacks(one: &ItemStack, other: &ItemStack) -> bool {
    one.item == other.item && one.damage == other.damage
}

/// How many more of `incoming` the place `slot` takes: up to the limit, or what stacks of it.
fn room(contents: &[Option<ItemStack>], slot: usize, incoming: &ItemStack, limit: u32, env: &mut dyn Env) -> u32 {
    match &contents[slot] {
        None => limit.min(env.max_stack(&incoming.item)),
        Some(held) if stacks(held, incoming) => limit.min(env.max_stack(&held.item)).saturating_sub(held.count),
        Some(_) => 0,
    }
}

/// `nextInsertionSlot`: the first place that holds the same thing with room for more, else the first empty one.
pub fn insertion_slot(contents: &[Option<ItemStack>], incoming: &ItemStack, limit: u32, env: &mut dyn Env) -> Option<usize> {
    for slot in 0..contents.len() {
        if contents[slot].is_some() && room(contents, slot, incoming, limit, env) > 0 {
            return Some(slot);
        }
    }
    contents.iter().position(Option::is_none)
}

/// `nextExtractionSlot` with nothing selected: the last place that holds something.
pub fn extraction_slot(contents: &[Option<ItemStack>]) -> Option<usize> {
    contents.iter().rposition(Option::is_some)
}

/// Puts as many of `from` as fit into the place `nextInsertionSlot` names. False when nothing moved.
fn insert(contents: &mut [Option<ItemStack>], from: &mut Option<ItemStack>, limit: u32, env: &mut dyn Env) -> bool {
    let Some(incoming) = from.clone() else {
        return false;
    };
    let Some(target) = insertion_slot(contents, &incoming, limit, env) else {
        return false;
    };
    let moved = incoming.count.min(room(contents, target, &incoming, limit, env));
    if moved == 0 {
        return false;
    }
    match &mut contents[target] {
        Some(held) => held.count += moved,
        empty => {
            let mut laid = incoming;
            laid.count = moved;
            *empty = Some(laid);
        }
    }
    shared::take(from, moved);
    true
}

/// `overrideStackedOnOther`: a right click with the shaker on a slot that holds something takes as much of it as
/// the shaker's next place has room for. False when the click is the game's own.
pub fn onto_slot(contents: &mut [Option<ItemStack>], slot: &mut Option<ItemStack>, limit: u32, env: &mut dyn Env) -> bool {
    insert(contents, slot, limit, env)
}

/// `overrideOtherStackedOnMe`: a right click on the shaker with an empty cursor takes the last thing out of it,
/// and with something on the cursor puts as much of that in as fits. False when the click is the game's own.
pub fn onto_shaker(contents: &mut [Option<ItemStack>], carried: &mut Option<ItemStack>, limit: u32, env: &mut dyn Env) -> bool {
    if carried.is_some() {
        return insert(contents, carried, limit, env);
    }
    let Some(source) = extraction_slot(contents) else {
        return false;
    };
    *carried = contents[source].take();
    true
}

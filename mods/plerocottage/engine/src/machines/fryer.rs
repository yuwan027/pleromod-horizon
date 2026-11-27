//! `FryerBlockEntity`: fries what lies in its four baskets in oil, each on its own. What has a
//! quick recipe is done at the first time, what has a slow one at the second, and whatever is
//! still in it at the third is burnt.
use super::*;

pub struct Fryer;

const OIL_FOR_A_START: i32 = 250;

fn finds<'a>(recipes: &'a [Recipe], stack: &ItemStack) -> Option<&'a Recipe> {
    recipes
        .iter()
        .find(|r| r.input.first().is_some_and(|p| p.iter().any(|i| **i == *stack.item)))
}

/// `MachineUtil.handlerFluidSlot`: the buckets in the tank's slot pour into the tank, one after the
/// other while a whole one fits, and leave empty buckets.
pub fn pour(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let (slot, Some(tank)) = (spec.slot("FLUID_TANK_SLOT"), &spec.tank) else {
        return;
    };
    let Some(stack) = &unit.items[slot] else {
        return;
    };
    let Some(fluid) = tank.buckets.get(&*stack.item) else {
        return;
    };
    if !tank.accepts.is_empty() && !tank.accepts.contains(fluid) {
        return;
    }
    let mut poured = 0;
    for _ in 0..stack.count {
        let fits = match &unit.tank {
            Some((held, amount)) => held == fluid && tank.capacity - amount >= 1000,
            None => tank.capacity >= 1000,
        };
        if !fits {
            break;
        }
        let amount = unit.tank_amount();
        unit.tank = Some((fluid.clone(), amount + 1000));
        poured += 1;
    }
    if poured == 0 {
        return;
    }
    let empty = ItemStack::new("minecraft:bucket", poured);
    shared::take(&mut unit.items[slot], poured);
    if unit.items[slot].is_none() {
        unit.items[slot] = Some(empty);
    } else {
        env.pop(empty);
    }
    // The mod plays the fluid's own bucket sound here (`FluidType.getSound`), which the extraction has not got.
    env.property("has_oil", unit.tank.is_some());
}

fn start(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    if unit.running || unit.tank_amount() < OIL_FOR_A_START {
        return;
    }
    let (fast, smoking, slow) = (
        env.recipes("fast_fry"),
        env.recipes("smoking"),
        env.recipes("slow_fry"),
    );
    let (mut count, mut fries) = (0, false);
    for &slot in spec.slots("INPUT_SLOTS") {
        let Some(stack) = &unit.items[slot] else {
            continue;
        };
        count += stack.count as i32;
        fries = fries
            || finds(&fast, stack).is_some()
            || finds(&smoking, stack).is_some()
            || finds(&slow, stack).is_some();
    }
    if count == 0 || !fries {
        return;
    }
    unit.running = true;
    unit.cooking = 0;
    // The more lies in it, the likelier a start costs oil.
    if env.next_int(10000) < count * 625 {
        unit.drain(OIL_FOR_A_START);
        env.property("has_oil", unit.tank.is_some());
        pour(unit, env, spec);
    }
}

pub fn stop(unit: &mut Unit, spec: &Spec) {
    unit.running = false;
    unit.cooking = 0;
    unit.count = 0;
    remember(unit, spec);
}

/// `FryerCraftContainer.reset`: what lies in each basket now.
fn remember(unit: &mut Unit, spec: &Spec) {
    unit.originals = spec
        .slots("INPUT_SLOTS")
        .iter()
        .map(|&s| unit.items[s].clone())
        .collect();
}

/// What a basket's content turns into, as many as there are, and what it leaves.
fn turn(unit: &mut Unit, env: &mut dyn Env, slot: usize, into: &str, left: &mut Vec<ItemStack>) {
    let Some(stack) = &unit.items[slot] else {
        return;
    };
    left.extend(shared::remaining(stack, stack.count, env));
    unit.items[slot] = Some(ItemStack::new(into, stack.count));
}

fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let time = |name: &str| spec.number(name);
    let (fast, slow, fail) = (
        time("FAST_COOKING_TIME"),
        time("SLOW_COOKING_TIME"),
        time("FAIL_COOKING_TIME"),
    );
    let stage = if unit.count == 0 && unit.cooking >= fast && unit.cooking < slow {
        0
    } else if unit.count == 1 && unit.cooking >= slow && unit.cooking < fail {
        1
    } else if unit.count == 2 && unit.cooking >= fail {
        2
    } else {
        return;
    };
    let inputs = spec.slots("INPUT_SLOTS");
    let slow_recipes = env.recipes("slow_fry");
    let burnt = spec.failure.as_deref().expect("fryer.json names what burnt things turn into");
    let mut left = Vec::new();
    unit.originals.resize(inputs.len(), None);
    for (basket, &slot) in inputs.iter().enumerate() {
        let Some(stack) = unit.items[slot].clone() else {
            continue;
        };
        match stage {
            0 => {
                unit.originals[basket] = Some(stack.clone());
                let quick = env.recipes("fast_fry");
                let smoked = env.recipes("smoking");
                if let Some(recipe) = finds(&quick, &stack).or_else(|| finds(&smoked, &stack)) {
                    turn(unit, env, slot, &output(recipe).0, &mut left);
                } else if finds(&slow_recipes, &stack).is_none() {
                    turn(unit, env, slot, burnt, &mut left);
                }
            }
            1 => {
                let recipe = unit.originals[basket]
                    .as_ref()
                    .and_then(|original| finds(&slow_recipes, original));
                turn(unit, env, slot, recipe.map_or(burnt, |r| &output(r).0), &mut left);
            }
            _ => turn(unit, env, slot, burnt, &mut left),
        }
    }
    for stack in left {
        env.pop(stack);
    }
    unit.count += 1;
    if stage == 2 {
        stop(unit, spec);
    }
}

impl Kind for Fryer {
    fn spec(&self) -> &'static Spec {
        spec::fryer()
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        let spec = self.spec();
        // The mod pours when the slot is set; here it is looked at each tick.
        pour(unit, env, spec);
        if !unit.running {
            return;
        }
        unit.cooking += 1;
        craft(unit, env, spec);
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        match packet {
            "FryingStartPacket" => start(unit, env, self.spec()),
            "StopFryingPacket" => stop(unit, self.spec()),
            _ => {}
        }
    }
}

//! The machines besides the oven. Each follows its block entity of the mod's (`BlenderBlockEntity`
//! and the others): what its start button checks, what a tick does, what is made and what is
//! left. `tests.rs` makes every run `machines.json` and native.py measured of the mod's own code.
use crate::shared::{self, Container, Env, Unit};
use crate::spec::{self, Machine as Spec, Recipe2 as Recipe};
use horizon_client::mod_api::ItemStack;

pub mod blender;
pub mod cutting_board;
pub mod fermenter;
pub mod fryer;
pub mod ice_cream;
pub mod saucepan;
pub mod shaker;
pub mod stock_pot;
pub mod teapot;

/// A machine of the mod's: its data, and what its block entity does.
pub trait Kind: Sync {
    fn spec(&self) -> &'static Spec;
    /// Whether its block entity has a server tick.
    fn ticks(&self) -> bool {
        true
    }
    fn tick(&self, _unit: &mut Unit, _env: &mut dyn Env) {}
    /// A packet of the mod's that a button of the screen sends (`BlenderStartPacket`).
    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str);
    /// `ContainerOpenersCounter`: the first player opened its menu, or the last closed it.
    fn opened(&self, _unit: &Unit, _env: &mut dyn Env, _open: bool) {}
    /// A new block entity.
    fn new_unit(&self) -> Unit {
        let spec = self.spec();
        let field = |name: &str| spec.fields.get(name).copied().unwrap_or(0);
        Unit {
            max: field("maxCookingTime"),
            ..Unit::new(spec.size)
        }
    }
}

pub fn all() -> [&'static dyn Kind; 7] {
    [
        &blender::Blender,
        &stock_pot::StockPot,
        &teapot::Teapot,
        &saucepan::Saucepan,
        &fryer::Fryer,
        &ice_cream::IceCream,
        &fermenter::Fermenter,
    ]
}

pub fn of(block: &str) -> Option<&'static dyn Kind> {
    all().into_iter().find(|k| k.spec().block == block)
}

fn output(recipe: &Recipe) -> &(String, u32) {
    recipe.output.as_ref().expect("a recipe makes something")
}

/// `setOpen`: a machine that runs is shut.
fn set_open(unit: &Unit, env: &mut dyn Env, open: bool) {
    env.property("open", !unit.running && open);
}

/// The mod's `TwoInputRecipe.isInputMatch`.
fn two_inputs(unit: &Unit, spec: &Spec, recipe: &Recipe) -> bool {
    shared::matches(&unit.items, spec.slots("MAIN_INPUT_SLOTS"), &recipe.main_input)
        && shared::matches(
            &unit.items,
            spec.slots("SECONDARY_INPUT_SLOTS"),
            &recipe.secondary_input,
        )
}

/// What `craftItem` does once a recipe's result is known: the ingredients are used, the result
/// lies in `result`, remainders go back into `inputs` or pop out. `previous` is what lay in the
/// result's slot, which pops out too.
fn finish(
    unit: &mut Unit,
    env: &mut dyn Env,
    inputs: &[usize],
    left: Vec<ItemStack>,
    result: usize,
    made: ItemStack,
    pops_previous: bool,
) {
    let previous = unit.items[result].take();
    unit.items[result] = Some(made);
    let mut left = left;
    // insertCraftRemaining first, then what the result's slot held joins what pops out.
    for &slot in inputs {
        if !left.is_empty() && unit.items[slot].is_none() {
            unit.items[slot] = Some(left.remove(0));
        }
    }
    if pops_previous {
        left.extend(previous);
    }
    for stack in left {
        env.pop(stack);
    }
}

#[cfg(test)]
mod tests;

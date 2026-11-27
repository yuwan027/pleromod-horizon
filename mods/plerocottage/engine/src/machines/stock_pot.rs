//! `StockPotBlockEntity`: cooks only over heat, for as long as its recipe says, and can be
//! stopped.
use super::*;

pub struct StockPot;

fn find<'a>(unit: &Unit, spec: &Spec, recipes: &'a [Recipe]) -> Option<&'a Recipe> {
    shared::first(recipes, |r| {
        shared::test(&unit.items[spec.slot("CONTAINER")], &r.container) && two_inputs(unit, spec, r)
    })
}

fn start(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    if unit.running || unit.items[spec.slot("RESULT")].is_some() || !env.heat() {
        return;
    }
    let recipes = env.recipes("stock_pot");
    let Some(recipe) = find(unit, spec, &recipes) else {
        return;
    };
    unit.running = true;
    unit.cooking = 0;
    unit.max = recipe.max_cooking_time;
    set_open(unit, env, false);
}

fn stop(unit: &mut Unit) {
    unit.running = false;
    unit.cooking = 0;
}

fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let recipes = env.recipes("stock_pot");
    if let Some(recipe) = find(unit, spec, &recipes) {
        let inputs = spec.slots("INPUT_SLOTS");
        let out = output(recipe);
        let times = shared::multiple(&unit.items, inputs, None, out, out.1, env);
        let left = shared::consume(&mut unit.items, inputs, times, None, env);
        let made = ItemStack::new(&out.0, out.1 * times);
        finish(unit, env, inputs, left, spec.slot("RESULT"), made, true);
        env.sound("stock_pot_cover_moving");
    }
    stop(unit);
}

impl Kind for StockPot {
    fn spec(&self) -> &'static Spec {
        spec::stock_pot()
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        if !unit.running {
            return;
        }
        if env.heat() {
            unit.cooking += 1;
        }
        if unit.cooking >= unit.max {
            craft(unit, env, self.spec());
        }
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        match packet {
            "StockPotStartPacket" => start(unit, env, self.spec()),
            "StockPotStopPacket" => stop(unit),
            _ => {}
        }
    }

    fn opened(&self, unit: &Unit, env: &mut dyn Env, open: bool) {
        env.sound("stock_pot_cover_moving");
        set_open(unit, env, open);
    }
}

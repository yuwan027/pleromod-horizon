//! `SaucepanAndWhiskBlockEntity`: no clock, the whisk is stirred by hand until the recipe is made.
use super::*;

pub struct Saucepan;

fn find<'a>(unit: &Unit, env: &mut dyn Env, spec: &Spec, recipes: &'a [Recipe]) -> Option<&'a Recipe> {
    let heat = env.heat();
    shared::first(recipes, |r| {
        let heated = match r.heat_state.as_deref() {
            Some("NO_CHECK") => true,
            Some("NO_NEED") => !heat,
            _ => heat,
        };
        heated
            && shared::test(&unit.items[spec.slot("CONTAINER")], &r.container)
            && unit.items[spec.slot("RESULT")].is_none()
            && two_inputs(unit, spec, r)
    })
}

/// `setStirsCount`.
fn set_stirs(unit: &mut Unit, spec: &Spec, value: i32) {
    unit.count = if value > spec.number("MAX_STIRS_COUNT") {
        0
    } else {
        value
    };
}

pub fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let recipes = env.recipes("saucepan_and_whisk");
    if let Some(recipe) = find(unit, env, spec, &recipes) {
        let inputs = spec.slots("INPUT_SLOTS");
        let out = output(recipe);
        let times = shared::multiple(&unit.items, inputs, None, out, out.1, env);
        let left = shared::consume(&mut unit.items, inputs, times, None, env);
        let made = ItemStack::new(&out.0, out.1 * times);
        finish(unit, env, inputs, left, spec.slot("RESULT"), made, true);
    }
    unit.count = 0;
}

impl Kind for Saucepan {
    fn spec(&self) -> &'static Spec {
        spec::saucepan_and_whisk()
    }

    fn tick(&self, unit: &mut Unit, _env: &mut dyn Env) {
        // The mod's block entity has no tick: its `setItem` and `removeItem` start the stirring over. Here
        // what lies in it is looked at each tick instead.
        let now = shared::contents(&unit.items);
        if unit.seen != now {
            if unit.seen != 0 {
                unit.count = 0;
            }
            unit.seen = now;
        }
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        if packet != "TrySaucepanAndWhiskCraftPacket" {
            return;
        }
        let spec = self.spec();
        // The screen's whisk sends nothing unless a recipe matches.
        let recipes = env.recipes("saucepan_and_whisk");
        if find(unit, env, spec, &recipes).is_none() {
            return;
        }
        let max = spec.number("MAX_STIRS_COUNT");
        set_stirs(unit, spec, unit.count + 1);
        if unit.count >= max {
            env.sound("saucepan_and_whisk_done");
            craft(unit, env, spec);
            set_stirs(unit, spec, max);
            unit.seen = shared::contents(&unit.items);
        } else if unit.count % 2 == 1 {
            env.sound("saucepan_and_whisk_stirs_1");
        } else {
            env.sound("saucepan_and_whisk_stirs_2");
        }
    }
}

//! `BlenderBlockEntity`: runs sixty ticks, then makes its recipe as often as the ingredients
//! allow, with a second result that each time is rolled for.
use super::*;

pub struct Blender;

fn find<'a>(unit: &Unit, spec: &Spec, recipes: &'a [Recipe]) -> Option<&'a Recipe> {
    let container = &unit.items[spec.slot("CONTAINER")];
    shared::first(recipes, |r| {
        let out = output(r).1;
        (r.container.is_empty()
            || shared::holds(container, &r.container)
                && container.as_ref().is_some_and(|c| c.count >= out))
            && two_inputs(unit, spec, r)
    })
}

fn start(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    if unit.running
        || unit.items[spec.slot("MAIN_OUTPUT")].is_some()
        || unit.items[spec.slot("SECONDARY_OUTPUT")].is_some()
        || find(unit, spec, &env.recipes("blender")).is_none()
    {
        return;
    }
    env.sound("blender_running");
    unit.running = true;
    unit.cooking = 0;
    set_open(unit, env, false);
}

fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let recipes = env.recipes("blender");
    if let Some(recipe) = find(unit, spec, &recipes) {
        let (container, inputs) = (spec.slot("CONTAINER"), spec.slots("INPUT_SLOTS"));
        let out = output(recipe);
        // BlenderRecipe.assemble: one making gives the larger of the two results' counts.
        let count = out.1.max(recipe.outgrowth.as_ref().map_or(0, |o| o.1));
        let has = !recipe.container.is_empty();
        let mode = if has {
            Container::Results
        } else {
            Container::Skip
        };
        let times = shared::multiple(&unit.items, inputs, Some((container, mode)), out, count, env);
        let made = ItemStack::new(&out.0, count * times);
        let left = shared::consume(
            &mut unit.items,
            inputs,
            times,
            Some((container, has.then_some(made.count))), env);
        env.sound(if shared::is(&made.item, "OutputFluid") {
            "blender_water_done"
        } else if shared::is(&made.item, "OutputPowder") {
            "blender_powder_done"
        } else {
            "minecraft:block.wool.place"
        });
        unit.items[spec.slot("MAIN_OUTPUT")] = Some(made);
        // HasOutgrowthRecipe.rollForOutgrowth: one roll for each making.
        if let Some(outgrowth) = &recipe.outgrowth {
            let rolled = (0..times)
                .filter(|_| env.next_float() < recipe.outgrowth_chance)
                .count() as u32;
            if rolled > 0 {
                unit.items[spec.slot("SECONDARY_OUTPUT")] =
                    Some(ItemStack::new(&outgrowth.0, outgrowth.1 * rolled));
            }
        }
        shared::insert(&mut unit.items, inputs, left, env);
    }
    unit.running = false;
    unit.cooking = 0;
}

impl Kind for Blender {
    fn spec(&self) -> &'static Spec {
        spec::blender()
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        if !unit.running {
            return;
        }
        unit.cooking += 1;
        if unit.cooking >= unit.max {
            craft(unit, env, self.spec());
        }
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        if packet == "BlenderStartPacket" {
            start(unit, env, self.spec());
        }
    }

    fn opened(&self, unit: &Unit, env: &mut dyn Env, open: bool) {
        env.sound("blender_open");
        set_open(unit, env, open);
    }
}

//! `TeapotBlockEntity`: takes its ingredients when it starts and gives the drink sixty ticks
//! later.
use super::*;

pub struct Teapot;

fn find<'a>(unit: &Unit, spec: &Spec, recipes: &'a [Recipe]) -> Option<&'a Recipe> {
    let items = &unit.items;
    let container = &items[spec.slot("CONTAINER")];
    shared::first(recipes, |r| {
        items[spec.slot("RESULT")].is_none()
            && shared::test(container, &r.container)
            && container.as_ref().map_or(0, |c| c.count) >= output(r).1
            && r.input.len() >= 2
            && shared::test(&items[spec.slot("DRINK")], &r.input[0])
            && shared::test(&items[spec.slot("MATERIAL")], &r.input[1])
            && shared::matches(items, spec.slots("INPUT_SLOTS"), &r.input)
    })
}

fn start(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let filled = |name: &str| unit.items[spec.slot(name)].is_some();
    if unit.running
        || filled("RESULT")
        || !filled("CONTAINER")
        || !filled("DRINK")
        || !filled("MATERIAL")
    {
        return;
    }
    let recipes = env.recipes("teapot");
    let Some(recipe) = find(unit, spec, &recipes) else {
        return;
    };
    unit.running = true;
    unit.count = 0;
    unit.cooking = 0;
    // handlerRecipe: the drink is made now and lies in the result's slot when the time is up.
    let (container, inputs, out) = (
        spec.slot("CONTAINER"),
        spec.slots("INPUT_SLOTS"),
        output(recipe),
    );
    let held = unit.items[container].as_ref().map_or(0, |c| c.count);
    let times = shared::multiple(&unit.items, inputs, None, out, out.1, env).min(held / out.1);
    let made = ItemStack::new(&out.0, out.1 * times);
    let every: Vec<usize> = (0..spec.slot("RESULT")).collect();
    let left = shared::consume(
        &mut unit.items,
        &every,
        times,
        Some((container, Some(made.count))), env);
    unit.pending = Some(made);
    shared::insert(&mut unit.items, inputs, left, env);
    set_open(unit, env, false);
}

impl Kind for Teapot {
    fn spec(&self) -> &'static Spec {
        spec::teapot()
    }

    fn new_unit(&self) -> Unit {
        let spec = self.spec();
        Unit {
            max: spec.fields["maxCookingTime"],
            count: spec.fields["playSoundCooldown"],
            ..Unit::new(spec.size)
        }
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        if !unit.running {
            return;
        }
        let spec = self.spec();
        unit.cooking += 1;
        if unit.cooking >= unit.max {
            unit.items[spec.slot("RESULT")] = unit.pending.take();
            unit.running = false;
            unit.cooking = 0;
            env.sound("teapot_done");
        }
        if unit.count <= 0 {
            env.sound("teapot_running");
            unit.count += spec.number("SOUND_TIME");
        }
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        if packet == "BrewingStartPacket" {
            start(unit, env, self.spec());
        }
    }

    fn opened(&self, unit: &Unit, env: &mut dyn Env, open: bool) {
        env.sound(if open { "teapot_open" } else { "teapot_close" });
        set_open(unit, env, open);
    }
}

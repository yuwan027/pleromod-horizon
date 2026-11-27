//! `FermenterBlockEntity`: ferments for the recipe's seconds in the light the recipe wants, faster
//! with a catalyst, and makes mold when the light changed or the recipe is gone.
use super::*;

pub struct Fermenter;

const KINDS: [&str; 3] = ["dim_fermentation", "normal_fermentation", "bright_fermentation"];

/// `getLightState`.
fn light_state(env: &mut dyn Env) -> &'static str {
    match env.light() {
        0..=5 => "DIM",
        6..=10 => "NORMAL",
        _ => "BRIGHT",
    }
}

/// `getPresentRecipe`: the dim recipes first, then the normal, then the bright; each only in its light.
fn find(unit: &Unit, env: &mut dyn Env, spec: &Spec) -> Option<Recipe> {
    let light = light_state(env);
    KINDS.iter().find_map(|kind| {
        let recipes = env.recipes(kind);
        shared::first(&recipes, |r| {
            r.light_state.as_deref() == Some(light)
                && (r.container.is_empty()
                    || shared::holds(&unit.items[spec.slot("CONTAINER")], &r.container))
                && shared::matches(&unit.items, spec.slots("MATERIAL_SLOTS"), &r.input)
        })
        .cloned()
    })
}

/// `handleMaxCookingTime`: each catalyst slot's item is used and shortens the time by its kind.
fn with_catalysts(unit: &mut Unit, spec: &Spec, mut value: i32) -> i32 {
    for slot in ["CATALYSTS_TOP", "CATALYSTS_BOTTOM"].map(|name| spec.slot(name)) {
        let Some(stack) = &unit.items[slot] else {
            continue;
        };
        let effect = ["A", "B", "C", "X"].into_iter().find_map(|kind| {
            shared::is(&stack.item, &format!("Catalysts{kind}"))
                .then(|| spec.constants[&format!("CATALYST_{kind}_EFFECT")])
        });
        if let Some(effect) = effect {
            shared::take(&mut unit.items[slot], 1);
            value = (f64::from(value) * effect) as i32;
        }
    }
    value.max(1)
}

fn start(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    if unit.running || unit.items[spec.slot("OUTPUT")].is_some() {
        return;
    }
    let Some(recipe) = find(unit, env, spec) else {
        return;
    };
    unit.light = recipe.light_state.clone();
    env.sound("minecraft:block.barrel.close");
    set_open(unit, env, false);
    unit.running = true;
    unit.cooking = 1;
    unit.max = with_catalysts(unit, spec, recipe.max_cooking_time);
}

fn stop(unit: &mut Unit, env: &mut dyn Env) {
    env.sound("minecraft:block.barrel.open");
    unit.running = false;
    unit.cooking = 0;
    unit.max = 0;
}

fn craft(unit: &mut Unit, env: &mut dyn Env, spec: &Spec) {
    let recipe = find(unit, env, spec);
    let (container, inputs) = (spec.slot("CONTAINER"), spec.slots("INPUT_SLOTS"));
    let has = recipe.as_ref().is_some_and(|r| !r.container.is_empty());
    let (times, made) = match &recipe {
        Some(recipe) => {
            let out = output(recipe);
            let mode = if has {
                Container::Results
            } else {
                Container::Plain
            };
            let times = shared::multiple(&unit.items, inputs, Some((container, mode)), out, out.1, env);
            (times, Some(ItemStack::new(&out.0, out.1 * times)))
        }
        None => (1, None),
    };
    let any = inputs.iter().any(|&s| unit.items[s].is_some());
    let takes = has.then(|| made.as_ref().map_or(0, |m| m.count));
    let left = shared::consume(&mut unit.items, inputs, times, Some((container, takes)), env);
    let output = spec.slot("OUTPUT");
    let light = light_state(env);
    unit.items[output] = if !any {
        None
    } else if unit.light.as_deref() != Some(light) || recipe.is_none() {
        // Spoilt: the longer it was to take, the more mold.
        let minutes = recipe.as_ref().map_or(unit.max, |r| r.max_cooking_time) / 60;
        let count = match minutes {
            40.. => 8,
            20.. => 4,
            10.. => 2,
            _ => 1,
        };
        let mold = spec.failure.as_deref().expect("fermenter.json names what spoilt things turn into");
        Some(ItemStack::new(mold, (count * times).min(env.max_stack(mold))))
    } else {
        made.filter(|m| m.count > 0)
    };
    shared::insert(&mut unit.items, spec.slots("MATERIAL_SLOTS"), left, env);
    stop(unit, env);
}

impl Kind for Fermenter {
    fn spec(&self) -> &'static Spec {
        spec::fermenter()
    }

    fn tick(&self, unit: &mut Unit, env: &mut dyn Env) {
        if !unit.running {
            return;
        }
        unit.count += 1;
        if unit.count >= 20 {
            unit.cooking += 1;
            unit.count = 0;
        }
        if unit.cooking - 1 >= unit.max {
            craft(unit, env, self.spec());
        }
    }

    fn packet(&self, unit: &mut Unit, env: &mut dyn Env, packet: &str) {
        match packet {
            "FermentationStartPacket" => start(unit, env, self.spec()),
            "FermentationStopPacket" => stop(unit, env),
            _ => {}
        }
    }

    fn opened(&self, unit: &Unit, env: &mut dyn Env, open: bool) {
        env.sound(if open {
            "minecraft:block.barrel.open"
        } else {
            "minecraft:block.barrel.close"
        });
        set_open(unit, env, open);
    }
}

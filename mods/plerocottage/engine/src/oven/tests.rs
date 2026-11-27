//! Every bake pleromod's extraction measured of the mod's own oven (`output/machines.json`: each
//! recipe once and in a batch of four, the smoking recipes it takes over, one recipe at every
//! setting; native.py's runs beyond those), baked again here and compared.
use super::*;
use serde_json::Value;

fn measured() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/machines.json"
    )))
    .unwrap()
}

/// The recipes native.py ran and the smoking ones the oven takes over, in the order the mod looks
/// for them (`getRecipe`, then `getSmokingRecipe`).
fn spec_recipes(spec: &Spec) -> Vec<Recipe> {
    spec.recipes
        .iter()
        .map(|r| Recipe::from_spec(r, false))
        .chain(spec.smoking.iter().map(|r| Recipe::from_spec(r, true)))
        .collect()
}

fn spec() -> &'static Spec {
    spec::oven()
}

/// machines.py's `set_oven`: one of each place's first item in the input slots, as many as
/// `count` where it stacks so far, and a stack of 64 of the container.
fn filled(spec: &Spec, recipe: &spec::Recipe, count: u32) -> Vec<Option<ItemStack>> {
    let mut items = vec![None; spec.size];
    for (&slot, place) in spec.input_slots.iter().zip(&recipe.input) {
        let item = &place[0];
        let most = spec.items.get(item).map_or(64, |i| i.max_stack);
        items[slot] = Some(ItemStack::new(item, count.min(most)));
    }
    if let Some(container) = recipe.container.first() {
        items[container_slot(spec)] = Some(ItemStack::new(container, 64));
    }
    items
}

/// Started, then ticked until it stops: the ticks it took.
fn bake(items: &mut [Option<ItemStack>], temperature: i32, time: i32) -> Option<u32> {
    let spec = spec();
    let recipes = spec_recipes(spec);
    let mut state = State {
        temperature,
        max_time: time,
        ..State::default()
    };
    if !start(&mut state, items, &recipes, spec) {
        return None;
    }
    let mut ticks = 0;
    while state.running {
        tick(&mut state, items, &recipes, spec);
        ticks += 1;
    }
    Some(ticks)
}

fn pairs(value: Option<&Value>) -> Vec<(String, u64)> {
    let mut out: Vec<_> = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].as_u64().unwrap()))
        .collect();
    out.sort();
    out
}

/// What machines.py's `left` reports of the slots: the result, what else lies there, what is worn.
fn left(items: &[Option<ItemStack>]) -> Value {
    let out = output_slot(spec());
    let list = |f: &dyn Fn(usize, &ItemStack) -> Option<Value>| -> Vec<Value> {
        items
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().and_then(|s| f(i, s)))
            .collect()
    };
    serde_json::json!({
        "made": list(&|i, s| (i == out).then(|| serde_json::json!([&*s.item, s.count]))),
        "kept": list(&|i, s| (i != out).then(|| serde_json::json!([&*s.item, s.count]))),
        "damaged": list(&|_, s| (s.damage > 0).then(|| serde_json::json!([&*s.item, s.damage]))),
    })
}

fn compare(
    name: &str,
    expected: &Value,
    items: &[Option<ItemStack>],
    ticks: Option<u32>,
) -> Vec<String> {
    let got = left(items);
    let mut wrong = Vec::new();
    if expected.get("ticks").and_then(Value::as_u64) != ticks.map(u64::from) {
        wrong.push(format!(
            "{name}: ticks {ticks:?}, measured {}",
            expected["ticks"]
        ));
    }
    for key in ["made", "kept", "damaged"] {
        if pairs(got.get(key)) != pairs(expected.get(key)) {
            wrong.push(format!(
                "{name}: {key} {}, measured {}",
                got[key],
                expected.get(key).unwrap_or(&Value::Null)
            ));
        }
    }
    wrong
}

fn recipe<'a>(list: &'a [spec::Recipe], file: &str) -> &'a spec::Recipe {
    list.iter()
        .find(|r| r.file == file)
        .unwrap_or_else(|| panic!("{file} is not in oven.json"))
}

#[test]
fn every_measured_oven_recipe_bakes_as_the_mod_does() {
    let (measured, spec) = (measured(), spec());
    let mut wrong = Vec::new();
    let mut runs = 0;
    for (kind, list) in [("oven", &spec.recipes), ("smoking in oven", &spec.smoking)] {
        for (file, run) in measured["recipes"][kind].as_object().unwrap() {
            let recipe = recipe(list, file);
            for (label, count) in [("one", 1), ("batch", 4)] {
                let mut items = filled(spec, recipe, count);
                let ticks = bake(&mut items, recipe.temperature, recipe.cooking_time);
                wrong.extend(compare(
                    &format!("{kind} {file} {label}"),
                    &run[label],
                    &items,
                    ticks,
                ));
                runs += 1;
            }
        }
    }
    assert_eq!(runs, (77 + 49) * 2, "machines.json's oven runs");
    assert!(
        wrong.is_empty(),
        "{} of {runs} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn another_setting_cinders_the_recipe() {
    let (measured, spec) = (measured(), spec());
    // machines.py's `first("oven")`: the first recipe whose places are not empty.
    let first = &spec.recipes[0];
    let mut wrong = Vec::new();
    let settings = measured["otherwise"]["oven at another setting"]
        .as_array()
        .unwrap();
    for run in settings {
        let (temperature, time) = (
            run["temperature"].as_i64().unwrap() as i32,
            run["time"].as_i64().unwrap() as i32,
        );
        let mut items = filled(spec, first, 1);
        let ticks = bake(&mut items, temperature, time);
        wrong.extend(compare(
            &format!("{temperature}° {time}"),
            run,
            &items,
            ticks,
        ));
    }
    assert_eq!(settings.len(), 9);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn stack(value: &[Value]) -> ItemStack {
    let mut stack = ItemStack::new(
        value[0].as_str().unwrap(),
        value[1].as_u64().unwrap() as u32,
    );
    stack.damage = value.get(2).and_then(Value::as_u64).unwrap_or(0) as u32;
    stack
}

#[test]
fn the_runs_beyond_the_recipes_end_in_the_same_slots() {
    let spec = spec();
    let mut wrong = Vec::new();
    for (name, run) in &spec.runs {
        let mut items = vec![None; spec.size];
        for (slot, value) in &run.items {
            items[slot.parse::<usize>().unwrap()] = Some(stack(value));
        }
        let ticks = bake(&mut items, run.temperature, run.time);
        if ticks.is_some() != run.started || ticks.unwrap_or(0) != run.ticks {
            wrong.push(format!(
                "{name}: ticks {ticks:?}, measured started {} after {}",
                run.started, run.ticks
            ));
        }
        let mut expected = vec![None; spec.size];
        for (slot, value) in &run.after {
            expected[slot.parse::<usize>().unwrap()] = Some(stack(value));
        }
        let same = items.iter().zip(&expected).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => a.item == b.item && a.count == b.count && a.damage == b.damage,
            (None, None) => true,
            _ => false,
        });
        if !same {
            wrong.push(format!("{name}: slots {items:?}\n  measured {expected:?}"));
        }
    }
    assert!(spec.runs.len() >= 20, "native.py's runs");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_recipes_are_the_content_mods() {
    // The recipes native.py ran are the files the content mod carries, read the same way.
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../web/mods/plerocottage/data/plerocottage/recipe/plerocottage_machine_oven"
    );
    let spec = spec();
    let mut files = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let recipe = recipe(
            &spec.recipes,
            &format!("data/plerocottage/recipes/plerocottage_machine_oven/{name}"),
        );
        assert_eq!(
            json["temperature"].as_i64(),
            Some(recipe.temperature.into()),
            "{name}"
        );
        assert_eq!(
            json["cookingTime"].as_i64(),
            Some(recipe.cooking_time.into()),
            "{name}"
        );
        assert_eq!(
            json["output"]["item"].as_str(),
            Some(&*recipe.output.0),
            "{name}"
        );
        assert_eq!(
            json["input"].as_array().unwrap().len(),
            recipe.input.len(),
            "{name}"
        );
        files += 1;
    }
    assert_eq!(files, spec.recipes.len());
}

#[test]
fn a_knob_turns_through_its_settings_and_back() {
    let spec = spec();
    let mut value = spec.defaults.temperature;
    let seen: Vec<i32> = (0..4)
        .map(|_| {
            value = turned(spec, true, value);
            value
        })
        .collect();
    assert_eq!(seen, [150, 200, 250, 0]);
    let mut value = spec.defaults.max_cooking_time;
    let seen: Vec<i32> = (0..4)
        .map(|_| {
            value = turned(spec, false, value);
            value
        })
        .collect();
    assert_eq!(seen, [100, 200, 300, 0]);
}

//! The rules against what pleromod's extraction says of the same plants for the site
//! (`data/grown.json`, its `output/growth.json`: what each block may become and give, found by
//! following every branch of its code), and the walk of a rule in a world of the tests' own.
use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

fn grown() -> Value {
    serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/grown.json"))).unwrap()
}

/// A world of blocks by place, with the draws it hands out in order (the least once they run out).
#[derive(Default)]
struct Field {
    blocks: BTreeMap<[i32; 3], (String, BTreeMap<String, String>)>,
    solid: BTreeSet<[i32; 3]>,
    draws: Vec<f64>,
    drawn: usize,
    dropped: Vec<(String, u32)>,
    given: Vec<(String, u32)>,
}

impl Field {
    fn next(&mut self) -> f64 {
        self.drawn += 1;
        self.draws.get(self.drawn - 1).copied().unwrap_or(0.0)
    }
}

impl Random for Field {
    fn int(&mut self, _bound: i32) -> i32 {
        self.next() as i32
    }
    fn float(&mut self) -> f32 {
        self.next() as f32
    }
    fn double(&mut self) -> f64 {
        self.next()
    }
    fn boolean(&mut self) -> bool {
        self.next() != 0.0
    }
}

impl Plot for Field {
    fn block(&self, at: [i32; 3]) -> Option<String> {
        self.blocks.get(&at).map(|block| block.0.clone())
    }
    fn replaceable(&self, at: [i32; 3]) -> bool {
        !self.solid.contains(&at) && !self.blocks.contains_key(&at)
    }
    fn set_properties(&mut self, at: [i32; 3], properties: &BTreeMap<String, String>) {
        if let Some(block) = self.blocks.get_mut(&at) {
            block.1.extend(properties.clone());
        }
    }
    fn set_block(&mut self, at: [i32; 3], block: Option<(&str, &BTreeMap<String, String>)>) {
        match block {
            None => self.blocks.remove(&at),
            Some((block, properties)) => self.blocks.insert(at, (block.to_owned(), properties.clone())),
        };
    }
    fn drop(&mut self, _at: [i32; 3], item: &str, count: u32) {
        self.dropped.push((item.to_owned(), count));
    }
    fn give(&mut self, _at: [i32; 3], item: &str, count: u32) {
        self.given.push((item.to_owned(), count));
    }
}

fn trees(rule: &Rule) -> impl Iterator<Item = &Node> {
    rule.cases.iter().map(|case| &case.tree).chain(&rule.enclosed)
}

fn methods(rules: &Rules) -> [(&'static str, &Option<Rule>); 4] {
    [
        ("randomTick", &rules.random_tick),
        ("performBonemeal", &rules.perform_bonemeal),
        ("isBonemealSuccess", &rules.is_bonemeal_success),
        ("use", &rules.used),
    ]
}

/// The draws that lead to the first end `wanted` holds of, if any does.
fn way_to(node: &Node, wanted: &dyn Fn(&Node) -> bool) -> Option<Vec<f64>> {
    if node.draw.is_none() {
        return wanted(node).then(Vec::new);
    }
    let mut branches: Vec<(f64, &Node)> = Vec::new();
    for (value, next) in node.each.iter().flatten().enumerate() {
        branches.push((value as f64, next));
    }
    if let Some(next) = &node.then {
        branches.push((0.0, next));
    }
    if let (Some(next), Some(below)) = (&node.other, node.below) {
        branches.push((below, next));
    }
    branches.into_iter().find_map(|(draw, next)| {
        let mut way = way_to(next, wanted)?;
        way.insert(0, draw);
        Some(way)
    })
}

#[test]
fn every_state_has_rules_and_every_draw_its_branches() {
    let growth = data();
    assert!(!growth.blocks.is_empty() && !growth.saplings.is_empty());
    fn check(node: &Node, plant: &str) {
        match node.draw {
            None => assert!(node.each.is_none() && node.then.is_none() && node.other.is_none(), "{plant}"),
            Some(draw) => {
                assert!(node.idle() && node.answer.is_none(), "{plant}: an end that draws");
                match (&node.each, node.below) {
                    (Some(each), None) => {
                        let wanted = if draw == Draw::Bool { 2 } else { node.bound as usize };
                        assert_eq!(each.len(), wanted, "{plant}: a branch for every number drawn");
                        assert!(matches!(draw, Draw::Int | Draw::Bool), "{plant}");
                    }
                    (None, Some(below)) => {
                        assert!(node.then.is_some() && node.other.is_some(), "{plant}");
                        let most = if draw == Draw::Int { f64::from(node.bound) } else { 1.0 };
                        assert!(below > 0.0 && below < most, "{plant}: a chance of {below}");
                    }
                    (None, None) => assert!(node.then.is_some() && node.other.is_none(), "{plant}"),
                    (Some(_), Some(_)) => panic!("{plant}: both"),
                }
                node.each.iter().flatten().chain(node.then.as_deref()).chain(node.other.as_deref()).for_each(|n| check(n, plant));
            }
        }
    }
    for (name, plant) in &growth.blocks {
        let properties = plant.properties();
        for (state, &rules) in &plant.states {
            assert!(rules < plant.rules.len(), "{name} {state}");
            let named: Vec<&str> = state.split(',').filter_map(|pair| pair.split('=').next()).filter(|n| !n.is_empty()).collect();
            assert_eq!(named, properties, "{name} {state}");
            let values: BTreeMap<&str, &str> = state.split(',').filter_map(|pair| pair.split_once('=')).collect();
            let found = plant.rules(|key| values.get(key).map(|v| (*v).to_owned())).unwrap();
            assert!(std::ptr::eq(found, &plant.rules[rules]), "{name} {state}");
        }
        for rules in &plant.rules {
            for (_, rule) in methods(rules) {
                let Some(rule) = rule else { continue };
                assert!(rule.cases.first().is_some_and(|case| case.light == 0), "{name}: a tree from no light up");
                assert!(rule.cases.windows(2).all(|pair| pair[0].light < pair[1].light), "{name}");
                assert!(rule.cases.len() == 1 || rule.light_at.is_some(), "{name}: light without a place");
                trees(rule).for_each(|tree| check(tree, name));
            }
            if let Some(rule) = &rules.is_bonemeal_success {
                assert!(trees(rule).flat_map(Node::ends).all(|end| end.answer.is_some()), "{name}");
            }
            // The plant's own "no" to bone meal and a rule for bone meal don't go together
            assert!(!(rules.no_bonemeal && rules.perform_bonemeal.is_some()), "{name}");
        }
    }
}

/// What a rule may make of a plant and give is what the extraction found its code may, for the
/// site, by other means: there every branch of the code was followed, here the code was run.
#[test]
fn rules_make_and_give_what_growth_json_says() {
    let (growth, grown) = (data(), grown());
    let (mut compared, mut same) = (0, 0);
    for (name, plant) in &growth.blocks {
        for method in ["randomTick", "performBonemeal", "use"] {
            let mut made = BTreeSet::new();
            let mut ruled = false;
            for rules in &plant.rules {
                let Some(rule) = methods(rules).into_iter().find(|m| m.0 == method).and_then(|m| m.1.as_ref()) else {
                    continue;
                };
                ruled = true;
                for end in trees(rule).flat_map(Node::ends) {
                    made.extend(end.set.iter().filter_map(|set| set.3.as_ref()?.block.as_deref()));
                    made.extend(end.drop.iter().chain(&end.give).map(|(item, _)| item.as_str()));
                }
            }
            if !ruled {
                continue;
            }
            made.remove(name.as_str());
            let said: BTreeSet<&str> = grown["blocks"][name][method].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            assert!(made.is_subset(&said), "{name} {method}: {made:?} of {said:?}");
            compared += 1;
            same += usize::from(made == said);
        }
    }
    // Nearly all of them say the same; the few that say less are uses with a tool in hand
    assert!(compared > 100 && same * 20 >= compared * 19, "{same} of {compared}");
    for (name, sapling) in &growth.saplings {
        let said: BTreeSet<&str> = grown["blocks"][name]["getConfiguredFeature"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        for tree in &sapling.trees {
            assert!(!tree.blocks.is_empty(), "{name}");
            for state in &tree.palette {
                assert!(said.contains(state.block.as_str()), "{name}: {} of {said:?}", state.block);
            }
            assert!(tree.blocks.iter().all(|block| block.3 < tree.palette.len()), "{name}");
        }
    }
}

#[test]
fn a_draw_below_its_chance_takes_the_first_branch() {
    let mut checked = 0;
    for plant in data().blocks.values() {
        for rules in &plant.rules {
            for tree in methods(rules).into_iter().filter_map(|m| m.1.as_ref()).flat_map(trees) {
                let (Some(below), Some(then), Some(other)) = (tree.below, &tree.then, &tree.other) else {
                    continue;
                };
                // Just below the chance and at it; the draws after it are the least there are
                let just_below = if tree.draw == Some(Draw::Int) { below - 1.0 } else { below * (1.0 - 1e-9) };
                let end = |first: f64| {
                    let mut field = Field { draws: vec![first], ..Field::default() };
                    tree.end(&mut field) as *const Node
                };
                let least = |node: &Node| node.end(&mut Field::default()) as *const Node;
                assert_eq!(end(just_below), least(then));
                assert_eq!(end(below), least(other));
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "{checked}");
}

/// A plant that changes takes the values the end names, and gives what it names.
#[test]
fn an_end_changes_the_plant_and_gives_its_items() {
    let (mut changed, mut gave) = (0, 0);
    for (name, plant) in &data().blocks {
        for (state, &number) in &plant.states {
            let properties: BTreeMap<String, String> = state.split(',').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
            for (method, rule) in methods(&plant.rules[number]) {
                let Some(tree) = rule.as_ref().and_then(|rule| rule.tree(15, false)) else { continue };
                let Some(way) = way_to(tree, &|end| !end.idle() && !end.tree) else { continue };
                let mut field = Field { draws: way, ..Field::default() };
                field.blocks.insert([0, 64, 0], (name.clone(), properties.clone()));
                let end = tree.end(&mut field);
                assert!(apply(end, [0, 64, 0], None, &mut field), "{name} {state} {method}");
                for Set(x, y, z, change) in &end.set {
                    let there = field.blocks.get(&[*x, 64 + *y, *z]);
                    match change {
                        None => assert!(there.is_none(), "{name} {state} {method}"),
                        Some(change) => {
                            let (block, values) = there.unwrap();
                            assert_eq!(block, change.block.as_ref().unwrap_or(name));
                            assert!(change.properties.iter().all(|(k, v)| values.get(k) == Some(v)), "{name} {state} {method}");
                            changed += 1;
                        }
                    }
                }
                assert_eq!(field.dropped, end.drop);
                assert_eq!(field.given, end.give);
                gave += field.dropped.len() + field.given.len();
            }
        }
    }
    assert!(changed > 100 && gave > 10, "{changed} {gave}");
}

/// A sapling whose rule ends in a tree becomes one of the trees the extraction grew of it, and
/// stays a sapling where its trunk has no room.
#[test]
fn a_sapling_becomes_one_of_its_trees() {
    let growth = data();
    let mut grown = 0;
    for (name, sapling) in &growth.saplings {
        let Some(plant) = growth.blocks.get(name) else { continue };
        let found = plant.states.iter().find_map(|(state, &number)| {
            let tree = plant.rules[number].random_tick.as_ref()?.tree(15, false)?;
            Some((state, tree, way_to(tree, &|end| end.tree)?))
        });
        let Some((state, rule, way)) = found else { continue };
        for (number, tree) in sapling.trees.iter().enumerate() {
            let pos = [8, 70, 8];
            let mut field = Field { draws: [way.clone(), vec![number as f64]].concat(), ..Field::default() };
            field.blocks.insert(pos, (name.clone(), BTreeMap::new()));
            let end = rule.end(&mut field);
            assert!(end.tree, "{name} {state}");
            assert!(apply(end, pos, Some(sapling), &mut field), "{name} {state}");
            for &(x, y, z, palette) in &tree.blocks {
                let there = field.blocks.get(&[pos[0] + x, pos[1] + y, pos[2] + z]).unwrap();
                assert_eq!(there.0, tree.palette[palette].block, "{name}");
            }
            assert_ne!(field.blocks.get(&pos).map(|block| &block.0), Some(name), "{name}: the sapling is gone");
            assert_eq!(field.blocks.len(), tree.blocks.len());
            // Something in the trunk's way
            let Some(&(x, y, z, _)) = tree.blocks.iter().find(|b| b.0 == 0 && b.2 == 0 && b.1 > 0) else { continue };
            let mut field = Field { draws: [way.clone(), vec![number as f64]].concat(), ..Field::default() };
            field.blocks.insert(pos, (name.clone(), BTreeMap::new()));
            field.solid.insert([pos[0] + x, pos[1] + y, pos[2] + z]);
            let end = rule.end(&mut field);
            assert!(!apply(end, pos, Some(sapling), &mut field), "{name}");
            assert_eq!(field.blocks.len(), 1, "{name}: only the sapling");
            grown += 1;
        }
    }
    assert!(grown > 20, "{grown}");
}

/// Where a rule goes by the light, less light never does more: a plant that does nothing in
/// some light does nothing in the dark.
#[test]
fn less_light_does_no_more() {
    let mut by_light = 0;
    for (name, plant) in &data().blocks {
        for rules in &plant.rules {
            for rule in methods(rules).into_iter().filter_map(|m| m.1.as_ref()).filter(|rule| rule.cases.len() > 1) {
                let does = |light: i32| rule.tree(light, false).is_some_and(|tree| tree.ends().iter().any(|end| !end.idle()));
                let first = (0..16).find(|&light| does(light));
                assert!(first.is_some_and(|first| (first..16).all(does)), "{name}");
                by_light += 1;
            }
        }
    }
    assert!(by_light > 10, "{by_light}");
}

/// A plant stands on the grounds its code took and on none of the others it was asked about;
/// of a ground nobody asked about, and of a plant that took none, nothing is said.
#[test]
fn a_plant_stands_on_the_grounds_its_code_took() {
    let grounds = &data().grounds;
    assert!(!grounds.asked.is_empty() && grounds.plants.len() > 20);
    for (plant, on) in &grounds.plants {
        assert!(on.iter().all(|ground| grounds.asked.contains(ground)), "{plant}");
        assert_eq!(grounds.stands(plant, "minecraft:bedrock"), None, "{plant}");
        for ground in &grounds.asked {
            let wanted = (!on.is_empty()).then(|| on.contains(ground));
            assert_eq!(grounds.stands(plant, ground), wanted, "{plant} on {ground}");
        }
    }
    assert_eq!(grounds.stands("minecraft:wheat", &grounds.asked[0]), None);
}

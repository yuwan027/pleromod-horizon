//! `CuttingBoardBlockEntity` and `CuttingBoardBlock.use`: no menu and no clock. Things are laid on
//! the board by hand, one in each of its six places, and a click with a tool (or an empty hand)
//! makes what lies there: one thing at a time from the last place down, or all of it together.
use super::*;

pub const BLOCK: &str = "plerocottage:cutting_board";

/// A new board's `craftIndex`.
pub fn new_unit(spec: &spec::Board) -> Unit {
    Unit {
        count: spec.fields["craftIndex"],
        ..Unit::new(spec.size)
    }
}

/// `toolHurt`: the tool wears by one and breaks at its last use.
fn hurt(tool: &mut Option<ItemStack>, env: &mut dyn Env) {
    let Some(stack) = tool else {
        return;
    };
    let max = env.max_damage(&stack.item);
    if max == 0 {
        return;
    }
    stack.damage += 1;
    if stack.damage >= max {
        shared::take(tool, 1);
    }
}

fn tool_sound(tool: &Option<ItemStack>, env: &mut dyn Env, bare: bool) {
    match tool {
        None if bare => env.sound("cutting_board_crafting"),
        None => {}
        Some(t) if shared::is(&t.item, "Knife") || shared::is(&t.item, "Axe") => {
            env.sound("cutting_board_cutting");
        }
        Some(t) if shared::is(&t.item, "RollingPin") => env.sound("cutting_board_roll_out"),
        Some(_) => {}
    }
}

fn outgrowths(recipe: &Recipe, env: &mut dyn Env) {
    for ((item, count), chance) in &recipe.outgrowths {
        if env.next_float() < *chance {
            env.pop(ItemStack::new(item, *count));
        }
    }
}

/// `craftSingleItem`: from `craftIndex` down, the first place whose thing the tool cuts (and
/// above which nothing lies) is cut; past the first place the board starts over.
pub fn craft_single(unit: &mut Unit, env: &mut dyn Env, spec: &spec::Board, tool: &mut Option<ItemStack>) -> bool {
    let recipes = env.recipes("cutting_board_single");
    let mut cut = false;
    while unit.count >= 0 {
        let at = unit.count as usize;
        let recipe = shared::first(&recipes, |r| {
            shared::test(tool, &r.tool) && r.input.first().is_some_and(|p| shared::test(&unit.items[at], p))
        });
        let above = unit.items.get(at + 1).is_some_and(Option::is_some);
        if let (Some(recipe), false) = (recipe, above) {
            hurt(tool, env);
            let left = unit.items[at].take().and_then(|s| shared::remaining(&s, 1, env));
            if let Some(left) = left {
                env.pop(left);
            }
            let out = output(recipe);
            env.pop(ItemStack::new(&out.0, out.1));
            outgrowths(recipe, env);
            tool_sound(tool, env, false);
            unit.count -= 1;
            unit.flag = true;
            cut = true;
            break;
        }
        unit.count -= 1;
    }
    if unit.count < 0 {
        unit.flag = false;
        unit.count = spec.fields["craftIndex"];
    }
    cut
}

/// `craftMultipleItem`: everything on the board makes one recipe, place by place when it is
/// shaped.
pub fn craft_multiple(unit: &mut Unit, env: &mut dyn Env, spec: &spec::Board, tool: &mut Option<ItemStack>) -> bool {
    let recipes = env.recipes("cutting_board_multiple");
    let every: Vec<usize> = (0..spec.size).collect();
    let Some(recipe) = shared::first(&recipes, |r| {
        shared::test(tool, &r.tool)
            && if r.is_shaped != 0 {
                r.input.iter().enumerate().all(|(i, p)| shared::test(&unit.items[i], p))
            } else {
                shared::matches(&unit.items, &every, &r.input)
            }
    }) else {
        return false;
    };
    let mut left = Vec::new();
    for slot in every {
        let Some(stack) = &unit.items[slot] else {
            continue;
        };
        let mut rest = shared::remaining(stack, 1, env);
        if shared::is(&stack.item, "DamageableMaterial") {
            // What wears and is not worn out comes back as it is.
            shared::wear(&mut unit.items[slot], 1, env);
            if let Some(worn) = &unit.items[slot] {
                rest = Some(worn.clone());
            }
        }
        left.extend(rest);
        unit.items[slot] = None;
    }
    for stack in left {
        env.pop(stack);
    }
    let out = output(recipe);
    env.pop(ItemStack::new(&out.0, out.1));
    outgrowths(recipe, env);
    hurt(tool, env);
    tool_sound(tool, env, true);
    true
}

fn full(unit: &Unit) -> bool {
    unit.items.iter().all(Option::is_some)
}

/// `addItem`: one of the stack into the first free place.
fn add(unit: &mut Unit, env: &mut dyn Env, held: &mut Option<ItemStack>) -> bool {
    let (Some(free), Some(stack)) = (unit.items.iter().position(Option::is_none), held.as_ref()) else {
        return false;
    };
    let mut one = stack.clone();
    one.count = 1;
    unit.items[free] = Some(one);
    shared::take(held, 1);
    env.sound("minecraft:entity.item_frame.add_item");
    true
}

/// `dropSingleItem`: the last thing laid on it pops off.
fn drop_one(unit: &mut Unit, env: &mut dyn Env) -> bool {
    let Some(last) = unit.items.iter().rposition(Option::is_some) else {
        return false;
    };
    env.pop(unit.items[last].take().expect("filled"));
    env.sound("minecraft:entity.item_frame.remove_item");
    true
}

/// What `CuttingBoardBlock.use` answers (`InteractionResult`): `Pass` lets the other hand be asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Pass,
    Success,
    ConsumePartial,
}

fn done(did: bool) -> Answer {
    if did {
        Answer::Success
    } else {
        Answer::ConsumePartial
    }
}

/// `CuttingBoardBlock.use` for one hand, as native.py's clicks on the mod's own block answered. `main` says
/// which hand `held` is; `other` is what the other one holds, `main_hand` what the main one does.
pub fn used(
    unit: &mut Unit,
    env: &mut dyn Env,
    spec: &spec::Board,
    held: &mut Option<ItemStack>,
    other: &Option<ItemStack>,
    main: bool,
    main_hand: &Option<ItemStack>,
    sneaking: bool,
) -> Answer {
    let is_tool = |stack: &Option<ItemStack>| {
        stack.as_ref().is_some_and(|s| spec.tools.iter().any(|t| **t == *s.item))
    };
    if sneaking && unit.items.iter().any(Option::is_some) {
        for stack in unit.items.iter_mut().filter_map(Option::take) {
            env.pop(stack);
        }
        env.sound("minecraft:entity.item_frame.remove_item");
        return Answer::Success;
    }
    if (main_hand.is_none() || is_tool(main_hand)) && craft_multiple(unit, env, spec, held) {
        return Answer::Success;
    }
    if held.is_some() {
        if !main {
            return done(add(unit, env, held));
        }
        // trySingleCraft: a tool, with something that is no tool in the other hand, on a board with room and
        // nothing begun, leaves the click to the other hand, which lays its thing on the board.
        if is_tool(held) && other.is_some() && !is_tool(other) && !full(unit) && !unit.flag {
            return Answer::Pass;
        }
        return done(craft_single(unit, env, spec, held) || (!full(unit) && add(unit, env, held)));
    }
    if other.is_none() {
        // tryMultipleCraft
        return done(craft_multiple(unit, env, spec, held) || drop_one(unit, env));
    }
    Answer::Pass
}

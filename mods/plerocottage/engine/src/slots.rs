//! A menu's slots by what its machine is doing (`native_menu_slot_state`): whether a player may
//! take from one, put into it, and whether it is there, as the slots of the mod's own menus
//! answered `mayPickup`, `mayPlace` and `isActive` when pleromod's extraction asked them with the
//! machine standing and working (`native.py`'s `machine_menu`).
use crate::spec::{Active, Busy, States};
use horizon_client::mod_api::*;

/// What a slot of a menu answers, standing and working.
pub struct Rules<'a> {
    pub may_pickup: &'a States<bool>,
    /// Whether it takes anything at all, standing and working; which items is `menu_slot_rules`'.
    pub takes: [bool; 2],
    pub active: Option<&'a States<Active>>,
}

/// The menu's `isIdle` turned round: any of `busy` holds.
pub fn working(busy: &[Busy], running: impl FnOnce() -> bool, data: &[i32]) -> bool {
    busy.iter().any(|b| matches!(b, Busy::Data { index } if data.get(*index).copied().unwrap_or(0) != 0))
        || (busy.contains(&Busy::Running) && running())
}

/// (`mayPickup`, `mayPlace` as far as the machine says, `isActive`) of a slot.
pub fn answer(rules: &Rules<'_>, working: bool, filled: bool) -> (bool, bool, bool) {
    let pick = |s: &States<bool>| if working { s.cooking } else { s.standing };
    let shown = rules.active.is_none_or(|active| {
        let now = if working { &active.cooking } else { &active.standing };
        if filled { now.filled } else { now.empty }
    });
    (pick(rules.may_pickup), rules.takes[usize::from(working)], shown)
}

/// The slot of the block's menu the event asks about, and what makes its machine busy.
fn rules(block: &str, slot: usize) -> Option<(Rules<'static>, &'static [Busy])> {
    if let Some(kind) = crate::machines::of(block) {
        let spec = kind.spec();
        let found = spec.menu.iter().find(|s| s.index == slot)?;
        let takes = [!found.may_place.standing.nothing(), !found.may_place.cooking.nothing()];
        return Some((Rules { may_pickup: &found.may_pickup, takes, active: found.active.as_ref() }, &spec.busy));
    }
    if block != crate::oven::BLOCK {
        return None;
    }
    let spec = crate::spec::oven();
    let found = spec.menu.iter().find(|s| s.index == slot)?;
    // The result's slot has no list: nothing is put into it.
    let takes = match &found.may_place {
        Some(place) => [!place.standing.is_empty(), !place.cooking.is_empty()],
        None => [false, false],
    };
    Some((Rules { may_pickup: &found.may_pickup, takes, active: found.active.as_ref() }, &spec.busy))
}

pub fn state(host: &dyn Host, event: &mut MenuSlotStateEvent<'_>) {
    let Some((rules, busy)) = rules(event.block, event.slot) else {
        return;
    };
    let running = || host.mod_block_entity(event.pos).is_some_and(|entity| crate::int(&entity.nbt, "isRunning") != 0);
    let (pickup, place, shown) = answer(&rules, working(busy, running, event.data), event.filled);
    event.may_pickup &= pickup;
    event.may_place &= place;
    event.shown &= shown;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (`mayPickup`, `mayPlace`, `isActive`) of a slot of `block`'s menu, standing or working, empty or filled.
    fn asked(block: &str, slot: usize, working: bool, filled: bool) -> (bool, bool, bool) {
        let (rules, _) = rules(block, slot).unwrap_or_else(|| panic!("{block} slot {slot}"));
        answer(&rules, working, filled)
    }

    #[test]
    fn the_oven_gives_up_nothing_while_it_bakes_and_shows_its_result_once_there_is_one() {
        let spec = crate::spec::oven();
        for slot in &spec.menu {
            let (pickup, place, _) = asked(crate::oven::BLOCK, slot.index, true, true);
            assert!(!pickup && !place, "oven slot {} while baking", slot.index);
            assert!(asked(crate::oven::BLOCK, slot.index, false, true).0, "oven slot {} standing", slot.index);
        }
        let output = crate::oven::output_slot(spec);
        assert!(!asked(crate::oven::BLOCK, output, false, false).2);
        assert!(asked(crate::oven::BLOCK, output, false, true).2);
    }

    #[test]
    fn every_machine_answers_its_slots_as_its_menu_did() {
        for kind in crate::machines::all() {
            let spec = kind.spec();
            for slot in &spec.menu {
                for (working, now) in [(false, &slot.may_pickup.standing), (true, &slot.may_pickup.cooking)] {
                    for filled in [false, true] {
                        let (pickup, _, shown) = asked(&spec.block, slot.index, working, filled);
                        assert_eq!(pickup, *now, "{} slot {}", spec.block, slot.index);
                        let expected = slot.active.as_ref().is_none_or(|a| {
                            let a = if working { &a.cooking } else { &a.standing };
                            if filled { a.filled } else { a.empty }
                        });
                        assert_eq!(shown, expected, "{} slot {} working {working} filled {filled}", spec.block, slot.index);
                    }
                }
            }
        }
        // The fryer's baskets go while it fries, full or not.
        let fryer = crate::machines::all().into_iter().find(|k| k.spec().block == "plerocottage:fryer").unwrap();
        let baskets: Vec<usize> = fryer.spec().menu.iter().filter(|s| s.active.is_some()).map(|s| s.index).collect();
        assert!(!baskets.is_empty());
        for slot in baskets {
            assert!(!asked("plerocottage:fryer", slot, true, true).2 && !asked("plerocottage:fryer", slot, true, false).2);
        }
    }
}

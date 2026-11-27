//! The mod's enchanting table mixins. `EnchantmentMenuMixin`: a raspberry in the table has one
//! offer of its own, and pressing it turns the stack into a blue raspberry for a piece of lapis;
//! `EnchantmentScreenMixin`: the offer shows whatever the player's level, with its own lines in
//! the tooltip. All of it is `data/enchanting.json`, which pleromod's extraction found by
//! running the mixins' handlers for every case they tell apart.
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Data {
    pub offers: Offers,
    pub click: Click,
    pub screen: Screen,
}

/// `slotsChanged` for the item: what the menu's three arrays hold after it.
#[derive(Deserialize)]
pub struct Offers {
    pub item: String,
    pub costs: [i32; 3],
    /// Registry ids of 1.20.1, which only the tooltip the screen replaces reads: not shown.
    pub enchant_clue: [i32; 3],
    pub level_clue: [i32; 3],
}

/// `clickMenuButton` with the item in the first slot.
#[derive(Deserialize)]
pub struct Click {
    /// The item the second slot is asked for.
    pub second: String,
    pub cases: Vec<Case>,
}

#[derive(Deserialize)]
pub struct Case {
    pub button: i32,
    /// Whether the second slot holds `second`.
    pub lapis: bool,
    pub creative: bool,
    /// What the menu answers; none where the click is left to Minecraft.
    pub answer: Option<bool>,
    #[serde(default)]
    pub does: Vec<Step>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(untagged)]
pub enum Step {
    /// `getSlot(set).set(new ItemStack(item))`.
    Set { set: usize, item: String, count: u32 },
    /// `shrink(by)` of the stack in slot `shrink`.
    Shrink { shrink: usize, by: u32 },
    /// `level.playSound(null, pos, ...)`.
    Sound { sound: String, source: String, volume: f32, pitch: f32 },
    /// `broadcastChanges()`: the menu's, once the click is taken.
    Broadcast { broadcast: bool },
}

#[derive(Deserialize)]
pub struct Screen {
    /// The player's level and what `renderBg` takes it for.
    pub level: Vec<[i32; 2]>,
    pub tooltip: Vec<Line>,
    /// Where `mouseClicked` presses the button itself; inside the button, whose click does the same.
    pub click: Area,
}

#[derive(Deserialize)]
pub struct Line {
    pub key: String,
    /// A `ChatFormatting` by name.
    pub color: Option<String>,
}

#[derive(Deserialize)]
pub struct Area {
    pub button: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/enchanting.json")))
            .unwrap_or_else(|e| panic!("data/enchanting.json is what enchanting.py writes: {e}"))
    })
}

impl Screen {
    /// Whether the screen takes the level for at least the offer's cost, whatever it is
    /// (`Math.max(player.experienceLevel, menu.costs[0])`).
    pub fn ignores_level(&self, costs: &[i32; 3]) -> bool {
        let cost = costs.iter().copied().max().unwrap_or(0);
        self.level.iter().any(|[level, shown]| shown != level)
            && self.level.iter().all(|[level, shown]| *shown == (*level).max(cost))
    }
}

/// `native_enchant_offers`: the item's own offer in place of vanilla's.
pub fn offers(item: &ItemStack, offers: &mut NativeEnchantOffers) {
    let data = data();
    if *item.item != *data.offers.item {
        return;
    }
    offers.replaced = true;
    offers.costs = data.offers.costs;
    offers.clues = [None, None, None];
    offers.ignores_level = data.screen.ignores_level(&data.offers.costs);
    offers.tooltip = data.screen.tooltip.iter().map(|line| (line.key.clone(), line.color.clone())).collect();
}

/// What a press of `button` does to the two slots: the menu's answer (none where the mixin leaves
/// the click to Minecraft) and the sounds to play, each `(event, source, volume, pitch)`.
pub fn click<'a>(button: i32, creative: bool, slots: &mut [Option<ItemStack>; 2]) -> (Option<bool>, Vec<&'a Step>) {
    let data = data();
    if !slots[0].as_ref().is_some_and(|s| *s.item == *data.offers.item) {
        return (None, Vec::new());
    }
    let lapis = slots[1].as_ref().is_some_and(|s| *s.item == *data.click.second && s.count > 0);
    let Some(case) = data
        .click
        .cases
        .iter()
        .find(|c| c.button == button && c.lapis == lapis && c.creative == creative)
    else {
        return (None, Vec::new());
    };
    let mut sounds = Vec::new();
    for step in &case.does {
        match step {
            Step::Set { set, item, count } => {
                if let Some(slot) = slots.get_mut(*set) {
                    *slot = Some(ItemStack::new(item, *count));
                }
            }
            Step::Shrink { shrink, by } => {
                if let Some(stack) = slots.get_mut(*shrink).and_then(Option::as_mut) {
                    stack.count = stack.count.saturating_sub(*by);
                }
            }
            Step::Sound { .. } => sounds.push(step),
            Step::Broadcast { .. } => {}
        }
    }
    for slot in slots.iter_mut() {
        if slot.as_ref().is_some_and(|s| s.count == 0) {
            *slot = None;
        }
    }
    (case.answer, sounds)
}

/// `SoundSource` by its name in lower case.
fn source(name: &str) -> Option<usize> {
    (name == "blocks").then_some(crate::BLOCKS)
}

/// `native_enchant_click`.
pub fn clicked(host: &mut dyn Host, pos: [i32; 3], button: i32, creative: bool, slots: &mut [Option<ItemStack>; 2]) -> Option<bool> {
    let (answer, sounds) = click(button, creative, slots);
    for step in sounds {
        if let Step::Sound { sound, source: from, volume, pitch } = step
            && let Some(from) = source(from)
        {
            host.play_sound(sound, crate::at(pos), from, *volume, *pitch);
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    const RASPBERRY: &str = "plerocottage:raspberry";
    const BLUE: &str = "plerocottage:blue_raspberry";
    const LAPIS: &str = "minecraft:lapis_lazuli";

    fn stack(item: &str, count: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    fn held(slot: &Option<ItemStack>) -> Option<(&str, u32)> {
        slot.as_ref().map(|s| (&*s.item, s.count))
    }

    #[test]
    fn the_data_is_the_raspberry_conversion() {
        let data = data();
        assert_eq!(data.offers.item, RASPBERRY);
        // `costs[0] = 1`, the others 0; the clues only feed the tooltip the screen replaces.
        assert_eq!(data.offers.costs, [1, 0, 0]);
        assert_eq!(data.offers.enchant_clue, [0, -1, -1]);
        assert_eq!(data.offers.level_clue, [0, 0, 0]);
        assert_eq!(data.click.second, LAPIS);
        // Three buttons, lapis or not, creative or not.
        assert_eq!(data.click.cases.len(), 12);
        // The screen's own click presses the offer's button, inside it (108x19 at 60, 14).
        let area = &data.screen.click;
        assert_eq!((area.button, area.x, area.y), (0, 60, 14));
        assert!(area.width <= 108 && area.height <= 20);
        // Every sound is of a source this crate plays.
        for case in &data.click.cases {
            for step in &case.does {
                if let Step::Sound { source: from, .. } = step {
                    assert!(source(from).is_some(), "{from}");
                }
            }
        }
    }

    #[test]
    fn a_raspberry_has_one_offer_whatever_the_level_and_another_item_none() {
        let mut made = NativeEnchantOffers::default();
        offers(&ItemStack::new(RASPBERRY, 3), &mut made);
        assert!(made.replaced);
        assert_eq!(made.costs, [1, 0, 0]);
        assert_eq!(made.clues, [None, None, None]);
        // `Math.max(player.experienceLevel, menu.costs[0])`: level 0 shows as the cost.
        assert!(made.ignores_level);
        assert_eq!(
            made.tooltip,
            [
                ("gui.plerocottage.raspberry_conversion".to_owned(), None),
                ("gui.plerocottage.raspberry_conversion_cost".to_owned(), Some("gray".to_owned())),
            ]
        );
        let mut other = NativeEnchantOffers::default();
        offers(&ItemStack::new("minecraft:diamond_sword", 1), &mut other);
        assert_eq!(other, NativeEnchantOffers::default());
        offers(&ItemStack::new(BLUE, 1), &mut other);
        assert!(!other.replaced);
    }

    #[test]
    fn the_first_button_turns_the_stack_into_one_blue_raspberry_for_one_lapis() {
        // `new ItemStack(BLUE_RASPBERRY)`: one, whatever the stack was.
        let mut slots = [stack(RASPBERRY, 5), stack(LAPIS, 3)];
        let (answer, sounds) = click(0, false, &mut slots);
        assert_eq!(answer, Some(true));
        assert_eq!(held(&slots[0]), Some((BLUE, 1)));
        assert_eq!(held(&slots[1]), Some((LAPIS, 2)));
        assert_eq!(
            sounds,
            [&Step::Sound { sound: "minecraft:block.enchantment_table.use".into(), source: "blocks".into(), volume: 1.0, pitch: 1.0 }]
        );
        // The last lapis leaves the slot empty.
        let mut slots = [stack(RASPBERRY, 1), stack(LAPIS, 1)];
        assert_eq!(click(0, false, &mut slots).0, Some(true));
        assert_eq!((held(&slots[0]), held(&slots[1])), (Some((BLUE, 1)), None));
    }

    #[test]
    fn without_lapis_the_click_is_refused_unless_creative() {
        for second in [None, stack("minecraft:diamond", 4)] {
            let mut slots = [stack(RASPBERRY, 5), second.clone()];
            let (answer, sounds) = click(0, false, &mut slots);
            assert_eq!(answer, Some(false));
            assert!(sounds.is_empty());
            assert_eq!((held(&slots[0]), &slots[1]), (Some((RASPBERRY, 5)), &second));
            // `instabuild`: converted, and nothing is taken.
            let (answer, sounds) = click(0, true, &mut slots);
            assert_eq!((answer, sounds.len()), (Some(true), 1));
            assert_eq!((held(&slots[0]), &slots[1]), (Some((BLUE, 1)), &second));
        }
        // A creative player's lapis stays.
        let mut slots = [stack(RASPBERRY, 2), stack(LAPIS, 7)];
        assert_eq!(click(0, true, &mut slots).0, Some(true));
        assert_eq!(held(&slots[1]), Some((LAPIS, 7)));
    }

    #[test]
    fn the_other_buttons_and_other_items_are_left_to_minecraft() {
        for button in [1, 2, 3, -1] {
            let mut slots = [stack(RASPBERRY, 5), stack(LAPIS, 3)];
            assert_eq!(click(button, false, &mut slots).0, None, "{button}");
            assert_eq!((held(&slots[0]), held(&slots[1])), (Some((RASPBERRY, 5)), Some((LAPIS, 3))));
        }
        let mut slots = [stack("minecraft:book", 1), stack(LAPIS, 3)];
        assert_eq!(click(0, false, &mut slots).0, None);
        let mut slots = [None, stack(LAPIS, 3)];
        assert_eq!(click(0, true, &mut slots).0, None);
    }

    /// The level's sounds, as the hook plays them.
    #[derive(Default)]
    struct Table {
        played: Vec<(String, [f64; 3], usize, f32, f32)>,
    }

    impl Host for Table {
        fn play_sound(&mut self, sound: &str, at: [f64; 3], source: usize, volume: f32, pitch: f32) -> bool {
            self.played.push((sound.to_owned(), at, source, volume, pitch));
            true
        }
    }

    #[test]
    fn the_hooks_answer_for_the_table() {
        let mods = [crate::hooks()];
        let mut table = Table::default();
        let mut slots = [stack(RASPBERRY, 2), stack(LAPIS, 1)];
        let mut taken = None;
        dispatch_enchant_click(&mut table, &mods, 7, "Steve", [3, 64, -2], 0, false, &mut slots, &mut taken);
        assert_eq!(taken, Some(true));
        assert_eq!((held(&slots[0]), held(&slots[1])), (Some((BLUE, 1)), None));
        // `SoundEvents.ENCHANTMENT_TABLE_USE` at the table, `SoundSource.BLOCKS`, 1.0 and 1.0.
        assert_eq!(table.played, [("minecraft:block.enchantment_table.use".to_owned(), [3.5, 64.5, -1.5], 4, 1.0, 1.0)]);
        // The blue raspberry it left has no offer and no click.
        let mut taken = None;
        dispatch_enchant_click(&mut table, &mods, 7, "Steve", [3, 64, -2], 0, false, &mut slots, &mut taken);
        assert_eq!((taken, table.played.len()), (None, 1));
        let mut made = NativeEnchantOffers::default();
        dispatch_enchant_offers(&Table::default(), &mods, [3, 64, -2], &ItemStack::new(RASPBERRY, 1), &mut made);
        assert!(made.replaced && made.costs == [1, 0, 0]);
    }
}

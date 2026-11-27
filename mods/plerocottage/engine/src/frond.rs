//! `BananaFrondBlock.use`: a shovel stands a laid banana frond up, or lays it flat again; the
//! frond's shape follows (`blockstate` 0 is flat, `FLAT`; 1 upright, `UPRIGHT`).
use horizon_client::mod_api::*;

pub const BLOCK: &str = "plerocottage:banana_frond";

/// A click on the frond with one hand. False when the click passes on (to the other hand).
pub fn used(host: &mut dyn Host, pos: [i32; 3], main: bool) -> bool {
    let Some(player) = host.acting_player() else {
        return false;
    };
    used_by(host, pos, player.uuid, player.sneaking, main)
}

/// The click of the player `uuid`.
fn used_by(host: &mut dyn Host, pos: [i32; 3], uuid: u128, sneaking: bool, main: bool) -> bool {
    // `isSecondaryUseActive` with something in a hand: the game does not ask the block.
    if sneaking {
        return false;
    }
    let hand = if main {
        host.held_stack(uuid)
    } else {
        host.player_equipment(uuid, EquipmentSlot::OffHand)
    };
    let Some(hand) = hand.filter(|stack| stack.count > 0) else {
        return false;
    };
    if !host.server_item_in_tag(&hand, "minecraft:shovels").unwrap_or(false) {
        return false;
    }
    let flat = host.block_property(pos, "blockstate").as_deref() == Some("0");
    host.set_block_property(pos, "blockstate", if flat { "1" } else { "0" });
    host.play_sound("minecraft:block.wool.place", crate::at(pos), crate::BLOCKS, 1.0, 1.0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Frond {
        state: String,
        main: Option<ItemStack>,
        off: Option<ItemStack>,
        sounds: Vec<String>,
    }

    impl Host for Frond {
        fn held_stack(&self, _uuid: u128) -> Option<ItemStack> {
            self.main.clone()
        }
        fn player_equipment(&self, _uuid: u128, slot: EquipmentSlot) -> Option<ItemStack> {
            matches!(slot, EquipmentSlot::OffHand).then(|| self.off.clone()).flatten()
        }
        fn server_item_in_tag(&self, stack: &ItemStack, tag: &str) -> Result<bool, NativeBlockReadError> {
            Ok(tag == "minecraft:shovels" && stack.item.ends_with("_shovel"))
        }
        fn block_property(&self, _pos: [i32; 3], key: &str) -> Option<String> {
            (key == "blockstate").then(|| self.state.clone())
        }
        fn set_block_property(&mut self, _pos: [i32; 3], key: &str, value: &str) -> bool {
            assert_eq!(key, "blockstate");
            self.state = value.to_owned();
            true
        }
        fn play_sound(&mut self, sound: &str, _at: [f64; 3], _source: usize, _volume: f32, _pitch: f32) -> bool {
            self.sounds.push(sound.to_owned());
            true
        }
    }

    fn frond(state: &str) -> Frond {
        Frond { state: state.to_owned(), ..Frond::default() }
    }

    #[test]
    fn a_shovel_stands_a_flat_frond_up_and_lays_any_other_flat() {
        let mut level = frond("0");
        level.main = Some(ItemStack::new("minecraft:iron_shovel", 1));
        assert!(used_by(&mut level, [0; 3], 0, false, true));
        assert_eq!(level.state, "1");
        assert!(used_by(&mut level, [0; 3], 0, false, true));
        assert_eq!(level.state, "0");
        level.state = "3".to_owned();
        assert!(used_by(&mut level, [0; 3], 0, false, true));
        assert_eq!(level.state, "0");
        assert_eq!(level.sounds, ["minecraft:block.wool.place"; 3]);
    }

    #[test]
    fn only_the_hand_holding_the_shovel_turns_it() {
        let mut level = frond("0");
        level.main = Some(ItemStack::new("minecraft:dirt", 1));
        level.off = Some(ItemStack::new("minecraft:wooden_shovel", 1));
        assert!(!used_by(&mut level, [0; 3], 0, false, true));
        assert!(used_by(&mut level, [0; 3], 0, false, false));
        assert_eq!(level.state, "1");
    }

    #[test]
    fn an_empty_hand_or_sneaking_passes() {
        let mut level = frond("0");
        assert!(!used_by(&mut level, [0; 3], 0, false, true));
        level.main = Some(ItemStack::new("minecraft:iron_shovel", 1));
        assert!(!used_by(&mut level, [0; 3], 0, true, true));
        assert_eq!(level.state, "0");
        assert!(level.sounds.is_empty());
    }
}

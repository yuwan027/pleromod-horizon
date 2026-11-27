//! The enchanting table for native mods: `native_enchant_offers` (a mixin at the head of
//! `EnchantmentMenu.slotsChanged`, with what `EnchantmentScreen` shows of the offers) and
//! `native_enchant_click` (one at the head of `EnchantmentMenu.clickMenuButton`), so a mod gives
//! an item that is not enchantable an offer of its own and does something else with the click.
//! The local table (`game/stations.rs`) and a guest's on the host (`game/lan_stations.rs`) ask
//! both; a client of a server asks for the offers again, for its own screen.

use super::Game;
use horizon_client::items::ItemStack;

/// What an enchanting table offers for the item in it, as `native_enchant_offers` reads and
/// changes it: `EnchantmentMenu`'s `costs`, `enchantClue` and `levelClue`, and what
/// `EnchantmentScreen` shows of them.
// Native mods read and set it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeEnchantOffers {
    /// Set: these are the table's offers, and vanilla's are not worked out (`slotsChanged`
    /// cancelled at its head).
    pub replaced: bool,
    /// The level each of the three buttons asks for; 0 for none.
    pub costs: [i32; 3],
    /// The enchantment and level each button hints at (`minecraft:sharpness`, 2).
    pub clues: [Option<(String, u32)>; 3],
    /// The screen takes the player's level for at least each cost: no offer is greyed out, or
    /// refused, for levels (`EnchantmentScreen.renderBg` reading `experienceLevel`).
    pub ignores_level: bool,
    /// The lines shown over a hovered offer in place of the clue's: a translation key and a
    /// `ChatFormatting` colour by name (`gray`), white without one. Empty: the clue's.
    pub tooltip: Vec<(String, Option<String>)>,
}

impl NativeEnchantOffers {
    /// The player's level as the screen and the click read it.
    pub(super) fn level(&self, level: i32) -> i32 {
        if self.ignores_level {
            self.costs.into_iter().fold(level, i32::max)
        } else {
            level
        }
    }
}

impl Game<'_> {
    /// The offers a mod makes for `item` in the table at `pos`, if one replaced vanilla's.
    pub(super) fn mod_enchant_offers(
        &self,
        pos: [i32; 3],
        item: Option<&ItemStack>,
    ) -> Option<NativeEnchantOffers> {
        let mut offers = NativeEnchantOffers::default();
        self.native_enchant_offers(pos, item?, &mut offers);
        offers.replaced.then_some(offers)
    }
    /// `native_enchant_click` for this window's own table: true when a mod took the click
    /// (whatever it answered), and vanilla's enchanting is left out.
    pub(super) fn mod_enchant_click(&mut self, button: usize) -> bool {
        let Some(s) = self.station.as_ref() else {
            return false;
        };
        if self.remote.is_some() {
            // A server's table: its mods answer. A mod's own offer is sent without the
            // screen's checks, which are vanilla's enchanting's.
            let offered = s.mod_offers.as_ref().is_some_and(|o| o.costs[button] > 0);
            if offered {
                self.remote_station_button(button as i32);
            }
            return offered;
        }
        let pos = s.pos;
        let mut slots = [s.inputs[0].clone(), s.inputs[1].clone()];
        let mut taken = None;
        let (player, name) = (self.own_uuid(), self.own_profile_name());
        let creative = self.creative;
        self.native_enchant_click(
            player,
            &name,
            pos,
            button as i32,
            creative,
            &mut slots,
            &mut taken,
        );
        let Some(done) = taken else {
            return false;
        };
        if done {
            if let Some(s) = self.station.as_mut() {
                let [item, lapis] = slots;
                s.inputs[0] = item.filter(|i| i.count > 0);
                s.inputs[1] = lapis.filter(|l| l.count > 0);
            }
            // `broadcastChanges`, and the `slotsChanged` the new item brings.
            self.station_changed();
        }
        true
    }
}

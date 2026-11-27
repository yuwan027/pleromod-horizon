//! The mod's combat mixins. `PlayerAttackMixin`: a hit with a utensil (`TieredSwordItem`: the
//! forks, knives and spoons) that took health feeds the player a little, by
//! `data/combat.json`, which pleromod's extraction found by running `restoreFoodOnHit` for every
//! roll it can draw.
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Data {
    /// By item: what `FoodData.eat` is given for a roll.
    pub food_on_hit: HashMap<String, Rolls>,
}

#[derive(Deserialize)]
pub struct Rolls {
    /// `random.nextInt(of)`.
    pub of: i32,
    /// Each from its first roll up to the next one's.
    pub gives: Vec<Given>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Given {
    pub from: i32,
    pub nutrition: i32,
    /// `FoodData.eat`'s saturation modifier.
    pub saturation: f32,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/combat.json")))
            .unwrap_or_else(|e| panic!("data/combat.json is what combat.py writes: {e}"))
    })
}

impl Rolls {
    pub fn given(&self, roll: i32) -> Option<Given> {
        self.gives.iter().rev().find(|g| g.from <= roll).copied()
    }
}

/// `FoodData.eat(nutrition, saturationModifier)`: the food level and saturation after it.
pub fn eat(food: i32, saturation: f32, given: Given) -> (i32, f32) {
    let food = (food + given.nutrition).min(20);
    let saturation = (saturation + given.nutrition as f32 * given.saturation * 2.0).min(food as f32);
    (food, saturation)
}

/// A player's hit landed with `stack` in hand (`Item.hurtEnemy`'s place, which a hit that did
/// nothing doesn't reach).
pub fn hit(host: &mut dyn Host, stack: &ItemStack) {
    let Some(rolls) = data().food_on_hit.get(&*stack.item) else {
        return;
    };
    let Some(player) = host.acting_player() else {
        return;
    };
    let Some(roll) = host
        .level_random(&player.dimension)
        .and_then(|r| r.next_int_bound(rolls.of).ok())
    else {
        return;
    };
    let (Some(given), Some((food, saturation, exhaustion))) = (rolls.given(roll), host.player_food(player.uuid)) else {
        return;
    };
    let (food, saturation) = eat(food, saturation, given);
    host.set_player_status(player.uuid, None, Some((food, saturation, exhaustion)), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_utensils_hit_feeds_one_point_and_saturates_two_times_in_three() {
        let data = data();
        // The mod's forks, knives and spoons of five tiers, less those it leaves out.
        assert!(data.food_on_hit.len() >= 10);
        for (item, rolls) in &data.food_on_hit {
            // `food.eat(1, player.getRandom().nextInt(100) < 67 ? 1.0F : 0.0F)`
            assert_eq!(rolls.of, 100, "{item}");
            for roll in [0, 66] {
                assert_eq!(rolls.given(roll).map(|g| (g.nutrition, g.saturation)), Some((1, 1.0)), "{item} {roll}");
            }
            for roll in [67, 99] {
                assert_eq!(rolls.given(roll).map(|g| (g.nutrition, g.saturation)), Some((1, 0.0)), "{item} {roll}");
            }
        }
        assert!(data.food_on_hit.contains_key("plerocottage:wooden_fork"));
    }

    #[test]
    fn food_is_eaten_as_food_data_eats_it() {
        let rich = Given { from: 0, nutrition: 1, saturation: 1.0 };
        let plain = Given { from: 67, nutrition: 1, saturation: 0.0 };
        // One point of food and two of saturation, the saturation never above the food.
        assert_eq!(eat(10, 3.0, rich), (11, 5.0));
        assert_eq!(eat(10, 10.0, rich), (11, 11.0));
        assert_eq!(eat(10, 3.0, plain), (11, 3.0));
        // Full stays full.
        assert_eq!(eat(20, 19.5, rich), (20, 20.0));
    }
}

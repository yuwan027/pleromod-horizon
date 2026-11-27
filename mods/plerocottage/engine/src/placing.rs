//! How the mod's blocks are turned when placed: `data/blocks.json`, where pleromod's extraction
//! wrote the state each block's own `getStateForPlacement` gives for every way its placer looks
//! (`tools/mod-content/horizon_blocks.py`).
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Blocks {
    /// By block and property, the value for each way the placer looks.
    pub turned: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>,
}

pub fn data() -> &'static Blocks {
    static DATA: OnceLock<Blocks> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/blocks.json")))
            .expect("data/blocks.json")
    })
}

/// `Direction.fromYRot`: the way somebody with this yaw looks.
pub fn looking(yaw: f32) -> &'static str {
    ["south", "west", "north", "east"][((f64::from(yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize]
}

/// The state a block of the mod is placed in, where its code turns it to its placer.
pub fn state(level: &dyn LevelHost, state: BlockStateId, yaw: f32) -> Option<BlockStateId> {
    let turned = data().turned.get(&level.state_block(state)?)?;
    let way = looking(yaw);
    let mut placed = state;
    for (property, values) in turned {
        placed = level.state_with_property(placed, property, values.get(way)?)?;
    }
    Some(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_way_somebody_looks() {
        // Direction.fromYRot: south at 0, then west, north and east, a quarter turn each
        assert_eq!([0.0, 90.0, 180.0, -90.0, 44.0, -46.0, 360.0].map(looking), ["south", "west", "north", "east", "south", "east", "south"]);
    }

    #[test]
    fn every_block_is_turned_all_four_ways() {
        for (block, properties) in &data().turned {
            for (property, values) in properties {
                for way in ["north", "east", "south", "west"] {
                    assert!(values.contains_key(way), "{block} {property} {way}");
                }
            }
        }
    }
}

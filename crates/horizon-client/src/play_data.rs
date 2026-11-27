//! The shared world of pleromod.com/play as data: what its API sends and takes (the
//! contract is `packages/contracts/src/play.ts` in the pleromod repository), the chunks'
//! form, and the world folder the game builds from a join.
//!
//! - A chunk (`PlayChunk.data`, in base64; raw in the console's upload) is the chunk's
//!   compound as a region file keeps it, its entities in its own `entities` list (as a chunk
//!   saved before 1.17 had them, which loading still reads), written as NBT and compressed
//!   with zlib (a region file's compression 2). `horizon-ctl play-world` makes them.
//! - A block entity (`blockEntity`, `PlayBlockEntity.data`) is its compound as a chunk's
//!   `block_entities` list holds it, as uncompressed NBT.
//! - The entities (`PlayJoin.entities`, `PlaySaveInput.entities`) are one compound whose
//!   `entities` list holds every lasting entity of the world, as NBT compressed with zlib.
use crate::save::{self, Level, WorldGen, WorldPreset};
use horizon_core::position::ChunkPos;
use horizon_world::{
    chunk::{BlockState, PalettedContainer},
    compression::Compression,
    nbt::{self, JavaString, Limits, NamedTag, Tag},
    region::{ChunkRecord, RegionFile},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    path::Path,
};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Standard base64 with padding, as the API's `z.base64()` takes it.
pub mod base64 {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let (mut acc, mut bits) = (0u32, 0u32);
        for c in text.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                b'=' => break,
                b'\n' | b'\r' | b' ' => continue,
                _ => return None,
            };
            acc = (acc << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        Some(out)
    }
}

// ---------------------------------------------------------------- the API's shapes

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Coins {
    pub gold: i64,
    pub silver: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct World {
    pub id: i64,
    pub chunk_side: i32,
    pub version: i64,
    pub spawn: Spawn,
    #[serde(default)]
    pub idle_seconds: Option<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Spawn {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub data: Option<String>,
}

/// The block at a position now.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub state: String,
    pub version: i64,
    #[serde(default)]
    pub block_entity: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SavedBlockEntity {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub data: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub pitch: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Player {
    pub position: Option<Position>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Price {
    pub action: String,
    pub block: String,
    pub silver: i64,
    pub gold: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Join {
    pub world: World,
    pub chunks: Vec<Chunk>,
    pub changes: Vec<Change>,
    pub block_entities: Vec<SavedBlockEntity>,
    pub entities: Option<String>,
    pub player: Player,
    pub coins: Coins,
    pub prices: Vec<Price>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Changes {
    pub version: i64,
    pub resync: bool,
    pub changes: Vec<Change>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Charge {
    pub silver: i64,
    pub gold: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ActionResult {
    pub status: String,
    pub charge: Charge,
    pub coins: Coins,
    #[serde(default)]
    pub current: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Applied {
    pub version: i64,
    pub resync: bool,
    pub changes: Vec<Change>,
    pub results: Vec<ActionResult>,
    pub coins: Coins,
}

/// The API's envelope: `{ data }`, or `{ error: { code, message } }`.
#[derive(Deserialize)]
pub struct Envelope<T> {
    pub data: Option<T>,
}

// ---------------------------------------------------------------- prices

/// Whose blocks are free: air is never charged.
pub fn is_air(block: &str) -> bool {
    matches!(
        block,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// `minecraft:wheat[age=7]` is `minecraft:wheat`.
pub fn block_id(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// The price of placing or breaking `block`: its own, or else `*`'s; `None` when free.
pub fn price<'p>(prices: &'p [Price], action: &str, block: &str) -> Option<&'p Price> {
    prices
        .iter()
        .find(|p| p.action == action && p.block == block)
        .or_else(|| prices.iter().find(|p| p.action == action && p.block == "*"))
}

/// What a price costs someone with these coins (`play_pay`): its silver price in silver
/// when the silver covers it, or else its gold price in gold when the gold covers it;
/// `None` when neither does.
pub fn pay(price: Option<&Price>, coins: Coins) -> Option<Charge> {
    let Some(price) = price else {
        return Some(Charge::default());
    };
    if coins.silver >= price.silver {
        Some(Charge {
            silver: price.silver,
            gold: 0,
        })
    } else if coins.gold >= price.gold {
        Some(Charge {
            silver: 0,
            gold: price.gold,
        })
    } else {
        None
    }
}

/// One action's two charges (`play_apply`): breaking the old block unless it is air, then
/// placing the new one unless it is air, both only when the block changes (not its
/// properties alone). `None` when the coins cover them not both.
pub fn action_charges(
    prices: &[Price],
    coins: Coins,
    expected: &str,
    state: &str,
) -> Option<[(Charge, &'static str); 2]> {
    let (from, to) = (block_id(expected), block_id(state));
    let mut left = coins;
    let mut charges = [(Charge::default(), "break"), (Charge::default(), "place")];
    if from != to && !is_air(from) {
        let charge = pay(price(prices, "break", from), left)?;
        left.silver -= charge.silver;
        left.gold -= charge.gold;
        charges[0].0 = charge;
    }
    if from != to && !is_air(to) {
        charges[1].0 = pay(price(prices, "place", to), left)?;
    }
    Some(charges)
}

// ---------------------------------------------------------------- states, chunks, block entities

/// `minecraft:oak_door[facing=north,open=true]` as a chunk palette's block state.
pub fn parse_state(text: &str) -> Option<BlockState> {
    let (name, props) = match text.split_once('[') {
        Some((name, rest)) => (name, rest.strip_suffix(']')?),
        None => (text, ""),
    };
    let mut properties = BTreeMap::new();
    for pair in props.split(',').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=')?;
        properties.insert(k.to_owned(), v.to_owned());
    }
    Some(BlockState {
        name: name.to_owned(),
        properties: properties.into_iter().collect(),
    })
}

/// A chunk as the API keeps it, from its compound.
pub fn chunk_bytes(root: &NamedTag) -> io::Result<Vec<u8>> {
    let bytes = nbt::write_named(root, Limits::default()).map_err(|e| invalid(e.to_string()))?;
    Compression::Zlib.encode(&bytes)
}

/// A chunk's compound, from the bytes the API keeps.
pub fn read_chunk_bytes(bytes: &[u8]) -> io::Result<NamedTag> {
    let raw = Compression::Zlib.decode(bytes, Limits::default().max_bytes)?;
    nbt::read_named(&raw, Limits::default())
        .map(|(root, _)| root)
        .map_err(|e| invalid(e.to_string()))
}

/// A compound as uncompressed NBT (a block entity's form).
pub fn tag_bytes(tag: &Tag) -> Vec<u8> {
    let root = NamedTag {
        name: JavaString::from(""),
        tag: tag.clone(),
    };
    nbt::write_named(&root, Limits::default()).unwrap_or_default()
}

/// The compound of uncompressed NBT.
pub fn read_tag(bytes: &[u8]) -> Option<Tag> {
    nbt::read_named(bytes, Limits::default())
        .ok()
        .map(|(root, _)| root.tag)
}

/// The world's entities as the API keeps them (one compound, its `entities` list, zlib).
pub fn entities_bytes(entities: Vec<Tag>) -> Vec<u8> {
    let root = NamedTag {
        name: JavaString::from(""),
        tag: Tag::compound([("entities", Tag::List(entities))]),
    };
    nbt::write_named(&root, Limits::default())
        .ok()
        .and_then(|raw| Compression::Zlib.encode(&raw).ok())
        .unwrap_or_default()
}

/// The entities of [`entities_bytes`].
pub fn read_entities(bytes: &[u8]) -> io::Result<Vec<Tag>> {
    match read_chunk_bytes(bytes)?.tag.get("entities") {
        Some(Tag::List(list)) => Ok(list.clone()),
        _ => Ok(Vec::new()),
    }
}

fn int(tag: &Tag, name: &str) -> Option<i32> {
    match tag.get(name)? {
        Tag::Int(v) => Some(*v),
        Tag::Short(v) => Some(i32::from(*v)),
        Tag::Byte(v) => Some(i32::from(*v)),
        _ => None,
    }
}

/// The chunk an entity is in, from its `Pos`.
fn entity_chunk(entity: &Tag) -> Option<(i32, i32)> {
    let Some(Tag::List(pos)) = entity.get("Pos") else {
        return None;
    };
    let coordinate = |i: usize| match pos.get(i)? {
        Tag::Double(bits) => Some(f64::from_bits(*bits)),
        _ => None,
    };
    let (x, z) = (coordinate(0)?, coordinate(2)?);
    Some(((x.floor() as i32) >> 4, (z.floor() as i32) >> 4))
}

fn list_mut<'t>(root: &'t mut Tag, name: &str) -> &'t mut Vec<Tag> {
    let Tag::Compound(map) = root else {
        unreachable!("a chunk is a compound")
    };
    let entry = map
        .entry(JavaString::from(name))
        .or_insert_with(|| Tag::List(Vec::new()));
    if !matches!(entry, Tag::List(_)) {
        *entry = Tag::List(Vec::new());
    }
    let Tag::List(list) = entry else {
        unreachable!()
    };
    list
}

/// Puts a block entity's compound at `pos` of a chunk, over the one there.
fn put_block_entity(root: &mut Tag, pos: [i32; 3], data: Option<Tag>) {
    let list = list_mut(root, "block_entities");
    list.retain(|e| [int(e, "x"), int(e, "y"), int(e, "z")] != pos.map(Some));
    if let Some(Tag::Compound(mut map)) = data {
        for (k, v) in ["x", "y", "z"].into_iter().zip(pos) {
            map.insert(JavaString::from(k), Tag::Int(v));
        }
        list.push(Tag::Compound(map));
    }
}

/// Adds an empty section where a chunk has none at `y` (a change high above the ground).
fn ensure_section(root: &mut Tag, y: i32) {
    let sy = (y >> 4) as i8;
    let sections = list_mut(root, "sections");
    if sections.iter().any(|s| s.get("Y") == Some(&Tag::Byte(sy))) {
        return;
    }
    let air = BlockState {
        name: "minecraft:air".into(),
        properties: Default::default(),
    };
    sections.push(Tag::compound([
        ("Y", Tag::Byte(sy)),
        (
            "block_states",
            Tag::compound([("palette", Tag::List(vec![air.to_nbt()]))]),
        ),
        (
            "biomes",
            Tag::compound([(
                "palette",
                Tag::List(vec![Tag::String(JavaString::from("minecraft:plains"))]),
            )]),
        ),
    ]));
}

/// What a join puts in the world's folder besides its chunks.
pub struct WorldSetup<'a> {
    pub join: &'a Join,
    /// The world's player's name, whose file the folder holds.
    pub player_uuid: [i32; 4],
}

/// The world's chunks of a join, the changes made since put over them (without drops or
/// neighbour updates: blocks only), then the block entities players saved, then the world's
/// entities when a game saved them. The chunks by position, ready for the region files, and
/// how many were built.
pub fn build_chunks(join: &Join) -> io::Result<HashMap<(i32, i32), NamedTag>> {
    let started = horizon_os::time::Instant::now();
    let mut chunks: HashMap<(i32, i32), NamedTag> = HashMap::new();
    for chunk in &join.chunks {
        let Some(data) = &chunk.data else {
            continue;
        };
        let bytes = base64::decode(data).ok_or_else(|| invalid("a chunk is not base64"))?;
        let root = read_chunk_bytes(&bytes)?;
        chunks.insert((chunk.x, chunk.z), root);
    }
    if let Some(entities) = &join.entities {
        let bytes = base64::decode(entities).ok_or_else(|| invalid("entities are not base64"))?;
        let entities = read_entities(&bytes)?;
        for root in chunks.values_mut() {
            list_mut(&mut root.tag, "entities").clear();
        }
        for entity in entities {
            if let Some(key) = entity_chunk(&entity)
                && let Some(root) = chunks.get_mut(&key)
            {
                list_mut(&mut root.tag, "entities").push(entity);
            }
        }
    }
    let decoded = started.elapsed();
    // Changes by chunk and section, oldest first: each section touched is decoded once and
    // written back once.
    let mut edited = std::collections::HashSet::new();
    let mut by_section: BTreeMap<(i32, i32, i32), Vec<&Change>> = BTreeMap::new();
    for change in &join.changes {
        let key = (change.x >> 4, change.z >> 4);
        if chunks.contains_key(&key) {
            edited.insert(key);
            by_section
                .entry((key.0, key.1, change.y >> 4))
                .or_default()
                .push(change);
        }
    }
    for ((cx, cz, sy), changes) in by_section {
        let root = chunks.get_mut(&(cx, cz)).expect("only chunks of the world");
        ensure_section(&mut root.tag, sy * 16);
        let sections = list_mut(&mut root.tag, "sections");
        let Some(section) = sections
            .iter_mut()
            .find(|s| s.get("Y") == Some(&Tag::Byte(sy as i8)))
        else {
            continue;
        };
        let Some(container) = section.get_mut("block_states") else {
            continue;
        };
        let mut palette = PalettedContainer::from_nbt(container, 4096, 4)?;
        let mut entities = Vec::new();
        for change in changes {
            let Some(state) = parse_state(&change.state) else {
                log::warn!("Horizon: play: a change has no block state: {}", change.state);
                continue;
            };
            let index = (((change.y & 15) << 8) | ((change.z & 15) << 4) | (change.x & 15)) as usize;
            let new_name = state.name.clone();
            let old = BlockState::from_nbt(&palette.set(index, state.to_nbt())?)?;
            // A placed or broken block takes its block entity with it; a toggled one keeps its
            // own unless the change brings one.
            let entity = change
                .block_entity
                .as_deref()
                .and_then(base64::decode)
                .and_then(|b| read_tag(&b));
            if entity.is_some() || old.name != new_name {
                entities.push(([change.x, change.y, change.z], entity));
            }
        }
        palette.compact()?;
        *container = palette.to_nbt();
        for (pos, entity) in entities {
            put_block_entity(&mut root.tag, pos, entity);
        }
    }
    log::info!(
        "Horizon: play: {} chunks decoded in {} ms, {} changes put over them in {} ms",
        chunks.len(),
        decoded.as_millis(),
        join.changes.len(),
        (started.elapsed() - decoded).as_millis()
    );
    for saved in &join.block_entities {
        let Some(root) = chunks.get_mut(&(saved.x >> 4, saved.z >> 4)) else {
            continue;
        };
        let Some(tag) = base64::decode(&saved.data).and_then(|b| read_tag(&b)) else {
            continue;
        };
        put_block_entity(&mut root.tag, [saved.x, saved.y, saved.z], Some(tag));
    }
    // The generated light no longer fits a chunk whose blocks changed, nor its neighbours'
    // edges: those are lit again as they load.
    for (key, root) in chunks.iter_mut() {
        let near = (-1..=1).any(|dx| (-1..=1).any(|dz| edited.contains(&(key.0 + dx, key.1 + dz))));
        if near && let Tag::Compound(map) = &mut root.tag {
            map.insert(JavaString::from("isLightOn"), Tag::Byte(0));
        }
    }
    Ok(chunks)
}

/// The world's folder: its chunks in region files, and a level that generates nothing
/// past them (a flat world without layers), in creative, the player where they were.
pub fn write_world(dir: &Path, setup: &WorldSetup<'_>) -> io::Result<usize> {
    let join = setup.join;
    let chunks = build_chunks(join)?;
    let _ = horizon_os::fs::remove_dir_all(dir);
    let regions = dir.join("dimensions/minecraft/overworld/region");
    horizon_os::fs::create_dir_all(&regions)?;
    let mut by_region: BTreeMap<(i32, i32), Vec<((i32, i32), &NamedTag)>> = BTreeMap::new();
    for (key, root) in &chunks {
        by_region
            .entry((key.0 >> 5, key.1 >> 5))
            .or_default()
            .push((*key, root));
    }
    for ((rx, rz), list) in by_region {
        let mut region =
            RegionFile::open_writable(regions.join(format!("r.{rx}.{rz}.mca")), true)?;
        for ((x, z), root) in list {
            let record = ChunkRecord::from_nbt(root, Compression::Zlib, 0, Limits::default())?;
            region.write(ChunkPos { x, z }, &record)?;
        }
    }
    let spawn = join.world.spawn;
    let position = join.player.position;
    let mut game_rules = BTreeMap::new();
    for (rule, value) in [
        ("mob_griefing", 0),
        ("fire_spread_radius_around_player", 0),
        ("tnt_explodes", 0),
        ("block_explosion_drop_decay", 0),
        ("mob_explosion_drop_decay", 0),
    ] {
        game_rules.insert(rule.to_owned(), value);
    }
    let mut level = Level {
        name: "PleroMod".into(),
        creative: true,
        game_mode: 1,
        allow_commands: Some(false),
        world_spawn: Some([spawn.x, spawn.y, spawn.z]),
        spawn: Some([
            f64::from(spawn.x) + 0.5,
            f64::from(spawn.y),
            f64::from(spawn.z) + 0.5,
        ]),
        position: Some(position.map_or(
            [
                f64::from(spawn.x) + 0.5,
                f64::from(spawn.y),
                f64::from(spawn.z) + 0.5,
            ],
            |p| [p.x, p.y, p.z],
        )),
        rotation: position.map_or([0.0, 0.0], |p| [p.yaw as f32, p.pitch as f32]),
        dimension: "minecraft:overworld".into(),
        player_uuid: Some(setup.player_uuid),
        health: 20.0,
        food: 20,
        saturation: 5.0,
        difficulty: "peaceful".into(),
        on_ground: true,
        game_rules,
        ..Level::default()
    };
    level
        .write(dir)
        .map_err(|e| io::Error::other(e.to_string()))?;
    // An empty Superflat: what lies past the world's chunks is nothing.
    let void = Tag::compound([
        ("biome", Tag::String(JavaString::from("minecraft:plains"))),
        ("features", Tag::Byte(0)),
        ("lakes", Tag::Byte(0)),
        ("layers", Tag::List(Vec::new())),
        ("structure_overrides", Tag::List(Vec::new())),
    ]);
    save::write_world_gen_settings_with(
        dir,
        0,
        WorldGen {
            preset: WorldPreset::Flat,
            structures: false,
            bonus_chest: false,
        },
        Some(void),
        None,
    )
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(chunks.len())
}

/// The square of chunks a world of `side` chunks on each side holds, around the origin
/// (`play_chunk_in_world`): 2 is -1..=0, 3 is -1..=1, 4 is -2..=1.
pub fn chunk_range(side: i32) -> std::ops::RangeInclusive<i32> {
    -(side / 2)..=side - 1 - side / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10\x80"] {
            assert_eq!(base64::decode(&base64::encode(bytes)).unwrap(), bytes);
        }
        assert_eq!(base64::encode(b"Man"), "TWFu");
        assert_eq!(base64::encode(b"Ma"), "TWE=");
    }

    #[test]
    fn charges_follow_play_pay() {
        let prices = vec![
            Price {
                action: "place".into(),
                block: "*".into(),
                silver: 2,
                gold: 1,
            },
            Price {
                action: "break".into(),
                block: "minecraft:stone".into(),
                silver: 5,
                gold: 1,
            },
        ];
        let coins = Coins { gold: 1, silver: 6 };
        // Breaking stone takes 5 silver, which leaves too little silver to place: gold.
        let [b, p] =
            action_charges(&prices, coins, "minecraft:stone", "minecraft:oak_planks").unwrap();
        assert_eq!(b.0, Charge { silver: 5, gold: 0 });
        assert_eq!(p.0, Charge { silver: 0, gold: 1 });
        // A toggle is free.
        assert!(
            action_charges(
                &prices,
                Coins::default(),
                "minecraft:lever[powered=false]",
                "minecraft:lever[powered=true]"
            )
            .is_some()
        );
        assert!(
            action_charges(&prices, Coins::default(), "minecraft:air", "minecraft:dirt").is_none()
        );
    }
}

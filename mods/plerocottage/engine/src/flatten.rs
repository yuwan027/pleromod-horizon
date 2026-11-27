//! How an entity under the mod's flatten effect is drawn. `LivingEntityMixin` keeps a flag on
//! every living entity, set each tick on the server from whether it has the effect, and
//! `LivingEntityRendererMixin` scales the pose its renderer starts from while the flag is set:
//! `data/flatten.json`, which pleromod's extraction found by running both handlers.
//!
//! The flag is the sets below. The server half fills them each tick; in single player and on
//! a host the window reads the same sets, and a client of a server gets them with a mod
//! message whenever they change. What the effect does to the entity is `effects.rs`.
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[derive(Deserialize)]
pub struct Data {
    pub effect: String,
    /// `poseStack.scale(x, y, z)`.
    pub scale: [f32; 3],
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/flatten.json")))
            .unwrap_or_else(|e| panic!("data/flatten.json is what flatten.py writes: {e}"))
    })
}

/// The server's message to its clients: the flattened entities, whole.
pub const CHANNEL: &str = "plerocottage:flattened";
/// A level's entities are all looked at this often; in between, only those known to have had
/// the effect.
const SCAN: u32 = 20;

/// Who is flattened now: mobs by entity id, players by UUID.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Flattened {
    pub mobs: BTreeSet<u32>,
    pub players: BTreeSet<u128>,
}

#[derive(Default)]
struct State {
    now: Flattened,
    /// The mobs of each level that had the effect when last looked at.
    watched: HashMap<String, BTreeSet<u32>>,
    /// Mobs just given the effect, until the tick of their level finds them: the ticks left.
    pending: HashMap<u32, u32>,
    ticks: HashMap<String, u32>,
    /// What each player was last sent.
    sent: HashMap<u128, Flattened>,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

impl Flattened {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = (self.mobs.len() as u32).to_le_bytes().to_vec();
        out.extend(self.mobs.iter().flat_map(|id| id.to_le_bytes()));
        out.extend(self.players.iter().flat_map(|uuid| uuid.to_le_bytes()));
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        let (count, rest) = data.split_first_chunk::<4>()?;
        let count = u32::from_le_bytes(*count) as usize;
        if rest.len() < count.checked_mul(4)? || (rest.len() - count * 4) % 16 != 0 {
            return None;
        }
        let (mobs, players) = rest.split_at(count * 4);
        Some(Self {
            mobs: mobs.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
            players: players.chunks_exact(16).filter_map(|b| Some(u128::from_le_bytes(b.try_into().ok()?))).collect(),
        })
    }
}

impl State {
    /// A level's tick found `found` of its mobs with the effect, and `flat` of the players
    /// `online`: the sets follow, and the players who have not heard of them as they are now
    /// are returned with the message for them.
    fn settle(&mut self, dimension: &str, found: BTreeSet<u32>, online: &[u128], flat: BTreeSet<u128>) -> (Vec<u128>, Vec<u8>) {
        self.pending.retain(|id, left| {
            *left = left.saturating_sub(1);
            *left > 0 && !found.contains(id)
        });
        self.watched.insert(dimension.to_owned(), found);
        self.now.mobs = self.watched.values().flatten().chain(self.pending.keys()).copied().collect();
        self.now.players = flat;
        // Every client hears of a change, and a player who just came of what there is.
        let now = self.now.clone();
        self.sent.retain(|uuid, _| online.contains(uuid));
        let behind: Vec<u128> = online
            .iter()
            .copied()
            .filter(|uuid| self.sent.get(uuid).map_or(now != Flattened::default(), |sent| *sent != now))
            .collect();
        for uuid in &behind {
            self.sent.insert(*uuid, now.clone());
        }
        (behind, now.encode())
    }
}

fn is_effect(id: &str) -> bool {
    let effect = &*data().effect;
    id == effect || effect.split_once(':').is_some_and(|(_, path)| id == path)
}

fn has_effect(host: &dyn Host, entity: u32) -> bool {
    host.entity(entity)
        .and_then(|e| host.entity_effects(e))
        .is_some_and(|effects| effects.iter().any(|(id, _, _)| is_effect(id)))
}

/// `MobEffectEvent.Added` on the server: a mob that got the effect is flattened at once, and
/// looked at by its level's ticks from now on.
pub fn added(effect: &str, entity: Option<i32>) {
    let (Some(id), true) = (entity.and_then(|id| u32::try_from(id).ok()), is_effect(effect)) else {
        return;
    };
    let mut state = state();
    state.pending.insert(id, SCAN * 3);
    state.now.mobs.insert(id);
}

/// `LivingEntity.tick` on the server, for a level: the flag follows `hasEffect`.
pub fn tick(host: &mut dyn Host, dimension: &str) {
    let (scan, known) = {
        let mut state = state();
        let ticks = state.ticks.entry(dimension.to_owned()).or_default();
        *ticks = ticks.wrapping_add(1);
        let scan = *ticks % SCAN == 1;
        let known: BTreeSet<u32> = state.watched.get(dimension).into_iter().flatten().chain(state.pending.keys()).copied().collect();
        (scan, known)
    };
    let candidates: Vec<u32> = if scan {
        host.level_entities(dimension).into_iter().map(|(id, _, _)| id).collect()
    } else {
        known.into_iter().collect()
    };
    let found: BTreeSet<u32> = candidates.into_iter().filter(|id| has_effect(host, *id)).collect();
    let online: Vec<u128> = host.server_players().iter().map(|p| p.uuid).collect();
    let flat: BTreeSet<u128> = online
        .iter()
        .copied()
        .filter(|uuid| host.effects(*uuid).is_some_and(|effects| effects.iter().any(|e| is_effect(&e.id))))
        .collect();
    let (behind, message) = state().settle(dimension, found, &online, flat);
    for uuid in behind {
        // This window's own player needs none: it reads the sets themselves.
        let _ = host.net_send_to_player(uuid, CHANNEL, &message);
    }
}

/// A server's message on a client: its sets are this window's.
pub fn heard(channel: &str, data: &[u8]) {
    if channel == CHANNEL
        && let Some(told) = Flattened::decode(data)
    {
        state().now = told;
    }
}

pub fn flattened(entity: u32, player: Option<u128>) -> bool {
    let state = state();
    match player {
        Some(uuid) => state.now.players.contains(&uuid),
        None => state.now.mobs.contains(&entity),
    }
}

/// `native_living_render_scale`: `poseStack.scale` while the flag is set.
pub fn scale(entity: u32, player: Option<u128>, scale: &mut [f32; 3]) {
    if flattened(entity, player) {
        for (axis, by) in scale.iter_mut().zip(data().scale) {
            *axis *= by;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level's mobs with their effects; nobody is on.
    #[derive(Default)]
    struct Level {
        mobs: Vec<(u32, Vec<&'static str>)>,
    }

    impl Host for Level {
        fn entity(&self, id: u32) -> Option<EntityRef> {
            Some(EntityRef::new(NativeWorldEpoch::new(1), id))
        }
        fn entity_effects(&self, entity: EntityRef) -> Option<Vec<(String, i32, i32)>> {
            let (_, effects) = self.mobs.iter().find(|(id, _)| *id == entity.id())?;
            Some(effects.iter().map(|e| ((*e).to_owned(), 200, 0)).collect())
        }
        fn level_entities(&mut self, _dimension: &str) -> Vec<(u32, String, [f64; 3])> {
            self.mobs.iter().map(|(id, _)| (*id, "minecraft:cow".to_owned(), [0.0; 3])).collect()
        }
        fn server_players(&self) -> Vec<NativeServerPlayer> {
            Vec::new()
        }
    }

    #[test]
    fn the_data_is_the_mixins_scale_of_the_flatten_effect() {
        let data = data();
        assert_eq!(data.effect, "plerocottage:flatten");
        // `poseStack.scale(1.3F, 0.5F, 1.3F)`
        assert_eq!(data.scale, [1.3, 0.5, 1.3]);
    }

    #[test]
    fn the_sets_cross_the_wire_whole() {
        let told = Flattened { mobs: [7, 4_000_000_000].into(), players: [u128::MAX - 3, 12].into() };
        assert_eq!(Flattened::decode(&told.encode()), Some(told));
        assert_eq!(Flattened::decode(&Flattened::default().encode()), Some(Flattened::default()));
        assert_eq!(Flattened::decode(&[1, 0, 0]), None);
        assert_eq!(Flattened::decode(&[2, 0, 0, 0, 1, 0, 0, 0]), None);
    }

    #[test]
    fn players_hear_of_the_sets_when_they_change_and_when_they_come() {
        const STEVE: u128 = 0x51;
        const ALEX: u128 = 0xA1;
        let mut state = State::default();
        // Nothing flattened: nothing to say.
        assert!(state.settle("a", BTreeSet::new(), &[STEVE], BTreeSet::new()).0.is_empty());
        let (behind, message) = state.settle("a", [4].into(), &[STEVE], BTreeSet::new());
        assert_eq!(behind, [STEVE]);
        assert_eq!(Flattened::decode(&message), Some(Flattened { mobs: [4].into(), players: BTreeSet::new() }));
        assert!(state.settle("a", [4].into(), &[STEVE], BTreeSet::new()).0.is_empty());
        // Another level's mobs are kept beside this one's; a player who drank it is one of the set.
        let (behind, message) = state.settle("b", [9].into(), &[STEVE], [STEVE].into());
        assert_eq!(behind, [STEVE]);
        assert_eq!(Flattened::decode(&message), Some(Flattened { mobs: [4, 9].into(), players: [STEVE].into() }));
        // Who comes is told what there is; who was told is not told again.
        assert_eq!(state.settle("b", [9].into(), &[STEVE, ALEX], [STEVE].into()).0, [ALEX]);
        // The effect runs out: everybody hears.
        let (behind, message) = state.settle("a", BTreeSet::new(), &[STEVE, ALEX], BTreeSet::new());
        assert_eq!(behind, [STEVE, ALEX]);
        assert_eq!(Flattened::decode(&message), Some(Flattened { mobs: [9].into(), players: BTreeSet::new() }));
        // Who left and comes back is told again.
        assert!(state.settle("a", BTreeSet::new(), &[ALEX], BTreeSet::new()).0.is_empty());
        assert_eq!(state.settle("a", BTreeSet::new(), &[STEVE, ALEX], BTreeSet::new()).0, [STEVE]);
    }

    /// One test drives the shared sets from start to end: the level's tick, the hooks and a
    /// client's message.
    #[test]
    fn a_mob_is_scaled_while_it_has_the_effect() {
        const DIMENSION: &str = "test:flatten";
        const STEVE: u128 = 0x51;
        let mods = [crate::hooks()];
        let mut level = Level { mobs: vec![(901, vec!["plerocottage:flatten"]), (902, vec!["minecraft:speed"])] };
        let drawn = |level: &Level, entity: u32, player: Option<u128>| {
            let mut scale = [1.0f32; 3];
            dispatch_living_render_scale(level, &mods, entity, "minecraft:cow", player, &mut scale);
            scale
        };
        assert_eq!(drawn(&level, 901, None), [1.0, 1.0, 1.0]);
        // The first tick looks at every entity of the level.
        tick(&mut level, DIMENSION);
        assert_eq!(drawn(&level, 901, None), [1.3, 0.5, 1.3]);
        assert_eq!(drawn(&level, 902, None), [1.0, 1.0, 1.0]);
        // A player with a mob's id is not that mob.
        assert_eq!(drawn(&level, 901, Some(STEVE)), [1.0, 1.0, 1.0]);
        // A mob given the effect between two looks at the whole level is flattened at once.
        level.mobs[1].1.push("plerocottage:flatten");
        added("minecraft:speed", Some(902));
        assert_eq!(drawn(&level, 902, None), [1.0, 1.0, 1.0]);
        added("plerocottage:flatten", Some(902));
        assert_eq!(drawn(&level, 902, None), [1.3, 0.5, 1.3]);
        // The first mob's runs out; the second is kept by the ticks that follow.
        level.mobs[0].1.clear();
        for _ in 0..SCAN * 4 {
            tick(&mut level, DIMENSION);
        }
        assert_eq!(drawn(&level, 901, None), [1.0, 1.0, 1.0]);
        assert_eq!(drawn(&level, 902, None), [1.3, 0.5, 1.3]);
        // A scale another mod set stays under it.
        let mut scale = [2.0f32, 2.0, 1.0];
        dispatch_living_render_scale(&level, &mods, 902, "minecraft:cow", None, &mut scale);
        assert_eq!(scale, [2.6, 1.0, 1.3]);
        // A client of a server has what the server sent, and nothing of another channel.
        let told = Flattened { mobs: [903].into(), players: [STEVE].into() };
        heard("other:channel", &told.encode());
        assert_eq!(drawn(&level, 903, None), [1.0, 1.0, 1.0]);
        heard(CHANNEL, &told.encode());
        assert_eq!(drawn(&level, 903, None), [1.3, 0.5, 1.3]);
        assert_eq!(drawn(&level, 902, None), [1.0, 1.0, 1.0]);
        assert_eq!(drawn(&level, 0, Some(STEVE)), [1.3, 0.5, 1.3]);
    }
}

//! The shared world of pleromod.com/play, in the browser: the page hands the game its API
//! (`horizon_os::play`), and the game plays that world instead of one of its own.
//!
//! - The window joins the world while its resources load, builds the world's folder from what
//!   the join sent (`horizon_client::play_data::write_world`) and opens it on the integrated
//!   server as any saved world. Nothing of it is kept in the browser's storage.
//! - The server sorts every block write of a tick (`World::play_log`): the player's own places,
//!   breaks and toggles are charged ahead of the API from the coins left, queued and sent in
//!   batches; what grew from random ticks is queued as growth; the rest (fluids, redstone,
//!   falling blocks, leaves) every game works out on its own.
//! - The window sends the queues one batch at a time, takes the others' changes from every
//!   answer and from `GET /changes` when the page is back in view, and the server puts them in
//!   the world. Every 30 s, on leaving and when the player closes a container, the server's
//!   save (touched block entities, the player's place, the world's entities) goes out.
use super::*;
use super::lan::LanHost;
use horizon_client::play_data::{self, Change, Charge, Coins, Join, Price, base64};
use horizon_client::text::Text;
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{Clientbound as C, Serverbound as S};
use horizon_world::nbt::{JavaString, Tag};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Blocks whose random tick grows them: crops, stems, saplings, canes, cacti, vines, kelp,
/// and every block of the cottage mod.
const GROWERS: &[&str] = &[
    "wheat",
    "carrots",
    "potatoes",
    "beetroots",
    "torchflower_crop",
    "pitcher_crop",
    "nether_wart",
    "cocoa",
    "sweet_berry_bush",
    "pumpkin_stem",
    "melon_stem",
    "oak_sapling",
    "spruce_sapling",
    "birch_sapling",
    "jungle_sapling",
    "acacia_sapling",
    "dark_oak_sapling",
    "cherry_sapling",
    "pale_oak_sapling",
    "mangrove_propagule",
    "sugar_cane",
    "bamboo_sapling",
    "bamboo",
    "cactus",
    "vine",
    "kelp",
    "weeping_vines",
    "twisting_vines",
    "cave_vines",
];
/// What may grow into a position besides the growers (`play_growables`): fruit, trees.
const GROWN: &[&str] = &[
    "torchflower",
    "pitcher_plant",
    "attached_pumpkin_stem",
    "attached_melon_stem",
    "pumpkin",
    "melon",
    "azalea",
    "flowering_azalea",
    "oak_log",
    "spruce_log",
    "birch_log",
    "jungle_log",
    "acacia_log",
    "dark_oak_log",
    "cherry_log",
    "pale_oak_log",
    "mangrove_log",
    "mangrove_roots",
    "oak_leaves",
    "spruce_leaves",
    "birch_leaves",
    "jungle_leaves",
    "acacia_leaves",
    "dark_oak_leaves",
    "cherry_leaves",
    "pale_oak_leaves",
    "mangrove_leaves",
    "azalea_leaves",
    "flowering_azalea_leaves",
    "pale_hanging_moss",
    "cactus_flower",
    "kelp_plant",
    "weeping_vines_plant",
    "twisting_vines_plant",
    "cave_vines_plant",
];

/// Whether a block's random tick grows it.
pub(super) fn grows(name: &str) -> bool {
    name.starts_with("plerocottage:")
        || name
            .strip_prefix("minecraft:")
            .is_some_and(|n| GROWERS.contains(&n))
}

/// Whether the API takes a block as grown (`play_is_growable`).
fn growable(name: &str) -> bool {
    grows(name)
        || name
            .strip_prefix("minecraft:")
            .is_some_and(|n| GROWN.contains(&n))
}

/// Growth as the API takes it (`play_apply`'s `p_grow`): a growable block's properties
/// changing, or a growable block in air, water or another growable block's place.
fn is_growth(from: &str, to: &str) -> bool {
    let (a, b) = (play_data::block_id(from), play_data::block_id(to));
    if a == b {
        return growable(b) && from != to;
    }
    growable(b) && (play_data::is_air(a) || a == "minecraft:water" || growable(a))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Action {
    x: i32,
    y: i32,
    z: i32,
    expected: String,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_entity: Option<String>,
    /// The other cells of the same block the click changed (≤3): free.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parts: Vec<Part>,
}

/// A cell of a block that spans several (`PlayAction.parts`).
#[derive(Clone, Debug, Serialize)]
struct Part {
    x: i32,
    y: i32,
    z: i32,
    expected: String,
    state: String,
}

impl Action {
    /// Every cell the action changes, the clicked one first.
    fn cells(&self) -> impl Iterator<Item = ([i32; 3], &str)> {
        std::iter::once(([self.x, self.y, self.z], self.expected.as_str())).chain(
            self.parts
                .iter()
                .map(|p| ([p.x, p.y, p.z], p.expected.as_str())),
        )
    }
}

#[derive(Clone, Debug, Serialize)]
struct Growth {
    x: i32,
    y: i32,
    z: i32,
    expected: String,
    state: String,
}

/// An action not yet answered, with what it was charged ahead and what to put back if the
/// API refuses it.
#[derive(Clone, Debug)]
struct Pending {
    action: Action,
    charge: Charge,
    old_entity: Option<Tag>,
}

/// What the window asks of the server thread.
enum ToServer {
    /// Others' changes, oldest first.
    Changes(Vec<Change>),
    /// The world has another block here.
    Conflict {
        pos: [i32; 3],
        state: String,
        entity: Option<String>,
    },
    /// Refused for want of coins: the block as it was.
    Revert {
        pos: [i32; 3],
        state: String,
        entity: Option<Tag>,
        /// What to say: short of gold (true) or silver (false); nothing when `None`.
        refusal: Option<bool>,
    },
}

struct Batch {
    id: String,
    body: String,
    actions: Vec<Pending>,
    grow: Vec<Growth>,
    request: Option<u32>,
    retry_at: Option<Instant>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Stage {
    #[default]
    Off,
    Joining,
    Joined,
    Built,
    Failed,
}

#[derive(Default)]
struct Shared {
    stage: Stage,
    failure: String,
    join: Option<Arc<Join>>,
    world_id: i64,
    version: i64,
    chunk_side: i32,
    prices: Arc<Vec<Price>>,
    coins: Coins,
    outbox: Vec<Pending>,
    grow: Vec<Growth>,
    inflight: Option<Batch>,
    to_server: Vec<ToServer>,
    /// The server's last save, and whether it is due now.
    save_body: Option<String>,
    save_due: bool,
    /// The notice about coins waits for the player to be in the world.
    notice_shown: bool,
    /// The world changed past what the game can follow (its size or chunks, or too far behind):
    /// the window leaves it and joins it anew.
    rejoin: bool,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Default::default);

/// The shared state, on the server thread (which may wait for it).
fn shared() -> MutexGuard<'static, Shared> {
    horizon_os::sync::lock(&SHARED).unwrap_or_else(|e| e.into_inner())
}

/// The shared state on the page's own thread, which the browser forbids to block: `None`
/// while the server thread holds it, and the caller tries again on its next frame.
fn shared_now() -> Option<MutexGuard<'static, Shared>> {
    match SHARED.try_lock() {
        Ok(s) => Some(s),
        Err(std::sync::TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Set once the page gave the game a shared world to play in.
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the game plays the shared world.
pub(super) fn active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

impl Shared {
    /// The coins left once what was charged ahead is paid.
    fn balance(&self) -> Coins {
        let mut coins = self.coins;
        let inflight = self.inflight.iter().flat_map(|b| &b.actions);
        for p in self.outbox.iter().chain(inflight) {
            coins.silver -= p.charge.silver;
            coins.gold -= p.charge.gold;
        }
        coins
    }
    /// Whether an action at `pos` waits for its answer.
    fn pending_at(&self, pos: [i32; 3]) -> bool {
        let inflight = self.inflight.iter().flat_map(|b| &b.actions);
        self.outbox
            .iter()
            .chain(inflight)
            .any(|p| p.action.cells().any(|(cell, _)| cell == pos))
    }
    fn in_world(&self, x: i32, z: i32) -> bool {
        let range = play_data::chunk_range(self.chunk_side);
        range.contains(&(x >> 4)) && range.contains(&(z >> 4))
    }
}

/// A version 4 UUID: the batch's name, the same when it is sent again.
fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut bytes = [0u8; 16];
    for (i, half) in bytes.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        h.write_u128(
            horizon_os::time::SystemTime::now()
                .duration_since(horizon_os::time::SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_usize(i);
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn block_name(state: &str) -> Text {
    let id = play_data::block_id(state);
    Text::translate(&format!("block.{}", id.replace(':', ".")), &[])
}

fn coins_text(charge: Charge) -> Text {
    if charge.gold > 0 {
        Text::translate("horizon.play.gold", &[Text::literal(charge.gold.to_string())])
    } else {
        Text::translate(
            "horizon.play.silver",
            &[Text::literal(charge.silver.to_string())],
        )
    }
}

/// The tooltip line of a block's prices: `放置 2 银币 / 1 金币 · 破坏 …`; `None` when both are
/// free.
pub(super) fn tooltip(block: &str) -> Option<Text> {
    let prices = shared_now()?.prices.clone();
    let mut parts = Vec::new();
    for (action, key) in [
        ("place", "horizon.play.tooltip.place"),
        ("break", "horizon.play.tooltip.break"),
    ] {
        // A silver price of nothing is always covered: free.
        let Some(price) = play_data::price(&prices, action, block).filter(|p| p.silver > 0) else {
            continue;
        };
        parts.push(Text::translate(
            key,
            &[Text::translate(
                "horizon.play.price",
                &[
                    Text::literal(price.silver.to_string()),
                    Text::literal(price.gold.to_string()),
                ],
            )],
        ));
    }
    let mut parts = parts.into_iter();
    let first = parts.next()?;
    Some(parts.fold(first, |line, part| line.append(" · ").append(part)))
}

/// Whether the coins left cover breaking a block (its state as text).
pub(super) fn break_affordable(state: &str) -> bool {
    // Busy: shown, and the server, which knows, decides.
    let Some(s) = shared_now() else {
        return true;
    };
    play_data::action_charges(&s.prices, s.balance(), state, "minecraft:air").is_some()
}

/// Whether the coins notice should show now (once a session, unless the player said never).
pub(super) fn take_notice() -> bool {
    let Some(mut s) = shared_now() else {
        return false;
    };
    if s.stage != Stage::Built || s.notice_shown {
        return false;
    }
    s.notice_shown = true;
    true
}

// ---------------------------------------------------------------- the server thread

#[derive(Default)]
struct ServerPlay {
    started: bool,
    /// Positions changed before the game opened the world, which get one block update each
    /// once their chunk is in.
    updates: Vec<[i32; 3]>,
    caught_up: bool,
    /// Block entities the player used or made.
    touched: BTreeSet<[i32; 3]>,
    ticks: u64,
    rng: u64,
    /// A container closed: the save goes out after this tick.
    save_soon: bool,
    /// The items last kept (`play_keep_inventory`).
    kept_inventory: Option<Vec<u8>>,
}

static SERVER: LazyLock<Mutex<ServerPlay>> = LazyLock::new(Default::default);

fn server() -> MutexGuard<'static, ServerPlay> {
    horizon_os::sync::lock(&SERVER).unwrap_or_else(|e| e.into_inner())
}

/// What a player's packet may change, taken before it is handled.
pub(super) struct Capture {
    start: usize,
    /// The clicked block, and the block entities there and beside it before.
    clicked: [i32; 3],
    old_entities: Vec<([i32; 3], Tag)>,
    /// Bone meal grows what it is used on: growth, not the player's.
    growth: bool,
    kind: ClickKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClickKind {
    /// A creative break.
    Break,
    /// A use on a block: a place or a toggle.
    UseOn,
    /// A use in the air: a bucket.
    UseItem,
}

pub(super) enum Before {
    Pass,
    Capture(Capture),
    Refused,
}

impl Game<'_> {
    pub(super) fn play_state_text(&self, state: BlockStateId) -> String {
        let Some(s) = self.pack.assets.registry.state(state) else {
            return "minecraft:air".into();
        };
        if s.properties.is_empty() {
            return s.name.clone();
        }
        let props: Vec<String> = s
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}[{}]", s.name, props.join(","))
    }

    fn play_owner_conn(&self) -> Option<u32> {
        let host = self.lan.as_ref()?;
        let owner = host.hub.singleplayer_owner;
        host.hub
            .guests
            .iter()
            .find(|g| Some(g.uuid) == owner)
            .or_else(|| host.hub.guests.first())
            .map(|g| g.conn)
    }

    fn play_tell(host: &LanHost, conn: u32, text: &Text) {
        host.hub.send_to(
            conn,
            &C::SystemChat(horizon_protocol::packets::play::SystemChat {
                content: horizon_client::lan::data::component_nbt(&text.component()),
                overlay: true,
            }),
        );
    }

    fn play_tell_owner(&self, text: &Text) {
        if let (Some(conn), Some(host)) = (self.play_owner_conn(), self.lan.as_ref()) {
            Self::play_tell(host, conn, text);
        }
    }

    /// Before a player's packet: places, breaks and toggles are watched; a creative break
    /// the coins left do not cover is refused at once, the client's block put back.
    pub(super) fn play_before(&mut self, host: &mut LanHost, conn: u32, packet: &S) -> Before {
        if !active() || self.world.play_log.is_none() {
            return Before::Pass;
        }
        let Some(guest) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return Before::Pass;
        };
        let (clicked, others, growth, kind) = match packet {
            S::PlayerAction(a) if a.action == 0 && guest.game_mode == 1 => {
                let state = self.world.block(a.pos[0], a.pos[1], a.pos[2]);
                let text = self.play_state_text(state);
                if !play_data::is_air(play_data::block_id(&text)) {
                    let s = shared();
                    if play_data::action_charges(&s.prices, s.balance(), &text, "minecraft:air")
                        .is_none()
                    {
                        let gold = s.balance().gold > 0;
                        drop(s);
                        host.hub.ack(conn, a.sequence);
                        host.hub.send_to(
                            conn,
                            &C::BlockUpdate(horizon_protocol::packets::play::BlockUpdate {
                                pos: a.pos,
                                state: state.0 as i32,
                            }),
                        );
                        Self::play_tell(host, conn, &refusal(gold));
                        return Before::Refused;
                    }
                }
                (a.pos, Vec::new(), false, ClickKind::Break)
            }
            S::UseItemOn(p) => {
                let held = if p.hand == 1 {
                    guest.offhand.as_ref()
                } else {
                    guest.inventory.get(guest.selected).and_then(Option::as_ref)
                };
                let growth = held.is_some_and(|s| &*s.item == "minecraft:bone_meal");
                let beside = super::hit::Hit::from_use_item_on(p).beside();
                (p.pos, vec![beside], growth, ClickKind::UseOn)
            }
            S::UseItem(_) => {
                // A bucket's fluid lands where the player looks.
                let pos = guest.position.map(|v| v.floor() as i32);
                (pos, Vec::new(), false, ClickKind::UseItem)
            }
            _ => return Before::Pass,
        };
        let mut old_entities = Vec::new();
        for pos in std::iter::once(clicked).chain(others) {
            if let Some(tag) = self.block_entity_nbt(pos) {
                old_entities.push((pos, tag));
            }
        }
        Before::Capture(Capture {
            start: self.world.play_log.as_ref().map_or(0, Vec::len),
            clicked,
            old_entities,
            growth,
            kind,
        })
    }

    /// After the packet: what it changed is the player's (charged ahead, queued for the API),
    /// or put back when the coins left do not cover it.
    pub(super) fn play_after(&mut self, host: &mut LanHost, conn: u32, capture: Capture) {
        let Some(log) = self.world.play_log.as_ref() else {
            return;
        };
        let writes: Vec<_> = log.get(capture.start..).unwrap_or_default().to_vec();
        if writes.is_empty() {
            return;
        }
        // Each position's first state and last.
        let mut order: Vec<[i32; 3]> = Vec::new();
        let mut changes: HashMap<[i32; 3], (u32, u32)> = HashMap::new();
        for ((x, y, z), old, new) in writes {
            let pos = [x, y, z];
            changes
                .entry(pos)
                .and_modify(|c| c.1 = new)
                .or_insert_with(|| {
                    order.push(pos);
                    (old, new)
                });
        }
        let mut list = Vec::new();
        {
            let s = shared();
            for pos in order {
                let (old, new) = changes[&pos];
                if old == new || !s.in_world(pos[0], pos[2]) {
                    continue;
                }
                let (from, to) = (
                    self.play_state_text(BlockStateId(old)),
                    self.play_state_text(BlockStateId(new)),
                );
                list.push((pos, BlockStateId(old), from, to));
            }
        }
        if list.is_empty() {
            return;
        }
        if capture.growth {
            let mut s = shared();
            for (pos, _, from, to) in list {
                if is_growth(&from, &to) {
                    push_growth(&mut s, pos, from, to);
                }
            }
            return;
        }
        let id = play_data::block_id;
        let changed = |c: &([i32; 3], BlockStateId, String, String)| id(&c.2) != id(&c.3);
        // The cell the click changed: the block broken, the block toggled, or the block placed.
        let primary = match capture.kind {
            ClickKind::Break => list.iter().position(|c| c.0 == capture.clicked),
            ClickKind::UseOn => list
                .iter()
                .position(|c| c.0 == capture.clicked && !changed(c))
                .or_else(|| {
                    list.iter()
                        .position(|c| changed(c) && !play_data::is_air(id(&c.3)))
                })
                .or_else(|| list.iter().position(changed)),
            ClickKind::UseItem => list.iter().position(changed),
        };
        let Some(primary) = primary else {
            return;
        };
        let (at, _, from, to) = list[primary].clone();
        // The other cells of the same block (a door's or a bed's other half, a tall plant's):
        // free, uploaded with it. Whatever else changed (a torch that lost its wall, water that
        // flows) every game works out for itself.
        let breaking = id(&from) != id(&to) && play_data::is_air(id(&to));
        let near = |p: [i32; 3]| (0..3).all(|i| (p[i] - at[i]).abs() <= 1);
        let parts: Vec<Part> = list
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                *i != primary
                    && near(c.0)
                    && if breaking {
                        id(&c.2) == id(&from) && changed(c)
                    } else {
                        id(&c.3) == id(&to)
                    }
            })
            .take(3)
            .map(|(_, c)| Part {
                x: c.0[0],
                y: c.0[1],
                z: c.0[2],
                expected: c.2.clone(),
                state: c.3.clone(),
            })
            .collect();
        // One charge for the click, from the coins left, as the API will.
        let mut s = shared();
        let mut left = s.balance();
        let Some(charges) = play_data::action_charges(&s.prices, left, &from, &to) else {
            let gold = left.gold > 0;
            drop(s);
            // Back as it was, in the opposite order, on the client too.
            for (pos, old, _, _) in list.iter().rev() {
                self.play_put(*pos, *old, None);
            }
            for (pos, tag) in &capture.old_entities {
                self.play_put_entity(*pos, tag.clone());
            }
            for (pos, old, _, _) in &list {
                host.hub.send_to(
                    conn,
                    &C::BlockUpdate(horizon_protocol::packets::play::BlockUpdate {
                        pos: *pos,
                        state: old.0 as i32,
                    }),
                );
            }
            Self::play_tell(host, conn, &refusal(gold));
            return;
        };
        let mut line: Option<Text> = None;
        let mut total = Charge::default();
        let (mut used_silver, mut used_gold) = (false, false);
        for (charge, action) in charges {
            if charge.silver + charge.gold == 0 {
                continue;
            }
            total.silver += charge.silver;
            total.gold += charge.gold;
            used_silver |= charge.silver > 0;
            used_gold |= charge.gold > 0;
            let (key, block) = if action == "break" {
                ("horizon.play.broke", &from)
            } else {
                ("horizon.play.placed", &to)
            };
            let part = Text::translate(key, &[block_name(block), coins_text(charge)]);
            line = Some(match line {
                Some(line) => line.append(" · ").append(part),
                None => part,
            });
        }
        left.silver -= total.silver;
        left.gold -= total.gold;
        let entity = self.block_entity_nbt(at);
        let touched = entity.is_some();
        s.outbox.push(Pending {
            action: Action {
                x: at[0],
                y: at[1],
                z: at[2],
                expected: from,
                state: to,
                block_entity: entity.map(|t| base64::encode(&play_data::tag_bytes(&t))),
                parts,
            },
            charge: total,
            old_entity: capture
                .old_entities
                .iter()
                .find(|(p, _)| *p == at)
                .map(|(_, t)| t.clone()),
        });
        drop(s);
        if touched {
            server().touched.insert(at);
        }
        if let Some(line) = line {
            let balance = match (used_silver, used_gold) {
                (true, true) => Text::translate(
                    "horizon.play.coins",
                    &[
                        Text::literal(left.silver.to_string()),
                        Text::literal(left.gold.to_string()),
                    ],
                ),
                (false, true) => coins_text(Charge {
                    silver: 0,
                    gold: left.gold,
                }),
                _ => Text::literal(left.silver.to_string()),
            };
            let line = line
                .append(" · ")
                .append(Text::translate("horizon.play.balance", &[balance]));
            Self::play_tell(host, conn, &line);
        }
    }

    /// A container the player closed: its block entity is saved now.
    pub(super) fn play_container_closed(pos: Option<(i32, i32, i32)>) {
        if !active() {
            return;
        }
        let mut sv = server();
        if let Some((x, y, z)) = pos {
            sv.touched.insert([x, y, z]);
        }
        sv.save_soon = true;
    }

    /// Sets a block as the world has it, without drops or neighbour updates.
    fn play_put(&mut self, pos: [i32; 3], state: BlockStateId, entity: Option<Tag>) {
        let log = self.world.play_log.take();
        let _ = self.place_state(pos, state, true, false);
        self.world.play_log = log;
        if let Some(tag) = entity {
            self.play_put_entity(pos, tag);
        }
    }

    fn play_put_entity(&mut self, pos: [i32; 3], tag: Tag) {
        let Tag::Compound(mut map) = tag else {
            return;
        };
        for (k, v) in ["x", "y", "z"].into_iter().zip(pos) {
            map.insert(JavaString::from(k), Tag::Int(v));
        }
        if let Some((key, entity)) = horizon_client::save::decode_block_entity(&Tag::Compound(map))
        {
            self.world.block_entities.insert(key, entity);
        }
    }

    /// One block update at `pos`: the block itself and its neighbours hear of it, so water
    /// flows and redstone powers as the world now is.
    fn play_block_update(&mut self, pos: [i32; 3]) {
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.settle_block(level, ticks, at));
        let _ = self.process_level_events();
    }

    fn play_set_text(&mut self, pos: [i32; 3], state: &str, entity: Option<Tag>) -> bool {
        let Ok(id) = super::render_control::parse_state(self.pack.assets.registry, state) else {
            log::warn!("Horizon: play: unknown block state {state}");
            return false;
        };
        if self.world.block(pos[0], pos[1], pos[2]) == id && entity.is_none() {
            return false;
        }
        self.play_put(pos, id, entity);
        true
    }

    /// The server thread's part, after each tick.
    pub(super) fn play_server_tick(&mut self) {
        if !active() || self.lan.is_none() {
            return;
        }
        let first = !server().started;
        if first {
            self.play_start();
        }
        // What the window asks.
        let tasks = std::mem::take(&mut shared().to_server);
        let mut updated = Vec::new();
        for task in tasks {
            match task {
                ToServer::Changes(changes) => {
                    for change in changes {
                        let pos = [change.x, change.y, change.z];
                        if shared().pending_at(pos) {
                            continue;
                        }
                        let entity = decode_entity(change.block_entity.as_deref());
                        if self.play_set_text(pos, &change.state, entity) {
                            updated.push(pos);
                        }
                    }
                }
                ToServer::Conflict { pos, state, entity } => {
                    if self.play_set_text(pos, &state, decode_entity(entity.as_deref())) {
                        updated.push(pos);
                    }
                    self.play_tell_owner(&Text::translate("horizon.play.conflict", &[]));
                }
                ToServer::Revert {
                    pos,
                    state,
                    entity,
                    refusal: told,
                } => {
                    if self.play_set_text(pos, &state, entity) {
                        updated.push(pos);
                    }
                    if let Some(gold) = told {
                        self.play_tell_owner(&refusal(gold));
                    }
                }
            }
        }
        for pos in updated {
            self.play_block_update(pos);
        }
        // The changes made before the world opened, once their chunks are in.
        let waiting = std::mem::take(&mut server().updates);
        let mut later = Vec::new();
        for pos in waiting {
            if self.world.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4)) {
                self.play_block_update(pos);
            } else {
                later.push(pos);
            }
        }
        server().updates.extend(later);
        self.play_catch_up();
        self.play_collect_growth();
        // The save: ready every 10 s for a page that goes away, sent every 30 s.
        let (ticks, soon) = {
            let mut sv = server();
            sv.ticks += 1;
            (sv.ticks, std::mem::take(&mut sv.save_soon))
        };
        // The items every 2 s (written only when they changed): a page can go at any moment.
        if soon || ticks % 40 == 0 {
            self.play_keep_inventory();
        }
        if soon || ticks % 200 == 0 {
            let body = self.play_save_body();
            let mut s = shared();
            s.save_body = body;
            if soon || ticks % 600 == 0 {
                s.save_due = true;
            }
        }
    }

    /// The world as the shared one: its border round its chunks, its rules, the changes made
    /// before it opened queued for their block updates.
    fn play_start(&mut self) {
        let (join, side) = {
            let s = shared();
            (s.join.clone(), s.chunk_side)
        };
        let Some(join) = join else {
            return;
        };
        let mut sv = server();
        sv.started = true;
        sv.rng = 0x9E37_79B9_7F4A_7C15 ^ join.world.version as u64;
        sv.updates = join
            .changes
            .iter()
            .map(|c| [c.x, c.y, c.z])
            .chain(join.block_entities.iter().map(|b| [b.x, b.y, b.z]))
            .collect();
        drop(sv);
        self.world.play_log = Some(Vec::new());
        let range = play_data::chunk_range(side);
        let (lo, hi) = (*range.start() * 16, (*range.end() + 1) * 16);
        self.border
            .set_center([f64::from(lo + hi) / 2.0, f64::from(lo + hi) / 2.0]);
        self.border.set_size(f64::from(hi - lo));
        use horizon_client::gamerules::{self, Value};
        for (rule, value) in [
            ("mob_griefing", Value::Bool(false)),
            ("tnt_explodes", Value::Bool(false)),
            ("fire_spread_radius_around_player", Value::Int(0)),
        ] {
            if let Some(r) = gamerules::rule(rule) {
                self.set_game_rule(r, value);
            }
        }
    }

    /// What grows catches up on the time nobody was in the world, at most a day: each growing
    /// block of the world gets the random ticks it would have had.
    fn play_catch_up(&mut self) {
        if server().caught_up {
            return;
        }
        let (side, idle) = {
            let s = shared();
            (
                s.chunk_side,
                s.join
                    .as_ref()
                    .and_then(|j| j.world.idle_seconds)
                    .unwrap_or(0),
            )
        };
        let range = play_data::chunk_range(side);
        let keys: Vec<(i32, i32)> = range
            .clone()
            .flat_map(|x| range.clone().map(move |z| (x, z)))
            .collect();
        // Every chunk of the world in and lit.
        if server().ticks < 40
            || !keys.iter().all(|k| {
                self.world.chunks.get(k).is_some_and(|c| c.lit) && self.streaming.is_lit(k.0, k.1)
            })
        {
            return;
        }
        server().caught_up = true;
        let ticks = (idle.max(0) * 20).min(24_000);
        if ticks == 0 {
            return;
        }
        let speed = i64::from(self.ui.rules.int("random_tick_speed").max(0));
        // A block gets `speed` of its section's 4096 random ticks a tick, on average.
        let mean = ticks as f64 * speed as f64 / 4096.0;
        let mut growers = Vec::new();
        let (min_y, max_y) = (self.world.shape.min_y, self.world.shape.max_y());
        for &(cx, cz) in &keys {
            for y in min_y..max_y {
                for lz in 0..16 {
                    for lx in 0..16 {
                        let (x, z) = (cx * 16 + lx, cz * 16 + lz);
                        let state = self.world.block(x, y, z);
                        if grows(&self.res.facts().get(state).name) {
                            growers.push([x, y, z]);
                        }
                    }
                }
            }
        }
        let started = Instant::now();
        for pos in &growers {
            let n = {
                let mut sv = server();
                poisson(&mut sv.rng, mean)
            };
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            for _ in 0..n {
                self.with_level(|b, level, ticks| {
                    b.random_tick(level, ticks, at);
                    if let Some(log) = &level.world.play_log {
                        let end = log.len();
                        level.world.play_marks.push((end, None));
                    }
                });
            }
            let _ = self.process_level_events();
        }
        log::info!(
            "Horizon: play: {} growing blocks caught up on {ticks} ticks in {} ms",
            growers.len(),
            started.elapsed().as_millis()
        );
    }

    /// The writes of the random ticks of growing blocks since the last time, as growth.
    fn play_collect_growth(&mut self) {
        let log = self.world.play_log.replace(Vec::new()).unwrap_or_default();
        let marks = std::mem::take(&mut self.world.play_marks);
        let mut s = shared();
        for (i, &(start, grows)) in marks.iter().enumerate() {
            if grows != Some(true) {
                continue;
            }
            let end = marks.get(i + 1).map_or(log.len(), |m| m.0);
            let mut seen: Vec<([i32; 3], u32, u32)> = Vec::new();
            for &((x, y, z), old, new) in log.get(start..end).unwrap_or_default() {
                let pos = [x, y, z];
                match seen.iter_mut().find(|e| e.0 == pos) {
                    Some(e) => e.2 = new,
                    None => seen.push((pos, old, new)),
                }
            }
            for (pos, old, new) in seen {
                if old == new || !s.in_world(pos[0], pos[2]) {
                    continue;
                }
                let (from, to) = (
                    self.play_state_text(BlockStateId(old)),
                    self.play_state_text(BlockStateId(new)),
                );
                if is_growth(&from, &to) {
                    push_growth(&mut s, pos, from, to);
                }
            }
        }
    }

    /// The player's items into the browser's storage (`inventory_file`), when they changed.
    fn play_keep_inventory(&mut self) {
        let world = shared().world_id;
        let Some(file) = inventory_file(world) else {
            return;
        };
        let Some(player) = self.lan.as_ref().and_then(|host| {
            let owner = host.hub.singleplayer_owner;
            let g = host
                .hub
                .guests
                .iter()
                .find(|g| Some(g.uuid) == owner)
                .or_else(|| host.hub.guests.first())?;
            Some(horizon_client::lan::playerdata::player_tag(g))
        }) else {
            return;
        };
        let kept = Tag::compound(
            KEPT_PLAYER
                .iter()
                .filter_map(|k| Some((*k, player.get(k)?.clone()))),
        );
        let bytes = play_data::tag_bytes(&kept);
        let mut sv = server();
        if sv.kept_inventory.as_deref() == Some(&bytes[..]) {
            return;
        }
        sv.kept_inventory = Some(bytes);
        drop(sv);
        if let Some(dir) = file.parent() {
            let _ = horizon_os::fs::create_dir_all(dir);
        }
        if let Err(e) = horizon_client::save::write_dat(&file, kept) {
            log::warn!("Horizon: play: keeping the items: {e}");
        }
    }

    /// `POST /save`'s body: the block entities the player touched, where the player is, and
    /// the world's lasting entities.
    fn play_save_body(&mut self) -> Option<String> {
        let (world_id, version, side) = {
            let s = shared();
            (s.world_id, s.version, s.chunk_side)
        };
        let touched: Vec<[i32; 3]> = server().touched.iter().copied().collect();
        let mut block_entities = Vec::new();
        for pos in touched {
            if let Some(tag) = self.block_entity_nbt(pos) {
                let bytes = play_data::tag_bytes(&tag);
                if bytes.len() <= 64 * 1024 {
                    block_entities.push(serde_json::json!({
                        "x": pos[0], "y": pos[1], "z": pos[2], "data": base64::encode(&bytes),
                    }));
                }
            }
        }
        let position = self.lan.as_ref().and_then(|host| {
            let owner = host.hub.singleplayer_owner;
            let g = host
                .hub
                .guests
                .iter()
                .find(|g| Some(g.uuid) == owner)
                .or_else(|| host.hub.guests.first())?;
            Some(serde_json::json!({
                "x": g.position[0], "y": g.position[1], "z": g.position[2],
                "yaw": f64::from(g.yaw), "pitch": f64::from(g.pitch),
            }))
        });
        let range = play_data::chunk_range(side);
        let keys: HashSet<(i32, i32)> = range
            .clone()
            .flat_map(|x| range.clone().map(move |z| (x, z)))
            .filter(|k| self.world.chunks.contains_key(k))
            .collect();
        let mut body = serde_json::json!({
            "worldId": world_id,
            "version": version,
            "blockEntities": block_entities,
        });
        if let Some(position) = position {
            body["position"] = position;
        }
        // Only when every chunk of the world is in: a part would lose the others' entities.
        if keys.len() as i32 == side * side {
            let entities: Vec<Tag> = self
                .entity_chunks(&keys, false)
                .into_iter()
                .flat_map(|(_, list)| list)
                .filter(|tag| {
                    let id = horizon_client::save::text(tag.get("id")).unwrap_or_default();
                    !TRANSIENT.contains(&id.as_str())
                })
                .collect();
            let bytes = play_data::entities_bytes(entities);
            if !bytes.is_empty() && bytes.len() <= 1024 * 1024 {
                body["entities"] = serde_json::json!(base64::encode(&bytes));
            }
        }
        Some(body.to_string())
    }
}

/// The world's player file's name (`players/data/<uuid>.dat`): one player per game.
const PLAYER_UUID: [i32; 4] = [0x504c_4552, 0x4f4d_4f44, 0x504c_4159, 0x0000_0001];
/// What of the player the game keeps in the browser, by world: their items.
const KEPT_PLAYER: &[&str] = &["Inventory", "equipment", "SelectedItemSlot", "EnderItems"];

/// Where the player's items in world `id` are kept: a file of the game's folder, which the
/// page keeps in the browser's storage (unlike `saves/`).
fn inventory_file(id: i64) -> Option<std::path::PathBuf> {
    Some(
        horizon_os::env::current_dir()
            .ok()?
            .join("play")
            .join(format!("inventory-{id}.dat")),
    )
}

/// The kept items back in the player's file of the world just built.
fn restore_inventory(world: &std::path::Path, id: i64, uuid: [i32; 4]) {
    let Some(kept) = inventory_file(id).and_then(|f| horizon_client::save::read_dat(&f)) else {
        return;
    };
    let file = horizon_client::lan::playerdata::path(
        world,
        horizon_client::lan::playerdata::ints_uuid(uuid),
    );
    let Some(Tag::Compound(mut player)) = horizon_client::save::read_dat(&file) else {
        return;
    };
    for key in KEPT_PLAYER {
        if let Some(tag) = kept.get(key) {
            player.insert(JavaString::from(*key), tag.clone());
        }
    }
    if let Err(e) = horizon_client::save::write_dat(&file, Tag::Compound(player)) {
        log::warn!("Horizon: play: the kept items: {e}");
    }
}

/// Entities a save leaves out: they last moments.
const TRANSIENT: &[&str] = &[
    "minecraft:item",
    "minecraft:experience_orb",
    "minecraft:falling_block",
    "minecraft:tnt",
    "minecraft:arrow",
    "minecraft:spectral_arrow",
    "minecraft:trident",
    "minecraft:snowball",
    "minecraft:egg",
    "minecraft:ender_pearl",
    "minecraft:potion",
    "minecraft:splash_potion",
    "minecraft:lingering_potion",
    "minecraft:experience_bottle",
    "minecraft:fireball",
    "minecraft:small_fireball",
    "minecraft:dragon_fireball",
    "minecraft:wither_skull",
    "minecraft:shulker_bullet",
    "minecraft:llama_spit",
    "minecraft:firework_rocket",
    "minecraft:wind_charge",
    "minecraft:breeze_wind_charge",
    "minecraft:fishing_bobber",
    "minecraft:evoker_fangs",
    "minecraft:area_effect_cloud",
    "minecraft:lightning_bolt",
    "minecraft:eye_of_ender",
    "minecraft:player",
];

fn refusal(gold: bool) -> Text {
    Text::translate(
        if gold {
            "horizon.play.no_gold"
        } else {
            "horizon.play.no_silver"
        },
        &[],
    )
}

fn decode_entity(data: Option<&str>) -> Option<Tag> {
    base64::decode(data?).and_then(|b| play_data::read_tag(&b))
}

fn push_growth(s: &mut Shared, pos: [i32; 3], from: String, to: String) {
    if let Some(i) = s.grow.iter().position(|g| [g.x, g.y, g.z] == pos) {
        // Back where it began (a tree that failed to grow puts its sapling back): nothing grew.
        if s.grow[i].expected == to {
            s.grow.remove(i);
        } else {
            s.grow[i].state = to;
        }
        return;
    }
    if from == to {
        return;
    }
    s.grow.push(Growth {
        x: pos[0],
        y: pos[1],
        z: pos[2],
        expected: from,
        state: to,
    });
}

/// A count of events with mean `mean` (Knuth's, normal for large means).
fn poisson(rng: &mut u64, mean: f64) -> u32 {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        (*rng >> 11) as f64 / (1u64 << 53) as f64
    };
    if mean > 30.0 {
        let (u, v) = (next().max(1e-12), next());
        let normal = (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
        return (mean + normal * mean.sqrt()).round().max(0.0) as u32;
    }
    let limit = (-mean).exp();
    let (mut k, mut p) = (0u32, 1.0);
    loop {
        p *= next();
        if p <= limit {
            return k;
        }
        k += 1;
    }
}

// ---------------------------------------------------------------- the window

#[cfg(target_arch = "wasm32")]
mod net {
    use super::*;
    use horizon_client::play_data::{ActionResult, Applied, Changes, Envelope};

    #[derive(Default)]
    pub(super) struct Net {
        join: Option<u32>,
        changes: Option<u32>,
        save: Option<u32>,
        last_actions: Option<Instant>,
        last_growth: Option<Instant>,
        last_save: Option<Instant>,
        farewell: (usize, usize, Option<usize>),
    }

    pub(super) static NET: LazyLock<Mutex<Net>> = LazyLock::new(Default::default);

    fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
        serde_json::from_str::<Envelope<T>>(text).ok()?.data
    }

    /// The batch's body.
    fn batch_body(s: &Shared, id: &str, actions: &[Pending], grow: &[Growth]) -> String {
        let actions: Vec<&Action> = actions.iter().map(|p| &p.action).collect();
        serde_json::json!({
            "worldId": s.world_id,
            "batchId": id,
            "baseVersion": s.version,
            "actions": actions,
            "grow": grow,
        })
        .to_string()
    }

    /// The window's part, between frames: joining, the batches, the changes, the saves.
    pub(super) fn pump() {
        let mut net = horizon_os::sync::lock(&NET).unwrap_or_else(|e| e.into_inner());
        let Some(mut s) = shared_now() else {
            return;
        };
        match s.stage {
            Stage::Off | Stage::Failed => return,
            Stage::Joining => {
                if net.join.is_none() {
                    net.join = Some(horizon_os::play::request("POST", "/join", ""));
                }
                if let Some(id) = net.join
                    && let Some((status, text)) = horizon_os::play::take(id)
                {
                    net.join = None;
                    match (status, parse::<Join>(&text)) {
                        (200, Some(join)) => {
                            log::info!(
                                "Horizon: play: joined world {} at version {} ({} chunks, {} changes)",
                                join.world.id,
                                join.world.version,
                                join.chunks.len(),
                                join.changes.len()
                            );
                            s.world_id = join.world.id;
                            s.version = join.world.version;
                            s.chunk_side = join.world.chunk_side;
                            s.prices = Arc::new(join.prices.clone());
                            s.coins = join.coins;
                            s.join = Some(Arc::new(join));
                            s.stage = Stage::Joined;
                        }
                        _ => {
                            s.stage = Stage::Failed;
                            s.failure = format!("{status} {}", text.chars().take(200).collect::<String>());
                        }
                    }
                }
                return;
            }
            Stage::Joined | Stage::Built => {}
        }
        if s.rejoin {
            return;
        }
        let now = Instant::now();
        // The answer to the batch on its way.
        if let Some(batch) = s.inflight.as_mut()
            && let Some(id) = batch.request
            && let Some((status, text)) = horizon_os::play::take(id)
        {
            batch.request = None;
            match (status, parse::<Applied>(&text)) {
                (200, Some(applied)) => {
                    let batch = s.inflight.take().expect("the batch on its way");
                    answered(&mut s, batch, applied);
                }
                // The network failed, or the API: the same batch again, a little later.
                (0 | 429 | 500..=599, _) => {
                    batch.retry_at = Some(now + Duration::from_secs(2));
                }
                // The game knows a version the world hasn't reached: join anew.
                (409, _) => {
                    log::warn!("Horizon: play: the world is behind the game ({text}): joining again");
                    s.inflight = None;
                    s.rejoin = true;
                }
                // Refused whole (signed out, banned, a malformed batch): its actions go back.
                _ => {
                    log::warn!("Horizon: play: a batch was refused ({status}): {text}");
                    let batch = s.inflight.take().expect("the batch on its way");
                    for pending in batch.actions {
                        for (pos, expected) in pending.action.cells() {
                            s.to_server.push(ToServer::Revert {
                                pos,
                                state: expected.to_owned(),
                                entity: (pos == [pending.action.x, pending.action.y, pending.action.z])
                                    .then(|| pending.old_entity.clone())
                                    .flatten(),
                                refusal: None,
                            });
                        }
                    }
                }
            }
        }
        if let Some(batch) = s.inflight.as_mut()
            && batch.request.is_none()
            && batch.retry_at.is_some_and(|t| now >= t)
        {
            batch.retry_at = None;
            batch.request = Some(horizon_os::play::request("POST", "/actions", &batch.body));
        }
        // The next batch: at once when none is on its way, at most one a second.
        let ready = net
            .last_actions
            .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1));
        // What grew goes with the player's actions, or on its own every 10 s: it is never urgent.
        let growth_due = !s.grow.is_empty()
            && net
                .last_growth
                .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(10));
        if s.inflight.is_none() && ready && (!s.outbox.is_empty() || growth_due) {
            if !s.grow.is_empty() {
                net.last_growth = Some(now);
            }
            s.grow.retain(|g| g.expected != g.state);
            let (a, g) = (s.outbox.len().min(64), s.grow.len().min(512));
            let actions: Vec<Pending> = s.outbox.drain(..a).collect();
            let grow: Vec<Growth> = s.grow.drain(..g).collect();
            let id = new_uuid();
            let body = batch_body(&s, &id, &actions, &grow);
            let request = Some(horizon_os::play::request("POST", "/actions", &body));
            s.inflight = Some(Batch {
                id,
                body,
                actions,
                grow,
                request,
                retry_at: None,
            });
            net.last_actions = Some(now);
        }
        // Back in view: what the others changed meanwhile.
        if horizon_os::play::regained() && net.changes.is_none() {
            net.changes = Some(horizon_os::play::request(
                "GET",
                &format!("/changes?since={}", s.version),
                "",
            ));
        }
        if let Some(id) = net.changes
            && let Some((status, text)) = horizon_os::play::take(id)
        {
            net.changes = None;
            if let (200, Some(changes)) = (status, parse::<Changes>(&text)) {
                if changes.resync {
                    s.rejoin = true;
                } else if changes.version >= s.version {
                    s.version = changes.version;
                    s.to_server.push(ToServer::Changes(changes.changes));
                }
            }
        }
        // The save.
        if net.save.is_none()
            && s.save_due
            && let Some(body) = s.save_body.clone()
        {
            s.save_due = false;
            net.save = Some(horizon_os::play::request("POST", "/save", &body));
            net.last_save = Some(now);
        }
        if let Some(id) = net.save
            && let Some((status, text)) = horizon_os::play::take(id)
        {
            net.save = None;
            if status != 200 {
                log::warn!("Horizon: play: the save was refused ({status}): {text}");
            }
        }
        // What goes if the page does: the next batch, and the last save.
        let key = (
            s.outbox.len() + s.grow.len(),
            s.inflight.as_ref().map_or(0, |b| b.actions.len()),
            s.save_body.as_ref().map(String::len),
        );
        if key != net.farewell {
            net.farewell = key;
            let mut list: Vec<(String, String, String)> = Vec::new();
            if let Some(batch) = &s.inflight {
                list.push(("POST".into(), "/actions".into(), batch.body.clone()));
            }
            if !s.outbox.is_empty() || !s.grow.is_empty() {
                let id = new_uuid();
                let body = batch_body(&s, &id, &s.outbox, &s.grow);
                list.push(("POST".into(), "/actions".into(), body));
            }
            if let Some(body) = &s.save_body {
                list.push(("POST".into(), "/save".into(), body.clone()));
            }
            let refs: Vec<(&str, &str, &str)> = list
                .iter()
                .map(|(m, p, b)| (m.as_str(), p.as_str(), b.as_str()))
                .collect();
            horizon_os::play::farewell(&refs);
        }
    }

    /// A batch's answer: each action ok, put right or put back; the others' changes in.
    fn answered(s: &mut Shared, batch: Batch, applied: Applied) {
        if applied.resync {
            s.rejoin = true;
            return;
        }
        s.coins = applied.coins;
        for (pending, result) in batch.actions.iter().zip(&applied.results) {
            let ActionResult {
                status, current, ..
            } = result;
            let pos = [pending.action.x, pending.action.y, pending.action.z];
            if status == "ok" {
                continue;
            }
            // The whole block back as the world has it: the clicked cell...
            if status == "CONFLICT" {
                let entity = applied
                    .changes
                    .iter()
                    .rev()
                    .find(|c| [c.x, c.y, c.z] == pos)
                    .and_then(|c| c.block_entity.clone());
                s.to_server.push(ToServer::Conflict {
                    pos,
                    state: current
                        .clone()
                        .unwrap_or_else(|| pending.action.expected.clone()),
                    entity,
                });
            } else {
                // Refused (for want of coins, or outside the world): back as it was.
                s.to_server.push(ToServer::Revert {
                    pos,
                    state: pending.action.expected.clone(),
                    entity: pending.old_entity.clone(),
                    refusal: (status == "INSUFFICIENT_COINS").then_some(applied.coins.gold > 0),
                });
            }
            // ...and its other cells, which the world never took.
            for part in &pending.action.parts {
                s.to_server.push(ToServer::Revert {
                    pos: [part.x, part.y, part.z],
                    state: part.expected.clone(),
                    entity: None,
                    refusal: None,
                });
            }
        }
        // Our own changes are among them: those the game already has are skipped there.
        let mine: HashSet<[i32; 3]> = batch
            .actions
            .iter()
            .flat_map(|p| p.action.cells().map(|(cell, _)| cell))
            .collect();
        let others: Vec<Change> = applied
            .changes
            .into_iter()
            .filter(|c| !mine.contains(&[c.x, c.y, c.z]))
            .collect();
        s.version = s.version.max(applied.version);
        if !others.is_empty() {
            s.to_server.push(ToServer::Changes(others));
        }
    }
}

impl Game<'_> {
    /// Between frames, on the window: the shared world's network.
    pub(super) fn play_client_idle(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(mut s) = shared_now()
                && s.stage == Stage::Off
                && horizon_os::play::available()
            {
                {
                    s.stage = Stage::Joining;
                    ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                    // The world comes from the API each time: none of it is kept.
                    horizon_os::fs::unkeep(horizon_os::env::current_dir().unwrap_or_default().join("saves"));
                }
            }
            net::pump();
        }
        self.play_rejoin();
        // Once in the world: what blocks cost, unless the player turned it off.
        if active()
            && self.screen == Screen::Playing
            && self.player.is_some()
            && self.command_prompt.is_none()
            && !self.settings.extras.play_notice_off
            && take_notice()
        {
            self.open_play_notice();
        }
    }

    /// The world changed past what the game can follow: out of it (the server stops and the
    /// window goes back to the title), then in again from a fresh join, which builds the world
    /// anew. What was queued is dropped: the world that answers it is no longer this one.
    fn play_rejoin(&mut self) {
        static LEAVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        use std::sync::atomic::Ordering::Relaxed;
        if !shared_now().is_some_and(|s| s.rejoin) || self.pending_save.is_some() {
            return;
        }
        if self.integrated.is_some() {
            if !LEAVING.swap(true, Relaxed) {
                log::info!("Horizon: play: the world changed under the game: joining it again");
                if let Err(e) = self.begin_closing_save(false) {
                    log::warn!("Horizon: play: leaving the world: {e}");
                }
            }
            return;
        }
        if self.integrated_starting.is_some() || self.pending_entry.is_some() {
            return;
        }
        LEAVING.store(false, Relaxed);
        *server() = ServerPlay::default();
        {
            let mut s = shared();
            let notice_shown = s.notice_shown;
            *s = Shared {
                stage: Stage::Joining,
                notice_shown,
                ..Shared::default()
            };
        }
        self.pending_entry = Some(PendingWorldEntry::Local {
            folder: self.options.world.clone().unwrap_or_else(|| "world".into()),
            name: None,
            seed: Some(self.options.seed),
            creative: true,
            create: None,
        });
    }

    /// Whether the world entry waits for the shared world: until the join came, and the
    /// world's folder is built from it. An error when the join failed.
    pub(super) fn play_world_waits(&mut self, folder: &str) -> Result<bool> {
        let (stage, join, failure) = {
            let Some(s) = shared_now() else {
                return Ok(true);
            };
            (s.stage, s.join.clone(), s.failure.clone())
        };
        match stage {
            Stage::Off | Stage::Built => Ok(false),
            Stage::Joining => Ok(true),
            Stage::Failed => Err(format!("could not join the world: {failure}").into()),
            Stage::Joined => {
                let join = join.ok_or("joined without the world")?;
                // The page's thread can't wait for a table another thread is filling (the
                // browser forbids it to block): filled here, before the server starts.
                let _ = horizon_core::geometry::trig::sin(0.0);
                let dir = horizon_os::env::current_dir()?.join("saves").join(folder);
                let started = Instant::now();
                let built = horizon_client::play_data::write_world(
                    &dir,
                    &horizon_client::play_data::WorldSetup {
                        join: &join,
                        player_uuid: PLAYER_UUID,
                    },
                )?;
                log::info!(
                    "Horizon: play: the world's {built} chunks built in {} ms",
                    started.elapsed().as_millis()
                );
                restore_inventory(&dir, join.world.id, PLAYER_UUID);
                shared().stage = Stage::Built;
                Ok(false)
            }
        }
    }
}

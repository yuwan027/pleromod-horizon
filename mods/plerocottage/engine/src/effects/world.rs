//! The mod's effects in the game. A hit reaches a mod with no game to ask (`living_incoming_damage_v3`),
//! so who has which effect is noted as each effect ticks (`apply_effect_tick`, every tick for the
//! mod's: `isDurationEffectTick`), and what a handler does beside changing the amount waits for
//! the end of the level's tick (`level_tick_post`), where the game can be told.
use super::*;
use horizon_client::brewing;
use horizon_client::mod_api::*;
use std::sync::{Mutex, MutexGuard};

/// `SoundSource.PLAYERS`.
const PLAYERS: usize = 7;
/// Ticks a note of an effect stays good without its tick being heard again.
const FRESH: i64 = 2;

type JavaString = crate::JavaString;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Mob(u32),
    Player(u128),
}

/// An entity by the id events carry, and whose effects those are.
#[derive(Clone, Copy, Debug)]
struct Target {
    id: u32,
    key: Key,
}

/// How a hit the mod itself deals is to be heard when it comes back as a hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Mode {
    #[default]
    Plain,
    /// `PleroCottageHappySweep`.
    Sweep,
    /// `PleroCottageTransmutationReentry`.
    Magic,
}

/// A player's attack in progress (`AttackCharge`, and what `isCritical` asks).
#[derive(Clone, Debug)]
struct Swing {
    player: u128,
    victim: u32,
    critical: bool,
    charge: f32,
    health: f32,
    adult_animal: bool,
    weapon: Option<String>,
    tick: i64,
}

#[derive(Default)]
struct Lurk {
    ticks: i32,
    fast: bool,
}

enum Queued {
    Add { to: Target, effect: String, ticks: i32, amplifier: i32, ambient: bool, visible: bool },
    Remove { from: Target, effect: String },
    Hurt { target: Target, damage_type: String, amount: f32, by: Option<u32>, mode: Mode },
    Sweep { around: Target, amount: f32, by: Target },
    Heal { target: Target, amount: f32 },
    Push { target: Target, from: Target, speed: f64 },
    Frozen { target: Target, ticks: i32 },
    Sound { at: Target, sound: Sound },
    Shatter { at: Target, block: String },
    Particles { at: Target, particle: Particle, eyes: bool },
    Break { target: Target },
    Instant { target: Target, purity: bool, amplifier: i32 },
    Lurk { player: u128, amplifier: i32, crouching: bool },
    Merge { target: Target },
    Menthol { player: u128 },
    Wallnut { player: u128 },
    WallnutGiven { player: u128, amplifier: i32 },
    Evolve { player: u128, amplifier: i32 },
    EndMeteor { player: u128, victim: u32 },
    Armor { target: Target },
}

#[derive(Default)]
struct State {
    /// `Level.getGameTime`, as last heard.
    now: i64,
    /// The tick `timers` last ran in.
    ticked: i64,
    /// Who has which of the mod's effects: the amplifier, and the tick it was last heard in.
    held: HashMap<Key, HashMap<String, (i32, i64)>>,
    /// The players by entity id.
    players: HashMap<u32, u128>,
    /// A player's health as their last effect tick saw it, for where the players are out of reach.
    health: HashMap<u128, f32>,
    /// The base temperature of the biome an entity with a veil stood in.
    temperature: HashMap<Key, f32>,
    /// `getArmorValue` of those with `hacked_thorns`.
    armor: HashMap<Key, i32>,
    swing: Option<Swing>,
    /// The tick of each player's last attack.
    swung: HashMap<u128, i64>,
    /// `TransmutationTarget` and `TransmutationCount`.
    counts: HashMap<u128, (u32, i32)>,
    /// `MomentaryMeteorPendingTarget`.
    pending: HashMap<u128, u32>,
    lurking: HashMap<u128, Lurk>,
    /// `BerryArmorTicks` by victim.
    broken: HashMap<u32, (Key, i32)>,
    /// The effects `menthol_awakening` raised, with the level it left them at.
    menthol: HashMap<u128, HashMap<String, i32>>,
    /// The absorption `wallnut_first_aid` gave.
    wallnut: HashMap<u128, f32>,
    queue: Vec<Queued>,
    mode: Mode,
    seed: u64,
}

impl State {
    fn effects_of(&self, key: Key) -> Held {
        self.held
            .get(&key)
            .map(|held| {
                held.iter()
                    .filter(|(_, (_, heard))| self.now - *heard <= FRESH)
                    .map(|(effect, (amplifier, _))| (effect.clone(), *amplifier))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn has(&self, key: Key, name: &str) -> Option<i32> {
        let (amplifier, heard) = *self.held.get(&key)?.get(&format!("{NS}{name}"))?;
        (self.now - heard <= FRESH).then_some(amplifier)
    }

    fn note(&mut self, key: Key, effect: &str, amplifier: i32) {
        let now = self.now;
        self.held.entry(key).or_default().insert(effect.to_owned(), (amplifier, now));
    }

    fn key(&self, id: u32, uuid: Option<u128>) -> Key {
        uuid.or_else(|| self.players.get(&id).copied()).map_or(Key::Mob(id), Key::Player)
    }

    /// A draw from 0 to 1, as the mod draws from its entity's random.
    fn roll(&mut self) -> f32 {
        if self.seed == 0 {
            self.seed = 0x9E37_79B9_7F4A_7C15 ^ self.now as u64;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    horizon_os::sync::lock(STATE.get_or_init(Mutex::default)).unwrap_or_else(|e| e.into_inner())
}

fn name(text: &str) -> JavaString {
    JavaString::from(text)
}

fn float(nbt: &Compound, key: &str) -> Option<f32> {
    match nbt.get(&name(key)) {
        Some(Tag::Float(bits)) => Some(f32::from_bits(*bits)),
        Some(Tag::Double(bits)) => Some(f64::from_bits(*bits) as f32),
        _ => None,
    }
}

fn text(nbt: &Compound, key: &str) -> Option<String> {
    match nbt.get(&name(key)) {
        Some(Tag::String(value)) => Some(value.to_string_lossy()),
        _ => None,
    }
}

fn list(nbt: &Compound, key: &str) -> Vec<Tag> {
    match nbt.get(&name(key)) {
        Some(Tag::List(list)) => list.clone(),
        _ => Vec::new(),
    }
}

/// `MobEffect.getCategory` of any effect: the mod's by its data, Minecraft's by the game's.
fn category(effect: &str) -> &'static str {
    if let Some(about) = data().effects.get(effect) {
        return match about.category.as_str() {
            "beneficial" => "beneficial",
            "harmful" => "harmful",
            _ => "neutral",
        };
    }
    match brewing::effect(effect).map(|e| e.category) {
        Some(brewing::Category::Beneficial) => "beneficial",
        Some(brewing::Category::Harmful) => "harmful",
        _ => "neutral",
    }
}

// ---- What the hooks hear ------------------------------------------------------------------------

/// `isDurationEffectTick`: every tick of the mod's effects.
pub fn ticks_now(event: &mut ShouldApplyEffectTickThisTickEvent<'_>) {
    if event.effect.starts_with(NS) {
        event.apply = true;
    }
}

/// `applyEffectTick`: the note that the entity has the effect, and what the effect's tick does.
pub fn applied(host: &dyn Host, event: &mut ApplyEffectTickEvent<'_>) {
    let Some(effect) = event.effect.strip_prefix(NS) else {
        return;
    };
    let (target, position, player) = match event.entity {
        Some(id) => {
            let id = id as u32;
            let position = host.entity(id).and_then(|e| host.entity_position(e));
            (Target { id, key: Key::Mob(id) }, position, None)
        }
        None => {
            let Some(player) = host.acting_player() else {
                return;
            };
            let id = player.entity_id as u32;
            state().health.insert(player.uuid, player.health);
            (Target { id, key: Key::Player(player.uuid) }, Some(player.position), Some(player.uuid))
        }
    };
    let now = host.level(event.dimension).and_then(|level| host.level_game_time(level));
    let d = data();
    let mut s = state();
    if let Some(now) = now {
        s.now = now;
    }
    if let Some(uuid) = player {
        s.players.insert(target.id, uuid);
    }
    s.note(target.key, event.effect, event.amplifier);
    let amplifier = event.amplifier;
    match effect {
        "purity" | "fragrant" => s.queue.push(Queued::Instant { target, purity: effect == "purity", amplifier }),
        "cherry_blossoms_weeping" => {
            let leaves = &d.ticks.cherry_blossoms_weeping;
            if s.roll() < leaves.chance {
                let [low, high] = leaves.count;
                let count = (low + (s.roll() * (high - low + 1) as f32) as i32).min(high);
                let particle = Particle {
                    particle: leaves.particle.clone(),
                    count,
                    spread: leaves.spread,
                    speed: leaves.speed,
                };
                s.queue.push(Queued::Particles { at: target, particle, eyes: true });
            }
        }
        "lurking_danger" => {
            if let Some(player) = player {
                let crouching = host
                    .entity(target.id)
                    .and_then(|e| host.entity_is_crouching(e))
                    .unwrap_or(false);
                s.queue.push(Queued::Lurk { player, amplifier, crouching });
            }
        }
        "ice_barrier" | "snow_veil" => {
            let temperature = position
                .and_then(|p| host.biome(p.map(|v| v.floor() as i32)))
                .and_then(|biome| d.biomes.get(&biome).copied());
            if let Some(temperature) = temperature {
                s.temperature.insert(target.key, temperature);
            }
        }
        "hacked_thorns" => s.queue.push(Queued::Armor { target }),
        "menthol_awakening" => {
            if let Some(player) = player {
                s.queue.push(Queued::Menthol { player });
            }
        }
        "wallnut_first_aid" => {
            if let Some(player) = player {
                s.queue.push(Queued::Wallnut { player });
            }
        }
        _ => {}
    }
}

/// `MobEffectEvent.Added`, every time the effect is given: the two veils that become one, what
/// the one keeps out, and the absorption of `wallnut_first_aid`.
pub fn added(host: &mut dyn Host, effect: &str, entity: Option<i32>, amplifier: i32) {
    if !effect.starts_with(NS) {
        return;
    }
    let (target, player) = match entity {
        Some(id) => (Target { id: id as u32, key: Key::Mob(id as u32) }, None),
        None => {
            let Some(player) = host.acting_player() else {
                return;
            };
            (Target { id: player.entity_id as u32, key: Key::Player(player.uuid) }, Some(player.uuid))
        }
    };
    let d = data();
    let mut s = state();
    let held = s.effects_of(target.key);
    s.note(target.key, effect, amplifier);
    if d.added.refuses.iter().any(|(holder, kept_out)| held.contains_key(holder) && kept_out.iter().any(|e| e == effect)) {
        // `MobEffectEvent.Applicable` denied: it is taken off again.
        s.held.get_mut(&target.key).map(|held| held.remove(effect));
        s.queue.push(Queued::Remove { from: target, effect: effect.to_owned() });
        return;
    }
    let merge = &d.added.merge;
    if let Some(other) = merge.of.iter().find(|e| *e != effect)
        && merge.of.iter().any(|e| e == effect)
        && held.contains_key(other)
    {
        s.queue.push(Queued::Merge { target });
    }
    if effect == format!("{NS}wallnut_first_aid")
        && let Some(player) = player
    {
        s.queue.push(Queued::WallnutGiven { player, amplifier });
    }
}

/// `AttackEntityEvent`: the charge of the attack that starts (`AttackCharge`), and what the
/// handlers will ask of its target.
pub fn swung(host: &mut dyn Host, event: &AttackEntityEvent<'_>) {
    let Some(player) = host.acting_player() else {
        return;
    };
    let speed = host.attribute(player.uuid, "minecraft:attack_speed").map_or(4.0, |value| value.1);
    let victim = event.entity as u32;
    let nbt = host.entity(victim).and_then(|e| host.entity_nbt(e));
    let health = nbt.as_ref().and_then(|nbt| float(nbt, "Health")).unwrap_or(0.0);
    let adult_animal = nbt
        .as_ref()
        .is_some_and(|nbt| nbt.contains_key(&name("InLove")) && crate::int(nbt, "Age") >= 0);
    let mut s = state();
    let now = s.now;
    // `getAttackStrengthScale(0.5F)`: the ticks since the last attack, by the attack speed.
    let charge = match s.swung.insert(player.uuid, now) {
        Some(last) => (((now - last) as f64 + 0.5) * speed / 20.0).clamp(0.0, 1.0) as f32,
        None => 1.0,
    };
    s.players.insert(player.entity_id as u32, player.uuid);
    s.swing = Some(Swing {
        player: player.uuid,
        victim,
        critical: false,
        charge,
        health,
        adult_animal,
        weapon: None,
        tick: now,
    });
}

/// `CriticalHitEvent`: whether the attack in progress is critical.
pub fn critical(event: &CriticalHitEvent<'_>) {
    if let Some(swing) = state().swing.as_mut() {
        swing.critical = event.critical;
    }
}

/// The weapon of the attack in progress.
pub fn weapon(event: &LivingIncomingDamageV2Event<'_>) {
    if let Some(swing) = state().swing.as_mut()
        && swing.victim == event.entity as u32
    {
        swing.weapon = event.weapon.map(|stack| stack.item.to_string());
    }
}

/// `LivingAttackEvent` and `LivingHurtEvent`: whether the hit is refused, with its amount changed.
pub fn incoming(event: &mut LivingIncomingDamageV3Event<'_>) -> bool {
    let d = data();
    let mut s = state();
    let victim = Target { id: event.victim, key: s.key(event.victim, event.victim_uuid) };
    let attacker = event.attacker.map(|id| Target { id, key: s.key(id, event.attacker_uuid) });
    let swing = s.swing.clone().filter(|swing| {
        attacker.is_some_and(|a| a.key == Key::Player(swing.player))
            && swing.victim == event.victim
            && s.now - swing.tick <= 1
    });
    let held = s.effects_of(victim.key);
    let theirs = attacker.map(|a| s.effects_of(a.key)).unwrap_or_default();
    if held.is_empty() && theirs.is_empty() && swing.as_ref().is_none_or(|swing| swing.weapon.is_none()) {
        return false;
    }
    let damage_type = event.damage_type;
    let direct = if among(&d.tags["is_projectile"], damage_type) {
        if matches!(bare(damage_type), "arrow" | "trident") { Direct::Arrow } else { Direct::Projectile }
    } else if attacker.is_some()
        && (event.direct == event.attacker
            || matches!(bare(damage_type), "player_attack" | "mob_attack" | "mob_attack_no_aggro"))
    {
        Direct::Attacker
    } else {
        Direct::Nothing
    };
    let by = match attacker {
        None => By::Nobody,
        Some(Target { key: Key::Player(_), .. }) => By::Player,
        Some(_) => By::Mob,
    };
    let player = attacker.and_then(|a| match a.key {
        Key::Player(uuid) => Some(uuid),
        Key::Mob(_) => None,
    });
    let count = player
        .and_then(|uuid| s.counts.get(&uuid))
        .filter(|(target, _)| *target == event.victim)
        .map_or(0, |(_, count)| *count);
    let roll = s.roll();
    let out = hit(
        d,
        &Hit {
            victim: &held,
            attacker: &theirs,
            by,
            direct,
            damage_type,
            amount: event.amount,
            critical: swing.as_ref().is_some_and(|swing| swing.critical),
            charge: swing.as_ref().map_or(1.0, |swing| swing.charge),
            temperature: s.temperature.get(&victim.key).copied().unwrap_or(0.8),
            roll,
            armor: s.armor.get(&victim.key).copied().unwrap_or(0),
            health: swing.as_ref().map_or(0.0, |swing| swing.health),
            adult_animal: swing.as_ref().is_some_and(|swing| swing.adult_animal),
            weapon: swing.as_ref().and_then(|swing| swing.weapon.as_deref()),
            // Who stands around is found when the sweep is dealt.
            nearby: 1,
            count,
            sweeping: s.mode == Mode::Sweep,
            reentry: s.mode == Mode::Magic,
            invulnerable: 0,
        },
    );
    event.amount = out.amount;
    let on = |who: Who| match who {
        Who::Attacker => attacker,
        _ => Some(victim),
    };
    for did in out.did {
        match did {
            Did::Add { who, effect, ticks, amplifier, ambient, visible } => {
                if let Some(to) = on(who) {
                    s.queue.push(Queued::Add { to, effect, ticks, amplifier, ambient, visible });
                }
            }
            Did::Remove { who, effect } => {
                if let Some(from) = on(who) {
                    s.held.get_mut(&from.key).map(|held| held.remove(&effect));
                    s.queue.push(Queued::Remove { from, effect });
                }
            }
            Did::Hurt { who: Who::Near(_), amount, .. } => {
                if let Some(by) = attacker {
                    s.queue.push(Queued::Sweep { around: victim, amount, by });
                }
            }
            Did::Hurt { who, damage_type, amount, magic } => {
                if let Some(target) = on(who).filter(|_| amount > 0.0) {
                    let (by, mode) = if magic { (None, Mode::Magic) } else { (Some(victim.id), Mode::Plain) };
                    s.queue.push(Queued::Hurt { target, damage_type, amount, by, mode });
                }
            }
            Did::Heal { who, amount } => {
                if let Some(target) = on(who) {
                    s.queue.push(Queued::Heal { target, amount });
                }
            }
            Did::Push { who, speed } => {
                if let Some(target) = on(who) {
                    s.queue.push(Queued::Push { target, from: victim, speed });
                }
            }
            Did::Frozen { who, ticks } => {
                if let Some(target) = on(who) {
                    s.queue.push(Queued::Frozen { target, ticks });
                }
            }
            Did::Sound(sound) => s.queue.push(Queued::Sound { at: victim, sound }),
            Did::LevelEvent(_, block) => s.queue.push(Queued::Shatter { at: victim, block }),
            Did::Particles(particle) => s.queue.push(Queued::Particles { at: victim, particle, eyes: false }),
            Did::Break { .. } => {
                s.broken.insert(victim.id, (victim.key, d.attack.bursting_berry.ticks));
                s.queue.push(Queued::Break { target: victim });
            }
            Did::Waits(ticks) => {
                if let Some(uuid) = player {
                    s.lurking.entry(uuid).or_default().ticks = ticks;
                }
            }
            Did::Count(count) => {
                if let Some(uuid) = player {
                    s.counts.insert(uuid, (victim.id, count));
                }
            }
            Did::Pending => {
                if let Some(uuid) = player {
                    s.pending.insert(uuid, victim.id);
                }
            }
        }
    }
    out.canceled
}

/// `LivingDamageEvent`: the stars of a `momentary_meteor` hit that landed, and the effect's end.
pub fn landed(event: &LivingDamagePostEvent<'_>) {
    let d = data();
    let mut s = state();
    let victim = event.entity as u32;
    let Some(player) = s.pending.iter().find(|(_, waiting)| **waiting == victim).map(|(uuid, _)| *uuid) else {
        return;
    };
    s.pending.remove(&player);
    let at = Target { id: victim, key: s.key(victim, None) };
    let stars = &d.damage.momentary_meteor;
    if event.amount > 0.0 {
        s.queue.push(Queued::Sound { at, sound: stars.sound.clone() });
        s.queue.push(Queued::Particles { at, particle: stars.particle.clone(), eyes: false });
    }
    if stars.ends_when_survived {
        s.queue.push(Queued::EndMeteor { player, victim });
    }
}

/// `LivingExperienceDropEvent`: the extra orb of `rutin_limonene`, here more of the one drop.
pub fn experience(host: &mut dyn Host, event: &mut LivingExperienceDropEvent<'_>) {
    let Some(player) = host.acting_player() else {
        return;
    };
    let amplifier = state().has(Key::Player(player.uuid), "rutin_limonene");
    if let Some(amplifier) = amplifier {
        event.xp += extra_experience(data(), amplifier, event.xp).max(0);
    }
}

/// `LivingDeathEvent`: a kill ends the killer's `dark_wrath` and lengthens, with
/// `original_evolution`, their other good effects.
pub fn died(host: &mut dyn Host, event: &LivingDeathEvent<'_>) {
    let d = data();
    let killer = if event.attacker != 0 {
        Some(Target { id: event.attacker, key: Key::Mob(event.attacker) })
    } else if bare(event.damage_type) == "player_attack" || among(&d.tags["is_projectile"], event.damage_type) {
        host.acting_player()
            .map(|player| Target { id: player.entity_id as u32, key: Key::Player(player.uuid) })
    } else {
        None
    };
    let Some(killer) = killer else {
        return;
    };
    let mut s = state();
    if d.death.dark_wrath.ends && s.has(killer.key, "dark_wrath").is_some() {
        let effect = format!("{NS}dark_wrath");
        s.held.get_mut(&killer.key).map(|held| held.remove(&effect));
        s.queue.push(Queued::Remove { from: killer, effect });
    }
    if let (Key::Player(player), Some(amplifier)) = (killer.key, s.has(killer.key, "original_evolution")) {
        s.queue.push(Queued::Evolve { player, amplifier });
    }
}

// ---- Telling the game, at the end of a level's tick -----------------------------------------------

/// What the handlers left to do, and the timers they started.
pub fn tick(host: &mut dyn Host, dimension: &str) {
    let d = data();
    let now = host.level(dimension).and_then(|level| host.level_game_time(level));
    let (queue, first) = {
        let mut s = state();
        if let Some(now) = now {
            s.now = now;
        }
        let first = s.ticked != s.now;
        s.ticked = s.now;
        (std::mem::take(&mut s.queue), first)
    };
    for queued in queue {
        run(host, dimension, d, queued);
    }
    if first {
        timers(host, d);
    }
}

fn effect_of(effect: &str, ticks: i32, amplifier: i32, ambient: bool, visible: bool) -> NativeEffect {
    NativeEffect {
        id: effect.to_owned(),
        amplifier: amplifier.clamp(0, 255) as u8,
        duration: ticks,
        ambient,
        particles: visible,
    }
}

/// Changes a mob's saved effects (`active_effects`), as `/data merge entity` does.
fn mob_effects(host: &mut dyn Host, id: u32, change: impl FnOnce(&mut Vec<Tag>)) {
    let Some(entity) = host.entity(id) else {
        return;
    };
    let Some(nbt) = host.entity_nbt(entity) else {
        return;
    };
    let mut effects = list(&nbt, "active_effects");
    change(&mut effects);
    let mut merge = Compound::new();
    merge.insert(name("active_effects"), Tag::List(effects));
    host.entity_merge_nbt(entity, merge);
}

fn is_effect(tag: &Tag, effect: &str) -> bool {
    matches!(tag, Tag::Compound(saved) if text(saved, "id").is_some_and(|id| id == effect))
}

fn add(host: &mut dyn Host, to: Target, effect: &str, ticks: i32, amplifier: i32, ambient: bool, visible: bool) {
    match to.key {
        Key::Player(uuid) => {
            host.add_effect(uuid, &effect_of(effect, ticks, amplifier, ambient, visible));
        }
        Key::Mob(id) => mob_effects(host, id, |effects| {
            effects.retain(|tag| !is_effect(tag, effect));
            let mut saved = Compound::new();
            saved.insert(name("id"), Tag::String(name(effect)));
            saved.insert(name("duration"), Tag::Int(ticks));
            saved.insert(name("amplifier"), Tag::Byte(amplifier.clamp(0, 255) as u8 as i8));
            saved.insert(name("ambient"), Tag::Byte(ambient.into()));
            saved.insert(name("show_particles"), Tag::Byte(visible.into()));
            saved.insert(name("show_icon"), Tag::Byte(1));
            effects.push(Tag::Compound(saved));
        }),
    }
}

fn remove(host: &mut dyn Host, from: Target, effect: &str) {
    match from.key {
        Key::Player(uuid) => {
            host.remove_effect(uuid, effect);
        }
        Key::Mob(id) => mob_effects(host, id, |effects| effects.retain(|tag| !is_effect(tag, effect))),
    }
}

/// The effects an entity really has: (id, ticks left, amplifier).
fn effects(host: &mut dyn Host, target: Target) -> Vec<(String, i32, i32)> {
    match target.key {
        Key::Player(uuid) => host
            .effects_v2(uuid)
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.effect.id, e.effect.duration, i32::from(e.effect.amplifier)))
            .collect(),
        Key::Mob(id) => host.entity(id).and_then(|e| host.entity_effects(e)).unwrap_or_default(),
    }
}

/// An entity's health and the most it can have.
fn health(host: &mut dyn Host, target: Target) -> Option<(f32, f32)> {
    match target.key {
        Key::Player(uuid) => {
            let health = host
                .server_players()
                .into_iter()
                .find(|p| p.uuid == uuid)
                .map(|p| p.health)
                .or_else(|| state().health.get(&uuid).copied())?;
            let most = host.attribute(uuid, "minecraft:max_health").map_or(health, |value| value.1 as f32);
            Some((health, most))
        }
        Key::Mob(id) => {
            let nbt = host.entity(id).and_then(|e| host.entity_nbt(e))?;
            let health = float(&nbt, "Health")?;
            let most = list(&nbt, "attributes")
                .iter()
                .find_map(|tag| match tag {
                    Tag::Compound(saved) if text(saved, "id").is_some_and(|id| bare(&id) == "max_health") => float(saved, "base"),
                    _ => None,
                })
                .unwrap_or(health);
            Some((health, most))
        }
    }
}

fn heal(host: &mut dyn Host, dimension: &str, target: Target, amount: f32) {
    let Some((health, _)) = health(host, target).filter(|(health, _)| *health > 0.0 && amount > 0.0) else {
        return;
    };
    match target.key {
        Key::Player(uuid) => {
            host.set_player_status(uuid, Some(health + amount), None, None);
        }
        Key::Mob(id) => {
            host.set_mob_health(dimension, id, health + amount);
        }
    }
}

/// Adds, or with no amount takes off, a modifier the mod's code puts on an attribute itself.
fn modifier(host: &mut dyn Host, target: Target, attribute: &str, id: &str, amount: Option<(f64, &str)>) {
    let id = if id.contains(':') { id.to_owned() } else { format!("{NS}{id}") };
    let entry = |amount: f64, operation: &str| {
        let mut saved = Compound::new();
        saved.insert(name("id"), Tag::String(name(&id)));
        saved.insert(name("amount"), Tag::Double(amount.to_bits()));
        saved.insert(name("operation"), Tag::String(name(operation)));
        Tag::Compound(saved)
    };
    let same = |tag: &Tag| matches!(tag, Tag::Compound(saved) if text(saved, "id").is_some_and(|held| held == id));
    match target.key {
        Key::Player(uuid) => {
            let Some((base, _)) = host.attribute(uuid, attribute) else {
                return;
            };
            let mut saved = Compound::new();
            saved.insert(name("id"), Tag::String(name(attribute)));
            saved.insert(name("base"), Tag::Double(base.to_bits()));
            saved.insert(name("modifiers"), Tag::List(amount.map(|(amount, operation)| entry(amount, operation)).into_iter().collect()));
            host.set_attributes(uuid, &Tag::List(vec![Tag::Compound(saved)]));
        }
        Key::Mob(mob) => {
            let Some(entity) = host.entity(mob) else {
                return;
            };
            let Some(nbt) = host.entity_nbt(entity) else {
                return;
            };
            let mut attributes = list(&nbt, "attributes");
            let Some(Tag::Compound(saved)) = attributes
                .iter_mut()
                .find(|tag| matches!(tag, Tag::Compound(saved) if text(saved, "id").is_some_and(|held| held == attribute)))
            else {
                return;
            };
            let mut modifiers = list(saved, "modifiers");
            modifiers.retain(|tag| !same(tag));
            modifiers.extend(amount.map(|(amount, operation)| entry(amount, operation)));
            saved.insert(name("modifiers"), Tag::List(modifiers));
            let mut merge = Compound::new();
            merge.insert(name("attributes"), Tag::List(attributes));
            host.entity_merge_nbt(entity, merge);
        }
    }
}

fn center(host: &dyn Host, target: Target, eyes: bool) -> Option<[f64; 3]> {
    let entity = host.entity(target.id)?;
    if eyes && let Some(at) = host.entity_eye_position(entity) {
        return Some(at);
    }
    match host.entity_bounding_box(entity) {
        Some([low, high]) => Some([(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, (low[2] + high[2]) / 2.0]),
        None => host.entity_position(entity),
    }
}

fn with_mode(mode: Mode, deal: impl FnOnce()) {
    state().mode = mode;
    deal();
    state().mode = Mode::Plain;
}

fn run(host: &mut dyn Host, dimension: &str, d: &Data, queued: Queued) {
    match queued {
        Queued::Add { to, effect, ticks, amplifier, ambient, visible } => add(host, to, &effect, ticks, amplifier, ambient, visible),
        Queued::Remove { from, effect } => remove(host, from, &effect),
        Queued::Hurt { target, damage_type, amount, by, mode } => {
            with_mode(mode, || {
                host.hurt_entity(dimension, target.id, amount, &damage_type, by);
            });
        }
        Queued::Sweep { around, amount, by } => {
            let Some([low, high]) = host.entity(around.id).and_then(|e| host.entity_bounding_box(e)) else {
                return;
            };
            let grow = d.hurt.happy_aura.around;
            let low = [low[0] - grow[0], low[1] - grow[1], low[2] - grow[2]];
            let high = [high[0] + grow[0], high[1] + grow[1], high[2] + grow[2]];
            for id in host.entities_in(dimension, low, high) {
                if id == around.id || id == by.id {
                    continue;
                }
                let Some(entity) = host.entity(id) else {
                    continue;
                };
                if host.living(entity).is_none() {
                    continue;
                }
                let held = {
                    let s = state();
                    s.effects_of(s.key(id, None))
                };
                let tame = host.entity_nbt(entity).is_some_and(|nbt| nbt.contains_key(&name("Owner")));
                if swept(&held, tame) {
                    with_mode(Mode::Sweep, || {
                        host.hurt_entity(dimension, id, amount, &d.hurt.happy_aura.damage_type, Some(by.id));
                    });
                }
            }
        }
        Queued::Heal { target, amount } => heal(host, dimension, target, amount),
        Queued::Push { target, from, speed } => {
            let (Some(entity), Some(to), Some(away)) = (
                host.entity(target.id),
                host.entity(target.id).and_then(|e| host.entity_position(e)),
                host.entity(from.id).and_then(|e| host.entity_position(e)),
            ) else {
                return;
            };
            let direction = [to[0] - away[0], to[1] - away[1], to[2] - away[2]];
            let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
            if length > 1e-4 {
                host.entity_set_velocity(entity, direction.map(|v| v / length * speed));
            }
        }
        Queued::Frozen { target, ticks } => {
            // A mob's `TicksFrozen`; a player's is their client's to count.
            if let Key::Mob(id) = target.key
                && let Some(entity) = host.entity(id)
                && let Some(nbt) = host.entity_nbt(entity)
                && crate::int(&nbt, "TicksFrozen") < ticks
            {
                let mut merge = Compound::new();
                merge.insert(name("TicksFrozen"), Tag::Int(ticks));
                host.entity_merge_nbt(entity, merge);
            }
        }
        Queued::Sound { at, sound } => {
            if let Some(place) = center(host, at, false) {
                host.play_sound(&sound.sound, place, PLAYERS, sound.volume, sound.pitch);
            }
        }
        Queued::Shatter { at, block } => {
            // `levelEvent(2001, above, block)`: the block's crumbs where it would break.
            if let Some(place) = center(host, at, false)
                && let Some(kind) = ParticleType::from_name("block")
            {
                let mut options = ParticleOptions::simple(kind);
                options.block = Some((block.clone(), 0xFF_FFFF));
                host.send_particles(&options, None, [place[0], place[1] + 1.0, place[2]], [0.25; 3], 0.05, 24);
            }
        }
        Queued::Particles { at, particle, eyes } => {
            if let Some(place) = center(host, at, eyes)
                && let Some(kind) = ParticleType::from_name(bare(&particle.particle))
            {
                host.send_particles(&ParticleOptions::simple(kind), None, place, particle.spread, particle.speed, particle.count);
            }
        }
        Queued::Break { target } => {
            let berry = &d.attack.bursting_berry;
            modifier(host, target, &berry.attribute, &berry.id, Some((berry.amount, &berry.operation)));
        }
        Queued::Instant { target, purity, amplifier } => {
            let about = if purity { &d.ticks.purity } else { &d.ticks.fragrant };
            if about.clears.iter().any(|kind| kind == "harmful") {
                for (effect, _, _) in effects(host, target) {
                    if category(&effect) == "harmful" {
                        remove(host, target, &effect);
                    }
                }
            }
            if let Some((health, most)) = health(host, target) {
                heal(host, dimension, target, heals(&about.heals, amplifier, health, most));
            }
        }
        Queued::Lurk { player, amplifier, crouching } => lurk(host, d, player, amplifier, crouching),
        Queued::Merge { target } => {
            let merge = &d.added.merge;
            let held = effects(host, target);
            let of = |effect: &String| held.iter().find(|(id, ..)| id == effect).map(|(_, ticks, amplifier)| (*amplifier, *ticks));
            if let (Some(first), Some(second)) = (of(&merge.of[0]), of(&merge.of[1])) {
                let (amplifier, ticks) = merged(first, second);
                for effect in &merge.of {
                    remove(host, target, effect);
                }
                add(host, target, &merge.into, ticks, amplifier, false, true);
            }
        }
        Queued::Menthol { player } => menthol(host, d, player),
        Queued::WallnutGiven { player, amplifier } => {
            let Some((_, most)) = host.attribute(player, "minecraft:max_health") else {
                return;
            };
            let given = absorption(d, amplifier, most as f32);
            let ticks = effects(host, Target { id: 0, key: Key::Player(player) })
                .iter()
                .find(|(id, ..)| *id == format!("{NS}wallnut_first_aid"))
                .map_or(0, |(_, ticks, _)| *ticks);
            if given > 0.0 && ticks != 0 {
                // The game holds absorption only under its own effect: the level that covers it.
                let level = (given / 4.0).ceil() as i32 - 1;
                host.add_effect(player, &effect_of("minecraft:absorption", ticks, level, false, false));
                state().wallnut.insert(player, given);
            }
        }
        Queued::Wallnut { player } => {
            if !d.ticks.wallnut_first_aid.ends_when_spent || !state().wallnut.contains_key(&player) {
                return;
            }
            let left = match host.player_compound(player) {
                Some(Tag::Compound(saved)) => float(&saved, "AbsorptionAmount").unwrap_or(0.0),
                _ => return,
            };
            if left <= 0.0 {
                state().wallnut.remove(&player);
                host.remove_effect(player, "minecraft:absorption");
                host.remove_effect(player, &format!("{NS}wallnut_first_aid"));
            }
        }
        Queued::Evolve { player, amplifier } => {
            let evolution = &d.death.original_evolution;
            let Some(held) = host.effects_v2(player) else {
                return;
            };
            for held in held {
                let effect = &held.effect;
                if effect.duration > 0
                    && evolution.lengthens.iter().any(|kind| kind == category(&effect.id))
                    && !evolution.leaves.contains(&effect.id)
                {
                    let ticks = (effect.duration + at(&evolution.ticks, amplifier)).min(evolution.at_most);
                    host.add_effect(player, &NativeEffect { duration: ticks, ..effect.clone() });
                }
            }
        }
        Queued::EndMeteor { player, victim } => {
            let alive = health(host, Target { id: victim, key: state().key(victim, None) }).is_some_and(|(health, _)| health > 0.0);
            if alive {
                host.remove_effect(player, &format!("{NS}momentary_meteor"));
            }
        }
        Queued::Armor { target } => {
            let armor = match target.key {
                Key::Player(uuid) => host.attribute(uuid, "minecraft:armor").map(|value| value.1),
                Key::Mob(id) => host.entity(id).and_then(|e| host.entity_nbt(e)).and_then(|nbt| {
                    list(&nbt, "attributes").iter().find_map(|tag| match tag {
                        Tag::Compound(saved) if text(saved, "id").is_some_and(|id| bare(&id) == "armor") => float(saved, "base").map(f64::from),
                        _ => None,
                    })
                }),
            };
            if let Some(armor) = armor {
                state().armor.insert(target.key, armor.floor() as i32);
            }
        }
    }
}

/// `tickLurkingDanger`: crouching long enough hides the player and quickens them; standing up
/// from that strikes harder for a moment.
fn lurk(host: &mut dyn Host, d: &Data, player: u128, amplifier: i32, crouching: bool) {
    let lurking = &d.ticks.lurking_danger;
    let target = Target { id: 0, key: Key::Player(player) };
    let (hide, quicken, strike) = {
        let mut s = state();
        let lurk = s.lurking.entry(player).or_default();
        if crouching {
            lurk.ticks += 1;
            let hidden = lurk.ticks >= at(&lurking.after, amplifier);
            let quicken = hidden && !lurk.fast;
            lurk.fast |= hidden;
            (hidden, quicken, false)
        } else {
            let strike = std::mem::take(&mut lurk.fast);
            lurk.ticks = 0;
            (false, false, strike)
        }
    };
    if hide {
        let hides = &lurking.hides;
        add(host, target, &hides.effect, hides.ticks, hides.amplifier, false, hides.particles);
    }
    if quicken {
        let speed = &lurking.speed;
        modifier(host, target, &speed.attribute, &speed.id, Some((at(&speed.amounts, amplifier), &speed.operation)));
    }
    if strike {
        let strikes = &lurking.strikes;
        add(host, target, &strikes.effect, strikes.ticks, strikes.amplifier, false, strikes.particles);
        modifier(host, target, &lurking.speed.attribute, &lurking.speed.id, None);
    }
}

/// `menthol_awakening` while it lasts: every good effect it may change is a level higher, those
/// given since as well.
fn menthol(host: &mut dyn Host, d: &Data, player: u128) {
    let Some(held) = host.effects_v2(player) else {
        return;
    };
    let mut raised = state().menthol.remove(&player).unwrap_or_default();
    raised.retain(|effect, _| held.iter().any(|e| e.effect.id == *effect));
    for held in &held {
        let effect = &held.effect;
        let level = i32::from(effect.amplifier);
        if !menthol_raises(d, &effect.id, category(&effect.id)) || raised.get(&effect.id).is_some_and(|left| level <= *left) {
            continue;
        }
        if level < d.menthol.highest {
            host.add_effect(player, &NativeEffect { amplifier: (level + 1) as u8, ..effect.clone() });
            raised.insert(effect.id.clone(), level + 1);
        } else {
            raised.insert(effect.id.clone(), level);
        }
    }
    state().menthol.insert(player, raised);
}

/// What runs out by itself, once a tick.
fn timers(host: &mut dyn Host, d: &Data) {
    let (mended, risen, sobered, spent) = {
        let mut s = state();
        let now = s.now;
        let mut mended = Vec::new();
        s.broken.retain(|id, (key, ticks)| {
            *ticks -= 1;
            if *ticks <= 0 {
                mended.push(Target { id: *id, key: *key });
            }
            *ticks > 0
        });
        let gone = |s: &State, player: &u128, effect: &str| s.has(Key::Player(*player), effect).is_none();
        let risen: Vec<u128> = s.lurking.keys().filter(|player| gone(&s, player, "lurking_danger")).copied().collect();
        let risen: Vec<(u128, bool)> = risen.into_iter().filter_map(|player| s.lurking.remove(&player).map(|lurk| (player, lurk.fast))).collect();
        let sobered: Vec<u128> = s.menthol.keys().filter(|player| gone(&s, player, "menthol_awakening")).copied().collect();
        for player in &sobered {
            s.menthol.remove(player);
        }
        let spent: Vec<u128> = s.wallnut.keys().filter(|player| gone(&s, player, "wallnut_first_aid")).copied().collect();
        for player in &spent {
            s.wallnut.remove(player);
        }
        // Notes of effects long gone, and of entities with them.
        s.held.retain(|_, held| {
            held.retain(|_, (_, heard)| now - *heard <= 200);
            !held.is_empty()
        });
        (mended, risen, sobered, spent)
    };
    let berry = &d.attack.bursting_berry;
    for target in mended {
        modifier(host, target, &berry.attribute, &berry.id, None);
    }
    let speed = &d.ticks.lurking_danger.speed;
    for (player, fast) in risen {
        if fast {
            modifier(host, Target { id: 0, key: Key::Player(player) }, &speed.attribute, &speed.id, None);
        }
    }
    // The effect ended: every good effect it may change goes a level down.
    for player in sobered {
        for held in host.effects_v2(player).unwrap_or_default() {
            let effect = &held.effect;
            if effect.amplifier > 0 && menthol_raises(d, &effect.id, category(&effect.id)) {
                host.remove_effect(player, &effect.id);
                host.add_effect(player, &NativeEffect { amplifier: effect.amplifier - 1, ..effect.clone() });
            }
        }
    }
    for player in spent {
        host.remove_effect(player, "minecraft:absorption");
    }
}

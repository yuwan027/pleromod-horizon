//! The mod's mob effects: what `ModMobEffect` and `MobEffectEvents` do, by `data/effects.json`,
//! which pleromod's extraction (effects.py) measured by running those classes for every amplifier.
//! The attribute modifiers are the content mod's (its effects declare them); the rest is here.
//!
//! `hit` is the mod's handlers of a hit in their order (`onTransmutationAttack`, `onLivingAttack`,
//! `onLivingHurt`) as a function of who had what; `data/effects_check.json` holds runs of the
//! mod's own code that the tests replay through it. `effects/world.rs` is what tells the game.
mod world;

pub use world::*;

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// The mod's namespace, which its effects' ids start with.
pub const NS: &str = "plerocottage:";

type Table = Vec<f32>;

#[derive(Deserialize)]
pub struct Data {
    /// 1.20.1's damage type tags the handlers ask about.
    pub tags: HashMap<String, Vec<String>>,
    /// `Biome.getBaseTemperature`, by biome.
    pub biomes: HashMap<String, f32>,
    pub effects: BTreeMap<String, About>,
    pub attack: Attack,
    pub transmutation: Transmutation,
    pub hurt: Hurt,
    pub damage: Damage,
    pub experience: Experience,
    pub death: Death,
    pub added: Added,
    pub ticks: Ticks,
    pub menthol: Menthol,
}

#[derive(Deserialize)]
pub struct About {
    pub category: String,
    #[allow(dead_code)]
    pub instant: bool,
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
}

#[derive(Deserialize, Clone)]
pub struct Modifier {
    pub attribute: String,
    pub id: String,
    pub operation: String,
    /// By amplifier.
    pub amounts: Vec<f64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Sound {
    pub sound: String,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Deserialize)]
pub struct Attack {
    pub rosa_hedge: Rosa,
    pub hacked_thorns: Refused,
    pub bursting_berry: Broken,
    pub lurking_danger: Wait,
}
#[derive(Deserialize)]
pub struct Rosa {
    pub at_most: Table,
    pub unless: Vec<String>,
}
#[derive(Deserialize)]
pub struct Refused {
    /// What touched the victim: `projectile`, `arrow`.
    pub refuses: Vec<String>,
}
#[derive(Deserialize)]
pub struct Broken {
    pub attribute: String,
    pub id: String,
    pub amount: f64,
    pub operation: String,
    pub ticks: i32,
    pub key: String,
    pub charge: f32,
}
#[derive(Deserialize)]
pub struct Wait {
    pub key: String,
    pub ticks: i32,
}

#[derive(Deserialize)]
pub struct Transmutation {
    pub every: i32,
    pub damage_type: String,
    #[serde(flatten)]
    pub sound: Sound,
    pub charge: f32,
    pub particle: Particle,
    /// By amplifier, the part of the target's health for each of the draws.
    pub part: Vec<Table>,
}
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Particle {
    pub particle: String,
    pub count: i32,
    #[serde(default)]
    pub spread: [f64; 3],
    pub speed: f64,
}

#[derive(Deserialize)]
pub struct Hurt {
    pub animal_knife: Option<Knife>,
    pub dark_wrath: Wrath,
    pub bursting_berry: Times,
    pub momentary_meteor: Meteor,
    pub happy_aura: Aura,
    pub kiwing_whereabouts: Kiwing,
    pub ice_barrier: Ice,
    pub snow_veil: Snow,
    pub aurora_veil: Aurora,
    pub hacked_thorns: Thorns,
    pub poison_heal_medicine: Poison,
}
#[derive(Deserialize)]
pub struct Knife {
    pub items: Vec<String>,
    pub times: f32,
}
#[derive(Deserialize)]
pub struct Wrath {
    pub taken: Table,
    pub dealt: Table,
}
#[derive(Deserialize)]
pub struct Times {
    pub times: Table,
}
#[derive(Deserialize)]
pub struct Meteor {
    pub times: Table,
    pub charge: f32,
}
#[derive(Deserialize)]
pub struct Aura {
    pub times: Table,
    /// `AABB.inflate` of the victim's box.
    pub around: [f64; 3],
    pub damage_type: String,
}
#[derive(Deserialize)]
pub struct Given {
    pub effect: String,
    pub amplifier: i32,
}
#[derive(Deserialize)]
pub struct Kiwing {
    pub effects: Vec<Given>,
    pub ticks: Vec<i32>,
    pub invulnerable: Vec<i32>,
    pub fall: Table,
}
#[derive(Deserialize)]
pub struct Ice {
    pub times: f32,
    pub frozen: i32,
    pub temperatures: Vec<f32>,
    pub breaks_at: Vec<Table>,
    pub breaks_on: Vec<String>,
    pub held: Sound,
    pub broken: Shattered,
}
#[derive(Deserialize)]
pub struct Shattered {
    #[serde(flatten)]
    pub sound: Sound,
    /// `Level.levelEvent`: its number and the block.
    pub level_event: (i32, String),
}
#[derive(Deserialize)]
pub struct Snow {
    pub temperatures: Vec<f32>,
    pub times: Vec<Table>,
    pub frozen: i32,
    pub melts_on: Vec<String>,
    pub melted: Sound,
}
#[derive(Deserialize)]
pub struct Aurora {
    pub times: Table,
    pub times_other: f32,
    pub frozen: i32,
    pub melts_on: Vec<String>,
    pub melted: Sound,
}
#[derive(Deserialize)]
pub struct Thorns {
    pub push: Vec<f64>,
    /// Of the amount, for each point of the victim's armour.
    pub back: Table,
    pub damage_type: String,
    #[serde(flatten)]
    pub sound: Sound,
    pub not_for: Vec<String>,
}
#[derive(Deserialize)]
pub struct Poison {
    pub heal: Table,
    pub amount: f32,
    pub of: Vec<String>,
}

#[derive(Deserialize)]
pub struct Damage {
    pub momentary_meteor: Stars,
}
#[derive(Deserialize)]
pub struct Stars {
    #[serde(flatten)]
    pub sound: Sound,
    pub particle: Particle,
    pub ends_when_survived: bool,
}

#[derive(Deserialize)]
pub struct Experience {
    pub rutin_limonene: Part,
}
#[derive(Deserialize)]
pub struct Part {
    pub part: Vec<f64>,
}

#[derive(Deserialize)]
pub struct Death {
    pub dark_wrath: Ends,
    pub original_evolution: Evolution,
}
#[derive(Deserialize)]
pub struct Ends {
    pub ends: bool,
}
#[derive(Deserialize)]
pub struct Evolution {
    pub ticks: Vec<i32>,
    pub at_most: i32,
    pub leaves: Vec<String>,
    pub lengthens: Vec<String>,
}

#[derive(Deserialize)]
pub struct Added {
    pub merge: Merge,
    /// By the effect an entity has: those it keeps out.
    pub refuses: HashMap<String, Vec<String>>,
}
#[derive(Deserialize)]
pub struct Merge {
    pub of: [String; 2],
    pub into: String,
    pub stronger: bool,
    pub then_longer: bool,
}

#[derive(Deserialize)]
pub struct Ticks {
    pub purity: Heals,
    pub fragrant: Heals,
    pub cherry_blossoms_weeping: Leaves,
    pub lurking_danger: Lurking,
    pub wallnut_first_aid: Wallnut,
}
#[derive(Deserialize)]
pub struct Heals {
    /// Of the health missing, rounded down.
    pub heals: Vec<f64>,
    pub clears: Vec<String>,
}
#[derive(Deserialize)]
pub struct Leaves {
    pub chance: f32,
    pub particle: String,
    pub count: [i32; 2],
    pub spread: [f64; 3],
    pub speed: f64,
}
#[derive(Deserialize)]
pub struct Lurking {
    pub after: Vec<i32>,
    pub hides: Lasting,
    pub speed: Modifier,
    pub strikes: Lasting,
}
#[derive(Deserialize)]
pub struct Lasting {
    pub effect: String,
    pub ticks: i32,
    pub amplifier: i32,
    pub particles: bool,
}
#[derive(Deserialize)]
pub struct Wallnut {
    /// Of the most health, rounded down.
    pub absorption: Vec<f64>,
    pub ends_when_spent: bool,
}

#[derive(Deserialize)]
pub struct Menthol {
    pub blacklist: Vec<String>,
    pub whitelist: Vec<String>,
    pub raises: bool,
    pub highest: i32,
    pub categories: Vec<String>,
}

pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/effects.json")))
            .unwrap_or_else(|e| panic!("data/effects.json is what effects.py writes: {e}"))
    })
}

/// A table's value for an amplifier (0 to 255).
pub fn at<T: Copy>(table: &[T], amplifier: i32) -> T {
    table[(amplifier.clamp(0, 255) as usize).min(table.len() - 1)]
}

fn bare(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// Whether a list of damage types (or a tag's) holds `damage_type`.
pub fn among(list: &[String], damage_type: &str) -> bool {
    list.iter().any(|t| bare(t) == bare(damage_type))
}

/// Which of the ranges the bounds cut the biome's temperature into.
pub fn climate(bounds: &[f32], temperature: f32) -> usize {
    bounds.iter().filter(|bound| temperature >= **bound).count()
}

/// The effects an entity has, by id, with their amplifiers.
pub type Held = HashMap<String, i32>;

fn has(held: &Held, name: &str) -> Option<i32> {
    held.get(&format!("{NS}{name}")).copied()
}

/// Who caused a hit (`DamageSource.getEntity`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum By {
    Nobody,
    Mob,
    Player,
}

/// What touched the victim (`DamageSource.getDirectEntity`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direct {
    Nothing,
    Attacker,
    Projectile,
    Arrow,
}

impl Direct {
    fn name(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::Attacker => "attacker",
            Self::Projectile => "projectile",
            Self::Arrow => "arrow",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Who {
    Victim,
    Attacker,
    /// One of those standing around the victim that a sweep reaches.
    Near(usize),
}

/// A hit as the handlers see it.
#[derive(Clone, Debug)]
pub struct Hit<'a> {
    pub victim: &'a Held,
    pub attacker: &'a Held,
    pub by: By,
    pub direct: Direct,
    pub damage_type: &'a str,
    pub amount: f32,
    /// A falling player's hit as `isCritical` asks (the charge aside), or a critical arrow.
    pub critical: bool,
    /// `AttackCharge.of`.
    pub charge: f32,
    /// The base temperature of the victim's biome.
    pub temperature: f32,
    /// The victim's `getRandom().nextFloat()`.
    pub roll: f32,
    pub armor: i32,
    pub health: f32,
    /// An `Animal` that is no baby.
    pub adult_animal: bool,
    /// The attacker's main hand.
    pub weapon: Option<&'a str>,
    /// How many a sweep around the victim reaches.
    pub nearby: usize,
    /// The charged hits the attacker has counted on this victim (`TransmutationCount`).
    pub count: i32,
    /// A hit of `happy_aura`'s sweep (`PleroCottageHappySweep`).
    pub sweeping: bool,
    /// The magic hit of `transmutation_elixir` (`TransmutationReentry`).
    pub reentry: bool,
    pub invulnerable: i32,
}

/// What a handler did beside changing the amount.
#[derive(Clone, Debug, PartialEq)]
pub enum Did {
    Add { who: Who, effect: String, ticks: i32, amplifier: i32, ambient: bool, visible: bool },
    Remove { who: Who, effect: String },
    Hurt { who: Who, damage_type: String, amount: f32, magic: bool },
    Heal { who: Who, amount: f32 },
    /// `setDeltaMovement`: away from the victim, this fast.
    Push { who: Who, speed: f64 },
    Frozen { who: Who, ticks: i32 },
    Sound(Sound),
    /// The armour a critical hit breaks for a while.
    Break { who: Who },
    LevelEvent(i32, String),
    Particles(Particle),
    /// `PleroCottageLurkingTicks` of the attacker.
    Waits(i32),
    /// `TransmutationCount` of the attacker, on this victim.
    Count(i32),
    /// The victim a `momentary_meteor` hit is waiting to land on.
    Pending,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub canceled: bool,
    pub amount: f32,
    pub did: Vec<Did>,
    pub invulnerable: i32,
}

/// Whether a sweep of `happy_aura` reaches someone with these effects.
pub fn swept(held: &Held, tame: bool) -> bool {
    has(held, "happy_aura").is_none() && !tame
}

/// The handlers of a hit, in Forge's order: `onTransmutationAttack` (highest), `onLivingAttack`,
/// then, unless one refused it, `onLivingHurt`.
pub fn hit(data: &Data, h: &Hit<'_>) -> Outcome {
    let mut did = Vec::new();
    let mut amount = h.amount;
    let mut invulnerable = h.invulnerable;
    let player = h.by == By::Player;
    let living = h.by != By::Nobody;
    let melee = living && h.direct == Direct::Attacker;
    let sweep = |did: &mut Vec<Did>, damage: f32, amplifier: i32| {
        let aura = &data.hurt.happy_aura;
        for index in 0..h.nearby {
            did.push(Did::Hurt {
                who: Who::Near(index),
                damage_type: aura.damage_type.clone(),
                amount: damage * at(&aura.times, amplifier),
                magic: false,
            });
        }
    };

    // onTransmutationAttack
    let elixir = &data.transmutation;
    if !h.reentry
        && !h.sweeping
        && player
        && melee
        && h.charge >= elixir.charge
        && let Some(amplifier) = has(h.attacker, "transmutation_elixir")
    {
        let count = h.count + 1;
        did.push(Did::Count(count % elixir.every));
        if count % elixir.every == 0 {
            let parts = &elixir.part[(amplifier.clamp(0, 255) as usize).min(elixir.part.len() - 1)];
            let draw = ((h.roll * parts.len() as f32) as usize).min(parts.len() - 1);
            let extra = h.health * parts[draw];
            did.push(Did::Hurt {
                who: Who::Victim,
                damage_type: elixir.damage_type.clone(),
                amount: extra,
                magic: true,
            });
            did.push(Did::Sound(elixir.sound.clone()));
            did.push(Did::Particles(elixir.particle.clone()));
            if let Some(aura) = has(h.attacker, "happy_aura") {
                sweep(&mut did, extra, aura);
            }
        }
    }

    // onLivingAttack
    let attack = &data.attack;
    let mut canceled = false;
    if let Some(amplifier) = has(h.victim, "rosa_hedge")
        && amount <= at(&attack.rosa_hedge.at_most, amplifier)
        && !among(&attack.rosa_hedge.unless, h.damage_type)
    {
        canceled = true;
    }
    if has(h.victim, "hacked_thorns").is_some()
        && attack.hacked_thorns.refuses.iter().any(|d| d == h.direct.name())
    {
        canceled = true;
    }
    if canceled {
        return Outcome { canceled, amount, did, invulnerable };
    }
    let critical = match h.direct {
        Direct::Arrow => h.critical,
        Direct::Attacker => player && h.critical && h.charge >= attack.bursting_berry.charge,
        _ => false,
    };
    if player && critical && has(h.attacker, "bursting_berry").is_some() {
        did.push(Did::Break { who: Who::Victim });
    }
    if player && has(h.attacker, "lurking_danger").is_some() {
        did.push(Did::Waits(attack.lurking_danger.ticks));
    }

    // onLivingHurt
    let hurt = &data.hurt;
    if let Some(knife) = &hurt.animal_knife
        && h.adult_animal
        && melee
        && h.weapon.is_some_and(|weapon| knife.items.iter().any(|item| item == weapon))
    {
        amount *= knife.times;
    }
    if living && let Some(amplifier) = has(h.victim, "dark_wrath") {
        amount *= at(&hurt.dark_wrath.taken, amplifier);
    }
    let own = player && melee && !h.sweeping;
    if own && let Some(amplifier) = has(h.attacker, "dark_wrath") {
        amount *= at(&hurt.dark_wrath.dealt, amplifier);
    }
    if player
        && !h.sweeping
        && critical
        && let Some(amplifier) = has(h.attacker, "bursting_berry")
    {
        amount *= at(&hurt.bursting_berry.times, amplifier);
    }
    if own
        && h.charge >= hurt.momentary_meteor.charge
        && let Some(amplifier) = has(h.attacker, "momentary_meteor")
    {
        amount *= at(&hurt.momentary_meteor.times, amplifier);
        did.push(Did::Pending);
    }
    if own && let Some(amplifier) = has(h.attacker, "happy_aura") {
        sweep(&mut did, amount, amplifier);
    }
    if let Some(amplifier) = has(h.victim, "kiwing_whereabouts") {
        let kiwing = &hurt.kiwing_whereabouts;
        if living {
            for given in &kiwing.effects {
                did.push(Did::Add {
                    who: Who::Victim,
                    effect: given.effect.clone(),
                    ticks: at(&kiwing.ticks, amplifier),
                    amplifier: given.amplifier,
                    ambient: false,
                    visible: false,
                });
            }
            invulnerable = invulnerable.max(at(&kiwing.invulnerable, amplifier));
        }
        if among(&data.tags["is_fall"], h.damage_type) {
            amount *= at(&kiwing.fall, amplifier);
        }
    }
    if let Some(amplifier) = has(h.victim, "ice_barrier") {
        let ice = &hurt.ice_barrier;
        let limit = at(&ice.breaks_at[climate(&ice.temperatures, h.temperature)], amplifier);
        let broken = amount >= limit || among(&ice.breaks_on, h.damage_type);
        amount *= ice.times;
        did.push(Did::Frozen { who: Who::Victim, ticks: ice.frozen });
        if broken {
            did.push(Did::Sound(ice.broken.sound.clone()));
            did.push(Did::LevelEvent(ice.broken.level_event.0, ice.broken.level_event.1.clone()));
            did.push(Did::Remove { who: Who::Victim, effect: format!("{NS}ice_barrier") });
        } else {
            did.push(Did::Sound(ice.held.clone()));
        }
    }
    if let Some(amplifier) = has(h.victim, "snow_veil") {
        let snow = &hurt.snow_veil;
        if among(&snow.melts_on, h.damage_type) {
            did.push(Did::Sound(snow.melted.clone()));
            did.push(Did::Remove { who: Who::Victim, effect: format!("{NS}snow_veil") });
        } else if living {
            amount *= at(&snow.times[climate(&snow.temperatures, h.temperature)], amplifier);
            did.push(Did::Frozen { who: Who::Victim, ticks: snow.frozen });
        }
    }
    if let Some(amplifier) = has(h.victim, "aurora_veil") {
        let aurora = &hurt.aurora_veil;
        did.push(Did::Frozen { who: Who::Victim, ticks: aurora.frozen });
        amount *= if living { at(&aurora.times, amplifier) } else { aurora.times_other };
        if among(&aurora.melts_on, h.damage_type) {
            did.push(Did::Sound(aurora.melted.clone()));
            did.push(Did::Remove { who: Who::Victim, effect: format!("{NS}aurora_veil") });
        }
    }
    if melee
        && let Some(amplifier) = has(h.victim, "hacked_thorns")
        && !among(&hurt.hacked_thorns.not_for, h.damage_type)
    {
        let thorns = &hurt.hacked_thorns;
        did.push(Did::Push { who: Who::Attacker, speed: at(&thorns.push, amplifier) });
        let back = amount * h.armor as f32 * at(&thorns.back, amplifier);
        if back > 0.0 {
            did.push(Did::Hurt {
                who: Who::Attacker,
                damage_type: thorns.damage_type.clone(),
                amount: back,
                magic: false,
            });
        }
        did.push(Did::Sound(thorns.sound.clone()));
    }
    if let Some(amplifier) = has(h.victim, "poison_heal_medicine")
        && among(&hurt.poison_heal_medicine.of, h.damage_type)
    {
        let heal = amount * at(&hurt.poison_heal_medicine.heal, amplifier);
        amount = hurt.poison_heal_medicine.amount;
        did.push(Did::Heal { who: Who::Victim, amount: heal });
    }
    Outcome { canceled, amount, did, invulnerable }
}

/// `ModMobEffect.applyInstantEffect`: the health given back of what is missing.
pub fn heals(table: &[f64], amplifier: i32, health: f32, most: f32) -> f32 {
    (f64::from(most - health) * at(table, amplifier)).floor() as f32
}

/// `onExperienceDrop`: the extra orb's points.
pub fn extra_experience(data: &Data, amplifier: i32, dropped: i32) -> i32 {
    (f64::from(dropped) * at(&data.experience.rutin_limonene.part, amplifier)).floor() as i32
}

/// `wallnut_first_aid`: the absorption it gives an entity of this much health at most.
pub fn absorption(data: &Data, amplifier: i32, most: f32) -> f32 {
    (at(&data.ticks.wallnut_first_aid.absorption, amplifier) as f32 * most).floor()
}

/// Whether `menthol_awakening` changes the level of this effect, by the mod's settings as they
/// come (`isMentholAffectedEffect`) and its category.
pub fn menthol_raises(data: &Data, effect: &str, category: &str) -> bool {
    let menthol = &data.menthol;
    let listed = |list: &[String]| list.iter().any(|id| id == effect);
    menthol.raises
        && effect != format!("{NS}menthol_awakening")
        && menthol.categories.iter().any(|c| c == category)
        && if menthol.whitelist.is_empty() { !listed(&menthol.blacklist) } else { listed(&menthol.whitelist) }
}

/// `selectAuroraVeilSource`: the amplifier and the time the merged effect takes, of the two it
/// replaces.
pub fn merged(first: (i32, i32), second: (i32, i32)) -> (i32, i32) {
    if first.0 != second.0 {
        if first.0 > second.0 { first } else { second }
    } else if first.1 > second.1 {
        first
    } else {
        second
    }
}

#[cfg(test)]
mod tests;

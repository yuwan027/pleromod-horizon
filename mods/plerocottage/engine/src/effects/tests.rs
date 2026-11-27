use super::*;
use serde_json::Value;

fn check() -> Value {
    serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/effects_check.json")))
        .expect("data/effects_check.json is what effects.py writes")
}

fn held(value: &Value) -> Held {
    value.as_object().unwrap().iter().map(|(id, amplifier)| (id.clone(), amplifier.as_i64().unwrap() as i32)).collect()
}

fn who(who: Who) -> String {
    match who {
        Who::Victim => "victim".into(),
        Who::Attacker => "attacker".into(),
        Who::Near(index) => format!("near {index}"),
    }
}

/// What the native half did, as the lines the mod's run is told in.
fn lines(data: &Data, did: &[Did]) -> Vec<String> {
    let mut lines: Vec<String> = did
        .iter()
        .map(|did| match did {
            Did::Add { who: on, effect, ticks, amplifier, ambient, visible } => {
                format!("add {} {effect} {ticks} {amplifier} {ambient} {visible}", who(*on))
            }
            Did::Remove { who: on, effect } => format!("remove {} {effect}", who(*on)),
            Did::Hurt { who: on, damage_type, amount, .. } => {
                format!("hurt {} {} {amount:.3}", who(*on), bare(damage_type))
            }
            Did::Heal { who: on, amount } => format!("heal {} {amount:.3}", who(*on)),
            Did::Push { who: on, speed } => format!("push {} {speed:.3}", who(*on)),
            Did::Frozen { who: on, ticks } => format!("frozen {} {ticks}", who(*on)),
            Did::Sound(sound) => format!("sound {} {:.3} {:.3}", sound.sound, sound.volume, sound.pitch),
            Did::Break { who: on } => format!("data {} {} {}", who(*on), data.attack.bursting_berry.key, data.attack.bursting_berry.ticks),
            Did::LevelEvent(event, block) => format!("level_event {event} {block}"),
            Did::Particles(particle) => format!("particles {} {} {:.3}", particle.particle, particle.count, particle.speed),
            Did::Waits(ticks) => format!("data attacker {} {ticks}", data.attack.lurking_danger.key),
            Did::Count(count) => format!("data attacker PleroCottageTransmutationCount {count}"),
            Did::Pending => "pending".into(),
        })
        .collect();
    lines.sort();
    lines
}

/// What the mod's run did, the same way. What only the mod's own bookkeeping is stays out.
fn told(data: &Data, did: &Value) -> Vec<String> {
    let text = |value: &Value| value.as_str().unwrap().to_owned();
    let number = |value: &Value| value.as_f64().unwrap();
    let mut lines = Vec::new();
    for entry in did.as_array().unwrap() {
        let line = match entry[0].as_str().unwrap() {
            "add" => format!(
                "add {} {} {} {} {} {}",
                text(&entry[1]), text(&entry[2]), entry[3], entry[4], entry[5][0], entry[5][1]
            ),
            "remove" => format!("remove {} {}", text(&entry[1]), text(&entry[2])),
            "hurt" => format!("hurt {} {} {:.3}", text(&entry[1]), text(&entry[2]).to_lowercase(), number(&entry[3])),
            "heal" => format!("heal {} {:.3}", text(&entry[1]), number(&entry[2])),
            "push" => format!("push {} {:.3}", text(&entry[1]), number(&entry[2])),
            "frozen" => format!("frozen {} {}", text(&entry[1]), entry[2]),
            "sound" => format!("sound {} {:.3} {:.3}", text(&entry[1]), number(&entry[2]), number(&entry[3])),
            "level_event" => format!("level_event {} {}", entry[1], text(&entry[2])),
            "particles" => format!("particles {} {} {:.3}", text(&entry[1]), entry[2], number(&entry[4])),
            "modifier" => {
                // The armour a critical hit breaks: told once more as its timer (`Did::Break`).
                let berry = &data.attack.bursting_berry;
                assert_eq!(entry[1], "victim ARMOR");
                assert_eq!((text(&entry[2]), number(&entry[3]), text(&entry[4])), (berry.id.clone(), berry.amount, berry.operation.clone()));
                continue;
            }
            "data" => {
                let key = text(&entry[2]);
                if key.ends_with("MomentaryMeteorPendingTarget") && !entry[3].is_null() {
                    "pending".to_owned()
                } else if (key == data.attack.bursting_berry.key || key == data.attack.lurking_danger.key || key == "PleroCottageTransmutationCount") && !entry[3].is_null() {
                    // What it is left at: a count starts over at 0 on a new target before it counts.
                    let start = format!("data {} {key} ", text(&entry[1]));
                    lines.retain(|line: &String| !line.starts_with(&start));
                    format!("{start}{}", entry[3])
                } else {
                    continue;
                }
            }
            other => panic!("the check tells of {other}"),
        };
        lines.push(line);
    }
    lines.sort();
    lines
}

#[test]
fn every_run_of_the_mods_handlers_comes_out_the_same() {
    let data = data();
    let check = check();
    let hits = check["hits"].as_array().unwrap();
    assert!(hits.len() > 100, "{} runs", hits.len());
    let mut differing = Vec::new();
    for run in hits {
        let (victim, attacker) = (held(&run["victim"]), held(&run["attacker"]));
        let h = Hit {
            victim: &victim,
            attacker: &attacker,
            by: match run["by"].as_str() {
                Some("player") => By::Player,
                Some(_) => By::Mob,
                None => By::Nobody,
            },
            direct: match run["direct"].as_str() {
                Some("attacker") => Direct::Attacker,
                Some("arrow") => Direct::Arrow,
                Some(_) => Direct::Projectile,
                None => Direct::Nothing,
            },
            damage_type: run["type"].as_str().unwrap(),
            amount: run["amount"].as_f64().unwrap() as f32,
            critical: run["critical"].as_bool().unwrap_or(false),
            charge: run["charge"].as_f64().unwrap_or(1.0) as f32,
            temperature: run["temperature"].as_f64().unwrap_or(0.8) as f32,
            roll: run["roll"].as_f64().unwrap_or(0.0) as f32,
            armor: run["armor"].as_i64().unwrap_or(0) as i32,
            health: run["health"].as_f64().unwrap_or(20.0) as f32,
            adult_animal: false,
            weapon: None,
            nearby: run["nearby"].as_array().unwrap().iter().filter(|near| swept(&held(near), false)).count(),
            count: run["count"].as_i64().unwrap() as i32,
            sweeping: false,
            reentry: false,
            invulnerable: 0,
        };
        let out = hit(data, &h);
        let expected = &run["out"];
        let same = out.canceled == expected["canceled"].as_bool().unwrap()
            && (f64::from(out.amount) - expected["amount"].as_f64().unwrap()).abs() < 1e-4
            && out.invulnerable == expected["invulnerable"].as_i64().unwrap() as i32
            && lines(data, &out.did) == told(data, &expected["did"]);
        if !same {
            differing.push(format!("{run}\n  here: {} {} {:?}", out.canceled, out.amount, lines(data, &out.did)));
        }
    }
    assert!(differing.is_empty(), "{} of {} differ:\n{}", differing.len(), hits.len(), differing[..differing.len().min(4)].join("\n"));
}

#[test]
fn every_effect_of_the_mod_is_measured() {
    let data = data();
    assert_eq!(data.effects.len(), 27);
    assert!(data.effects.keys().all(|id| id.starts_with(NS)));
    // A table for every amplifier an effect can have.
    assert_eq!(data.attack.rosa_hedge.at_most.len(), 256);
    assert_eq!(data.transmutation.part.len(), 256);
    // The declared modifiers are the content mod's, at the first level and for each further one.
    for (id, about) in &data.effects {
        let path = format!("{}/../../../web/mods/plerocottage/data/plerocottage/horizon/mob_effect/{}.json", env!("CARGO_MANIFEST_DIR"), &id[NS.len()..]);
        let declared: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap();
        assert_eq!(declared["category"], about.category.as_str(), "{id}");
        let listed = declared["attribute_modifiers"].as_array().map(Vec::as_slice).unwrap_or_default();
        assert_eq!(listed.len(), about.modifiers.len(), "{id}");
        for (modifier, listed) in about.modifiers.iter().zip(listed) {
            assert_eq!((listed["attribute"].as_str(), listed["id"].as_str(), listed["operation"].as_str()), (Some(&*modifier.attribute), Some(&*modifier.id), Some(&*modifier.operation)));
            for amplifier in [0, 1, 7, 255] {
                let value = listed["amount"].as_f64().unwrap() + listed["per_level"].as_f64().unwrap() * f64::from(amplifier);
                assert!((value - at(&modifier.amounts, amplifier)).abs() < 1e-9, "{id} {} at {amplifier}", modifier.attribute);
            }
        }
    }
}

#[test]
fn what_is_rounded_down_is_rounded_as_the_mod_rounds_it() {
    let data = data();
    // floor(missing * ((amplifier + 1) * 0.025 + 0.45)) and + 0.6
    assert_eq!(heals(&data.ticks.purity.heals, 0, 0.0, 20.0), (20.0f64 * (0.025 + 0.45)).floor() as f32);
    assert_eq!(heals(&data.ticks.fragrant.heals, 1, 5.0, 20.0), (15.0f64 * (2.0 * 0.025 + 0.6)).floor() as f32);
    assert_eq!(data.ticks.purity.clears, ["harmful"]);
    assert!(data.ticks.fragrant.clears.is_empty());
    // floor(dropped * (0.8 + 0.05 * (amplifier + 1)))
    assert_eq!(extra_experience(data, 0, 5), (5.0f64 * (0.8 + 0.05)).floor() as i32);
    assert_eq!(extra_experience(data, 3, 7), 7);
    // floor((0.60F + (amplifier + 1) * 0.15F) * maxHealth)
    assert_eq!(absorption(data, 0, 20.0), ((0.60f32 + 0.15f32) * 20.0).floor());
    assert_eq!(absorption(data, 2, 30.0), ((0.60f32 + 3.0 * 0.15f32) * 30.0).floor());
}

#[test]
fn the_two_veils_become_one_and_menthol_keeps_to_good_effects() {
    let data = data();
    let merge = &data.added.merge;
    assert!(merge.stronger && merge.then_longer);
    assert_eq!(merged((2, 100), (1, 900)), (2, 100));
    assert_eq!(merged((1, 100), (1, 900)), (1, 900));
    assert_eq!(data.added.refuses[&merge.into].len(), 2);
    assert!(merge.of.iter().all(|effect| data.added.refuses[&merge.into].contains(effect)));
    assert!(menthol_raises(data, "minecraft:speed", "beneficial"));
    assert!(!menthol_raises(data, "minecraft:poison", "harmful"));
    for listed in &data.menthol.blacklist {
        assert!(!menthol_raises(data, listed, "beneficial"));
    }
    assert!(!menthol_raises(data, &format!("{NS}menthol_awakening"), "beneficial"));
    assert_eq!(climate(&data.hurt.ice_barrier.temperatures, 0.0), 0);
    assert_eq!(climate(&data.hurt.ice_barrier.temperatures, 0.8), 1);
    assert_eq!(climate(&data.hurt.ice_barrier.temperatures, 2.0), 2);
}

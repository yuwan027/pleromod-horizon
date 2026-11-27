//! The sound a machine keeps making while it runs (the mod's `OvenSoundInstance` and the like, a
//! looping `AbstractTickableSoundInstance`): which sound, how loud and where is what the sound's
//! own class set when pleromod's extraction made one (`native.py`'s `loop`). Its block entity's
//! `clientTick` starts it while the machine runs, again when the sound engine dropped it, and the
//! sound's own `tick` stops it once the machine stands or is gone.
use crate::spec::SoundLoop;
use horizon_client::mod_api::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// `SoundSource.BLOCKS`.
const BLOCKS: usize = 4;
/// How far from the player a machine's client tick is followed: past where a sound of the
/// loudest volume the mod plays one at is heard.
const REACH: f64 = 48.0;

/// The loops playing, by their machine's place: the handle, and the client ticks it has played.
static PLAYING: Mutex<Option<HashMap<[i32; 3], (u64, u32)>>> = Mutex::new(None);

/// The loop of the machine at `pos`, while it runs.
fn running(host: &dyn Host, pos: [i32; 3]) -> Option<&'static SoundLoop> {
    let block = host.block(pos);
    if block == crate::oven::BLOCK {
        // The oven's block says whether it runs.
        let on = host.block_property(pos, "running").is_some_and(|v| v == "true");
        return crate::spec::oven().sound_loop.as_ref().filter(|_| on);
    }
    let found = crate::machines::of(&block)?.spec().sound_loop.as_ref()?;
    let on = host.mod_block_entity(pos).is_some_and(|entity| crate::int(&entity.nbt, "isRunning") != 0);
    on.then_some(found)
}

/// A client tick: what each machine's `clientTick` and each loop's own `tick` do.
pub fn tick(host: &mut dyn Host) {
    let Ok(mut playing) = PLAYING.lock() else {
        return;
    };
    let playing = playing.get_or_insert_with(HashMap::new);
    let near = host.nearby_mod_block_entities(REACH);
    if near.is_empty() && playing.is_empty() {
        return;
    }
    let mut wanted: HashMap<[i32; 3], &SoundLoop> = HashMap::new();
    for pos in near {
        if let Some(found) = running(host, pos) {
            wanted.insert(pos, found);
        }
    }
    // The sound's tick: stopped when its machine no longer runs or is gone.
    playing.retain(|pos, (handle, _)| {
        let keep = wanted.contains_key(pos);
        if !keep {
            host.stop_sound_loop(*handle);
        }
        keep
    });
    for (pos, found) in wanted {
        let again = match playing.get_mut(&pos) {
            // `getGameTime() % 20 == 0 && !SoundUtil.isLoopActive`: once a second.
            Some((handle, ticks)) => {
                *ticks += 1;
                let lost = *ticks % 20 == 0 && !host.sound_loop_active(*handle);
                if lost {
                    host.stop_sound_loop(*handle);
                }
                lost
            }
            None => true,
        };
        if again {
            let at = [0, 1, 2].map(|axis| f64::from(pos[axis]) + found.at[axis]);
            match host.play_sound_loop(&found.sound, at, BLOCKS, found.volume, found.pitch) {
                Some(handle) => playing.insert(pos, (handle, 0)),
                None => playing.remove(&pos),
            };
        }
    }
}

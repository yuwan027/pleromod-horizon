//! Sounds a client mod keeps playing (docs/native-mods.md, "Sounds a client mod
//! plays"): a looping sound at a place, as a NeoForge mod's own
//! `AbstractTickableSoundInstance` with `looping` set is, for a machine that
//! hums while it runs.
use super::Game;
use horizon_client::{
    random_source,
    sound_engine::{Attenuation, SoundInstance, Sounds},
};

impl Game<'_> {
    /// Starts sound event `sound` at `at`, repeating until
    /// `native_stop_sound_loop` (`SoundManager.play` of a looping
    /// `SoundInstance` with linear attenuation): in category `source`
    /// (`SoundSource`'s ordinal, 4 for blocks), at `volume` and `pitch`. The
    /// handle it answers names the loop to the other two helpers. The sound
    /// engine may end a loop by itself, when the resources reload or the level
    /// changes: ask `native_sound_loop_active` now and then and start it again,
    /// as a mod's `SoundManager.isActive` check does. None, with nothing
    /// played, on a server and for a value out of range; an event the
    /// resources lack plays nothing and is never active.
    pub(super) fn native_play_sound_loop(
        &mut self,
        sound: &str,
        at: [f64; 3],
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> Option<u64> {
        if !self.native_window()
            || !super::native_hooks::local_sound_ok(sound, at, volume, pitch, 0)
            || source >= horizon_client::settings::SOUND_SOURCES.len()
        {
            return None;
        }
        let event = horizon_client::sound_events::normalize(sound);
        let instance = SoundInstance::new(
            &event,
            source,
            volume,
            pitch,
            random_source::create(),
            true,
            0,
            Attenuation::Linear,
            at,
            false,
        );
        Some(self.audio.play_instance(instance, true).0)
    }
    /// Stops the loop `native_play_sound_loop` started and lets go of it
    /// (`SoundInstance.stop`). False on a server. A handle already stopped is
    /// stopped still.
    pub(super) fn native_stop_sound_loop(&mut self, id: u64) -> bool {
        if !self.native_window() {
            return false;
        }
        self.audio.stop_and_release(id);
        true
    }
    /// Whether the loop is still playing (`SoundManager.isActive`): false once
    /// it was stopped, by the mod or by the sound engine.
    pub(super) fn native_sound_loop_active(&self, id: u64) -> bool {
        self.native_window() && self.audio.engine.is_active(id)
    }
    /// Where the mod block entities are that this client has within `radius`
    /// blocks of its player, in no order: what a mod's client-side
    /// `BlockEntityTicker` would be run for, so that a client tick
    /// (`native_client_tick_post`) can do what its `clientTick` does for each
    /// (start the sound of a machine that runs, stop it when it stands). Read
    /// each with `native_mod_block_entity`. Empty on a server and before the
    /// player is in a world.
    pub(super) fn native_nearby_mod_block_entities(&self, radius: f64) -> Vec<[i32; 3]> {
        let Some(at) = self.player.as_ref().map(|p| p.position) else {
            return Vec::new();
        };
        if !self.native_window() || !radius.is_finite() || radius < 0.0 {
            return Vec::new();
        }
        self.world
            .block_entities
            .mods_near(at.x, at.z, radius)
            .filter(|pos| {
                let (dx, dy, dz) = (
                    f64::from(pos.0) + 0.5 - at.x,
                    f64::from(pos.1) + 0.5 - at.y,
                    f64::from(pos.2) + 0.5 - at.z,
                );
                dx * dx + dy * dy + dz * dz <= radius * radius
            })
            .map(|pos| [pos.0, pos.1, pos.2])
            .collect()
    }
}

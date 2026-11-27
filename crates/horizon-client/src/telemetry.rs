//! What is left of the game's opt-in data collection: the start-up marks the spans record, the
//! adapter's name and the build's version. This build keeps no reports for anyone to send.
use horizon_os::time::Instant;

/// The adapter's name, once the renderer has one.
pub fn gpu_name() -> Option<String> {
    crate::gfx::adapter().map(|gpu| gpu.name)
}

/// The release (`v0.2.3`), or the client's crate version for a build from source.
pub fn version() -> String {
    crate::crash::release_tag().unwrap_or_else(|| format!("dev {}", env!("CARGO_PKG_VERSION")))
}

/// A play session's start-up marks: the first title frame and each world's first frame.
#[derive(Default)]
pub struct Session {
    /// The first title frame was marked: only the first session carries it.
    title_measured: bool,
    opening: Option<Instant>,
}

impl Session {
    /// The title screen drew a frame: the first time, marks the start's end.
    pub fn title_shown(&mut self) {
        if !self.title_measured {
            self.title_measured = true;
            crate::spans::startup::mark("title.shown");
        }
    }

    /// A world starts loading.
    pub fn world_opening(&mut self) {
        if self.opening.is_none() {
            crate::spans::startup::arm("world.opening");
        }
        self.opening.get_or_insert_with(Instant::now);
    }

    /// An entry queued during startup was cancelled before opening a world.
    pub fn cancel_world_opening(&mut self) {
        self.opening = None;
    }

    /// A frame drawn while playing (not in menus or loading).
    pub fn frame(&mut self, _ms: f32, _spans: &[crate::spans::Span]) {
        if self.opening.take().is_some() {
            crate::spans::startup::mark("world.first_frame");
        }
    }

    /// One tick of the integrated server, in single player.
    pub fn tick(&mut self, _ms: f32) {}

    /// The session ended: the next one starts.
    pub fn finish(&mut self) {
        *self = Self {
            title_measured: self.title_measured,
            ..Self::default()
        };
    }
}

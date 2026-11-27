//! The browser game with native mods: crates/horizon-client/src/main.rs with the trait mods of
//! the features web/build.sh turned on, as the launcher's generated executable passes them.
#![forbid(unsafe_code)]

#[global_allocator]
static ALLOCATOR: horizon_threads::GameAllocator = horizon_threads::GAME_ALLOCATOR;

static MOD_API_LEVEL: [u8; horizon_mods::api_level::MARK.len()] = horizon_mods::api_level::MARK;

// Visible to src/content.rs, the same program under another name.
pub(crate) fn main() {
    #[cfg(target_arch = "wasm32")]
    horizon_os::web::on_start(start);
    #[cfg(not(target_arch = "wasm32"))]
    start();
}

fn start() {
    std::hint::black_box(&MOD_API_LEVEL);
    // In the order the mods depend on each other (none do yet).
    let mods: Vec<(&'static str, Box<dyn horizon_client::mod_api::Hooks>)> = vec![
        #[cfg(feature = "plerocottage")]
        ("plerocottage", plerocottage_native::hooks()),
    ];
    let ids: &'static [&'static str] = mods.iter().map(|(id, _)| *id).collect::<Vec<_>>().leak();
    horizon_client::mod_api::name_mods(ids);
    horizon_client::entry::main_with(
        mods.into_iter()
            .map(|(_, hooks)| hooks)
            .collect::<Vec<_>>()
            .into(),
    );
}

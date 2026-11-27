//! What the mod draws with GeckoLib, drawn here with `gecko`'s player: the blocks whose block
//! entity is a `GeoBlockEntity` (display foods, the banana plant's parts, coconuts, the bowl, the
//! blender and the saucepan) and the mobs with a `GeoEntityRenderer` (the mallard and the turkey).
//!
//! `data/gecko.json` says which model, texture and animation each block state and mob has and
//! what its controllers play, as the mod's own renderers, models and controllers answered when
//! pleromod's extraction ran them. The model and animation files are the content mod's.
use crate::gecko::{self, Animation, Controller, Geometry, Loop, Pose};
use horizon_client::mod_api::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Deserialize)]
pub struct Index {
    pub blocks: HashMap<String, Block>,
    pub entities: HashMap<String, Entity>,
    /// The sprites' prefix: `plerocottage:entity/geckolib/`, then the texture's name.
    pub sprites: String,
}

#[derive(Deserialize)]
pub struct Block {
    /// The state properties what is drawn depends on; `states` is keyed by their values.
    pub properties: Vec<String>,
    pub states: HashMap<String, Drawn>,
    pub controllers: Vec<ControllerSpec>,
}

#[derive(Deserialize)]
pub struct Entity {
    pub adult: Drawn,
    pub baby: Drawn,
    pub controllers: Vec<ControllerSpec>,
}

#[derive(Deserialize)]
pub struct Drawn {
    pub model: String,
    pub texture: String,
    #[serde(default)]
    pub animation: Option<String>,
    /// The way `GeoBlockRenderer.getFacing` turns it.
    #[serde(default)]
    pub facing: Option<String>,
    /// What the renderer's layers draw over it (`StaticModelLayer`: a soup's bowl).
    #[serde(default)]
    pub layers: Vec<Layer>,
}

#[derive(Deserialize)]
pub struct Layer {
    pub model: String,
    pub texture: String,
    pub tint: [f32; 4],
}

#[derive(Deserialize)]
pub struct ControllerSpec {
    /// The controller's name in the mod, which `triggerAnim` names it by.
    #[allow(dead_code)]
    pub name: String,
    /// `transitionTickTime`.
    pub transition: f32,
    /// What `triggerAnim` may start, by name.
    #[serde(default)]
    pub triggers: HashMap<String, Vec<(String, String)>>,
    /// What its handler sets, by what it asked of the entity.
    pub plays: Vec<Play>,
}

#[derive(Deserialize)]
pub struct Play {
    #[serde(default)]
    pub on_ground: Option<bool>,
    #[serde(default)]
    pub moving: Option<bool>,
    #[serde(default)]
    pub baby: Option<bool>,
    /// A `RawAnimation`'s stages: the animation's name and how it loops.
    pub animation: Vec<(String, String)>,
    /// False for `PlayState.STOP`: nothing plays until it is triggered.
    pub plays: bool,
}

pub fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/gecko.json")))
            .unwrap_or_else(|e| panic!("data/gecko.json is what gecko.py writes: {e}"))
    })
}

/// The content mod's files, read once: a model, an animation file.
#[derive(Default)]
struct Files {
    geometry: HashMap<String, Option<Arc<Geometry>>>,
    animations: HashMap<String, Arc<HashMap<String, Animation>>>,
}

fn files() -> &'static Mutex<Files> {
    static FILES: OnceLock<Mutex<Files>> = OnceLock::new();
    FILES.get_or_init(Mutex::default)
}

/// Forgets what was read: the resource packs changed.
pub fn reloaded() {
    if let Ok(mut files) = files().lock() {
        *files = Files::default();
    }
}

fn read(host: &dyn Host, folder: &str, name: &str, ending: &str) -> Option<serde_json::Value> {
    let path = format!("assets/plerocottage/{folder}/{name}.{ending}.json");
    serde_json::from_slice(&host.client_resources().read(&path)?).ok()
}

pub(crate) fn geometry(host: &dyn Host, name: &str) -> Option<Arc<Geometry>> {
    let mut files = files().lock().ok()?;
    if let Some(found) = files.geometry.get(name) {
        return found.clone();
    }
    let found = read(host, "geo", name, "geo")
        .and_then(|json| Geometry::parse(&json))
        .map(Arc::new);
    files.geometry.insert(name.to_owned(), found.clone());
    found
}

fn animations(host: &dyn Host, name: &str) -> Arc<HashMap<String, Animation>> {
    let Ok(mut files) = files().lock() else {
        return Arc::default();
    };
    if let Some(found) = files.animations.get(name) {
        return found.clone();
    }
    let found = Arc::new(
        read(host, "animations", name, "animation")
            .map(|json| gecko::animations(&json))
            .unwrap_or_default(),
    );
    files.animations.insert(name.to_owned(), found.clone());
    found
}

pub(crate) fn tint(rgba: [f32; 4]) -> u32 {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    c(rgba[0]) << 16 | c(rgba[1]) << 8 | c(rgba[2])
}

/// Draws `geometry` under `draw`'s pose in `pose` with `texture`.
pub(crate) fn submit(host: &dyn Host, draw: &mut BlockEntityDraw, geometry: &Geometry, pose: Option<&Pose>, texture: &str, tint: u32) {
    let sprite = format!("{}{texture}", index().sprites);
    for quad in geometry.quads(pose) {
        // `native_draw_quad` takes the texture's rectangle from the corner that holds its
        // start, then round by the left: the quad's corners run the other way round.
        let turns = quad.turns as usize;
        let corners = [0usize, 3, 2, 1].map(|i| quad.corners[(i + turns) % 4]);
        if !host.draw_quad(draw, &sprite, corners, quad.rect.map(|v| v * 16.0), tint) {
            return;
        }
    }
}

/// The first stage of what a controller's handler sets for these answers.
fn chosen<'a>(spec: &'a ControllerSpec, baby: bool, on_ground: bool, moving: bool) -> Option<&'a Play> {
    spec.plays.iter().find(|p| {
        p.baby.is_none_or(|b| b == baby)
            && p.on_ground.is_none_or(|g| g == on_ground)
            && p.moving.is_none_or(|m| m == moving)
    })
}

/// A block entity's controllers, and what was last seen of it.
#[derive(Default)]
struct BlockState {
    controllers: Vec<Controller>,
    state: String,
    running: bool,
    count: i32,
    /// The tick `triggerAnim` last started each controller's animation at.
    triggered: Option<f32>,
    used: i64,
}

fn blocks() -> &'static Mutex<HashMap<[i32; 3], BlockState>> {
    static BLOCKS: OnceLock<Mutex<HashMap<[i32; 3], BlockState>>> = OnceLock::new();
    BLOCKS.get_or_init(Mutex::default)
}

/// Whether the mod's `GeoBlockRenderer` draws `block`.
pub fn is_block(block: &str) -> bool {
    index().blocks.contains_key(block)
}

/// `GeoBlockRenderer.actuallyRender`: the model at the block's middle, turned as it faces.
pub fn draw_block(host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
    let Some(block) = index().blocks.get(event.block) else {
        return;
    };
    let key = block
        .properties
        .iter()
        .map(|p| host.block_state_property(event.state, p).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",");
    let Some(drawn) = block.states.get(&key) else {
        return;
    };
    let Some(geometry) = geometry(host, &drawn.model) else {
        return;
    };
    let tick = event.draw.game_time as f32 + event.draw.partial;
    let mut pose = None;
    if !block.controllers.is_empty()
        && let Some(file) = &drawn.animation
    {
        let known = animations(host, file);
        let Ok(mut all) = blocks().lock() else {
            return;
        };
        let now = event.draw.game_time;
        // Block entities long out of view are forgotten.
        if all.len() > 4096 {
            all.retain(|_, b| (now - b.used).abs() < 1200);
        }
        let seen = all.entry(event.pos).or_default();
        seen.used = now;
        seen.controllers.resize_with(block.controllers.len(), Controller::default);
        // The mod's handlers call `forceAnimationReset` when the block's state changed.
        if seen.state != key {
            seen.state = key.clone();
            seen.controllers.iter_mut().for_each(|c| c.restart(tick));
        }
        // `triggerAnim`: the blender's as it starts running, the saucepan's at each stir.
        let (running, count) = (crate::int(&event.entity.nbt, "isRunning") != 0, crate::int(&event.entity.nbt, "count"));
        if (running && !seen.running) || count > seen.count {
            seen.triggered = Some(tick);
            seen.controllers.iter_mut().for_each(|c| c.restart(tick));
        }
        (seen.running, seen.count) = (running, count);
        let mut whole = Pose::default();
        for (spec, controller) in block.controllers.iter().zip(&mut seen.controllers) {
            let play = chosen(spec, false, true, false);
            let stage = match play {
                Some(play) if play.plays => play.animation.first(),
                // Stopped by its handler: only what was triggered plays, once through.
                _ => spec.triggers.values().next().and_then(|stages| stages.first()).filter(|_| seen.triggered.is_some()),
            };
            let animation = stage.and_then(|(name, kind)| Some((name.as_str(), known.get(name)?, Loop::of(kind))));
            whole.bones.extend(controller.pose(animation, spec.transition, tick).bones);
        }
        pose = Some(whole);
    }
    let draw = &mut *event.draw;
    draw.push();
    draw.translate([0.5, 0.0, 0.5]);
    // GeoBlockRenderer.rotateBlock
    match drawn.facing.as_deref() {
        Some("south") => draw.rotate_about([0.0, 1.0, 0.0], 180.0, [0.0; 3]),
        Some("west") => draw.rotate_about([0.0, 1.0, 0.0], 90.0, [0.0; 3]),
        Some("east") => draw.rotate_about([0.0, 1.0, 0.0], 270.0, [0.0; 3]),
        Some("up") => draw.rotate_about([1.0, 0.0, 0.0], 90.0, [0.0; 3]),
        Some("down") => draw.rotate_about([1.0, 0.0, 0.0], -90.0, [0.0; 3]),
        _ => {}
    }
    submit(host, draw, &geometry, pose.as_ref(), &drawn.texture, 0xFF_FFFF);
    for layer in &drawn.layers {
        if let Some(part) = self::geometry(host, &layer.model) {
            submit(host, draw, &part, None, &layer.texture, tint(layer.tint));
        }
    }
    draw.pop();
    // Nothing of it moves: the game may draw it again as it is.
    if pose.is_none() {
        draw.keep();
    }
}

/// Where the model of a block reaches, for culling: its faces at rest, with a margin for what an
/// animation moves.
pub fn block_bounds(host: &dyn Host, event: &mut BlockEntityRenderBoundingBoxEvent<'_>) {
    let Some(block) = index().blocks.get(event.block) else {
        return;
    };
    let mut reach: f32 = 1.0;
    for drawn in block.states.values() {
        if let Some(geometry) = geometry(host, &drawn.model) {
            for quad in geometry.quads(None) {
                for corner in quad.corners {
                    reach = reach.max(corner[0].abs().max(corner[2].abs()) + 0.5).max(corner[1].abs());
                }
            }
        }
    }
    let margin = reach + 0.5;
    *event.bounds = [0.5 - margin, -margin, 0.5 - margin, 0.5 + margin, 1.0 + margin, 0.5 + margin];
}

struct MobState {
    controllers: Vec<Controller>,
    used: f32,
}

fn mobs() -> &'static Mutex<HashMap<u32, MobState>> {
    static MOBS: OnceLock<Mutex<HashMap<u32, MobState>>> = OnceLock::new();
    MOBS.get_or_init(Mutex::default)
}

/// `GeoRenderer.getMotionAnimThreshold`, which the mod's renderers leave as it is.
const MOTION_THRESHOLD: f32 = 0.015;

/// `AnimationState.isMoving` as `GeoEntityRenderer.actuallyRender` sets it: half the sum of the
/// mob's speeds along x and z is at the threshold or above, and its walk animation is not still.
pub fn is_moving(velocity: [f64; 3], walk_speed: f32) -> bool {
    let average = ((velocity[0].abs() + velocity[2].abs()) / 2.0) as f32;
    average >= MOTION_THRESHOLD && walk_speed != 0.0
}

/// `GeoEntityRenderer.actuallyRender`: the mob's model, turned with its body, in the pose its
/// controllers give. True when the mod draws this mob.
pub fn draw_mob(host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> bool {
    let mob = event.mob;
    let Some(entity) = index().entities.get(mob.kind) else {
        return false;
    };
    let drawn = if mob.baby { &entity.baby } else { &entity.adult };
    let Some(geometry) = geometry(host, &drawn.model) else {
        return false;
    };
    let known = drawn.animation.as_deref().map(|file| animations(host, file));
    let mut whole = Pose::default();
    if let (Some(known), Ok(mut all)) = (known, mobs().lock()) {
        if all.len() > 1024 {
            all.retain(|_, m| (mob.age - m.used).abs() < 600.0);
        }
        let seen = all.entry(mob.id).or_insert_with(|| MobState { controllers: Vec::new(), used: mob.age });
        seen.used = mob.age;
        seen.controllers.resize_with(entity.controllers.len(), Controller::default);
        for (spec, controller) in entity.controllers.iter().zip(&mut seen.controllers) {
            let stage = chosen(spec, mob.baby, mob.on_ground, is_moving(mob.velocity, mob.walk_speed))
                .filter(|play| play.plays)
                .and_then(|play| play.animation.first());
            let animation = stage.and_then(|(name, kind)| Some((name.as_str(), known.get(name)?, Loop::of(kind))));
            whole.bones.extend(controller.pose(animation, spec.transition, mob.age).bones);
        }
    }
    let draw = &mut *event.draw;
    draw.push();
    // LivingEntityRenderer's death: tipped over on its side, as GeoEntityRenderer.applyRotations does.
    if mob.death_time > 0 {
        let fallen = ((mob.death_time as f32 + mob.partial - 1.0) / 20.0 * 1.6).max(0.0).sqrt().min(1.0);
        draw.rotate_about([0.0, 0.0, 1.0], fallen * 90.0, [0.0; 3]);
    }
    // The model looks towards GeckoLib's -z; the frame's +z is the way the mob faces.
    draw.rotate_about([0.0, 1.0, 0.0], 180.0, [0.0; 3]);
    draw.translate([0.0, 0.01, 0.0]);
    // OverlayTexture's red while hurt or dying.
    let colour = if mob.hurt_time > 0 || mob.death_time > 0 { 0xFF_9999 } else { 0xFF_FFFF };
    submit(host, draw, &geometry, Some(&whole), &drawn.texture, colour);
    draw.pop();
    true
}

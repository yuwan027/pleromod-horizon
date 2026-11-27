//! Plero Cottage's behaviour in Horizon: the native half of the content mod `plerocottage`
//! (pleromod's `tools/mod-content/extract.py` writes both): its machines.
//!
//! What a machine does comes from `data/*.json`, which the extraction found by running the mod's
//! own classes; the code here only follows it.
mod combat;
mod display;
mod effects;
mod enchanting;
mod flatten;
mod gecko;
mod glue;
mod growth;
mod held;
mod machines;
mod mobs;
mod oven;
mod placing;
mod screen;
mod shaker;
mod shared;
mod slots;
mod sounds;
mod spec;

use horizon_client::mod_api::*;
use oven::State;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Plerocottage)
}

struct Plerocottage;

/// `SoundSource.BLOCKS`.
const BLOCKS: usize = 4;
/// The menu's buttons (`native_menu_button`): the two knobs, then start.
const TEMPERATURE_KNOB: i32 = 0;
const TIME_KNOB: i32 = 1;
const START: i32 = 2;

/// Whether the open oven screen's result slot holds something: the screen draws by it, and only
/// a client tick may read the menu's stacks.
static RESULT_SHOWN: AtomicBool = AtomicBool::new(false);

pub(crate) fn int(nbt: &Compound, key: &str) -> i32 {
    match nbt.get(&JavaString::from(key)) {
        Some(Tag::Int(v)) => *v,
        Some(Tag::Byte(v)) => i32::from(*v),
        _ => 0,
    }
}

use horizon_client::mod_api::Tag;
pub(crate) type JavaString = <Compound as MapKey>::Key;
pub(crate) trait MapKey {
    type Key;
}
impl<K, V> MapKey for std::collections::BTreeMap<K, V> {
    type Key = K;
}

/// The oven's knobs and bake, saved as the mod saves them (`OvenBlockEntity.saveAdditional`).
fn state(nbt: &Compound) -> State {
    State {
        temperature: int(nbt, "temperature"),
        max_time: int(nbt, "maxCookingTime"),
        cooking: int(nbt, "cookingTime"),
        running: int(nbt, "isRunning") != 0,
    }
}

fn save(entity: &mut ModBlockEntity, state: State) {
    let spec = spec::oven();
    let nbt = &mut entity.nbt;
    for (key, value) in [
        ("temperature", state.temperature),
        ("maxCookingTime", state.max_time),
        ("cookingTime", state.cooking),
    ] {
        nbt.insert(JavaString::from(key), Tag::Int(value));
    }
    nbt.insert(
        JavaString::from("isRunning"),
        Tag::Byte(state.running.into()),
    );
    let mut data = vec![0; 3];
    data[spec.data_slots.temperature] = state.temperature;
    data[spec.data_slots.cooking_time] = state.cooking;
    data[spec.data_slots.max_cooking_time] = state.max_time;
    entity.data = data;
}

/// The oven's recipes as the world has them (data packs included), then the smoking recipes the
/// mod takes over, in the order it looks for them.
fn recipes(host: &dyn Host) -> Vec<oven::Recipe> {
    let spec = spec::oven();
    // The jar writes 1.20's ingredients ({"item"}, {"tag"}, or a list of them, any of which
    // will do); the host reads 26.3's: an id, a #tag, or a list of those.
    fn modern(json: &serde_json::Value) -> serde_json::Value {
        match json {
            serde_json::Value::Array(list) => list
                .iter()
                .flat_map(|j| match modern(j) {
                    serde_json::Value::Array(inner) => inner,
                    one => vec![one],
                })
                .collect(),
            serde_json::Value::Object(o) => match (o.get("item"), o.get("tag")) {
                (Some(item), _) => item.clone(),
                (_, Some(serde_json::Value::String(tag))) => format!("#{tag}").into(),
                _ => serde_json::Value::Null,
            },
            other => other.clone(),
        }
    }
    let place = |json: &serde_json::Value| -> Vec<String> {
        host.recipe_ingredient(&modern(json))
            .map(|i| i.order)
            .unwrap_or_default()
    };
    let own = host
        .recipes_of_type("plerocottage:oven")
        .iter()
        .filter_map(|r| {
            let json = &r.json;
            let output = &json["output"];
            Some(oven::Recipe {
                inputs: json["input"].as_array()?.iter().map(place).collect(),
                container: json.get("container").map(place).unwrap_or_default(),
                output: (
                    output["item"].as_str()?.to_owned(),
                    output["count"].as_u64().unwrap_or(1) as u32,
                ),
                temperature: json["temperature"].as_i64()? as i32,
                time: json["cookingTime"].as_i64()? as i32,
                smoking: false,
            })
        });
    let smoking = spec
        .smoking
        .iter()
        .map(|r| oven::Recipe::from_spec(r, true));
    own.chain(smoking).collect()
}

fn at(pos: [i32; 3]) -> [f64; 3] {
    pos.map(|v| f64::from(v) + 0.5)
}

fn sound(host: &mut dyn Host, pos: [i32; 3], name: &str) {
    host.play_sound(&format!("plerocottage:{name}"), at(pos), BLOCKS, 1.0, 1.0);
}

/// A `GuiGraphics` call the extraction recorded, drawn at the screen's top left `origin`.
fn draw(host: &dyn Host, batch: &mut HudBatch, origin: [f32; 2], call: &[serde_json::Value]) {
    let n = |i: usize| {
        call.get(i)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32
    };
    match call.first().and_then(serde_json::Value::as_str) {
        Some("blit") => {
            // `plerocottage:textures/gui/x.png` is the GUI sprite `plerocottage:gui/x`.
            let texture = call[1].as_str().unwrap_or_default();
            let sprite = texture
                .replacen("textures/", "", 1)
                .trim_end_matches(".png")
                .to_owned();
            // blit(texture, x, y, u, v, w, h) and ImageButton's renderTexture(.., x, y, u, v, w, h, 256, 256)
            let (x, y, u, v, w, h) = (n(2), n(3), n(4), n(5), n(6), n(7));
            host.sprite(
                batch,
                &sprite,
                [u, v, w, h],
                [origin[0] + x, origin[1] + y, w, h],
            );
        }
        Some("text") => {
            let text = call[1].as_str().unwrap_or_default();
            let colour = call[4].as_i64().unwrap_or(-1) as u32 & 0xFF_FFFF;
            // GuiGraphics.drawString(font, text, x, y, colour) draws a shadow.
            host.text(
                batch,
                text,
                [origin[0] + n(2), origin[1] + n(3)],
                colour,
                true,
            );
        }
        _ => {}
    }
}

struct Shown {
    temperature: i32,
    max_time: i32,
    cooking: i32,
    filled: bool,
}

impl Shown {
    fn of(data: &[i32]) -> Self {
        let spec = spec::oven();
        let get = |i: usize| data.get(i).copied().unwrap_or(0);
        Self {
            temperature: get(spec.data_slots.temperature),
            max_time: get(spec.data_slots.max_cooking_time),
            cooking: get(spec.data_slots.cooking_time),
            filled: RESULT_SHOWN.load(Ordering::Relaxed),
        }
    }
}

impl Hooks for Plerocottage {
    fn block_entity_submit(&self, host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
        if mobs::holding(event.block).is_some() {
            mobs::draw_held(host, event);
            return;
        }
        if event.block == machines::cutting_board::BLOCK {
            glue::draw_board(host, event);
            return;
        }
        display::draw_block(host, event);
    }

    fn block_entity_render_bounding_box(
        &self,
        host: &dyn Host,
        event: &mut BlockEntityRenderBoundingBoxEvent<'_>,
    ) {
        display::block_bounds(host, event);
    }

    fn item_submit(&self, host: &dyn Host, event: &mut ItemSubmitEvent<'_>) -> Flow {
        if held::draw(host, event) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn mob_submit(&self, host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> Flow {
        if display::draw_mob(host, event) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn client_resources_reloaded(&self, _host: &mut dyn Host, _event: &mut ClientResourcesReloadedEvent) {
        display::reloaded();
    }

    fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
        mobs::register_goals(event);
    }

    fn living_incoming_damage_v3(&self, event: &mut LivingIncomingDamageV3Event<'_>) -> Flow {
        if mobs::refuses(event.kind, event.damage_type) || effects::incoming(event) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    // ---- The mod's mob effects (effects.rs) ----

    fn should_apply_effect_tick_this_tick(&self, _host: &dyn Host, event: &mut ShouldApplyEffectTickThisTickEvent<'_>) {
        effects::ticks_now(event);
    }

    fn apply_effect_tick(&self, host: &dyn Host, event: &mut ApplyEffectTickEvent<'_>) {
        effects::applied(host, event);
    }

    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        effects::swung(host, event);
        Flow::Continue
    }

    fn critical_hit(&self, _host: &mut dyn Host, event: &mut CriticalHitEvent<'_>) {
        effects::critical(event);
    }

    fn living_incoming_damage_v2(&self, _host: &mut dyn Host, event: &mut LivingIncomingDamageV2Event<'_>) -> Flow {
        effects::weapon(event);
        Flow::Continue
    }

    fn living_damage_post(&self, event: &mut LivingDamagePostEvent<'_>) {
        effects::landed(event);
    }

    fn living_experience_drop(&self, host: &mut dyn Host, event: &mut LivingExperienceDropEvent<'_>) {
        effects::experience(host, event);
    }

    fn living_death(&self, host: &mut dyn Host, event: &mut LivingDeathEvent<'_>) -> Flow {
        effects::died(host, event);
        Flow::Continue
    }

    fn interact_entity_v2(&self, host: &mut dyn Host, event: &mut InteractEntityV2Event<'_>) -> Flow {
        if matches!(event.hand, Hand::Main)
            && let Ok(entity) = u32::try_from(event.entity)
        {
            mobs::feed(host, entity, event.stack);
        }
        if mobs::refuses_bucket(event.kind, event.stack) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        if mobs::use_thrown(host, event.stack, true) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn use_item(&self, host: &mut dyn Host, event: &mut UseItemEvent<'_>) -> Flow {
        if mobs::use_thrown(host, event.stack, false) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn projectile_impact(&self, host: &mut dyn Host, event: &mut ProjectileImpactEvent<'_>) -> Flow {
        mobs::egg_hit(host, event);
        Flow::Continue
    }

    fn hurt_enemy(&self, host: &mut dyn Host, event: &mut HurtEnemyEvent<'_>) {
        combat::hit(host, event.stack);
        // `AlfalfaItem.hurtEnemy` feeds what it hits.
        mobs::feed(host, event.entity, Some(event.stack));
    }

    // ---- Plants (growth.rs) and how blocks are turned when placed (placing.rs) ----

    fn random_tick(&self, level: &mut dyn LevelWriteHost, event: &mut RandomTickEvent) -> Flow {
        if growth::random_tick(level, event.pos, event.state) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn is_valid_bonemeal_target(
        &self,
        level: &dyn LevelHost,
        event: &mut IsValidBonemealTargetEvent,
    ) -> Option<bool> {
        growth::bonemeal_target(level, event.state)
    }

    fn is_bonemeal_success(&self, level: &mut dyn LevelRandomHost, event: &mut IsBonemealSuccessEvent) {
        if let Some(success) = growth::bonemeal_success(level, event.pos, event.state) {
            event.success = success;
        }
    }

    fn perform_bonemeal(&self, level: &mut dyn LevelWriteHost, event: &mut PerformBonemealEvent) {
        growth::bonemeal(level, event.pos, event.state);
    }

    fn may_place_on(&self, level: &dyn LevelHost, event: &mut MayPlaceOnEvent) -> Option<bool> {
        growth::may_place_on(level, event.state, event.ground)
    }

    fn can_survive(&self, level: &dyn LevelHost, event: &mut CanSurviveEvent) -> Option<bool> {
        growth::can_survive(level, event.pos, event.state)
    }

    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
        growth::flush_drops(host);
        shaker::apply(host);
        effects::tick(host, event.dimension);
        flatten::tick(host, event.dimension);
        mobs::flush(host, event.dimension);
    }

    fn mob_effect_added(&self, host: &mut dyn Host, event: &mut MobEffectAddedEvent<'_>) {
        flatten::added(event.effect, event.entity);
        effects::added(host, event.effect, event.entity, event.amplifier);
    }

    fn net_from_server(&self, _host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        flatten::heard(event.channel, event.data);
    }

    fn living_render_scale(&self, _host: &dyn Host, event: &mut LivingRenderScaleEvent<'_>) {
        flatten::scale(event.entity, event.player, event.scale);
    }

    fn enchant_offers(&self, _host: &dyn Host, event: &mut EnchantOffersEvent<'_>) {
        enchanting::offers(event.item, event.offers);
    }

    fn enchant_click(&self, host: &mut dyn Host, event: &mut EnchantClickEvent<'_>) {
        if event.taken.is_none() {
            *event.taken = enchanting::clicked(host, event.pos, event.button, event.creative, event.slots);
        }
    }

    fn state_for_placement(
        &self,
        level: &dyn LevelHost,
        event: &mut StateForPlacementEvent<'_>,
    ) -> Option<Option<BlockStateId>> {
        placing::state(level, event.state, event.context.yaw).map(Some)
    }

    fn block_entity(
        &self,
        _host: &mut dyn Host,
        event: &mut BlockEntityEvent<'_>,
    ) -> Option<(String, Compound)> {
        if mobs::holding(event.block).is_some() {
            // A nest: it keeps what lies in it, in its slots.
            return Some((event.block.to_owned(), Compound::new()));
        }
        if let Some(kind) = machines::of(event.block) {
            let mut entity = ModBlockEntity::default();
            glue::store(kind, &mut entity, &kind.new_unit());
            return Some((event.block.to_owned(), entity.nbt.clone()));
        }
        if event.block == machines::cutting_board::BLOCK {
            let mut entity = ModBlockEntity::default();
            let unit = machines::cutting_board::new_unit(spec::cutting_board());
            glue::store_board(&mut entity, &unit);
            return Some((event.block.to_owned(), entity.nbt.clone()));
        }
        if display::is_block(event.block) {
            // A `GeoBlockEntity` that keeps nothing: it is there to be drawn.
            return Some((event.block.to_owned(), Compound::new()));
        }
        (event.block == oven::BLOCK).then(|| {
            let defaults = &spec::oven().defaults;
            let mut entity = ModBlockEntity::default();
            save(
                &mut entity,
                State {
                    temperature: defaults.temperature,
                    max_time: defaults.max_cooking_time,
                    ..State::default()
                },
            );
            (oven::BLOCK.to_owned(), entity.nbt.clone())
        })
    }

    fn block_entity_ticks(&self, _host: &dyn Host, event: &mut BlockEntityTicksEvent<'_>) {
        if event.block == oven::BLOCK || machines::of(event.block).is_some() {
            event.ticks = true;
        }
    }

    fn block_entity_container(
        &self,
        _host: &dyn Host,
        event: &mut BlockEntityContainerEvent<'_>,
    ) -> Option<ModContainer> {
        if let Some(nest) = mobs::holding(event.block) {
            // No menu and no side a hopper reaches: things are laid in it by hand, and by its birds.
            return Some(ModContainer {
                slots: nest.size,
                ..ModContainer::default()
            });
        }
        if let Some(kind) = machines::of(event.block) {
            return Some(ModContainer {
                slots: kind.spec().size,
                menu: Some(event.block.to_owned()),
                ..ModContainer::default()
            });
        }
        if event.block == machines::cutting_board::BLOCK {
            // No menu: things are laid on it and taken off by hand.
            return Some(ModContainer {
                slots: spec::cutting_board().size,
                ..ModContainer::default()
            });
        }
        (event.block == oven::BLOCK).then(|| ModContainer {
            slots: spec::oven().size,
            menu: Some(oven::BLOCK.to_owned()),
            ..ModContainer::default()
        })
    }

    fn right_click_block_v2(&self, host: &mut dyn Host, event: &mut RightClickBlockV2Event) -> Flow {
        if host.block(event.pos) == machines::cutting_board::BLOCK
            && glue::use_board(host, event.pos, matches!(event.hand, Hand::Main))
        {
            return Flow::Cancel;
        }
        if mobs::use_held(host, event.pos, matches!(event.hand, Hand::Main)) {
            return Flow::Cancel;
        }
        if matches!(event.hand, Hand::Main) && growth::used(host, event.pos) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn block_entity_fluids(
        &self,
        _host: &dyn Host,
        event: &mut BlockEntityFluidsEvent<'_>,
    ) -> Option<ModFluidTanks> {
        let tank = machines::of(event.block)?.spec().tank.as_ref()?;
        Some(ModFluidTanks {
            tanks: vec![tank.capacity],
            faces: std::array::from_fn(|_| vec![0]),
            inputs: vec![0],
            outputs: vec![0],
        })
    }

    fn block_entity_accepts_fluid(
        &self,
        _host: &dyn Host,
        event: &mut BlockEntityAcceptsFluidEvent<'_>,
    ) -> Flow {
        // FluidTank's validator: the fryer takes oil, the ice cream machine its recipes' fluids.
        match machines::of(event.block).and_then(|k| k.spec().tank.as_ref()) {
            Some(tank) if !tank.accepts.is_empty() && !tank.accepts.iter().any(|f| f == event.fluid) => {
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }

    fn block_entity_update_tag(&self, event: &mut BlockEntityUpdateTagEvent<'_>) -> Option<Compound> {
        if mobs::holding(&event.entity.id).is_some() {
            return Some(mobs::held_tag(event.entity));
        }
        if event.entity.id == machines::cutting_board::BLOCK {
            // What lies on it, for its renderer: a client's copy has no slots.
            return Some(glue::board_tag(event.entity));
        }
        // What a screen reads of the block entity itself: whether it runs, its tank.
        machines::of(&event.entity.id).map(|_| event.entity.nbt.clone())
    }

    fn block_entity_tick(&self, host: &mut dyn Host, event: &mut BlockEntityTickEvent<'_>) {
        if let Some(kind) = machines::of(&event.entity.id) {
            let before = glue::load(kind, event.entity);
            let mut unit = before.clone();
            kind.tick(&mut unit, &mut glue::HostEnv::new(host, event.pos, Some(kind.spec())));
            if unit != before || event.entity.data.len() != kind.spec().data_count() {
                glue::store(kind, event.entity, &unit);
                if unit.running != before.running || unit.tank != before.tank {
                    host.block_entity_sync(event.pos);
                }
            }
            return;
        }
        if event.entity.id != oven::BLOCK {
            return;
        }
        let mut state = state(&event.entity.nbt);
        let before = state;
        if state.running {
            let recipes = recipes(host);
            let spec = spec::oven();
            let mut items = event.entity.items.clone();
            items.resize(spec.size, None);
            if oven::tick(&mut state, &mut items, &recipes, spec) {
                event.entity.items = items;
                sound(host, event.pos, "oven_ding");
            }
        }
        if state.running != before.running {
            host.set_block_property(
                event.pos,
                "running",
                if state.running { "true" } else { "false" },
            );
        }
        if state != before || event.entity.data.len() != 3 {
            save(event.entity, state);
        }
    }

    fn menu_button(&self, host: &mut dyn Host, event: &mut MenuButtonEvent<'_>) {
        if let Some(kind) = machines::of(&event.entity.id) {
            let shown = kind.spec().screen.as_ref();
            let packets = shown.map(|s| &s.packets[..]).unwrap_or_default();
            let Some(packet) = usize::try_from(event.id).ok().and_then(|id| packets.get(id)) else {
                // A press that only tells the player something (`displayClientMessage(.., true)`).
                let said = shown.map(screen::says).unwrap_or_default();
                let key = usize::try_from(event.id).ok().and_then(|id| said.get(id.checked_sub(packets.len())?).copied());
                if let (Some(key), Some(player)) = (key, host.acting_player()) {
                    host.action_bar(player.uuid, Text::translate(key, &[]));
                }
                return;
            };
            let before = glue::load(kind, event.entity);
            let mut unit = before.clone();
            kind.packet(&mut unit, &mut glue::HostEnv::new(host, event.pos, Some(kind.spec())), packet);
            if unit != before {
                glue::store(kind, event.entity, &unit);
                host.block_entity_sync(event.pos);
            }
            return;
        }
        if event.entity.id != oven::BLOCK {
            return;
        }
        let spec = spec::oven();
        let mut state = state(&event.entity.nbt);
        match event.id {
            // The mod's knobs send nothing while it bakes.
            TEMPERATURE_KNOB | TIME_KNOB if state.cooking == 0 && !state.running => {
                if event.id == TEMPERATURE_KNOB {
                    state.temperature = oven::turned(spec, true, state.temperature);
                } else {
                    state.max_time = oven::turned(spec, false, state.max_time);
                }
                sound(host, event.pos, "oven_knob_turn");
            }
            START => {
                let mut items = event.entity.items.clone();
                items.resize(spec.size, None);
                if oven::start(&mut state, &items, &recipes(host), spec) {
                    host.set_block_property(event.pos, "running", "true");
                }
            }
            _ => return,
        }
        save(event.entity, state);
    }

    fn mod_container_viewers(&self, host: &mut dyn Host, event: &mut ModContainerViewersEvent<'_>) {
        // ContainerOpenersCounter: the door opens for the first viewer and shuts after the last.
        let open = event.viewers > 0;
        if open == (event.previous > 0) {
            return;
        }
        let block = host.block(event.pos);
        if let Some(kind) = machines::of(&block) {
            if let Some(entity) = host.mod_block_entity(event.pos) {
                let unit = glue::load(kind, &entity);
                kind.opened(&unit, &mut glue::HostEnv::new(host, event.pos, Some(kind.spec())), open);
            }
            return;
        }
        if block != oven::BLOCK {
            return;
        }
        host.set_block_property(event.pos, "open", if open { "true" } else { "false" });
        sound(host, event.pos, "oven_open");
    }

    // ---- The shaker (shaker.rs) ----

    fn inventory_click(&self, host: &mut dyn Host, event: &mut InventoryClickEvent<'_>) -> Flow {
        if shaker::clicked(host, event) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn use_duration(&self, _host: &dyn Host, event: &mut UseDurationEvent<'_>) {
        if shaker::is(event.stack) {
            event.duration = spec::shaker().use_duration;
        }
    }

    fn use_animation(&self, _host: &dyn Host, event: &mut UseAnimationEvent<'_>) {
        if shaker::is(event.stack)
            && let Some(animation) = shaker::animation()
        {
            *event.animation = animation;
        }
    }

    fn living_entity_use_item_start(&self, host: &mut dyn Host, event: &mut LivingEntityUseItemStartEvent<'_>) -> Flow {
        if shaker::is(event.stack) && !shaker::may_start(host, event.stack, matches!(event.hand, Hand::Main)) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn living_entity_use_item_finish(&self, host: &mut dyn Host, event: &mut LivingEntityUseItemFinishEvent<'_>) {
        if shaker::is(event.stack)
            && matches!(event.hand, Hand::Main)
            && let Some(shaken) = shaker::finish(host, event.stack)
        {
            *event.result = Some(shaken);
        }
    }

    fn menu_slot_state(&self, host: &dyn Host, event: &mut MenuSlotStateEvent<'_>) {
        slots::state(host, event);
    }

    fn menu_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
        if let Some(kind) = machines::of(event.menu) {
            let Some(slot) = kind.spec().menu.iter().find(|s| s.index == event.slot) else {
                return;
            };
            if let spec::Placed::Items(items) = &slot.may_place.standing {
                event.may_place &= items.iter().any(|item| **item == *event.stack.item);
            }
            event.max_stack = event.max_stack.min(slot.max_stack);
            return;
        }
        if event.menu != oven::BLOCK {
            return;
        }
        let Some(slot) = spec::oven().menu.iter().find(|s| s.index == event.slot) else {
            return;
        };
        if let Some(place) = &slot.may_place {
            event.may_place &= place
                .standing
                .iter()
                .any(|item| **item == *event.stack.item);
        }
        event.max_stack = event.max_stack.min(slot.max_stack);
    }

    fn client_tick_post(&self, host: &mut dyn Host, _event: &mut ClientTickPostEvent) {
        let output = oven::output_slot(spec::oven());
        let stacks = host.menu_stacks();
        if let Ok(mut seen) = screen::STACKS.lock() {
            *seen = stacks
                .iter()
                .flatten()
                .map(|s| s.as_ref().map(|s| s.item.to_string()))
                .collect();
        }
        if stacks.is_none() {
            // No menu is open: the next screen starts with its switches as a new screen has them.
            if let Ok(mut toggles) = screen::TOGGLES.lock() {
                *toggles = None;
            }
        }
        let filled = stacks
            .is_some_and(|s| s.get(output).is_some_and(Option::is_some));
        RESULT_SHOWN.store(filled, Ordering::Relaxed);
        sounds::tick(host);
    }

    fn screen(&self, _host: &dyn Host, event: &mut ScreenEvent<'_>) -> Option<NativeScreen> {
        if let Some(kind) = machines::of(event.block) {
            let spec = kind.spec();
            let shown = spec.screen.as_ref()?;
            let mut slots = vec![None; spec.size];
            for slot in &spec.menu {
                slots[slot.index] = Some([slot.x, slot.y]);
            }
            return Some(NativeScreen {
                width: shown.width,
                height: shown.height,
                slots,
                inventory_x: spec.inventory.x,
                inventory_y: spec.inventory.y,
                // BaseMachineScreen.renderLabels draws nothing, and the screen's picture is its background.
                own_labels: true,
                own_background: true,
            });
        }
        if event.block != oven::BLOCK {
            return None;
        }
        let spec = spec::oven();
        let mut slots = vec![None; spec.size];
        for slot in &spec.menu {
            slots[slot.index] = Some([slot.x, slot.y]);
        }
        Some(NativeScreen {
            width: spec.screen.width,
            height: spec.screen.height,
            slots,
            inventory_x: spec.inventory.x,
            inventory_y: spec.inventory.y,
            // OvenScreen.renderLabels draws nothing, and its picture is its background.
            own_labels: true,
            own_background: true,
        })
    }

    fn screen_draw(&self, host: &dyn Host, event: &mut ScreenDrawEvent<'_>) {
        if let Some(kind) = machines::of(event.block) {
            if let Some(shown) = &kind.spec().screen {
                let seen = screen::Seen::of(host, kind.spec(), event.data);
                screen::draw_screen(host, event.batch, event.origin, shown, &seen);
            }
            return;
        }
        if event.block != oven::BLOCK {
            return;
        }
        let spec = spec::oven();
        let shown = Shown::of(event.data);
        let origin = event.origin;
        draw(host, event.batch, origin, &spec.screen.background);
        // OvenScreen.renderBg's parts, in its order, as drawn in this state.
        for part in [
            "renderTemperature",
            "renderMaxCookingTime",
            "renderRunning",
            "renderInputSlot",
            "renderOutputSlot",
            "renderContainerSlot",
        ] {
            if part == "renderMaxCookingTime" {
                if let Some(text) = time_text(spec, shown.max_time, shown.cooking) {
                    draw(host, event.batch, origin, text);
                }
                continue;
            }
            let found = spec.screen.parts[part].iter().find(|p| {
                p.temperature == shown.temperature
                    && p.max_cooking_time == shown.max_time
                    && p.filled == shown.filled
                    && p.cooking_time == shown.cooking.min(1)
            });
            for call in found.map(|p| &p.draws[..]).unwrap_or_default() {
                draw(host, event.batch, origin, call);
            }
        }
        for button in &spec.screen.buttons {
            if let Some(state) = button_state(button, &shown) {
                for call in &state.draws {
                    draw(host, event.batch, origin, call);
                }
            }
        }
    }

    fn screen_click(&self, host: &dyn Host, event: &mut ScreenClickEvent<'_>) -> Option<i32> {
        if event.right {
            return None;
        }
        if let Some(kind) = machines::of(event.block) {
            let shown = kind.spec().screen.as_ref()?;
            let seen = screen::Seen::of(host, kind.spec(), event.data);
            return screen::click(shown, &seen, event.at);
        }
        if event.block != oven::BLOCK {
            return None;
        }
        let shown = Shown::of(event.data);
        let [x, y] = event.at;
        spec::oven().screen.buttons.iter().find_map(|button| {
            let inside = x >= button.x
                && y >= button.y
                && x < button.x + button.width
                && y < button.y + button.height;
            let sends = button_state(button, &shown).is_some_and(|s| !s.sends.is_empty());
            (inside && sends).then(|| match button.knob.as_str() {
                Some("temperature") => TEMPERATURE_KNOB,
                Some(_) => TIME_KNOB,
                None => START,
            })
        })
    }
}

/// The text the time knob shows: the last change at or before `cooking`.
fn time_text<'a>(spec: &'a spec::Oven, max_time: i32, cooking: i32) -> Option<&'a spec::Draw> {
    let changes = spec.screen.time_text.get(&max_time.to_string())?;
    changes
        .iter()
        .rev()
        .find(|(from, _)| *from <= cooking)
        .map(|(_, draw)| draw)
}

fn button_state<'a>(button: &'a spec::Button, shown: &Shown) -> Option<&'a spec::ButtonState> {
    button.states.iter().find(|s| {
        s.temperature == shown.temperature
            && s.max_cooking_time == shown.max_time
            && s.cooking_time == shown.cooking.min(1)
            && s.filled == shown.filled
    })
}

//! What pleromod's extraction found by running the mod's own classes (`tools/mod-content/native.py`):
//! the oven's slots, settings, screen and recipes. Nothing here is typed in by hand.
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Oven {
    pub size: usize,
    pub slots: HashMap<String, usize>,
    /// The order a recipe's places fill the input slots in (`OvenBlockEntity.INPUT_SLOTS`).
    pub input_slots: Vec<usize>,
    pub defaults: Settings,
    pub menu: Vec<Slot>,
    /// What makes the slots answer as while it bakes (`OvenMenu.isIdle`).
    #[serde(default)]
    pub busy: Vec<Busy>,
    /// The sound that goes on while it bakes.
    #[serde(default, rename = "loop")]
    pub sound_loop: Option<SoundLoop>,
    pub inventory: Inventory,
    pub data_slots: DataSlots,
    pub settings: Knobs,
    pub knobs: KnobValues,
    pub screen: Screen,
    #[cfg_attr(not(test), allow(dead_code))]
    pub recipes: Vec<Recipe>,
    pub smoking: Vec<Recipe>,
    pub items: HashMap<String, Item>,
    pub failure: String,
    #[cfg_attr(not(test), allow(dead_code))]
    #[serde(default)]
    pub runs: HashMap<String, Run>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub temperature: i32,
    pub max_cooking_time: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub max_stack: u32,
    pub may_pickup: States<bool>,
    #[serde(default)]
    pub may_place: Option<States<Vec<String>>>,
    #[serde(default)]
    pub active: Option<States<Active>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSlots {
    pub temperature: usize,
    pub cooking_time: usize,
    pub max_cooking_time: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Knobs {
    pub temperature: Vec<Setting>,
    pub max_cooking_time: Vec<Setting>,
}

#[derive(Debug, Deserialize)]
pub struct Setting {
    pub state: i32,
    pub value: i32,
}

/// What the block entity's setter makes of the number a knob sends, by that number.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnobValues {
    pub temperature: HashMap<String, i32>,
    pub max_cooking_time: HashMap<String, i32>,
}

#[derive(Debug, Deserialize)]
pub struct Screen {
    pub width: f32,
    pub height: f32,
    pub background: Draw,
    pub parts: HashMap<String, Vec<Part>>,
    /// The time knob's text, by the time set: from which tick of the bake each text shows.
    #[serde(rename = "timeText")]
    pub time_text: HashMap<String, Vec<(i32, Draw)>>,
    pub buttons: Vec<Button>,
}

/// One call of the screen's `GuiGraphics`: `["blit", texture, x, y, u, v, w, h, ...]` or
/// `["text", text, x, y, colour]`.
pub type Draw = Vec<serde_json::Value>;

/// What a part of the screen draws in one state of the oven.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub temperature: i32,
    pub max_cooking_time: i32,
    pub filled: bool,
    pub cooking_time: i32,
    pub draws: Vec<Draw>,
}

#[derive(Debug, Deserialize)]
pub struct Button {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// `"temperature"` or `"maxCookingTime"` for a knob, `false` for the start button.
    pub knob: serde_json::Value,
    pub states: Vec<ButtonState>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonState {
    pub temperature: i32,
    pub max_cooking_time: i32,
    pub cooking_time: i32,
    pub filled: bool,
    pub draws: Vec<Draw>,
    pub sends: Vec<serde_json::Map<String, serde_json::Value>>,
}

/// Only the tests read `recipes`, `runs`, a recipe's `file` and an item's `max_stack`: the game
/// has its own recipes and stack sizes.
/// A recipe as the mod's serializer reads it: each place's items (the first is what the runs
/// put in), the container, the result and the setting it wants.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    #[cfg_attr(not(test), allow(dead_code))]
    pub file: String,
    pub input: Vec<Vec<String>>,
    #[serde(default)]
    pub container: Vec<String>,
    pub output: (String, u32),
    pub temperature: i32,
    pub cooking_time: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[cfg_attr(not(test), allow(dead_code))]
    pub max_stack: u32,
    pub max_damage: u32,
    #[serde(default)]
    pub remainder: Option<String>,
}

/// A bake measured beyond `machines.json`: the slots before and after. Only the tests bake them.
#[derive(Debug, Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Run {
    pub items: HashMap<String, Vec<serde_json::Value>>,
    pub temperature: i32,
    pub time: i32,
    pub started: bool,
    pub ticks: u32,
    pub after: HashMap<String, Vec<serde_json::Value>>,
}

pub fn oven() -> &'static Oven {
    static OVEN: OnceLock<Oven> = OnceLock::new();
    OVEN.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data/oven.json"
        )))
        .expect("data/oven.json is what native.py writes")
    })
}

/// A machine other than the oven, as native.py's shared capture writes it (`machine_spec`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub block: String,
    pub size: usize,
    /// The block entity's constants by their names: its slots, its times.
    pub constants: HashMap<String, f64>,
    /// Its lists of slots (`INPUT_SLOTS`), in its order.
    pub slot_lists: HashMap<String, Vec<usize>>,
    /// A new block entity's numbers.
    pub fields: HashMap<String, i32>,
    pub menu: Vec<MenuSlot>,
    /// What makes the slots answer as while it works (the menu's `isIdle`).
    #[serde(default)]
    pub busy: Vec<Busy>,
    /// The sound that goes on while it works.
    #[serde(default, rename = "loop")]
    pub sound_loop: Option<SoundLoop>,
    /// How loud and how high the mod plays each sound, where its code says.
    #[serde(default)]
    pub sound_levels: HashMap<String, Level>,
    pub inventory: Inventory,
    pub data_slots: HashMap<String, usize>,
    /// Only the tests read these: the game has its own recipes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub recipes: HashMap<String, Vec<Recipe2>>,
    #[serde(default)]
    pub smoking: Vec<Recipe2>,
    /// The blocks that heat it from below: `true`, or by a property (`{"none": .., "running": {"true": ..}}`).
    #[serde(default)]
    pub heat: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub tank: Option<Tank>,
    /// What the machine makes of what it spoils.
    #[serde(default)]
    pub failure: Option<String>,
    #[serde(default)]
    pub screen: Option<Screen2>,
    #[cfg_attr(not(test), allow(dead_code))]
    #[serde(default)]
    pub runs: HashMap<String, serde_json::Value>,
}

impl Machine {
    pub fn slot(&self, name: &str) -> usize {
        self.constants[name] as usize
    }

    pub fn number(&self, name: &str) -> i32 {
        self.constants[name] as i32
    }

    pub fn slots(&self, name: &str) -> &[usize] {
        &self.slot_lists[name]
    }

    pub fn data_count(&self) -> usize {
        self.data_slots.values().max().map_or(0, |m| m + 1)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSlot {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub max_stack: u32,
    pub may_pickup: States<bool>,
    pub may_place: States<Placed>,
    /// Whether the slot is there (`Slot.isActive`), when not always.
    #[serde(default)]
    pub active: Option<States<Active>>,
}

/// What a slot answers while its machine stands and while it works.
#[derive(Debug, Deserialize)]
pub struct States<T> {
    pub standing: T,
    pub cooking: T,
}

/// One of the things any of which makes a machine's slots answer as while it works.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Busy {
    /// The block entity's `isRunning`.
    Running,
    /// A data slot of the menu that is not nought.
    Data { index: usize },
}

/// A sound that repeats while a machine runs (`AbstractTickableSoundInstance`), as its class makes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundLoop {
    pub sound: String,
    pub volume: f32,
    pub pitch: f32,
    /// Where it sounds, from the block's corner.
    pub at: [f64; 3],
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Level {
    pub volume: f32,
    pub pitch: f32,
}

/// What a slot takes: `"any"`, or the items.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Placed {
    Any(String),
    Items(Vec<String>),
}

impl Placed {
    pub fn nothing(&self) -> bool {
        matches!(self, Placed::Items(items) if items.is_empty())
    }
}

/// Whether a slot is there while it is empty and while it is filled.
#[derive(Debug, Deserialize)]
pub struct Active {
    pub empty: bool,
    pub filled: bool,
}

#[derive(Debug, Deserialize)]
pub struct Tank {
    pub capacity: i32,
    #[serde(default)]
    pub accepts: Vec<String>,
    pub buckets: HashMap<String, String>,
}

/// A recipe of any machine as the mod's serializer reads it (native.py's `recipe`): the fields it
/// has by the mod's names, the others empty. A place's list is the items that will do there.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Recipe2 {
    pub file: String,
    pub output: Option<(String, u32)>,
    pub container: Vec<String>,
    pub main_input: Vec<Vec<String>>,
    pub secondary_input: Vec<Vec<String>>,
    #[serde(deserialize_with = "places")]
    pub input: Vec<Vec<String>>,
    pub outgrowth: Option<(String, u32)>,
    pub outgrowth_chance: f32,
    pub outgrowths: Vec<((String, u32), f32)>,
    pub max_cooking_time: i32,
    pub heat_state: Option<String>,
    pub light_state: Option<String>,
    pub is_tow_flavor: i32,
    pub fluid_stack: Option<Fluid>,
    pub tool: Vec<String>,
    pub is_shaped: i32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Fluid {
    pub fluid: String,
    pub amount: i32,
}

/// One ingredient (the fryer's `input`) reads as one place.
fn places<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Vec<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        One(Vec<String>),
        Many(Vec<Vec<String>>),
    }
    Ok(match Either::deserialize(d)? {
        Either::One(items) if items.is_empty() => Vec::new(),
        Either::One(items) => vec![items],
        Either::Many(places) => places,
    })
}

/// What the machines ask of an item (native.py's `item_facts`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub max_stack: u32,
    pub max_damage: u32,
    #[serde(default)]
    pub remainder: Option<String>,
    /// The tests of the mod's `PleroCottageCompat` it passes, without their `is`.
    #[serde(default)]
    pub is: Vec<String>,
}

/// A machine's screen: what it draws, in order, and its buttons (native.py's `machine_screen`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen2 {
    pub width: f32,
    pub height: f32,
    pub layers: Vec<Layer>,
    pub buttons: Vec<Button2>,
    /// What a press may send, by the number the menu's button carries.
    pub packets: Vec<String>,
}

/// What a state of the machine is read by: a data slot, the kind of item in a slot, the block below, the light, the
/// fluid in the tank and how much, whether it runs, a switch of the screen's own that a button turns.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Factor {
    Data {
        index: usize,
    },
    /// The class of the item in `slot`: `classes` by item, `other` for the rest, 0 for none.
    Slot {
        slot: usize,
        classes: HashMap<String, i64>,
        other: i64,
    },
    Heat,
    Light,
    Running,
    Toggle {
        name: String,
    },
    /// The tank's millibuckets.
    Tank,
    /// The class of the tank's fluid, 0 for none.
    Fluid {
        classes: HashMap<String, i64>,
        other: i64,
    },
}

/// What is drawn while each factor has the case's value, or lies in its range (`[from, to]`).
#[derive(Debug, Deserialize)]
pub struct Case<T> {
    pub when: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub then: T,
}

#[derive(Debug, Deserialize)]
pub struct Draws {
    pub draws: Vec<Draw>,
}

#[derive(Debug, Deserialize)]
pub struct Layer {
    pub factors: Vec<Factor>,
    pub cases: Vec<Case<Draws>>,
}

#[derive(Debug, Deserialize)]
pub struct Pressed {
    /// Whether a click there reaches the button.
    pub active: bool,
    #[serde(default)]
    pub sends: Vec<String>,
    /// What a press tells the player above the hotbar instead: translation keys.
    #[serde(default)]
    pub says: Vec<String>,
    #[serde(default)]
    pub toggles: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Button2 {
    /// Where a click presses it: rectangles `[x, y, width, height]`.
    pub hit: Vec<[f32; 4]>,
    pub factors: Vec<Factor>,
    pub cases: Vec<Case<Pressed>>,
}

/// The cutting board, which has no menu (native.py's `cutting_board`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub size: usize,
    pub fields: HashMap<String, i32>,
    /// What `CuttingBoardBlock.isTool` takes for a tool.
    pub tools: Vec<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub recipes: HashMap<String, Vec<Recipe2>>,
    /// Clicks on the mod's own block, for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    #[serde(default)]
    pub uses: HashMap<String, serde_json::Value>,
    /// What lies on it as its renderer draws it, by the way the block faces: the calls on the pose stack in order
    /// (`["translate", x, y, z]`, `["rotate", axis, degrees]`, `["scale", x, y, z]`, `["push"]`, `["pop"]`) and
    /// `["item", place, display]` where the thing in a place is drawn.
    #[serde(default)]
    pub drawn: HashMap<String, Vec<Draw>>,
    /// How loud and how high the mod plays each sound, where its code says.
    #[serde(default, rename = "soundLevels")]
    pub sound_levels: HashMap<String, Level>,
}

/// The shaker, an item (native.py's `shaker`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shaker {
    pub item: String,
    pub constants: HashMap<String, f64>,
    /// `Item.getUseAnimation`, by its name in the game.
    pub use_animation: String,
    /// `Item.getUseDuration`, in ticks.
    pub use_duration: i32,
    /// Only the tests read these: the game has its own recipes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub recipes: HashMap<String, Vec<Recipe2>>,
}

macro_rules! data {
    ($name:ident, $kind:ty, $file:literal) => {
        pub fn $name() -> &'static $kind {
            static IT: OnceLock<$kind> = OnceLock::new();
            IT.get_or_init(|| {
                serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/", $file)))
                    .unwrap_or_else(|e| panic!("data/{} is what native.py writes: {e}", $file))
            })
        }
    };
}

data!(blender, Machine, "blender.json");
data!(stock_pot, Machine, "stock_pot.json");
data!(teapot, Machine, "teapot.json");
data!(saucepan_and_whisk, Machine, "saucepan_and_whisk.json");
data!(fryer, Machine, "fryer.json");
data!(ice_cream_machine, Machine, "ice_cream_machine.json");
data!(fermenter, Machine, "fermenter.json");
data!(cutting_board, Board, "cutting_board.json");
data!(shaker, Shaker, "shaker.json");
data!(facts, HashMap<String, Fact>, "items.json");

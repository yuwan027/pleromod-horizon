//! How the mod's plants grow: `data/growth.json`, which pleromod's extraction measured by running
//! each plant's own code state by state (`tools/mod-content/horizon_growth.py`), followed here.
//!
//! A state has a rule for each thing the game does to it: a random tick, bone meal (and whether
//! it works), a use by hand. A rule is a tree of the random numbers the plant's code draws, in
//! its order, with what happens at each end; which tree depends on the light the code asks for
//! and on whether the plant is enclosed. Nothing here knows a plant.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

mod level;
#[cfg(test)]
mod tests;

pub use level::{bonemeal, bonemeal_success, bonemeal_target, can_survive, flush_drops, may_place_on, random_tick, used};

#[derive(Deserialize)]
pub struct Growth {
    pub blocks: BTreeMap<String, Plant>,
    pub saplings: BTreeMap<String, Sapling>,
    #[serde(default)]
    pub grounds: Grounds,
}

/// The grounds the extraction stood each plant on, and those its own `canSurvive` took.
#[derive(Deserialize, Default)]
pub struct Grounds {
    pub asked: Vec<String>,
    pub plants: BTreeMap<String, Vec<String>>,
}

impl Grounds {
    /// Whether a plant stands on a ground, where the extraction asked: none for a ground it didn't
    /// ask about, and for a plant that took none of them (its ground is one of the mod's own, which
    /// its template decides as before).
    pub fn stands(&self, plant: &str, ground: &str) -> Option<bool> {
        let on = self.plants.get(plant).filter(|on| !on.is_empty())?;
        self.asked.iter().any(|asked| asked == ground).then(|| on.iter().any(|taken| taken == ground))
    }
}

#[derive(Deserialize)]
pub struct Plant {
    /// The number of its rules by state, as `age=3,rare=false` with the names in order.
    pub states: BTreeMap<String, usize>,
    pub rules: Vec<Rules>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    pub random_tick: Option<Rule>,
    pub perform_bonemeal: Option<Rule>,
    pub is_bonemeal_success: Option<Rule>,
    #[serde(rename = "use")]
    pub used: Option<Rule>,
    /// The plant's own `isValidBonemealTarget` says no.
    #[serde(default)]
    pub no_bonemeal: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// The tree from each light up, least first.
    pub cases: Vec<Case>,
    /// Where the code reads the light, from the plant.
    pub light_at: Option<[i32; 3]>,
    /// The tree with a block that can't be replaced on every side, where that differs.
    pub enclosed: Option<Node>,
}

#[derive(Deserialize)]
pub struct Case {
    pub light: i32,
    pub tree: Node,
}

/// A draw with what follows it, or, without `draw`, what happens.
#[derive(Deserialize, Default, PartialEq, Debug)]
pub struct Node {
    pub draw: Option<Draw>,
    #[serde(default)]
    pub bound: i32,
    /// `then` for a number drawn below this, `else` otherwise; without it, `then` whatever is drawn.
    pub below: Option<f64>,
    /// What follows each whole number drawn.
    pub each: Option<Vec<Node>>,
    pub then: Option<Box<Node>>,
    #[serde(rename = "else")]
    pub other: Option<Box<Node>>,
    #[serde(default)]
    pub set: Vec<Set>,
    #[serde(default)]
    pub drop: Vec<(String, u32)>,
    #[serde(default)]
    pub give: Vec<(String, u32)>,
    /// The sapling becomes a tree.
    #[serde(default)]
    pub tree: bool,
    pub answer: Option<bool>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Draw {
    Int,
    Float,
    Double,
    Bool,
}

/// A place from the plant and what becomes of the block there: nothing (it is removed), other
/// values of its properties, or another block.
#[derive(Deserialize, PartialEq, Debug)]
pub struct Set(pub i32, pub i32, pub i32, pub Option<Change>);

#[derive(Deserialize, PartialEq, Debug)]
pub struct Change {
    pub block: Option<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct Sapling {
    /// The trees Minecraft's own feature code made of it in the extraction.
    pub trees: Vec<Tree>,
}

#[derive(Deserialize)]
pub struct Tree {
    pub palette: Vec<Placed>,
    /// A place from the sapling and a number of the palette.
    pub blocks: Vec<(i32, i32, i32, usize)>,
}

#[derive(Deserialize)]
pub struct Placed {
    pub block: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

pub fn data() -> &'static Growth {
    static DATA: OnceLock<Growth> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/growth.json")))
            .expect("data/growth.json")
    })
}

impl Plant {
    /// The names of the state properties its rules go by, in the order of the keys.
    pub fn properties(&self) -> Vec<&str> {
        self.states
            .keys()
            .next()
            .map(|key| key.split(',').filter_map(|pair| pair.split('=').next()).filter(|n| !n.is_empty()).collect())
            .unwrap_or_default()
    }

    /// The rules of the state whose properties `value` gives.
    pub fn rules(&self, mut value: impl FnMut(&str) -> Option<String>) -> Option<&Rules> {
        let mut key = String::new();
        for name in self.properties() {
            if !key.is_empty() {
                key.push(',');
            }
            key.push_str(name);
            key.push('=');
            key.push_str(&value(name)?);
        }
        self.rules.get(*self.states.get(&key)?)
    }
}

/// The random numbers a plant's code draws (`RandomSource`).
pub trait Random {
    fn int(&mut self, bound: i32) -> i32;
    fn float(&mut self) -> f32;
    fn double(&mut self) -> f64;
    fn boolean(&mut self) -> bool;
}

impl Rule {
    /// The tree in this light, enclosed or not.
    pub fn tree(&self, light: i32, enclosed: bool) -> Option<&Node> {
        if enclosed && let Some(tree) = &self.enclosed {
            return Some(tree);
        }
        self.cases.iter().rev().find(|case| case.light <= light).map(|case| &case.tree)
    }
}

impl Node {
    /// What happens: the end the draws lead to, each drawn as the plant's code draws it.
    pub fn end(&self, random: &mut dyn Random) -> &Node {
        let Some(draw) = self.draw else {
            return self;
        };
        let drawn = match draw {
            Draw::Int => f64::from(random.int(self.bound)),
            Draw::Float => f64::from(random.float()),
            Draw::Double => random.double(),
            Draw::Bool => f64::from(u8::from(random.boolean())),
        };
        let next = match (&self.each, self.below) {
            (Some(each), _) => each.get(drawn as usize),
            (None, Some(below)) if drawn >= below => self.other.as_deref(),
            _ => self.then.as_deref(),
        };
        next.map_or(self, |node| node.end(random))
    }

    /// Every end below this node.
    #[cfg(test)]
    pub fn ends(&self) -> Vec<&Node> {
        if self.draw.is_none() {
            return vec![self];
        }
        self.each
            .iter()
            .flatten()
            .chain(self.then.as_deref())
            .chain(self.other.as_deref())
            .flat_map(Node::ends)
            .collect()
    }

    /// Whether nothing happens at this end.
    #[cfg(test)]
    pub fn idle(&self) -> bool {
        self.set.is_empty() && self.drop.is_empty() && self.give.is_empty() && !self.tree
    }

    /// Whether all that happens is that the plant goes: what its template block does by itself (leaves decay).
    pub fn gone(&self) -> bool {
        !self.tree && self.set.len() == 1 && self.set[0] == Set(0, 0, 0, None)
    }
}

/// The world around a plant, as an end changes it.
pub trait Plot {
    /// The name of the block at a place.
    fn block(&self, at: [i32; 3]) -> Option<String>;
    /// `BlockState.canBeReplaced` of the block at a place.
    fn replaceable(&self, at: [i32; 3]) -> bool;
    /// Other values of the properties of the block at a place.
    fn set_properties(&mut self, at: [i32; 3], properties: &BTreeMap<String, String>);
    /// Another block at a place; none removes what is there.
    fn set_block(&mut self, at: [i32; 3], block: Option<(&str, &BTreeMap<String, String>)>);
    /// An item dropped at a place (`Block.popResource`).
    fn drop(&mut self, at: [i32; 3], item: &str, count: u32);
    /// An item handed to whoever used the plant.
    fn give(&mut self, at: [i32; 3], item: &str, count: u32);
    /// A block of a tree stands: its leaves find out how far their trunk is, as after `TreeFeature.place`.
    fn settle(&mut self, _at: [i32; 3], _block: &str) {}
}

/// The world and its random numbers, as the game hands both to a plant.
pub trait World: Plot + Random {}
impl<T: Plot + Random> World for T {}

fn moved(pos: [i32; 3], by: (i32, i32, i32)) -> [i32; 3] {
    [pos[0] + by.0, pos[1] + by.1, pos[2] + by.2]
}

/// Does to the world what an end says, for the plant at `pos`; a sapling's tree is `sapling`'s.
/// Whether anything happened.
pub fn apply(
    end: &Node,
    pos: [i32; 3],
    sapling: Option<&Sapling>,
    plot: &mut dyn World,
) -> bool {
    for Set(x, y, z, change) in &end.set {
        let at = moved(pos, (*x, *y, *z));
        match change {
            None => plot.set_block(at, None),
            Some(Change { block: None, properties }) => plot.set_properties(at, properties),
            Some(Change { block: Some(block), properties }) => plot.set_block(at, Some((block, properties))),
        }
    }
    for (item, count) in &end.drop {
        plot.drop(pos, item, *count);
    }
    for (item, count) in &end.give {
        plot.give(pos, item, *count);
    }
    let grown = end.tree && sapling.is_some_and(|sapling| grow(sapling, pos, plot));
    grown || !end.set.is_empty() || !end.drop.is_empty() || !end.give.is_empty()
}

/// A sapling's tree: one of those the extraction grew, where its trunk (the blocks straight above
/// the sapling) has room; its other blocks go where something can be replaced, as a tree's leaves do.
pub fn grow(sapling: &Sapling, pos: [i32; 3], plot: &mut dyn World) -> bool {
    if sapling.trees.is_empty() {
        return false;
    }
    let tree = &sapling.trees[plot.int(sapling.trees.len() as i32) as usize % sapling.trees.len()];
    let free = |plot: &dyn World, at: [i32; 3]| at == pos || plot.replaceable(at);
    let trunk = |(x, _, z, _): &(i32, i32, i32, usize)| *x == 0 && *z == 0;
    if tree.blocks.iter().filter(|b| trunk(b)).any(|&(x, y, z, _)| !free(plot, moved(pos, (x, y, z)))) {
        return false;
    }
    // TreeGrower.growTree takes the sapling away first
    if !tree.blocks.iter().any(|&(x, y, z, _)| (x, y, z) == (0, 0, 0)) {
        plot.set_block(pos, None);
    }
    let mut placed = false;
    for block in &tree.blocks {
        let &(x, y, z, state) = block;
        let at = moved(pos, (x, y, z));
        let Some(state) = tree.palette.get(state) else {
            continue;
        };
        if trunk(block) || free(plot, at) {
            plot.set_block(at, Some((&state.block, &state.properties)));
            placed = true;
        }
    }
    for &(x, y, z, state) in &tree.blocks {
        if let Some(state) = tree.palette.get(state) {
            plot.settle(moved(pos, (x, y, z)), &state.block);
        }
    }
    placed
}

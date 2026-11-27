//! A small GeckoLib player: Bedrock geometry (`geo/*.geo.json`) laid out as GeckoLib 4 lays it
//! out, and its keyframe animations (`animations/*.animation.json`) with GeckoLib's linear and
//! Catmull-Rom easings and a controller's transition.
//!
//! The files are the mod's own, carried in the content mod; which of them a block state or a mob
//! is drawn with, and which animation its controller plays, is `data/gecko.json`, which pleromod's
//! extraction found by running the mod's renderers and controllers.
//!
//! Everything is in GeckoLib's frame: Bedrock's with x mirrored, as `BakedModelFactory` builds
//! bones and cubes (`-x` origins and pivots, x and y turns negated), in blocks.
use serde_json::Value;
use std::collections::HashMap;

/// A column-major 3x4 transform: `m[0..3]` the axes, `m[3]` the translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat(pub [[f32; 3]; 4]);

impl Mat {
    pub const IDENTITY: Mat = Mat([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]]);

    pub fn translation(by: [f32; 3]) -> Mat {
        let mut m = Mat::IDENTITY;
        m.0[3] = by;
        m
    }

    pub fn scale(by: [f32; 3]) -> Mat {
        Mat([[by[0], 0.0, 0.0], [0.0, by[1], 0.0], [0.0, 0.0, by[2]], [0.0; 3]])
    }

    /// A turn of `degrees` about axis 0 (x), 1 (y) or 2 (z), counterclockwise seen from its
    /// positive end (`Axis.rotationDegrees`).
    pub fn rotation(axis: usize, degrees: f32) -> Mat {
        let (s, c) = degrees.to_radians().sin_cos();
        let mut m = Mat::IDENTITY;
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        m.0[a][a] = c;
        m.0[a][b] = s;
        m.0[b][a] = -s;
        m.0[b][b] = c;
        m
    }

    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        std::array::from_fn(|i| m[0][i] * p[0] + m[1][i] * p[1] + m[2][i] * p[2] + m[3][i])
    }

    fn vector(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        std::array::from_fn(|i| m[0][i] * p[0] + m[1][i] * p[1] + m[2][i] * p[2])
    }

    /// `self` after `then`: `then` is applied to a point first.
    pub fn mul(&self, then: &Mat) -> Mat {
        Mat([
            self.vector(then.0[0]),
            self.vector(then.0[1]),
            self.vector(then.0[2]),
            self.point(then.0[3]),
        ])
    }
}

/// `RenderUtils.prepMatrixForBone` and `rotateMatrixAroundCube`: about `pivot`, a turn about z,
/// then y, then x (GeckoLib's angles: Bedrock's with x and y negated), then the scale.
fn about(pivot: [f32; 3], bedrock_degrees: [f32; 3], scale: [f32; 3]) -> Mat {
    let [x, y, z] = bedrock_degrees;
    Mat::translation(pivot)
        .mul(&Mat::rotation(2, z))
        .mul(&Mat::rotation(1, -y))
        .mul(&Mat::rotation(0, -x))
        .mul(&Mat::scale(scale))
        .mul(&Mat::translation(pivot.map(|v| -v)))
}

#[derive(Clone, Debug)]
enum Uv {
    /// Bedrock's box layout from this corner.
    Box([f32; 2]),
    /// Each face's corner and size, by `FACES`' order; a face left out is not drawn.
    Faces([Option<([f32; 2], [f32; 2], i32)>; 6]),
}

#[derive(Clone, Debug)]
struct Cube {
    origin: [f32; 3],
    size: [f32; 3],
    pivot: [f32; 3],
    rotation: [f32; 3],
    inflate: f32,
    mirror: bool,
    uv: Uv,
}

#[derive(Clone, Debug)]
struct Bone {
    name: String,
    parent: Option<usize>,
    pivot: [f32; 3],
    rotation: [f32; 3],
    cubes: Vec<Cube>,
}

/// A model: `minecraft:geometry`'s first entry.
#[derive(Clone, Debug)]
pub struct Geometry {
    texture: [f32; 2],
    bones: Vec<Bone>,
}

/// One face: its corners as seen from outside (top left, top right, bottom right, bottom
/// left), in blocks, and the texture's corner at each as a fraction of its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// The texture's rectangle (start, end; fractions), and the corner that holds its start:
    /// `uv` is the rectangle's corners from there on round.
    pub rect: [f32; 4],
    pub turns: u8,
}

const FACES: [&str; 6] = ["north", "south", "east", "west", "up", "down"];
/// Each face's corners of the unit box, seen from outside: top left, top right, bottom right,
/// bottom left.
const CORNERS: [[[usize; 3]; 4]; 6] = [
    [[1, 1, 0], [0, 1, 0], [0, 0, 0], [1, 0, 0]],
    [[0, 1, 1], [1, 1, 1], [1, 0, 1], [0, 0, 1]],
    [[1, 1, 1], [1, 1, 0], [1, 0, 0], [1, 0, 1]],
    [[0, 1, 0], [0, 1, 1], [0, 0, 1], [0, 0, 0]],
    [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1]],
    [[0, 0, 1], [1, 0, 1], [1, 0, 0], [0, 0, 0]],
];

fn numbers<const N: usize>(value: &Value) -> [f32; N] {
    std::array::from_fn(|i| value.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)
}

impl Geometry {
    pub fn parse(json: &Value) -> Option<Geometry> {
        let geometry = json.get("minecraft:geometry")?.get(0)?;
        let description = &geometry["description"];
        let texture = [
            description["texture_width"].as_f64().unwrap_or(64.0) as f32,
            description["texture_height"].as_f64().unwrap_or(64.0) as f32,
        ];
        let listed = geometry["bones"].as_array()?;
        let names: Vec<&str> = listed
            .iter()
            .map(|b| b["name"].as_str().unwrap_or_default())
            .collect();
        let bones = listed
            .iter()
            .map(|bone| Bone {
                name: bone["name"].as_str().unwrap_or_default().to_owned(),
                parent: bone["parent"]
                    .as_str()
                    .and_then(|p| names.iter().position(|n| *n == p)),
                pivot: numbers(&bone["pivot"]),
                rotation: numbers(&bone["rotation"]),
                cubes: bone["cubes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cube| Cube {
                        origin: numbers(&cube["origin"]),
                        size: numbers(&cube["size"]),
                        pivot: numbers(&cube["pivot"]),
                        rotation: numbers(&cube["rotation"]),
                        inflate: cube["inflate"].as_f64().unwrap_or(0.0) as f32,
                        mirror: cube["mirror"].as_bool().unwrap_or(false),
                        uv: match &cube["uv"] {
                            Value::Object(faces) => Uv::Faces(FACES.map(|face| {
                                let face = faces.get(face)?;
                                Some((
                                    numbers(&face["uv"]),
                                    numbers(&face["uv_size"]),
                                    face["uv_rotation"].as_i64().unwrap_or(0) as i32 / 90,
                                ))
                            })),
                            uv => Uv::Box(numbers(uv)),
                        },
                    })
                    .collect(),
            })
            .collect();
        Some(Geometry { texture, bones })
    }

    #[cfg(test)]
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    /// Each bone's transform under `pose` (its parents' included), in blocks.
    fn transforms(&self, pose: Option<&Pose>) -> Vec<Mat> {
        let mut done: Vec<Option<Mat>> = vec![None; self.bones.len()];
        fn of(g: &Geometry, pose: Option<&Pose>, done: &mut Vec<Option<Mat>>, i: usize, depth: usize) -> Mat {
            if let Some(m) = done[i] {
                return m;
            }
            let bone = &g.bones[i];
            let moved = pose.and_then(|p| p.bones.get(&bone.name));
            let turn = moved.map_or([0.0; 3], |m| m.rotation);
            let offset = moved.map_or([0.0; 3], |m| m.position);
            let scale = moved.map_or([1.0; 3], |m| m.scale);
            let pivot = [-bone.pivot[0] / 16.0, bone.pivot[1] / 16.0, bone.pivot[2] / 16.0];
            let degrees = std::array::from_fn(|a| bone.rotation[a] + turn[a]);
            // RenderUtils.translateMatrixToBone: the offset's x is mirrored as the model is.
            let own = Mat::translation([-offset[0] / 16.0, offset[1] / 16.0, offset[2] / 16.0])
                .mul(&about(pivot, degrees, scale));
            // A bone that names itself among its parents is drawn as a root.
            let m = match bone.parent.filter(|_| depth < g.bones.len()) {
                Some(parent) => of(g, pose, done, parent, depth + 1).mul(&own),
                None => own,
            };
            done[i] = Some(m);
            m
        }
        (0..self.bones.len())
            .map(|i| of(self, pose, &mut done, i, 0))
            .collect()
    }

    /// The model's faces under `pose` (at rest without one).
    pub fn quads(&self, pose: Option<&Pose>) -> Vec<Quad> {
        let transforms = self.transforms(pose);
        let mut out = Vec::new();
        for (bone, transform) in self.bones.iter().zip(&transforms) {
            for cube in &bone.cubes {
                let (size, grow) = (cube.size, cube.inflate);
                // BakedModelFactory.constructCube: the box starts at the mirrored far x.
                let start = [
                    (-(cube.origin[0] + size[0]) - grow) / 16.0,
                    (cube.origin[1] - grow) / 16.0,
                    (cube.origin[2] - grow) / 16.0,
                ];
                let end: [f32; 3] = std::array::from_fn(|a| start[a] + (size[a] + 2.0 * grow) / 16.0);
                let pivot = [-cube.pivot[0] / 16.0, cube.pivot[1] / 16.0, cube.pivot[2] / 16.0];
                let placed = transform.mul(&about(pivot, cube.rotation, [1.0; 3]));
                for (index, corners) in CORNERS.iter().enumerate() {
                    let Some((mut u, v, mut du, dv, turns)) = face_uv(cube, index) else {
                        continue;
                    };
                    // VertexSet.verticesForQuad: a mirrored box draws east's side on west's.
                    let side = match (cube.mirror, index) {
                        (true, 2) => 3,
                        (true, 3) => 2,
                        _ => index,
                    };
                    let corners = if side == index { corners } else { &CORNERS[side] };
                    if cube.mirror {
                        // GeoQuad.build
                        (u, du) = (u + du, -du);
                    }
                    let points = corners.map(|c| {
                        placed.point(std::array::from_fn(|a| if c[a] == 1 { end[a] } else { start[a] }))
                    });
                    let at = [[u, v], [u + du, v], [u + du, v + dv], [u, v + dv]];
                    let uv = std::array::from_fn(|i| {
                        let [u, v] = at[(i + 4 - turns.rem_euclid(4) as usize) % 4];
                        [u / self.texture[0], v / self.texture[1]]
                    });
                    let [w, h] = self.texture;
                    out.push(Quad {
                        corners: points,
                        uv,
                        rect: [u / w, v / h, (u + du) / w, (v + dv) / h],
                        turns: turns.rem_euclid(4) as u8,
                    });
                }
            }
        }
        out
    }
}

/// A face's texture rectangle: its corner, its size (negative when flipped) and its quarter
/// turns.
fn face_uv(cube: &Cube, face: usize) -> Option<(f32, f32, f32, f32, i32)> {
    match &cube.uv {
        Uv::Faces(faces) => {
            let ([u, v], [du, dv], turns) = faces[face]?;
            // GeoQuad.build: up and down are read from their far corner.
            Some(if face >= 4 {
                (u + du, v + dv, -du, -dv, turns)
            } else {
                (u, v, du, dv, turns)
            })
        }
        Uv::Box([u, v]) => {
            let [w, h, d] = cube.size.map(f32::floor);
            let (u, v) = (*u, *v);
            Some(match face {
                0 => (u + d, v + d, w, h, 0),
                1 => (u + 2.0 * d + w, v + d, w, h, 0),
                2 => (u, v + d, d, h, 0),
                3 => (u + d + w, v + d, d, h, 0),
                4 => (u + d + w, v + d, -w, -d, 0),
                _ => (u + d + 2.0 * w, v, -w, d, 0),
            })
        }
    }
}

/// What an animation gives a bone: its turn in Bedrock's degrees, added to the model's own;
/// its offset in texels; its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub rotation: [f32; 3],
    pub position: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for BonePose {
    fn default() -> Self {
        BonePose { rotation: [0.0; 3], position: [0.0; 3], scale: [1.0; 3] }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub bones: HashMap<String, BonePose>,
}

impl Pose {
    /// `self` on its way to `to`, `t` of the way (a controller's transition:
    /// a bone only one of them moves goes from or to its rest).
    pub fn toward(&self, to: &Pose, t: f32) -> Pose {
        let mut bones = HashMap::new();
        for name in self.bones.keys().chain(to.bones.keys()) {
            let (a, b) = (
                self.bones.get(name).copied().unwrap_or_default(),
                to.bones.get(name).copied().unwrap_or_default(),
            );
            let mix = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
            bones.insert(
                name.clone(),
                BonePose {
                    rotation: mix(a.rotation, b.rotation),
                    position: mix(a.position, b.position),
                    scale: mix(a.scale, b.scale),
                },
            );
        }
        Pose { bones }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Easing {
    Linear,
    /// `EasingType.CatmullRomEasing`, with the values before its start and after its end.
    CatmullRom([f32; 3], [f32; 3]),
}

/// GeckoLib's `Keyframe`: `length` ticks from `start` to `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Keyframe {
    length: f32,
    start: [f32; 3],
    end: [f32; 3],
    easing: Easing,
}

#[derive(Clone, Debug, Default)]
struct Channel(Vec<Keyframe>);

impl Channel {
    /// `BakedAnimationsAdapter.getKeyframes` and `buildKeyframeStack`: a value, or values by
    /// their seconds, each an array or Bedrock's `{post, lerp_mode}`.
    fn parse(json: &Value) -> Channel {
        let mut entries: Vec<(f32, [f32; 3], bool)> = Vec::new();
        let vector = |v: &Value| -> Option<[f32; 3]> {
            match v {
                Value::Number(n) => Some([n.as_f64()? as f32; 3]),
                Value::Array(_) => Some(numbers(v)),
                _ => None,
            }
        };
        match json {
            Value::Null => {}
            Value::Object(o) if !o.contains_key("vector") => {
                let mut timed: Vec<(f32, &Value)> = o
                    .iter()
                    .filter_map(|(k, v)| Some((k.parse::<f32>().ok()?, v)))
                    .collect();
                timed.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (time, frame) in timed {
                    match frame {
                        Value::Object(f) => {
                            let smooth = f.get("lerp_mode").and_then(Value::as_str) == Some("catmullrom")
                                || f.get("easing").and_then(Value::as_str) == Some("catmullrom");
                            let inner = |v: &Value| vector(v.get("vector").unwrap_or(v));
                            if let Some(pre) = f.get("pre").and_then(inner) {
                                entries.push((time, pre, false));
                            }
                            if let Some(post) = f.get("post").or(f.get("vector")).and_then(inner) {
                                entries.push((time, post, smooth));
                            }
                        }
                        other => entries.extend(vector(other).map(|v| (time, v, false))),
                    }
                }
            }
            other => entries.extend(vector(other.get("vector").unwrap_or(other)).map(|v| (0.0, v, false))),
        }
        let mut frames: Vec<Keyframe> = Vec::new();
        let mut smooth = Vec::new();
        for (i, (time, value, catmull)) in entries.iter().enumerate() {
            let previous = i.checked_sub(1).map(|p| entries[p]);
            frames.push(Keyframe {
                length: (time - previous.map_or(0.0, |p| p.0)) * 20.0,
                start: previous.map_or(*value, |p| p.1),
                end: *value,
                easing: Easing::Linear,
            });
            smooth.push(*catmull);
        }
        // addSplineArgs: a lone keyframe is linear; a Catmull-Rom one takes the keyframe
        // before's end and the one after's.
        if frames.len() > 1 {
            for i in 0..frames.len() {
                if smooth[i] {
                    let before = if i == 0 { frames[i].start } else { frames[i - 1].end };
                    let after = frames.get(i + 1).map_or(frames[i].end, |f| f.end);
                    frames[i].easing = Easing::CatmullRom(before, after);
                }
            }
        }
        Channel(frames)
    }

    /// `AnimationController.getAnimationPointAtTick` and the keyframe's easing: the value
    /// `tick` ticks in. None for a channel with no keyframes.
    fn at(&self, tick: f32) -> Option<[f32; 3]> {
        let mut passed = 0.0;
        let mut found = self.0.last()?;
        let mut within = tick;
        for frame in &self.0 {
            passed += frame.length;
            if passed > tick {
                found = frame;
                within = tick - (passed - frame.length);
                break;
            }
        }
        // AnimationPoint: at or past the keyframe's length it is at its end.
        if within >= found.length {
            return Some(found.end);
        }
        let t = within / found.length;
        Some(std::array::from_fn(|a| match found.easing {
            Easing::Linear => found.start[a] + (found.end[a] - found.start[a]) * t,
            Easing::CatmullRom(before, after) => {
                catmull_rom(t, before[a], found.start[a], found.end[a], after[a])
            }
        }))
    }

    fn length(&self) -> f32 {
        self.0.iter().map(|f| f.length).sum()
    }
}

/// `CatmullRomEasing.getPointOnSpline`.
pub fn catmull_rom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}

#[derive(Clone, Debug, Default)]
struct BoneAnimation {
    rotation: Channel,
    position: Channel,
    scale: Channel,
}

#[derive(Clone, Debug)]
pub struct Animation {
    /// In ticks.
    pub length: f32,
    bones: Vec<(String, BoneAnimation)>,
}

impl Animation {
    fn parse(json: &Value) -> Animation {
        let bones: Vec<(String, BoneAnimation)> = json["bones"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, bone)| {
                (
                    name.clone(),
                    BoneAnimation {
                        rotation: Channel::parse(&bone["rotation"]),
                        position: Channel::parse(&bone["position"]),
                        scale: Channel::parse(&bone["scale"]),
                    },
                )
            })
            .collect();
        // BakedAnimationsAdapter.calculateAnimationLength where the file gives none.
        let length = json["animation_length"].as_f64().map_or_else(
            || {
                bones
                    .iter()
                    .flat_map(|(_, b)| [b.rotation.length(), b.position.length(), b.scale.length()])
                    .fold(0.0, f32::max)
            },
            |seconds| seconds as f32 * 20.0,
        );
        Animation { length, bones }
    }

    /// The pose `tick` ticks into the animation.
    pub fn pose(&self, tick: f32) -> Pose {
        let bones = self
            .bones
            .iter()
            .map(|(name, bone)| {
                let rest = BonePose::default();
                (
                    name.clone(),
                    BonePose {
                        rotation: bone.rotation.at(tick).unwrap_or(rest.rotation),
                        position: bone.position.at(tick).unwrap_or(rest.position),
                        scale: bone.scale.at(tick).unwrap_or(rest.scale),
                    },
                )
            })
            .collect();
        Pose { bones }
    }
}

/// An animation file: its animations by name.
pub fn animations(json: &Value) -> HashMap<String, Animation> {
    json["animations"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, animation)| (name.clone(), Animation::parse(animation)))
        .collect()
}

/// How a stage of a `RawAnimation` plays (`Animation.LoopType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    Loop,
    PlayOnce,
    HoldOnLastFrame,
}

impl Loop {
    pub fn of(name: &str) -> Loop {
        match name {
            "loop" => Loop::Loop,
            "hold_on_last_frame" => Loop::HoldOnLastFrame,
            // `DEFAULT` is the file's own, which the mod's looping animations are played with `thenLoop`.
            _ => Loop::PlayOnce,
        }
    }
}

/// An `AnimationController`: the animation it plays, since when, and what it came from.
#[derive(Clone, Debug, Default)]
pub struct Controller {
    playing: Option<String>,
    /// The tick the animation started at, its transition included.
    since: f32,
    /// The pose it left, for the transition.
    from: Pose,
    last: Pose,
}

impl Controller {
    /// `AnimationController.process`: plays `animation` (None stops it), starting over when
    /// it is another than before, and gives the pose at `tick`. `transition` ticks lead from
    /// the pose it had into the new animation's start.
    pub fn pose(
        &mut self,
        animation: Option<(&str, &Animation, Loop)>,
        transition: f32,
        tick: f32,
    ) -> Pose {
        let name = animation.map(|a| a.0);
        if name != self.playing.as_deref() {
            self.playing = name.map(str::to_owned);
            self.since = tick;
            self.from = std::mem::take(&mut self.last);
        }
        // A tick that went back (a world joined again) starts over.
        if tick < self.since {
            self.since = tick;
        }
        let pose = match animation {
            None => Pose::default(),
            Some((_, animation, looped)) => {
                let passed = tick - self.since;
                if passed < transition {
                    self.from.toward(&animation.pose(0.0), passed / transition)
                } else {
                    let run = passed - transition;
                    match looped {
                        _ if animation.length <= 0.0 => animation.pose(0.0),
                        Loop::Loop => animation.pose(run % animation.length),
                        Loop::HoldOnLastFrame => animation.pose(run.min(animation.length)),
                        // Played through, the bones go back to rest.
                        Loop::PlayOnce if run >= animation.length => Pose::default(),
                        Loop::PlayOnce => animation.pose(run),
                    }
                }
            }
        };
        self.last = pose.clone();
        pose
    }

    /// Starts the animation over at `tick` (`triggerAnim`, `forceAnimationReset`).
    pub fn restart(&mut self, tick: f32) {
        self.playing = None;
        self.since = tick;
    }
}

#[cfg(test)]
mod tests;

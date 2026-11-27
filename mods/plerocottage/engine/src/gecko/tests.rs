//! The player against the mod's own files: geometry as pleromod's `render.py` lays it out
//! (`data/gecko_check.json`), and keyframes by GeckoLib's formulas worked out by hand.
use super::*;
use crate::display;
use std::path::PathBuf;

/// The content mod pleromod writes, which the browser build links as `web/mods/plerocottage`.
fn content() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../web/mods/plerocottage/assets/plerocottage"))
}

fn json(path: PathBuf) -> Value {
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn geometry(name: &str) -> Geometry {
    Geometry::parse(&json(content().join(format!("geo/{name}.geo.json")))).expect("a geometry")
}

fn close(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}

#[test]
fn models_at_rest_are_laid_out_as_the_site_draws_them() {
    let checked: HashMap<String, Vec<Vec<Vec<f32>>>> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/gecko_check.json"
    )))
    .expect("gecko_check.json is what gecko.py writes");
    assert!(checked.len() >= 4, "models to check");
    for (name, expected) in &checked {
        let quads = geometry(name).quads(None);
        assert_eq!(quads.len(), expected.len(), "{name}: faces");
        for (index, (quad, expected)) in quads.iter().zip(expected).enumerate() {
            for corner in 0..4 {
                for axis in 0..3 {
                    // render.py lays them out in texels.
                    let (got, want) = (quad.corners[corner][axis] * 16.0, expected[corner][axis]);
                    assert!(close(got, want, 2e-3), "{name} face {index} corner {corner}: {got} for {want}");
                }
                for axis in 0..2 {
                    let (got, want) = (quad.uv[corner][axis], expected[4 + corner][axis]);
                    assert!(close(got, want, 1e-5), "{name} face {index} uv {corner}: {got} for {want}");
                }
            }
        }
    }
}

#[test]
fn a_catmull_rom_keyframe_follows_geckolibs_spline() {
    // Three keyframes half a second apart: 0, 10, 0 about x.
    let channel = Channel::parse(&serde_json::json!({
        "0.0": {"post": {"vector": [0, 0, 0]}, "lerp_mode": "catmullrom"},
        "0.5": {"post": {"vector": [10, 4, 0]}, "lerp_mode": "catmullrom"},
        "1.0": {"post": {"vector": [0, 0, 0]}, "lerp_mode": "catmullrom"}
    }));
    // Half way through the second keyframe (tick 5 of 10): getPointOnSpline(0.5, 0, 0, 10, 0)
    // = 0.5 * (10 * 0.5 + 40 * 0.25 - 30 * 0.125) = 5.625, and 2.25 for 4.
    let [x, y, z] = channel.at(5.0).unwrap();
    assert!(close(x, 5.625, 1e-5) && close(y, 2.25, 1e-5) && z == 0.0, "{x} {y} {z}");
    // Half way back (tick 15), by addSplineArgs: before it the keyframe before's end, 10, which
    // is where it starts; after it its own end, being the last: getPointOnSpline(0.5, 10, 10, 0, 0)
    // = 0.5 * (20 - 10 * 0.5 - 30 * 0.25 + 20 * 0.125) = 5.
    assert!(close(channel.at(15.0).unwrap()[0], 5.0, 1e-5));
    // On a keyframe, and past the last one, its value.
    assert_eq!(channel.at(10.0).unwrap(), [10.0, 4.0, 0.0]);
    assert_eq!(channel.at(40.0).unwrap(), [0.0; 3]);
}

#[test]
fn plain_keyframes_are_linear_and_one_value_is_constant() {
    let channel = Channel::parse(&serde_json::json!({"0.25": [0, 8, 0], "0.75": [0, 16, 0]}));
    // Before the first keyframe its value; then a straight line over ten ticks.
    assert_eq!(channel.at(2.0).unwrap(), [0.0, 8.0, 0.0]);
    assert!(close(channel.at(7.5).unwrap()[1], 10.0, 1e-5));
    let constant = Channel::parse(&serde_json::json!({"vector": [0, 15, 0]}));
    assert_eq!(constant.at(123.0).unwrap(), [0.0, 15.0, 0.0]);
    assert_eq!(Channel::parse(&serde_json::json!(2.0)).at(0.0).unwrap(), [2.0; 3]);
}

#[test]
fn a_bone_turns_about_its_pivot_with_x_and_y_mirrored() {
    // One 2x2x2 box at the origin on a bone pivoting at (0, 0, 0), turned 90 about Bedrock's z.
    let model = serde_json::json!({"minecraft:geometry": [{
        "description": {"texture_width": 16, "texture_height": 16},
        "bones": [{"name": "b", "pivot": [0, 0, 0],
                   "cubes": [{"origin": [0, 0, 0], "size": [2, 2, 2], "uv": [0, 0]}]}]}]});
    let geometry = Geometry::parse(&model).unwrap();
    let rest = geometry.quads(None);
    // Bedrock's +x is GeckoLib's -x: the box spans x -2/16..0.
    let xs: Vec<f32> = rest.iter().flat_map(|q| q.corners.map(|c| c[0])).collect();
    assert!(close(xs.iter().copied().fold(f32::MAX, f32::min), -0.125, 1e-6));
    assert!(close(xs.iter().copied().fold(f32::MIN, f32::max), 0.0, 1e-6));
    let mut pose = Pose::default();
    pose.bones.insert("b".into(), BonePose { rotation: [0.0, 0.0, 90.0], ..BonePose::default() });
    // A turn of 90 about z takes (-0.125, 0.125) to (-0.125, -0.125): the box now spans y -0.125..0.
    let ys: Vec<f32> = geometry.quads(Some(&pose)).iter().flat_map(|q| q.corners.map(|c| c[1])).collect();
    assert!(close(ys.iter().copied().fold(f32::MAX, f32::min), -0.125, 1e-6));
    assert!(close(ys.iter().copied().fold(f32::MIN, f32::max), 0.0, 1e-6));
    // An offset of one texel along Bedrock's x moves it one texel towards GeckoLib's -x.
    pose.bones.insert("b".into(), BonePose { position: [1.0, 0.0, 0.0], ..BonePose::default() });
    let xs: Vec<f32> = geometry.quads(Some(&pose)).iter().flat_map(|q| q.corners.map(|c| c[0])).collect();
    assert!(close(xs.iter().copied().fold(f32::MAX, f32::min), -0.1875, 1e-6));
}

#[test]
fn a_controller_eases_into_another_animation_and_loops_it() {
    let files = animations(&serde_json::json!({"animations": {
        "still": {"loop": true, "bones": {"b": {"rotation": {"vector": [0, 0, 0]}}}},
        "nod": {"loop": true, "animation_length": 1.0,
                "bones": {"b": {"rotation": {"0.0": [20, 0, 0], "1.0": [40, 0, 0]}}}}}}));
    let (still, nod) = (&files["still"], &files["nod"]);
    let mut controller = Controller::default();
    let turn = |pose: Pose| pose.bones["b"].rotation[0];
    assert_eq!(turn(controller.pose(Some(("still", still, Loop::Loop)), 4.0, 100.0)), 0.0);
    // The new animation: four ticks from where the bone was to the animation's start.
    assert!(close(turn(controller.pose(Some(("nod", nod, Loop::Loop)), 4.0, 101.0)), 0.0, 1e-5));
    assert!(close(turn(controller.pose(Some(("nod", nod, Loop::Loop)), 4.0, 103.0)), 10.0, 1e-5));
    // Then the animation itself, from its start: five ticks in, a quarter of the way.
    assert!(close(turn(controller.pose(Some(("nod", nod, Loop::Loop)), 4.0, 110.0)), 25.0, 1e-5));
    // A loop of twenty ticks is back where it was twenty ticks later.
    assert!(close(turn(controller.pose(Some(("nod", nod, Loop::Loop)), 4.0, 130.0)), 25.0, 1e-4));
    // Played once, it rests after its length.
    let mut once = Controller::default();
    assert!(close(turn(once.pose(Some(("nod", nod, Loop::PlayOnce)), 0.0, 10.0)), 20.0, 1e-5));
    assert!(once.pose(Some(("nod", nod, Loop::PlayOnce)), 0.0, 31.0).bones.is_empty());
}

#[test]
fn everything_the_index_draws_is_in_the_content_mod_and_moves_bones_it_has() {
    let index = display::index();
    assert!(index.blocks.len() > 50 && index.entities.len() >= 2);
    let mut drawn: Vec<(&display::Drawn, &[display::ControllerSpec])> = Vec::new();
    for block in index.blocks.values() {
        drawn.extend(block.states.values().map(|d| (d, &block.controllers[..])));
    }
    for entity in index.entities.values() {
        drawn.push((&entity.adult, &entity.controllers[..]));
        drawn.push((&entity.baby, &entity.controllers[..]));
    }
    for (drawn, controllers) in drawn {
        let geometry = geometry(&drawn.model);
        assert!(!geometry.quads(None).is_empty(), "{}: nothing to draw", drawn.model);
        let texture = content().join(format!("textures/entity/geckolib/{}.png", drawn.texture));
        assert!(texture.exists(), "{}", texture.display());
        for layer in &drawn.layers {
            assert!(!self::geometry(&layer.model).quads(None).is_empty());
        }
        let Some(file) = &drawn.animation else { continue };
        let known = animations(&json(content().join(format!("animations/{file}.animation.json"))));
        for controller in controllers {
            for (name, _) in controller.plays.iter().flat_map(|p| &p.animation) {
                // A name the file doesn't have plays nothing, as in GeckoLib.
                let Some(animation) = known.get(name) else { continue };
                // GeckoLib passes over a bone the model lacks (the mallard's walk names a wing
                // it has no more); the rest are the model's.
                let moved = animation.pose(0.0);
                assert!(
                    moved.bones.is_empty() || moved.bones.keys().any(|bone| geometry.bone(bone).is_some()),
                    "{}: {name} moves none of its bones",
                    drawn.model
                );
            }
        }
    }
}

#[test]
fn the_turkeys_walk_swings_its_legs_within_its_loop() {
    let known = animations(&json(content().join("animations/turkey.animation.json")));
    let walk = &known["animation.turkey.ground_walk"];
    // `animation_length` 0.8 seconds.
    assert!(close(walk.length, 16.0, 1e-4));
    let quads = |tick: f32| geometry("turkey").quads(Some(&walk.pose(tick)));
    let moved = |a: &[Quad], b: &[Quad]| {
        a.iter().zip(b).any(|(a, b)| (0..4).any(|c| (0..3).any(|x| !close(a.corners[c][x], b.corners[c][x], 1e-4))))
    };
    assert!(moved(&quads(0.0), &quads(4.0)), "the walk moves the model");
    assert!(!moved(&quads(0.0), &quads(16.0)), "and comes round after its length");
}

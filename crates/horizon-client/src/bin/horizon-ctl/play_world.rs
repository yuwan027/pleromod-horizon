//! `horizon-ctl play-world`: the base world of pleromod.com/play. Generates the chunks around
//! the origin with Horizon's own world generation (a dedicated server in a scratch folder,
//! whose `/pregen` generates, lights and saves them), then exports each chunk of the play
//! world's largest size (4 x 4: chunks -2..=1 on each axis, the square `play_chunk_in_world`
//! takes) in the form the API keeps (`horizon_client::play_data`):
//!
//! - `<out>/<x>.<z>.bin`: the chunk's bytes, the body of the console's
//!   `PUT /v1/admin/play/chunks/:x/:z`;
//! - `<out>/chunks.json`: every chunk as the API's join sends it (`{ x, z, data }`, base64),
//!   with the world's seed and a spawn on the ground at the origin.
use horizon_client::play_data::{self, base64};
use horizon_core::position::ChunkPos;
use horizon_world::{
    nbt::{JavaString, Limits, Tag},
    region::RegionFile,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const USAGE: &str = "horizon-ctl play-world --out DIR [--seed N] [--game PATH] [--resources PATH] [--keep]

Generates the base world of pleromod.com/play with Horizon's world generation and exports its
chunks (-2..=1 on each axis) for the API: DIR/<x>.<z>.bin (the console's upload) and
DIR/chunks.json (base64, as the join sends them).

  --seed N           world seed (default 20261206)
  --game PATH        the horizon-client to run (default: the one beside horizon-ctl)
  --resources PATH   prepared resources (default .local/horizon-client/26.3)
  --keep             keep the generated world's scratch folder";

/// Chunks on each side of the largest play world, around the origin.
const SIDE: i32 = 4;

pub fn run(args: &[String]) -> Result<Value, String> {
    let mut out = None;
    let mut seed: i64 = 20_261_206;
    let mut game = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("horizon-client")));
    let mut resources = PathBuf::from(".local/horizon-client/26.3");
    let mut keep = false;
    let mut i = 0;
    while i < args.len() {
        let value = || args.get(i + 1).cloned().ok_or_else(|| USAGE.to_owned());
        match args[i].as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--seed" => seed = value()?.parse().map_err(|_| "--seed takes a number")?,
            "--game" => game = Some(PathBuf::from(value()?)),
            "--resources" => resources = PathBuf::from(value()?),
            "--keep" => {
                keep = true;
                i += 1;
                continue;
            }
            "-h" | "--help" => return Err(USAGE.to_owned()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
        i += 2;
    }
    let out = out.ok_or_else(|| USAGE.to_owned())?;
    let game = game.filter(|g| g.is_file()).ok_or("no horizon-client: give --game")?;
    let resources = std::fs::canonicalize(&resources)
        .map_err(|e| format!("{}: {e}", resources.display()))?;
    let scratch = std::env::temp_dir().join(format!("horizon-play-world-{}", std::process::id()));
    std::fs::create_dir_all(scratch.join("home")).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let result = generate(&game, &resources, &scratch, seed)
        .and_then(|world| export(&world, &out, seed));
    if keep {
        eprintln!("play-world: the generated world is in {}", scratch.display());
    } else {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    let mut report = result?;
    report["seconds"] = json!(started.elapsed().as_secs());
    Ok(report)
}

/// A dedicated server in `scratch` makes a new world of `seed` and pregenerates the chunks
/// within three of the origin's (one ring past the exported square, so its edge is lit and
/// decorated as inside a world), then stops and saves.
fn generate(game: &Path, resources: &Path, scratch: &Path, seed: i64) -> Result<PathBuf, String> {
    let write = |name: &str, text: &str| {
        std::fs::write(scratch.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write("eula.txt", "eula=true\n")?;
    write(
        "server.properties",
        &format!(
            "level-seed={seed}\nlevel-name=world\nonline-mode=false\nhorizon-pregen=true\n\
             spawn-protection=0\nserver-port=0\nview-distance=3\nsimulation-distance=3\n"
        ),
    )?;
    let home = scratch.join("home");
    let mut child = Command::new(game)
        .arg("--resources")
        .arg(resources)
        .arg("--dedicated")
        .current_dir(scratch)
        .env("HORIZON_HOME", &home)
        .env("XDG_DATA_HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("HORIZON_NO_AUDIO", "1")
        .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("{}: {e}", game.display()))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut asked = false;
    let mut stopping = false;
    for line in stdout.lines() {
        let line = line.map_err(|e| e.to_string())?;
        eprintln!("server: {line}");
        if !asked && line.contains("Done (") {
            asked = true;
            writeln!(stdin, "pregen start 48 square 0 0").map_err(|e| e.to_string())?;
        } else if asked && !stopping && line.contains("Done. ") {
            stopping = true;
            writeln!(stdin, "stop").map_err(|e| e.to_string())?;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err("the server did not finish within 15 minutes".into());
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !stopping {
        return Err(format!("the server stopped before the chunks were done ({status})"));
    }
    Ok(scratch.join("world/dimensions/minecraft/overworld"))
}

fn read(dir: &Path, x: i32, z: i32) -> Result<Option<horizon_world::nbt::NamedTag>, String> {
    let path = dir.join(format!("r.{}.{}.mca", x >> 5, z >> 5));
    if !path.is_file() {
        return Ok(None);
    }
    let mut region = RegionFile::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match region.read(ChunkPos { x, z }).map_err(|e| e.to_string())? {
        Some(record) => Ok(Some(record.nbt(Limits::default()).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

/// Each chunk of the square with its entities, as the API keeps it.
fn export(world: &Path, out: &Path, seed: i64) -> Result<Value, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut chunks = Vec::new();
    let mut total = 0usize;
    let mut sizes = Vec::new();
    let mut ground = None;
    for x in play_data::chunk_range(SIDE) {
        for z in play_data::chunk_range(SIDE) {
            let mut root = read(&world.join("region"), x, z)?
                .ok_or_else(|| format!("chunk {x}, {z} was not generated"))?;
            let entities = read(&world.join("entities"), x, z)?
                .and_then(|e| match e.tag.get("Entities") {
                    Some(Tag::List(list)) => Some(list.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let entity_count = entities.len();
            if let Tag::Compound(map) = &mut root.tag {
                map.insert(JavaString::from("entities"), Tag::List(entities));
            }
            if (x, z) == (0, 0) {
                ground = surface(&root);
            }
            let bytes = play_data::chunk_bytes(&root).map_err(|e| e.to_string())?;
            std::fs::write(out.join(format!("{x}.{z}.bin")), &bytes).map_err(|e| e.to_string())?;
            total += bytes.len();
            sizes.push(json!({ "x": x, "z": z, "bytes": bytes.len(), "entities": entity_count }));
            chunks.push(json!({ "x": x, "z": z, "data": base64::encode(&bytes) }));
        }
    }
    let spawn = json!({ "x": 0, "y": ground.unwrap_or(64), "z": 0 });
    let file = json!({ "seed": seed, "spawn": spawn, "chunks": chunks });
    let text = serde_json::to_string(&file).map_err(|e| e.to_string())?;
    std::fs::write(out.join("chunks.json"), &text).map_err(|e| e.to_string())?;
    Ok(json!({
        "out": out,
        "seed": seed,
        "spawn": spawn,
        "chunks": sizes,
        "bytes": total,
        "base64Bytes": text.len(),
    }))
}

/// One above the highest block at the origin's column with motion blocking (`MOTION_BLOCKING`
/// heightmap), where a player stands.
fn surface(root: &horizon_world::nbt::NamedTag) -> Option<i32> {
    let data = horizon_world::chunk::ChunkData::new(root.clone()).ok()?;
    for y in (-64..320).rev() {
        let state = data
            .block(horizon_core::position::BlockPos::new(0, y, 0))
            .ok()
            .flatten();
        if let Some(state) = state
            && !play_data::is_air(&state.name)
        {
            return Some(y + 1);
        }
    }
    None
}

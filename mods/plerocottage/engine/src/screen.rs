//! A machine's screen, drawn and clicked as pleromod's extraction found the mod's own screen
//! class draws and answers in each state (`native.py`'s `Screen`): this only looks the state up.
use crate::spec::{Button2, Case, Draw, Factor, Machine, Screen2};
use horizon_client::mod_api::{Host, HudBatch};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;

/// What the open menu's slots hold, by the server's slot number: only a client tick may read the
/// menu's stacks, and the screen draws by them.
pub static STACKS: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
/// The switches of the open screen that its buttons turned (`SaucepanAndWhiskScreen.whiskStir`).
pub static TOGGLES: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// What a screen reads of its machine, gathered once a frame.
pub struct Seen<'a> {
    pub data: &'a [i32],
    stacks: Vec<Option<String>>,
    heat: bool,
    light: i64,
    running: bool,
    tank: Option<(String, i64)>,
    toggles: HashSet<String>,
}

impl<'a> Seen<'a> {
    pub fn of(host: &dyn Host, spec: &Machine, data: &'a [i32]) -> Self {
        let pos = host.screen_pos();
        // What the server sent of the block entity: whether it runs, its tank.
        let entity = pos.and_then(|p| host.mod_block_entity(p));
        let nbt = entity.as_ref().map(|e| &e.nbt);
        let tank = nbt.and_then(crate::glue::tank_of);
        Self {
            data,
            stacks: STACKS.lock().map(|s| s.clone()).unwrap_or_default(),
            heat: pos.is_some_and(|p| crate::glue::heated(host, spec, p)),
            light: pos.map_or(0, |p| {
                let (block, sky) = host.light(p);
                i64::from(block.max(sky))
            }),
            running: nbt.is_some_and(|n| crate::int(n, "isRunning") != 0),
            tank: tank.map(|(fluid, amount)| (fluid, i64::from(amount))),
            toggles: TOGGLES.lock().ok().and_then(|t| t.clone()).unwrap_or_default(),
        }
    }

    fn value(&self, factor: &Factor) -> Value {
        match factor {
            Factor::Data { index } => i64::from(self.data.get(*index).copied().unwrap_or(0)).into(),
            Factor::Slot { slot, classes, other } => match self.stacks.get(*slot) {
                Some(Some(item)) => classes.get(item).copied().unwrap_or(*other).into(),
                _ => 0.into(),
            },
            Factor::Heat => self.heat.into(),
            Factor::Light => self.light.into(),
            Factor::Running => self.running.into(),
            Factor::Toggle { name } => self.toggles.contains(name).into(),
            Factor::Tank => self.tank.as_ref().map_or(0, |t| t.1).into(),
            Factor::Fluid { classes, other } => match &self.tank {
                Some((fluid, _)) => classes.get(fluid).copied().unwrap_or(*other).into(),
                None => 0.into(),
            },
        }
    }

    /// The case the machine is in: every factor has the case's value, or lies in its range.
    pub fn case<'c, T>(&self, factors: &[Factor], cases: &'c [Case<T>]) -> Option<&'c T> {
        let values: Vec<Value> = factors.iter().map(|f| self.value(f)).collect();
        cases
            .iter()
            .find(|case| {
                case.when.iter().zip(&values).all(|(when, value)| match when {
                    Value::Null => true,
                    Value::Array(range) => {
                        let at = value.as_i64().unwrap_or(0);
                        range[0].as_i64().is_some_and(|from| from <= at)
                            && range[1].as_i64().is_some_and(|to| at <= to)
                    }
                    other => other == value || other.as_i64().is_some_and(|n| Some(n) == value.as_i64()),
                })
            })
            .map(|case| &case.then)
    }
}

fn number(draw: &Draw, at: usize) -> f32 {
    draw.get(at).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

/// `plerocottage:textures/gui/x.png` is the GUI sprite `plerocottage:gui/x`.
fn sprite(texture: &str) -> String {
    texture
        .replacen("textures/", "", 1)
        .trim_end_matches(".png")
        .to_owned()
}

/// One call of the screen's `GuiGraphics`, at the screen's top left `origin`, inside `scissor`.
fn draw(
    host: &dyn Host,
    batch: &mut HudBatch,
    origin: [f32; 2],
    call: &Draw,
    scissor: Option<[f32; 4]>,
    data: [i32; 2],
) {
    let kind = call.first().and_then(Value::as_str);
    let text = |batch: &mut HudBatch, text: &str, from: usize| {
        let (x, y) = (number(call, from), number(call, from + 1));
        let colour = call[from + 2].as_i64().unwrap_or(-1) as u32 & 0xFF_FFFF;
        let shadow = call.get(from + 3).is_some_and(|s| s.as_i64() != Some(0) && s.as_bool() != Some(false));
        // Where the mod's code places it by its width.
        let align = call.get(from + 4).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let x = x - align * host.text_width(text);
        host.text(batch, text, [origin[0] + x, origin[1] + y], colour, shadow);
    };
    match kind {
        Some("blit") => {
            // blit(texture, x, y, u, v, w, h[, textureWidth, textureHeight]); a fluid's still picture comes as
            // blit(x, y, z, w, h, sprite, r, g, b, a), its whole frame.
            let (name, x, y, u, v, w, h) = match call.get(1) {
                Some(Value::String(texture)) => (
                    sprite(texture),
                    number(call, 2),
                    number(call, 3),
                    number(call, 4),
                    number(call, 5),
                    number(call, 6),
                    number(call, 7),
                ),
                _ => (
                    // A picture of the block atlas: the content mod carries it among its GUI textures (`block/x` as
                    // `gui/block/x`), where a screen's sprites are found.
                    call.get(6).and_then(Value::as_str).unwrap_or_default().replacen(':', ":gui/", 1),
                    number(call, 1),
                    number(call, 2),
                    0.0,
                    0.0,
                    number(call, 4),
                    number(call, 5),
                ),
            };
            let (mut src, mut dst) = ([u, v, w, h], [x, y, w, h]);
            // A sprite's whole frame fills its place, however large that is: the frame's size follows its colour.
            let frame = match call.get(1) {
                Some(Value::String(_)) => None,
                _ => Some([number(call, 11), number(call, 12)]).filter(|f| f[0] > 0.0 && f[1] > 0.0),
            };
            if let Some([wide, high]) = frame {
                src = [0.0, 0.0, wide, high];
            }
            if let Some([left, top, right, bottom]) = scissor {
                let (x0, y0) = (dst[0].max(left), dst[1].max(top));
                let (x1, y1) = ((dst[0] + dst[2]).min(right), (dst[1] + dst[3]).min(bottom));
                if x1 <= x0 || y1 <= y0 {
                    return;
                }
                // The part of the picture that shows: by the pixel where it is drawn one to one, by its share otherwise.
                let (sx, sy) = (src[2] / dst[2], src[3] / dst[3]);
                src = [src[0] + (x0 - dst[0]) * sx, src[1] + (y0 - dst[1]) * sy, (x1 - x0) * sx, (y1 - y0) * sy];
                dst = [x0, y0, x1 - x0, y1 - y0];
            }
            dst[0] += origin[0];
            dst[1] += origin[1];
            host.sprite(batch, &name, src, dst);
        }
        Some("text") => {
            let shown = call[1].as_str().unwrap_or_default().to_owned();
            text(batch, &shown, 2);
        }
        Some("number") => {
            // A text that counts: so many of the clock, so many of its end, and a number.
            let sum = &call[1];
            let part = |name: &str| sum[name].as_i64().unwrap_or(0);
            let value = part("clock") * i64::from(data[0]) + part("end") * i64::from(data[1]) + part("plus");
            let shown = format!(
                "{}{value}{}",
                call[2].as_str().unwrap_or_default(),
                call[3].as_str().unwrap_or_default()
            );
            text(batch, &shown, 4);
        }
        _ => {}
    }
}

/// Everything the screen draws in the machine's state, in the game's order.
pub fn draw_screen(host: &dyn Host, batch: &mut HudBatch, origin: [f32; 2], screen: &Screen2, seen: &Seen<'_>) {
    for layer in &screen.layers {
        let Some(found) = seen.case(&layer.factors, &layer.cases) else {
            continue;
        };
        // The data slots a counting text counts with: the layer's clock and its end.
        let mut counted = layer.factors.iter().filter_map(|f| match f {
            Factor::Data { index } => Some(seen.data.get(*index).copied().unwrap_or(0)),
            _ => None,
        });
        let data = [counted.next().unwrap_or(0), counted.next().unwrap_or(0)];
        let mut scissor = None;
        for call in &found.draws {
            match call.first().and_then(Value::as_str) {
                Some("scissor") => {
                    scissor = Some([number(call, 1), number(call, 2), number(call, 3), number(call, 4)]);
                }
                Some("endScissor") => scissor = None,
                _ => draw(host, batch, origin, call, scissor, data),
            }
        }
    }
}

fn hit(button: &Button2, at: [f32; 2]) -> bool {
    button
        .hit
        .iter()
        .any(|[x, y, w, h]| at[0] >= *x && at[1] >= *y && at[0] < x + w && at[1] < y + h)
}

/// A click at `at`: the first button it reaches that can be pressed turns its switches, and sends
/// its packet (the number of it in `screen.packets`) if it sends one.
pub fn click(screen: &Screen2, seen: &Seen<'_>, at: [f32; 2]) -> Option<i32> {
    for button in screen.buttons.iter().filter(|b| hit(b, at)) {
        let Some(pressed) = seen.case(&button.factors, &button.cases) else {
            continue;
        };
        if !pressed.active {
            continue;
        }
        if let Ok(mut toggles) = TOGGLES.lock() {
            let set = toggles.get_or_insert_with(HashSet::new);
            for name in &pressed.toggles {
                if !set.remove(name) {
                    set.insert(name.clone());
                }
            }
        }
        let sent = pressed.sends.first().and_then(|sent| screen.packets.iter().position(|p| p == sent));
        // What the press says instead goes to the server as a button past the packets: it tells the player.
        let said = pressed.says.first().and_then(|key| says(screen).iter().position(|k| k == key));
        return sent.or(said.map(|n| screen.packets.len() + n)).map(|n| n as i32);
    }
    None
}

/// Everything a press of the screen's buttons may tell the player, in the buttons' order.
pub fn says(screen: &Screen2) -> Vec<&str> {
    let mut said: Vec<&str> = Vec::new();
    for key in screen.buttons.iter().flat_map(|b| &b.cases).flat_map(|c| &c.then.says) {
        if !said.contains(&key.as_str()) {
            said.push(key);
        }
    }
    said
}

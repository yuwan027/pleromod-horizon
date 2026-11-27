//! The page's bridge to the shared world's API (`globalThis.horizonPlay`), on the page's own
//! thread. The page sets `horizonPlay.api` to the API's address (`…/v1/play`), and may set
//! `horizonPlay.request(method, path, body, { keepalive })`, a function answering
//! `{ status, text }`, to send the requests itself; without it they are `fetch`es with the
//! page's cookies. Requests are started here and their answers taken later, once ready:
//! nothing waits for the network.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
const answers = new Map();
let next = 1;
let regained = false;
let farewell = [];
let listening = false;
function listen() {
    if (listening || typeof document === "undefined") return;
    listening = true;
    addEventListener("focus", () => (regained = true));
    document.addEventListener("visibilitychange", () => {
        if (document.visibilityState === "visible") regained = true;
    });
    // Only once the page goes: a page merely hidden goes on sending its batches itself, and
    // the same actions sent twice under two batch ids would be applied twice.
    addEventListener("pagehide", sendFarewell);
}
function send(method, path, body, keepalive) {
    const play = globalThis.horizonPlay;
    if (play && typeof play.request === "function") return play.request(method, path, body, { keepalive });
    const init = { method, credentials: "include", keepalive, cache: "no-store" };
    if (body !== undefined && body !== null && body !== "") {
        init.body = body;
        init.headers = { "content-type": "application/json" };
    }
    return fetch(`${play.api}${path}`, init).then(async (r) => ({ status: r.status, text: await r.text() }));
}
// Once the page is going away: what the game had not sent yet, with keepalive (which the
// browser takes only up to 64 KiB, so a larger body goes without it).
function sendFarewell() {
    const last = farewell;
    farewell = [];
    for (const [method, path, body] of last) {
        try {
            send(method, path, body, body.length < 60000).catch(() => {});
        } catch (e) {}
    }
}
export function horizon_play_available() {
    const play = globalThis.horizonPlay;
    return !!play && (typeof play.request === "function" || typeof play.api === "string");
}
export function horizon_play_request(method, path, body) {
    listen();
    const id = next++;
    let started;
    try {
        started = Promise.resolve(send(method, path, body, false));
    } catch (e) {
        started = Promise.reject(e);
    }
    started.then(
        (r) => answers.set(id, [r.status | 0, String(r.text ?? "")]),
        (e) => answers.set(id, [0, String(e)]),
    );
    return id;
}
export function horizon_play_take(id) {
    const answer = answers.get(id);
    if (answer === undefined) return null;
    answers.delete(id);
    return answer;
}
export function horizon_play_regained() {
    listen();
    const was = regained;
    regained = false;
    return was;
}
export function horizon_play_farewell(list) {
    listen();
    farewell = list;
}
export function horizon_play_reload() {
    location.reload();
}
"#)]
extern "C" {
    fn horizon_play_available() -> bool;
    fn horizon_play_request(method: &str, path: &str, body: &str) -> u32;
    fn horizon_play_take(id: u32) -> JsValue;
    fn horizon_play_regained() -> bool;
    fn horizon_play_farewell(list: js_sys::Array);
    fn horizon_play_reload();
}

/// Whether the page gave the game a shared world to play in.
pub fn available() -> bool {
    horizon_play_available()
}

/// Starts a request to the API (`path` below `horizonPlay.api`); its answer comes from [`take`].
pub fn request(method: &str, path: &str, body: &str) -> u32 {
    horizon_play_request(method, path, body)
}

/// The answer to request `id` once it came: its HTTP status (0 when the network failed) and
/// body.
pub fn take(id: u32) -> Option<(u16, String)> {
    let answer = horizon_play_take(id);
    if answer.is_null() || answer.is_undefined() {
        return None;
    }
    let answer = js_sys::Array::from(&answer);
    let status = answer.get(0).as_f64().unwrap_or(0.0) as u16;
    Some((status, answer.get(1).as_string().unwrap_or_default()))
}

/// Whether the page came back into view or focus since the last call.
pub fn regained() -> bool {
    horizon_play_regained()
}

/// The requests to send if the page goes away before the game sends them itself: each
/// `(method, path, body)`, replacing those given before.
pub fn farewell(requests: &[(&str, &str, &str)]) {
    let list = js_sys::Array::new();
    for (method, path, body) in requests {
        let entry = js_sys::Array::new();
        entry.push(&JsValue::from_str(method));
        entry.push(&JsValue::from_str(path));
        entry.push(&JsValue::from_str(body));
        list.push(&entry);
    }
    horizon_play_farewell(list);
}

/// Loads the page again: the game joins the world anew.
pub fn reload() {
    horizon_play_reload();
}

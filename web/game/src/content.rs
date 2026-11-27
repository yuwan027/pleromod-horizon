//! The browser game built for the machine that builds the page: web/build.sh runs it once to
//! build the content root the page's game would build at every start (HORIZON_WEB_CONTENT).
//! The same program as the page's, with the same mods, under a name of its own.
#![forbid(unsafe_code)]

#[path = "main.rs"]
mod page;

fn main() {
    page::main();
}

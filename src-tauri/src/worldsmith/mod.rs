//! WORLDSMITH (S-1126-7, S-1126-9): the AI cheat engine for the member's own offline single-player
//! games, as a room. Its parts:
//!
//! - `helper`: the small user-mode memory helper (`worldsmith-helper.exe`, its own crate in this repo,
//!   released on its own tag), never in the installer: fetched on the member's first press of Get it,
//!   its SHA-256 pinned here and checked again before every start.
//! - `pipe`: the helper's life (its `serve` starts only while the room is open and ends with the room,
//!   its switch or Castle) and the page's own calls (games, attach, a lock), through the same client
//!   verbs the Alice Keep uses.
//!
//! The helper refuses anti-cheat and online games at attach; Castle only passes on its one line.

pub mod companion;
pub mod helper;
pub mod keys;
pub mod page;
pub mod pipe;
pub mod room;
pub mod store;

/// The room's id (Settings > Rooms, the launcher, `rooms::need`).
pub const ROOM: &str = "worldsmith";

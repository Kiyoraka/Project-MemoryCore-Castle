//! Scriptorium: our own word processor (Scroll), spreadsheet (Ledger), slides (Stage) and PDF editor
//! (Folio), with an AI chat beside every page. Everything here is ours - Rust's standard library and
//! nothing else. Rust thinks (files, documents, layout, formulas) and hands the host a display list;
//! the host's canvas only paints it. Every action is a command on the bus, and the AI's door
//! (`describe`, `read`, `do`, `shot`) works over the same commands.
//!
//! This crate holds nothing of any host: the MemoryCore Castle and the Alice Keep each reach it through
//! their own thin host edge.

pub mod ground;

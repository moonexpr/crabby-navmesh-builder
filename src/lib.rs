//! Crabby Navmesh Builder: generates Source engine navigation meshes
//! (`.nav`) from compiled maps (`.bsp`), and analyzes them.
//!
//! Three layers (see `docs/architecture.md`): `core` holds the math, the
//! domain values, and the algorithms; `format` parses and compiles bytes and
//! `spec` says what each game expects; `app` runs the tools' use cases and
//! renders their reports.

pub mod app;
pub mod core;
pub mod format;
pub mod spec;

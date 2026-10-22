//! Crabby Navmesh Builder: generates Source engine navigation meshes
//! (`.nav`) from compiled maps (`.bsp`).
//!
//! The pipeline is `bsp::parse` (raw bytes to valid solids and entities), a
//! [`collision::Collision`] world built from those solids, `generate::generate`
//! (a multi-threaded heightfield-to-areas pass), and `nav::encode`.
//! `analysis` and `report` back the `navinfo` and `navgen` tools' output.

pub mod analysis;
pub mod bsp;
pub mod cli;
pub mod collision;
pub mod generate;
pub mod geom;
pub mod nav;
pub mod report;
pub mod solid;

//! What is true whatever the game and the file: geometry, solids, collision
//! queries, a map's world, the nav mesh as a value, generation, and
//! analysis. No I/O and no byte layouts live here.

pub mod analysis;
pub mod collision;
pub mod generate;
pub mod geom;
pub mod mesh;
pub mod solid;
pub mod world;

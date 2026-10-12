//! Segmented HTTP download engine with `Range`, pause and resume (SPEC 6.2).
//! Milestone 0: structure only, no download logic.

pub mod segments;
pub mod worker;

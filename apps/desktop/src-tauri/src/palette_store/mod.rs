//! What the palette searches, kept below the areas that write it (projects, agents) and the
//! palette that reads it. A write bumps a revision the palette follows.

pub(crate) mod lists;
pub(crate) mod places;
mod store;

pub(crate) use store::PaletteStore;

//! What the palette searches, kept below the areas that write or read it. A write to the lists
//! bumps a revision the palette follows.

#[cfg(desktop)]
pub(crate) mod copies;
pub(crate) mod lists;
pub(crate) mod places;
mod store;

pub(crate) use store::PaletteStore;

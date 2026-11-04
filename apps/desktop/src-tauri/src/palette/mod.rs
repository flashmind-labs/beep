//! The command center's Cmd+K search. The app keeps an index of everything
//! it finds (`index`), built from lists read in the background and saved
//! for the next launch (`lists`); the window ranks it in memory on each
//! keystroke (`palette-rank.ts`), with no call to this side.

pub(crate) mod index;
pub(crate) mod lists;
mod projects;

pub(crate) use lists::PaletteCache;

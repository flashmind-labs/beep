//! The user's files: shared folders (`folders`), project sync (`sync`,
//! `syncthing`), node-to-node transfers (`transfer`) and the search for
//! git checkouts (`search`).

host! {
    mod folders;
    pub(crate) mod fsat;
    pub(crate) mod grants;
    pub(crate) mod place;
    pub(crate) mod note;
    pub(crate) mod paths;
    pub(crate) mod search;
    pub mod sync;
    pub mod syncthing;
    pub mod transfer;

    pub use folders::Folders;
}

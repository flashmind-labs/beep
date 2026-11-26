//! The user's files: shared folders (`folders`), project sync (`sync`,
//! `syncthing`) and node-to-node transfers (`transfer`).

host! {
    mod folders;
    pub(crate) mod git_read;
    pub(crate) mod grants;
    pub mod holds;
    pub(crate) mod place;
    pub mod sync;
    pub mod syncthing;
    pub mod transfer;

    pub use folders::Folders;
}

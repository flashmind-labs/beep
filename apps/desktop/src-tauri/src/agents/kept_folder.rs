//! Records waiting on another machine live in app data. Their image files
//! live in the app cache. Each queue has a thread that writes the records in
//! order, and a second that writes the large files and removes the forgotten
//! ones, so a record never waits on a file of hundreds of megabytes.

use crate::agents::steer::PastedImage;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fmt::Display,
    fs::{self, File},
    io::{self, ErrorKind, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

/// Image subfolder in the cache, formerly under the records folder.
pub(crate) const IMAGES: &str = "images";

/// The bytes of an image from which the records' thread leaves its write to the files' thread:
/// a record is written at once, and may name such an image a moment before it is on disk. A
/// crash between the two shows as a message whose image could not be read, never as a lost one.
const LARGE_IMAGE: usize = 1024 * 1024;

/// An image of a kept record. Its bytes, in base64, are kept in a file of
/// their own, not in the records' file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct KeptImage {
    pub(crate) name: String,
    pub(crate) mime: String,
    #[serde(skip)]
    /// Shared, so a copy of the record for a flusher or a writer does not copy the bytes.
    pub(crate) data: Arc<str>,
}

impl From<PastedImage> for KeptImage {
    fn from(image: PastedImage) -> Self {
        Self {
            name: image.name,
            mime: image.mime,
            data: image.data.into(),
        }
    }
}

impl From<KeptImage> for PastedImage {
    fn from(image: KeptImage) -> Self {
        Self {
            name: image.name,
            mime: image.mime,
            data: image.data.to_string(),
        }
    }
}

/// The file image `at` of record `id` is kept in.
pub(crate) fn image_file(id: impl Display, at: usize) -> String {
    format!("{id}-{at}")
}

/// What the records' thread hands on to the files' thread, in the order it came.
enum Later {
    Image { file: String, data: Arc<str> },
    Forget(Vec<String>),
    Synced(mpsc::Sender<()>),
}

/// A change to the folder, done in order by its thread.
pub(crate) enum Write {
    /// The records' file, as it is now.
    Records(Vec<u8>),
    /// An image of a record, kept until it goes.
    Image { file: String, data: Arc<str> },
    /// The images of a record that went.
    Forget(Vec<String>),
    /// Answers once what was asked before is on disk.
    Synced(mpsc::Sender<()>),
}

/// A kept folder and the thread that writes it.
pub(crate) struct KeptFolder {
    dir: PathBuf,
    images: PathBuf,
    records: &'static str,
    writes: mpsc::Sender<Write>,
}

impl KeptFolder {
    /// The folder `dir`, whose records are in its file `records`, with its
    /// writer thread `name` started.
    pub(crate) fn open(dir: PathBuf, cache: PathBuf, records: &'static str, name: &str) -> Self {
        let (writes, queue) = mpsc::channel();
        let writer = dir.clone();
        let images = cache.join(IMAGES);
        let cached = images.clone();
        let (later, files) = mpsc::channel();
        let (files_dir, files_images) = (dir.clone(), images.clone());
        let spawned = thread::Builder::new()
            .name(name.into())
            .spawn(move || write_all(&writer, &cached, records, &queue, &later))
            .and_then(|_| {
                thread::Builder::new()
                    .name(format!("{name}-files"))
                    .spawn(move || write_files(&files_dir, &files_images, &files))
            });

        if let Err(e) = spawned {
            log::warn!("starting the {name} writer: {e}");
        }

        Self {
            dir,
            images,
            records,
            writes,
        }
    }

    /// The records kept, or none when the file is missing or unreadable.
    pub(crate) fn read<T: DeserializeOwned>(&self) -> Vec<T> {
        let file = self.dir.join(self.records);

        match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("dropping the unreadable {}: {e}", file.display());
                Vec::new()
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                log::warn!("reading {}: {e}", file.display());
                Vec::new()
            }
        }
    }

    /// Reads back the data of `images`, kept for record `id`; false when
    /// one could not be read.
    pub(crate) fn read_images(&self, id: impl Display, images: &mut [KeptImage]) -> bool {
        let mut whole = true;

        for (at, image) in images.iter_mut().enumerate() {
            let file = image_file(&id, at);
            let data = fs::read_to_string(self.images.join(&file)).or_else(|error| {
                if error.kind() == ErrorKind::NotFound {
                    return fs::read_to_string(self.dir.join(IMAGES).join(file));
                }
                Err(error)
            });

            match data {
                Ok(data) => image.data = data.into(),
                Err(_) => whole = false,
            }
        }

        whole
    }

    /// Has `writes` done, in order.
    pub(crate) fn write(&self, writes: impl IntoIterator<Item = Write>) {
        for write in writes {
            let _ = self.writes.send(write);
        }
    }

    /// The write that replaces the records' file with `records`, for `write`.
    /// Image bytes are not part of it. Quick to make, so a caller may make it
    /// under its lock and hand it on after.
    pub(crate) fn records<T: Serialize>(&self, records: &[T]) -> Option<Write> {
        match serde_json::to_vec(records) {
            Ok(bytes) => Some(Write::Records(bytes)),
            Err(e) => {
                log::warn!("keeping {}: {e}", self.dir.display());
                None
            }
        }
    }

    /// Returns once what changed so far is on disk.
    pub(crate) fn sync(&self) {
        let (done, wait) = mpsc::channel();

        if self.writes.send(Write::Synced(done)).is_ok() {
            let _ = wait.recv();
        }
    }
}

/// The writes of the images of record `id`, to keep them.
pub(crate) fn image_writes(id: impl Display, images: &[KeptImage]) -> Vec<Write> {
    images
        .iter()
        .enumerate()
        .map(|(at, image)| Write::Image {
            file: image_file(&id, at),
            data: image.data.clone(),
        })
        .collect()
}

/// The write that forgets the `count` images of record `id`, if it has any.
pub(crate) fn forget_images(id: impl Display, count: usize) -> Option<Write> {
    (count > 0).then(|| Write::Forget((0..count).map(|at| image_file(&id, at)).collect()))
}

/// Does each change asked for, in order. Of several records' files asked
/// for together only the newest is written, after the small images it names. Large images,
/// the images forgotten and what waits to be answered go on to `write_files` after it, in
/// order: a crash between a record and its large image leaves a message whose image is
/// missing, which shows as a failed row, and a removed message never leaves a record that
/// names files already deleted.
fn write_all(
    dir: &Path,
    images: &Path,
    records: &str,
    queue: &mpsc::Receiver<Write>,
    later: &mpsc::Sender<Later>,
) {
    while let Ok(first) = queue.recv() {
        let mut latest = None;
        let mut behind = Vec::new();

        for write in std::iter::once(first).chain(queue.try_iter()) {
            match write {
                Write::Records(bytes) => latest = Some(bytes),
                Write::Image { file, data } if data.len() > LARGE_IMAGE => {
                    behind.push(Later::Image { file, data });
                }
                Write::Image { file, data } => {
                    if let Err(e) = replace(images, &file, data.as_bytes()) {
                        log::warn!("keeping an image in {}: {e}", dir.display());
                    }
                }
                Write::Forget(files) => behind.push(Later::Forget(files)),
                Write::Synced(done) => behind.push(Later::Synced(done)),
            }
        }

        if let Some(bytes) = latest
            && let Err(e) = replace(dir, records, &bytes)
        {
            log::warn!("writing {}: {e}", dir.display());
        }

        for write in behind {
            let _ = later.send(write);
        }
    }
}

/// Writes the large images and removes the forgotten ones, in the order `write_all` hands them on.
fn write_files(dir: &Path, images: &Path, queue: &mpsc::Receiver<Later>) {
    while let Ok(write) = queue.recv() {
        match write {
            Later::Image { file, data } => {
                if let Err(e) = replace(images, &file, data.as_bytes()) {
                    log::warn!("keeping an image in {}: {e}", dir.display());
                }
            }
            Later::Forget(files) => forget(dir, images, &files),
            Later::Synced(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// Removes the image files `files`, in the cache and where they were kept before.
fn forget(dir: &Path, images: &Path, files: &[String]) {
    let legacy = dir.join(IMAGES);

    for path in files
        .iter()
        .flat_map(|file| [images.join(file), legacy.join(file)])
    {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => log::warn!("removing an image in {}: {e}", dir.display()),
        }
    }
}

/// Replaces `dir/name` with `bytes` whole: written beside it, flushed to
/// the disk, then renamed over it.
fn replace(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{name}.part"));
    let mut file = File::create(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&partial, dir.join(name))?;

    // The rename itself reaches the disk with the folder; Windows cannot
    // open a folder this way, and there the rename is kept as it is.
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/agents_kept_folder.rs"]
mod tests;

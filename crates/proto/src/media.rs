//! Files and images sent to a coding agent.

/// The MIME type of a media file `GetAgentFile` reads, by the extension of
/// `path`, any case; `None` for any other file.
pub fn media_mime(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next()?;
    let (stem, ext) = name.rsplit_once('.')?;

    if stem.is_empty() {
        return None;
    }

    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "obj" => "model/obj",
        "stl" => "model/stl",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// A short name for an image's bytes, the same on the node and the desktop.
pub fn image_key(data: &[u8]) -> String {
    blake3::hash(data).to_hex()[..16].to_owned()
}

wire_structs! {
    /// A file sent with a prompt, such as a picture or a PDF.
    #[derive(Eq)]
    pub struct Upload {
        pub name: String,
        pub mime: String,
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }
}

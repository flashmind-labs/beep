//! Checking a release archive and taking the binary out of it.

use super::{
    target::{Format, Target},
    types::Error,
};
use sha2::{Digest, Sha256};
use std::io::{self, Cursor, Read};

/// Checks `packed` against its line in `sums`, a `SHA256SUMS` file.
pub(super) fn verify(archive: &str, packed: &[u8], sums: &str) -> Result<(), Error> {
    let expected = expected_sum(archive, sums).ok_or_else(|| Error::NotListed {
        archive: archive.to_owned(),
    })?;
    let actual = hex::encode(Sha256::digest(packed));

    if actual != expected {
        return Err(Error::Checksum {
            archive: archive.to_owned(),
            expected,
            actual,
        });
    }

    Ok(())
}

/// The checksum `sums` lists for `archive`, in either `sha256sum` mode.
fn expected_sum(archive: &str, sums: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        let name = fields.next()?;
        let named = name == archive || name.strip_prefix('*') == Some(archive);
        (named && fields.next().is_none()).then(|| sum.to_ascii_lowercase())
    })
}

/// The binary inside the release archive.
pub(super) fn unpack(target: Target, archive: &str, packed: &[u8]) -> Result<Vec<u8>, Error> {
    let unpack_error = |source| Error::Unpack {
        archive: archive.to_owned(),
        source,
    };
    let found = match Format::from(target) {
        Format::TarGz => untar(packed, target.binary()),
        Format::Zip => unzip(packed, target.binary()),
    }
    .map_err(unpack_error)?;
    found.ok_or_else(|| Error::Missing {
        archive: archive.to_owned(),
        binary: target.binary(),
    })
}

/// The file named `binary` in a gzipped tarball.
fn untar(packed: &[u8], binary: &str) -> io::Result<Option<Vec<u8>>> {
    let mut tarball = tar::Archive::new(flate2::read::GzDecoder::new(packed));

    for entry in tarball.entries()? {
        let mut entry = entry?;
        let named = entry.path()?.file_name().is_some_and(|name| name == binary);

        if named && entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

/// The file named `binary` in a zip archive.
fn unzip(packed: &[u8], binary: &str) -> io::Result<Option<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(packed))?;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let named = file
            .enclosed_name()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == binary));
        if named && file.is_file() {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

#[cfg(test)]
#[path = "../../tests/upkeep_update_archive.rs"]
mod tests;

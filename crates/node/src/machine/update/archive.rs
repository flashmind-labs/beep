//! Checking a release archive and taking the binary out of it.

use super::{
    Error,
    target::{Format, Target},
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
    let found = match target.format() {
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
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn the_sum_is_read_in_either_mode() {
        let sums = "AAAA  beep-node-1-x.tar.gz\nbbbb *beep-node-1-y.zip\n";
        assert_eq!(
            expected_sum("beep-node-1-x.tar.gz", sums).as_deref(),
            Some("aaaa")
        );
        assert_eq!(
            expected_sum("beep-node-1-y.zip", sums).as_deref(),
            Some("bbbb")
        );
        assert_eq!(expected_sum("beep-node-1-z.zip", sums), None);
    }

    #[test]
    fn a_download_that_does_not_match_its_sum_is_refused() {
        let packed = b"archive";
        let sum = hex::encode(Sha256::digest(packed));
        let sums = format!("{sum}  a.tar.gz\n");
        assert!(verify("a.tar.gz", packed, &sums).is_ok());
        assert!(matches!(
            verify("a.tar.gz", b"tampered", &sums),
            Err(Error::Checksum { .. })
        ));
        assert!(matches!(
            verify("b.tar.gz", packed, &sums),
            Err(Error::NotListed { .. })
        ));
    }

    fn tarball(name: &str, body: &[u8]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, body)
            .expect("appending to an in-memory tarball");
        builder
            .into_inner()
            .and_then(|gz| gz.finish())
            .expect("finishing an in-memory tarball")
    }

    #[test]
    fn the_binary_comes_out_of_the_tarball() {
        let packed = tarball("beep-node", b"new binary");
        let found = unpack(Target::LinuxX86_64, "a.tar.gz", &packed).expect("unpacked");
        assert_eq!(found, b"new binary");

        let other = tarball("README", b"text");
        assert!(matches!(
            unpack(Target::MacAarch64, "a.tar.gz", &other),
            Err(Error::Missing { .. })
        ));
        assert!(matches!(
            unpack(Target::LinuxX86_64, "a.tar.gz", b"not gzip"),
            Err(Error::Unpack { .. })
        ));
    }

    #[test]
    fn the_binary_comes_out_of_the_zip() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("beep-node.exe", zip::write::SimpleFileOptions::default())
            .expect("starting an in-memory zip entry");
        zip.write_all(b"new exe").expect("writing to memory");
        let packed = zip
            .finish()
            .expect("finishing an in-memory zip")
            .into_inner();
        let found = unpack(Target::WindowsX86_64, "a.zip", &packed).expect("unpacked");
        assert_eq!(found, b"new exe");
    }
}

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

use super::*;
use iroh::SecretKey;

fn sender() -> Sender {
    Sender {
        id: SecretKey::generate().public(),
        mesh: None,
        app: "desktop",
        app_version: "0.2.0".into(),
    }
}

fn share(name: &str, log: String, status: &str) -> Share {
    Share {
        id: SecretKey::generate().public(),
        name: name.into(),
        kind: DeviceKind::Node,
        answer: Answer::Answered,
        log,
        status: status.into(),
        version: "0.1.0".into(),
        os: "linux".into(),
        os_version: "6.1".into(),
    }
}

#[test]
fn a_draft_redacts_logs_status_and_text() {
    let shares = vec![share(
        "build-box",
        "GET /x?token=abc&page=2\n".into(),
        "Authorization: Bearer abc.def\n",
    )];
    let d = draft(
        "my key is ghp_abcdefghijklmnopqrstuvwx",
        Period::Today,
        0,
        sender(),
        shares,
    )
    .expect("a draft of plain text builds");
    assert_eq!(d.manifest.text, "my key is [redacted]");
    assert_eq!(d.files.len(), 2);

    for file in &d.files {
        assert!(file.text.contains("[redacted]"), "{}", file.text);
        assert!(!file.text.contains("abc"), "{}", file.text);
    }

    assert!(d.files[0].path.starts_with("build-box-"));
    assert!(d.files[0].path.ends_with("/log.txt"));
    assert_eq!(d.manifest.files[0].bytes, d.files[0].text.len() as u64);
}

#[test]
fn a_draft_leaves_out_empty_files() {
    let d = draft(
        "",
        Period::Hour,
        0,
        sender(),
        vec![share("gpu-box", String::new(), "")],
    )
    .expect("an empty share drafts");
    assert!(d.files.is_empty());
    assert_eq!(d.manifest.machines.len(), 1);
}

#[test]
fn a_machine_name_cannot_leave_its_folder() {
    let id = SecretKey::generate().public();
    let short = &id.to_string()[..6];
    assert_eq!(folder("../..\\c:/x", id), format!("----c--x-{short}"));
    assert_eq!(folder(".hidden", id), format!("-hidden-{short}"));
    assert_eq!(
        folder("a*b?c\"<d>|e\tf", id),
        format!("a-b-c--d--e-f-{short}")
    );
}

#[test]
fn zip_refuses_a_file_outside_a_machine_folder() {
    for path in [
        "../log.txt",
        "/log.txt",
        "build-box/../../log.txt",
        "build-box/notes.txt",
        "c:/log.txt",
        "log.txt",
    ] {
        let mut d = draft("", Period::Today, 0, sender(), Vec::new()).expect("an empty draft");
        d.files.push(DraftFile {
            path: path.into(),
            text: "x\n".into(),
        });
        assert!(matches!(zip(&d), Err(BundleError::BadPath(_))), "{path}");
    }
}

#[test]
fn zip_round_trips_the_manifest() {
    let shares = vec![share("build-box", "started\n".into(), "running\n")];
    let d = draft("it hangs", Period::Today, 1_700_000_000, sender(), shares)
        .expect("a plain draft builds");
    let body = zip(&d).expect("a small draft zips");
    let back = read_manifest(&body).expect("the zip has its manifest");
    assert_eq!(
        serde_json::to_value(&back).expect("a manifest is JSON"),
        serde_json::to_value(&d.manifest).expect("a manifest is JSON")
    );
}

#[test]
fn zip_redacts_a_draft_changed_after_drafting() {
    let mut d = draft("", Period::Today, 0, sender(), Vec::new()).expect("an empty draft");
    d.files.push(DraftFile {
        path: "build-box-abcdef/log.txt".into(),
        text: "sent Bearer abc123XYZ now\n".into(),
    });
    let body = zip(&d).expect("the draft zips");
    let mut archive = ZipArchive::new(Cursor::new(body)).expect("a zip reads back");
    let mut text = String::new();
    archive
        .by_name("build-box-abcdef/log.txt")
        .expect("the file is in the zip")
        .read_to_string(&mut text)
        .expect("the file reads");
    assert_eq!(text, "sent Bearer [redacted] now\n");
}

#[test]
fn a_zip_without_a_manifest_says_so() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("x.txt", SimpleFileOptions::default())
        .expect("a file starts");
    let body = writer.finish().expect("the zip ends").into_inner();
    assert!(matches!(read_manifest(&body), Err(BundleError::NoManifest)));
}

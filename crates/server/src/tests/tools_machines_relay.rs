use super::*;
use iroh::SecretKey;

fn exec() -> Work {
    Work::Exec {
        command: "ls".into(),
        machine: None,
        cwd: None,
    }
}

#[tokio::test]
async fn work_needs_a_desktop_with_its_stream_open() {
    let desktops = Desktops::default();
    assert_eq!(
        run(&desktops, "u", exec()).await,
        Err(NO_DESKTOP.to_owned())
    );
    let open = desktops.open("u", SecretKey::generate().public());
    assert_eq!(run(&desktops, "u", exec()).await, Err(NOT_BUILT.to_owned()));
    assert_eq!(
        run(&desktops, "other", exec()).await,
        Err(NO_DESKTOP.to_owned())
    );
    drop(open);
    assert_eq!(
        run(&desktops, "u", exec()).await,
        Err(NO_DESKTOP.to_owned())
    );
}

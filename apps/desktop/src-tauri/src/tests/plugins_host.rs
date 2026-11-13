use super::*;

#[test]
fn the_app_data_folder_holds_plugins_by_default() {
    let data = Path::new("/data/app");

    assert_eq!(plugins_dir(None, data), data.join("plugins"));
    assert_eq!(
        plugins_dir(Some(OsString::new()), data),
        data.join("plugins")
    );
}

#[test]
fn the_environment_names_another_folder() {
    let dir = plugins_dir(
        Some(OsString::from("/work/examples")),
        Path::new("/data/app"),
    );

    assert_eq!(dir, Path::new("/work/examples"));
}

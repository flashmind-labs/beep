use beep_plugins::Manifest;

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

#[test]
fn the_embedded_composer_cat_matches_its_source() {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/examples/composer-cat");
    let manifest = Manifest::load(&source).unwrap();
    let embedded = |name: &str| COMPOSER_CAT.iter().find(|(file, _)| *file == name);

    assert_eq!(builtins()[0].id, manifest.id);
    assert!(embedded("beep-plugin.json").is_some());
    assert!(embedded(&manifest.entry.to_string_lossy()).is_some());

    // Every file `main.js` imports must be embedded too.
    let main = std::fs::read_to_string(source.join("main.js")).unwrap();
    let imports: Vec<&str> = main
        .lines()
        .filter(|line| line.starts_with("import "))
        .filter_map(|line| line.split('"').nth(1))
        .collect();
    for import in imports {
        assert!(
            embedded(import.trim_start_matches("./")).is_some(),
            "{import}"
        );
    }

    // Every frame `main.js` names through `art("...")` must be embedded.
    let frames: Vec<&str> = main
        .split("art(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert!(!frames.is_empty());
    for frame in frames {
        assert!(
            embedded(&format!("art/cat-{frame}.png")).is_some(),
            "{frame}"
        );
    }

    // Each embedded file is the one on disk, and every art file on disk is embedded.
    for (name, bytes) in COMPOSER_CAT {
        assert_eq!(&std::fs::read(source.join(name)).unwrap(), bytes, "{name}");
    }
    let art = std::fs::read_dir(source.join("art")).unwrap().count();
    assert_eq!(art, COMPOSER_CAT.len() - 2);
}

#[test]
fn every_built_in_plugin_is_embedded_under_its_id() {
    let ids: Vec<String> = builtins()
        .iter()
        .map(|builtin| {
            let (_, text) = builtin
                .files
                .iter()
                .find(|(file, _)| *file == "beep-plugin.json")
                .unwrap();
            let manifest = Manifest::parse(std::str::from_utf8(text).unwrap()).unwrap();
            assert_eq!(manifest.id, builtin.id);
            builtin.id.to_string()
        })
        .collect();

    assert_eq!(ids, ["composer-cat", "dot-matrix", "hardware-dark"]);
}

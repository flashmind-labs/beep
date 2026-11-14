use super::*;

#[test]
fn a_screen_dump_lists_what_says_something_or_has_an_id() {
    let dumped = "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>\
        <hierarchy rotation=\"0\">\
        <node index=\"0\" text=\"\" resource-id=\"\" class=\"android.widget.FrameLayout\" content-desc=\"\" bounds=\"[0,0][1000,2000]\">\
        <node index=\"0\" text=\"Sign in\" resource-id=\"com.example:id/login\" class=\"android.widget.Button\" content-desc=\"\" bounds=\"[100,200][300,300]\" />\
        <node index=\"1\" text=\"\" resource-id=\"\" class=\"android.widget.ImageView\" content-desc=\"Back\" bounds=\"[0,0][100,100]\" />\
        <node index=\"2\" text=\"\" resource-id=\"\" class=\"android.view.View\" content-desc=\"\" bounds=\"[0,0][10,10]\" />\
        </node></hierarchy>UI hierchary dumped to: /dev/tty";
    let found = elements(dumped).expect("read");
    assert_eq!(
        found,
        [
            Element {
                label: "Sign in".to_owned(),
                kind: "Button".to_owned(),
                id: "login".to_owned(),
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.05,
            },
            Element {
                label: "Back".to_owned(),
                kind: "ImageView".to_owned(),
                id: String::new(),
                x: 0.0,
                y: 0.0,
                width: 0.1,
                height: 0.05,
            },
        ]
    );
}

#[test]
fn only_package_names_are_launched_or_cleared() {
    assert!(package("com.example.app").is_ok());
    assert!(package("com.example; rm -rf /").is_err());
    assert!(package("app").is_err());
}

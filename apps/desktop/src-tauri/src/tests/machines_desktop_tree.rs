use super::*;

fn node(role: &str, title: Option<&str>, handle: u32, children: Vec<Node<u32>>) -> Node<u32> {
    Node {
        role: role.into(),
        title: title.map(str::to_owned),
        enabled: true,
        visible: true,
        children,
        handle,
        ..Node::default()
    }
}

#[test]
fn containers_drop_out_and_controls_are_numbered() {
    let mut field = node("AXTextField", None, 4, vec![]);
    field.description = Some("Search".into());
    field.value = Some("rust".into());
    field.focused = true;
    let mut text = node("AXStaticText", None, 5, vec![]);
    text.value = Some("3 results".into());
    let mut off = node("AXButton", Some("Next"), 6, vec![]);
    off.enabled = false;
    let hidden = Node {
        visible: false,
        ..node("AXButton", Some("Hidden"), 7, vec![])
    };
    let window = node(
        "AXWindow",
        Some("Docs"),
        1,
        vec![node(
            "AXGroup",
            None,
            2,
            vec![
                node("AXButton", Some("Back"), 3, vec![]),
                node("AXScrollArea", None, 8, vec![field, text]),
                off,
                hidden,
            ],
        )],
    );
    let compact = compact(window, 10);
    assert_eq!(
        compact.text,
        "window \"Docs\"\n  \
             [1] button \"Back\"\n  \
             [2] text field \"Search\" value=\"rust\" (focused)\n  \
             text \"3 results\"\n  \
             [3] button \"Next\" (disabled)"
    );
    assert_eq!(compact.handles, [3, 4, 6]);
    assert!(!compact.truncated);
}

#[test]
fn a_pressable_element_is_numbered_whatever_its_role() {
    let mut image = node("AXImage", Some("Avatar"), 2, vec![]);
    image.pressable = true;
    let compact = compact(node("AXWindow", None, 1, vec![image]), 10);
    assert_eq!(compact.text, "window\n  [1] image \"Avatar\"");
}

#[test]
fn long_trees_are_cut_at_the_limit() {
    let buttons = (0..50)
        .map(|i| node("AXButton", Some(&format!("b{i}")), i, vec![]))
        .collect();
    let compact = compact(node("AXWindow", Some("w"), 99, buttons), 5);
    assert_eq!(compact.handles.len(), 5);
    assert!(compact.truncated);
}

#[test]
fn long_labels_are_clipped_to_one_line() {
    assert_eq!(clip("short"), "short");
    assert_eq!(clip("two\nlines"), "two...");
    assert!(clip(&"x".repeat(200)).ends_with("..."));
    assert_eq!(role_name("AXPopUpButton"), "pop up button");
    assert_eq!(role_name("AXStaticText"), "text");
}

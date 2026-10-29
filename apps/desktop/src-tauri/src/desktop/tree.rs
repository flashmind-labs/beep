//! Turns an accessibility tree into the few lines a model reads: one
//! element a line, indented by depth, the ones that can be clicked numbered.
//! Unlabelled containers drop out and their children move up, which is most
//! of what an app's tree is.

use std::fmt::Write;

/// The longest label shown; longer ones are cut.
const MAX_LABEL: usize = 80;
/// How many unnumbered lines, such as text, may come per numbered element.
const LINES_PER_ELEMENT: usize = 3;

/// One element as read from the app, its children already read.
#[derive(Debug, Clone, Default)]
pub struct Node<H> {
    /// The accessibility role, such as `AXButton`.
    pub role: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub focused: bool,
    /// It answers `AXPress`.
    pub pressable: bool,
    /// It has a size on screen.
    pub visible: bool,
    pub children: Vec<Node<H>>,
    pub handle: H,
}

/// The lines for the model and the handles their indexes name.
#[derive(Debug)]
pub struct Compact<H> {
    pub text: String,
    /// `handles[i]` is element `i + 1`.
    pub handles: Vec<H>,
    pub truncated: bool,
}

/// Roles worth a number even without a press action.
const CONTROLS: &[&str] = &[
    "AXButton",
    "AXCheckBox",
    "AXRadioButton",
    "AXTextField",
    "AXTextArea",
    "AXSearchField",
    "AXSecureTextField",
    "AXComboBox",
    "AXPopUpButton",
    "AXMenuButton",
    "AXMenuItem",
    "AXMenuBarItem",
    "AXLink",
    "AXTab",
    "AXSlider",
    "AXIncrementor",
    "AXDisclosureTriangle",
    "AXCell",
    "AXRow",
    "AXSwitch",
    "AXToggle",
    "AXDockItem",
];

/// Roles that only hold others; without a label they are left out.
const CONTAINERS: &[&str] = &[
    "AXGroup",
    "AXScrollArea",
    "AXSplitGroup",
    "AXLayoutArea",
    "AXLayoutItem",
    "AXUnknown",
    "AXSplitter",
    "AXToolbar",
    "AXTabGroup",
    "AXList",
    "AXOutline",
    "AXTable",
    "AXColumn",
    "AXBrowser",
    "AXWebArea",
    "AXGenericElement",
    "AXSection",
];

/// Compacts `root`, numbering at most `max_elements` elements.
pub fn compact<H>(root: Node<H>, max_elements: usize) -> Compact<H> {
    let mut out = Compact {
        text: String::new(),
        handles: Vec::new(),
        truncated: false,
    };
    let max_lines = max_elements * (LINES_PER_ELEMENT + 1);
    let mut lines = 0;
    walk(root, 0, max_elements, max_lines, &mut lines, &mut out);
    if out.text.ends_with('\n') {
        out.text.pop();
    }
    out
}

fn walk<H>(
    node: Node<H>,
    depth: usize,
    max_elements: usize,
    max_lines: usize,
    lines: &mut usize,
    out: &mut Compact<H>,
) {
    if !node.visible && depth > 0 {
        return;
    }
    if out.handles.len() >= max_elements || *lines >= max_lines {
        out.truncated = true;
        return;
    }
    let label = label(&node);
    let control = node.pressable || CONTROLS.contains(&node.role.as_str());
    let shown =
        depth == 0 || control || label.is_some() && !CONTAINERS.contains(&node.role.as_str());
    let Node {
        role,
        value,
        enabled,
        focused,
        children,
        handle,
        ..
    } = node;
    let child_depth = if shown {
        let indent = "  ".repeat(depth);
        let index = if control {
            out.handles.push(handle);
            format!("[{}] ", out.handles.len())
        } else {
            String::new()
        };
        let _ = write!(out.text, "{indent}{index}{}", role_name(&role));
        if let Some(label) = &label {
            let _ = write!(out.text, " {label:?}");
        }
        // A field's value is what it holds, not its name.
        if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty())
            && label.as_deref() != Some(&clip(value))
        {
            let _ = write!(out.text, " value={:?}", clip(value));
        }
        if !enabled {
            out.text.push_str(" (disabled)");
        }
        if focused {
            out.text.push_str(" (focused)");
        }
        out.text.push('\n');
        *lines += 1;
        depth + 1
    } else {
        depth
    };
    for child in children {
        walk(child, child_depth, max_elements, max_lines, lines, out);
    }
}

/// What the element is called: its title, else its description, else the
/// text it shows.
fn label<H>(node: &Node<H>) -> Option<String> {
    let text_role = matches!(node.role.as_str(), "AXStaticText" | "AXHeading");
    [&node.title, &node.description]
        .into_iter()
        .chain(text_role.then_some(&node.value))
        .flatten()
        .map(|text| text.trim())
        .find(|text| !text.is_empty())
        .map(clip)
}

fn clip(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= MAX_LABEL && line.len() == text.len() {
        return line.to_owned();
    }
    let cut: String = line.chars().take(MAX_LABEL).collect();
    format!("{cut}...")
}

/// `AXPopUpButton` reads as `pop up button`, `AXStaticText` as `text`.
fn role_name(role: &str) -> String {
    let bare = role.strip_prefix("AX").unwrap_or(role);
    if bare == "StaticText" {
        return "text".into();
    }
    let mut out = String::new();
    for (i, c) in bare.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    if out.is_empty() {
        "element".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
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
}

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
#[path = "../../tests/machines_desktop_tree.rs"]
mod tests;

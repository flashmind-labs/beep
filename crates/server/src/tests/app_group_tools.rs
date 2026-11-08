use super::*;

#[test]
fn every_group_tool_is_in_the_catalog() {
    for tool in GROUP_TOOLS {
        assert!(
            permissions::call_acts(tool, &Value::Null).is_some(),
            "{tool}"
        );
    }
}

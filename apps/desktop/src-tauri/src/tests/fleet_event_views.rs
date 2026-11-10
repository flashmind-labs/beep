use super::*;

#[test]
fn failed_views_replace_values_and_successful_null_is_still_a_value() {
    let mut views = Vec::new();
    append::<Value>(
        &mut views,
        "center_home",
        Err(crate::error::DesktopError::BadRequest(
            "temporary source failure".into(),
        )),
    );
    assert_eq!(
        serde_json::to_value(&views[0]).unwrap(),
        json!({
            "command": "center_home", "args": {}, "error": "temporary source failure",
        })
    );
    let recovered = View::new("center_home", json!({}), Value::Null).unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap(),
        json!({
            "command": "center_home", "args": {}, "value": null,
        })
    );
}

#[test]
fn processes_do_not_draw_home_again() {
    assert!(affected("processes").is_empty());
    assert_eq!(affected("agent"), ["home"]);
}

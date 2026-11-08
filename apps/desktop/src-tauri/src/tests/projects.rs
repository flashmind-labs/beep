use super::*;

#[test]
fn an_old_server_finds_nothing() {
    assert!(unknown_op(&DesktopError::Api(ApiError::bad_request(
        "unknown op 169"
    ))));
    assert!(!unknown_op(&DesktopError::Api(ApiError::bad_request(
        "offline"
    ))));
}

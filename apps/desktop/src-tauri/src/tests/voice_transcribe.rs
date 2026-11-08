use super::*;

#[test]
fn a_busy_server_is_told_apart() {
    assert!(busy(Some(&ApiError::Conflict {
        message: "live transcriptions busy".into()
    })));
    assert!(!busy(Some(&ApiError::Internal {})));
    assert!(!busy(None));
}

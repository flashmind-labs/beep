use super::*;

#[test]
fn pending_is_taken_once() {
    let pending = Pending::default();
    pending.put(CenterTarget::Settings);
    assert_eq!(pending.take(), Some(CenterTarget::Settings));
    assert_eq!(pending.take(), None);
}

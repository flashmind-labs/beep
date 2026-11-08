#[test]
fn every_op_is_routed_once() {
    super::router().check().unwrap();
}

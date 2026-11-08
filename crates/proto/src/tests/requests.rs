use super::*;

#[test]
fn a_request_reads_without_the_fields_that_default() {
    #[derive(Serialize)]
    struct Old {
        group_id: String,
        request_id: Uuid,
        message: String,
    }
    let old = Old {
        group_id: "g1".into(),
        request_id: Uuid::new_v4(),
        message: "hi".into(),
    };
    let decoded: SendToGroup = http::decode(&http::encode(&old).unwrap()).unwrap();
    assert_eq!(decoded.message, "hi");
    assert!(decoded.images.is_empty());
}

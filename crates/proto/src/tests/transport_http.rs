use super::*;
use iroh::SecretKey;

#[test]
fn a_request_path_is_its_name() {
    assert_eq!(path(Op::ListSessions), "/v1/ListSessions");
}

#[test]
fn a_struct_encodes_with_its_field_names_and_reads_without_new_ones() {
    #[derive(Serialize)]
    struct Newer {
        a: u8,
        b: u8,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct Older {
        a: u8,
    }
    let bytes = encode(&Newer { a: 1, b: 2 }).unwrap();
    assert_eq!(decode::<Older>(&bytes).unwrap(), Older { a: 1 });
}

#[test]
fn a_session_message_names_server_key_and_time() {
    let server = SecretKey::from_bytes(&[1; 32]).public();
    let key = SecretKey::from_bytes(&[2; 32]).public();
    let one = session_message(&server, &key, 7);
    assert_ne!(one, session_message(&key, &server, 7));
    assert_ne!(one, session_message(&server, &key, 8));
}

#[test]
fn a_tagged_frame_reads_back_through_the_codec() {
    use tokio_util::codec::Decoder;
    let mut buf = BytesMut::from(&tagged(OK, b"hi")[..]);
    let frame = codec().decode(&mut buf).unwrap().unwrap();
    assert_eq!(&frame[..], &[OK, b'h', b'i']);
}

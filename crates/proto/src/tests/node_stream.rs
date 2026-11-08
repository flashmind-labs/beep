use super::*;
use crate::node::Down;

#[tokio::test]
async fn a_link_writes_compact_or_named_as_it_was_built() {
    let (to_node, mut raw) = duplex(LOCAL_BUFFER);
    let mut tx = LinkTx::new(Out::Local(to_node), Encoding::Named);
    tx.send(&Down::Welcome { tools: true })
        .await
        .expect("sends");
    let mut named = vec![0u8; 64];
    let n = tokio::io::AsyncReadExt::read(&mut raw, &mut named)
        .await
        .expect("reads");
    assert!(named[..n].windows(5).any(|w| w == b"tools"));
    let (to_node, mut raw) = duplex(LOCAL_BUFFER);
    let mut tx = LinkTx::new(Out::Local(to_node), Encoding::Compact);
    tx.send(&Down::Welcome { tools: true })
        .await
        .expect("sends");
    let mut compact = vec![0u8; 64];
    let n = tokio::io::AsyncReadExt::read(&mut raw, &mut compact)
        .await
        .expect("reads");
    assert!(!compact[..n].windows(5).any(|w| w == b"tools"));
}

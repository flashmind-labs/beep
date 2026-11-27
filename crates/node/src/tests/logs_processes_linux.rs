use super::*;
use std::net::{Ipv4Addr, TcpListener};

#[tokio::test]
async fn listening_ports_are_matched_to_the_process_socket_inodes() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (boot, ticks) = timing().unwrap();
    let process = read(std::process::id(), boot, ticks).unwrap();
    let result = ports(&[process]).await;
    assert!(result.get(&std::process::id()).unwrap().contains(&port));
}

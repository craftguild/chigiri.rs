//! Wire proof that endpoint connection admission precedes HTTP 101.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;

use super::{REQUEST, assert_switching_response, endpoint};
use crate::streaming::streaming_service;

#[tokio::test]
async fn second_live_connection_is_rejected_before_101() {
    let (service, http_driver, guard) = streaming_service(endpoint()).unwrap();
    let (first_service, first_driver) = service.websocket_connection();
    let (second_service, second_driver) = service.websocket_connection();
    drop(service);

    let first_listener = bind().await;
    let first_address = first_listener.local_addr().unwrap();
    let (ready_send, ready_receive) = mpsc::sync_channel(1);
    let (release_send, release_receive) = mpsc::sync_channel(1);
    let first_client = std::thread::spawn(move || {
        let response = exchange_and_hold(first_address, ready_send, release_receive);
        assert_switching_response(&response);
    });
    let (first_stream, _) = first_listener.accept().await.unwrap();
    let first_connection = http1::Builder::new()
        .serve_connection(TokioIo::new(first_stream), first_service)
        .with_upgrades();
    let first_server = tokio::spawn(async move {
        let (connection, driver) = tokio::join!(first_connection, first_driver);
        connection.unwrap();
        driver.unwrap();
    });
    tokio::task::spawn_blocking(move || ready_receive.recv().unwrap())
        .await
        .unwrap();

    let second_listener = bind().await;
    let second_address = second_listener.local_addr().unwrap();
    let second_client = tokio::task::spawn_blocking(move || exchange(second_address));
    let (second_stream, _) = second_listener.accept().await.unwrap();
    let second_connection = http1::Builder::new()
        .serve_connection(TokioIo::new(second_stream), second_service)
        .with_upgrades();
    let (connection, driver, response) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(second_connection, second_driver, second_client)
    })
    .await
    .expect("capacity rejection must finish");
    connection.unwrap();
    driver.unwrap();
    let response = String::from_utf8(response.unwrap()).unwrap();
    assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    assert!(!response.contains("101 Switching Protocols"));
    assert!(!response.contains("sec-websocket-accept"));

    release_send.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), first_server)
        .await
        .expect("first connection must drain")
        .unwrap();
    first_client.join().unwrap();
    http_driver.await.unwrap();
    guard.shutdown().unwrap().await.unwrap();
}

async fn bind() -> tokio::net::TcpListener {
    tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap()
}

fn exchange(address: SocketAddr) -> Vec<u8> {
    let mut stream = connect(address);
    stream.write_all(REQUEST).unwrap();
    read_head(&mut stream)
}

fn exchange_and_hold(
    address: SocketAddr,
    ready: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
) -> Vec<u8> {
    let mut stream = connect(address);
    stream.write_all(REQUEST).unwrap();
    let response = read_head(&mut stream);
    ready.send(()).unwrap();
    release.recv().unwrap();
    response
}

fn connect(address: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
}

fn read_head(stream: &mut TcpStream) -> Vec<u8> {
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1);
        response.push(byte[0]);
    }
    response
}

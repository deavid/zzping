use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

fn spawn_echo_server() -> Child {
    Command::new("cargo")
        .args(&["run", "-p", "secure-echo-server"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn echo server")
}

#[test]
fn e2e_echo_acl_allows_and_denies() {
    let mut child = spawn_echo_server();
    // Give server time to start
    thread::sleep(Duration::from_millis(500));

    // Connect as allowed identity 'collector'
    let mut s = std::net::TcpStream::connect("127.0.0.1:9009").expect("connect");
    s.write_all(b"collector").unwrap();
    let mut buf = [0u8; 16];
    let n = s.read(&mut buf).unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(resp.contains("WELCOME"));

    // Connect as denied identity 'mallory@client-admin'
    let mut s2 = std::net::TcpStream::connect("127.0.0.1:9009").expect("connect2");
    s2.write_all(b"mallory@client-admin").unwrap();
    let mut buf2 = [0u8; 16];
    let n2 = s2.read(&mut buf2).unwrap();
    let resp2 = String::from_utf8_lossy(&buf2[..n2]).to_string();
    assert!(resp2.contains("DENIED"));

    // Clean up
    let _ = child.kill();
}

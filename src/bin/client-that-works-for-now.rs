use std::io::{stdin, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver};
use std::thread;

fn reader(mut stream: TcpStream) {
    let mut buf = vec![0u8; 1024];
    loop {
        let payload = match stream.read(&mut buf) {
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
            Ok(0) => {
                eprintln!("Server gave up for the night");
                std::process::exit(1);
            }
            Ok(n) => std::str::from_utf8(&buf[..n]).unwrap(),
        };
        eprintln!("{payload}");
    }
}

fn writer(mut stream: TcpStream, rx: Receiver<Vec<u8>>) {
    while let Ok(bytes) = rx.recv() {
        stream.write(&bytes);
        stream.flush();
    }
}

fn main() {
    let stream = TcpStream::connect("127.0.0.1:5555").unwrap();
    let (sender, receiver) = mpsc::channel();

    // Writer thread
    thread::spawn({
        let stream = stream.try_clone().unwrap();
        move || {
            writer(stream, receiver);
        }
    });

    // Reader thread
    thread::spawn(move || {
        reader(stream);
    });

    // Read forever and ever
    let stdin = stdin();
    let mut lines = stdin.lines();
    while let Some(Ok(mut line)) = lines.next() {
        eprintln!("you said: {line}");
        line.push('\n');
        sender.send(line.into_bytes());
    }
}

use std::io::{stdin, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

fn handle_stream(mut stream: TcpStream, rx: Receiver<Vec<u8>>) {
    let mut buf = vec![0u8; 1024];
    stream.set_nonblocking(true);

    loop {
        // write
        if let Ok(bytes) = rx.try_recv() {
            stream.write(&bytes);
            stream.flush();
        }

        // read
        let payload = match stream.read(&mut buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(_) | Ok(0) => {
                eprintln!("Ooops");
                std::process::exit(1);
            }
            Ok(n) => std::str::from_utf8(&buf[..n]).unwrap(),
        };
        eprintln!("{payload}");
    }
}

fn main() {
    let stream = TcpStream::connect("127.0.0.1:5555").unwrap();
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || handle_stream(stream, receiver));

    // Read forever and ever
    let stdin = stdin();
    let mut lines = stdin.lines();
    while let Some(Ok(mut line)) = lines.next() {
        eprintln!("you said: {line}");
        line.push('\n');
        sender.send(line.into_bytes());
    }
}

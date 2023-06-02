use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

type Room = String;
type RoomSender = mpsc::Sender<(Command, Sender)>;
type RoomReceiver = Receiver<(Command, Sender)>;

#[derive(Debug, Clone)]
struct Sender {
    inner: mpsc::Sender<Arc<[u8]>>,
    id: usize,
}

impl PartialEq for Sender {
    fn eq(&self, other: &Sender) -> bool {
        self.id == other.id
    }
}

// -----------------------------------------------------------------------------
//   - Overview -
// -----------------------------------------------------------------------------
// * Commands
// * Parse / frame input
// * Rooms
//
// 1. Listener listens for incoming connections

// Scenario 1: bytes\n
//
// Scenario 2: by
// Scenario 2a: t
// Scenario 2b: es\n
//
// Scenario 3: bytes\nbytes\nbyt
// Scenario 3: bytes\nbyt

struct Frame {
    buf: Vec<u8>,
    index: usize,
}

impl Frame {
    pub fn new() -> Self {
        Self {
            buf: vec![0; 1024],
            index: 0,
        }
    }

    pub fn update(&mut self, bytes_read: usize) {
        self.index += bytes_read;
    }

    fn frame(&mut self) -> Option<Vec<u8>> {
        // Find the position of the newline char if there is one...
        // otherwise we return None
        let pos = self.buf[..self.index].iter().position(|b| *b == b'\n')?;

        // Split the buffer at the position of the newline char.
        // This will set self.buffer to be the value we want to return,
        // and `let mut buf` will contain what we want to keep as self.buf,
        // there fore...
        let mut buf = self.buf.split_off(pos + 1);
        // ... we need to swap self.buf with buf
        std::mem::swap(&mut buf, &mut self.buf);

        // Resize our inner buffer to fit another kb
        self.buf.resize(1024, 0);

        Some(buf)
    }
}

// -----------------------------------------------------------------------------
//   - Commands -
// -----------------------------------------------------------------------------
// * join <room name>\n
// * part <room name>\n
// * msg <room name> <msg>\n
#[derive(Debug)]
enum Command {
    Join(Room),
    Part(Room),
    Msg { room: Room, msg: String },
}

impl Command {
    fn parse(bytes: Vec<u8>) -> Option<Self> {
        // Convert bytes to String
        let mut command = String::from_utf8(bytes).ok()?;
        command.pop();

        let pos = command.find(' ')?;
        let mut rest = command.split_off(pos + 1);
        // Remover the trailing whitespace char
        command.pop();

        match command.as_str() {
            // If there is no room name, return None
            "join" | "part" if rest.is_empty() => None,
            // If the room name contains a whitespace, return None
            "join" | "part" if rest.contains(' ') => None,

            "join" => Some(Self::Join(rest)),
            "part" => Some(Self::Part(rest)),
            "msg" => {
                let pos = rest.find(' ')?;
                let msg = rest.split_off(pos + 1);
                rest.pop(); // remove trailing whitespace
                let room = rest;
                Some(Self::Msg { room, msg })
            }
            _ => return None,
        }
    }
}

// -----------------------------------------------------------------------------
//   - Parse -
// -----------------------------------------------------------------------------
// input -> splitn(' ', 3)

fn rooms(receiver: RoomReceiver) {
    let mut rooms = HashMap::new(); // contains room names as key, and a bunch of senders

    while let Ok((command, sender)) = receiver.recv() {
        match command {
            Command::Join(room) => {
                let room = rooms.entry(room).or_insert(vec![]);
                room.push(sender);
                eprintln!("User joined room");
            }
            Command::Part(room_name) => {
                let Some(room) = rooms.get_mut(&room_name) else { continue };
                let Some(pos) = room.iter().position(|s| s == &sender) else { continue };
                room.remove(pos);
                eprintln!("User left room");
                // If the room is empty after the last user left
                // then remove the room
                if room.is_empty() {
                    rooms.remove(&room_name);
                    eprintln!("Empty room: {room_name}, removing...");
                }
            }
            Command::Msg { room, msg } => {
                let Some(room) = rooms.get(&room) else { continue };
                let bytes = msg.into_bytes();
                let bytes: Arc<[u8]> = bytes.into();
                room.iter().filter(|s| *s != &sender).for_each(|recipient| {
                    recipient.inner.send(bytes.clone());
                });
            }
        }
    }
}

fn handle_reader(mut reader: TcpStream, sender: Sender, room_sender: mpsc::Sender<(Command, Sender)>) {
    let mut frame = Frame::new();
    'reader: loop {
        // Step 1: read into the `frame`
        match reader.read(&mut frame.buf) {
            // Read zero bytes means the socket hung up on the other end.
            // This could be that the user just closed the connection, or
            // killed the program, or just turned off their computer?!?!
            Ok(0) => {
                eprintln!("Socket closed");
                break 'reader;
            }
            // An actual error happened! (oops)
            Err(e) => {
                eprintln!("Failed to read from socket: {e}");
                break 'reader;
            }
            // This is the only thing we return
            // from the match expression
            Ok(num_bytes) => frame.update(num_bytes),
        };

        // Step 2: get messages out of the frame (frame messages)
        while let Some(payload) = frame.frame() {
            // Step 3: parse message
            let Some(command) = Command::parse(payload) else { continue };

            // Step 4: send message to rooms
            room_sender.send((command, sender.clone()));
        }
    }
}

fn handle_writer(mut writer: TcpStream, receiver: Receiver<Arc<[u8]>>) {
    while let Ok(message) = receiver.recv() {
        writer.write_all(&message);
        writer.flush();
    }
}

fn handle_connection(reader: TcpStream, room_sender: RoomSender) {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    let (sender, receiver) = mpsc::channel();
    let sender = Sender { inner: sender, id };

    let writer = reader.try_clone().unwrap();
    thread::spawn(move || handle_writer(writer, receiver));
    thread::spawn(move || handle_reader(reader, sender, room_sender));
}

fn main() {
    let mut listener = TcpListener::bind("127.0.0.1:5555").unwrap();

    // Setup rooms here
    let (room_sender, room_receiver) = mpsc::channel();
    thread::spawn(move || rooms(room_receiver));

    loop {
        let (stream, _addr) = listener.accept().unwrap();
        handle_connection(stream, room_sender.clone());
    }
}

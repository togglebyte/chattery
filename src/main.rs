use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{self, Receiver};

type Room = String;
type RoomSender = mpsc::Sender<(Command, Arc<Sender>)>;
type RoomReceiver = Receiver<(Command, Arc<Sender>)>;


// -----------------------------------------------------------------------------
//   - User connection state -
//   Tracks username and user id,
//   but also makes sure we get a username before we allow the user to chat
// -----------------------------------------------------------------------------
enum State {
    Anon,
    User(Arc<Sender>),
}

#[derive(Debug, Clone)]
struct Sender {
    inner: mpsc::Sender<Arc<[u8]>>,
    id: usize,
    username: String,
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

async fn rooms(mut receiver: RoomReceiver) {
    let mut rooms = HashMap::new(); // contains room names as key, and a bunch of senders

    while let Some((command, sender)) = receiver.recv().await {
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
                eprintln!("why not?");
                static SEPARATOR: &'static str = ": ";
                static NL: u8 = b'\n';

                let Some(room) = rooms.get(&room) else { continue };
                let mut payload = Vec::<u8>::with_capacity(msg.len() + SEPARATOR.len() + sender.username.len() + 1); // 1 = len of nl char
                payload.extend(sender.username.as_bytes());
                payload.extend(SEPARATOR.as_bytes());
                payload.extend(msg.as_bytes());
                payload.push(NL);

                let bytes: Arc<[u8]> = payload.into();
                for recipient in room.iter().filter(|s| *s != &sender) {
                    recipient.inner.send(bytes.clone()).await;
                }
            }
        }
    }
}

async fn handle_reader(mut reader: OwnedReadHalf, sender: mpsc::Sender<Arc<[u8]>>, id: usize, room_sender: RoomSender) {
    let mut state = State::Anon;
    let mut frame = Frame::new();
    'reader: loop {
        // Step 1: read into the `frame`
        match reader.read(&mut frame.buf).await {
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
        while let Some(mut payload) = frame.frame() {
            match &state {
                // Step 3: move from anon state to have a username
                State::Anon => {
                    payload.pop();
                    let username = String::from_utf8(payload).expect("pleased do proper error handling");
                    let sender = Sender {
                        inner: sender.clone(),
                        id,
                        username,
                    };

                    let sender = Arc::new(sender);
                    // Transition into the named state
                    state = State::User(sender);
                }
                State::User(sender) => {
                    // Step 4: parse message
                    let Some(command) = Command::parse(payload) else { continue };
                    eprintln!("{command:?}");

                    // Step 5: send message to rooms
                    room_sender.send((command, sender.clone())).await;
                }
            }
        }
    }
}

async fn handle_writer(mut writer: OwnedWriteHalf, mut receiver: Receiver<Arc<[u8]>>) {
    writer.write(b"enter username\n").await;
    while let Some(message) = receiver.recv().await {
        writer.write_all(&message).await;
        writer.flush();
    }
}

async fn handle_connection(stream: TcpStream, room_sender: RoomSender) {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    let (sender, receiver) = mpsc::channel(2);
    // let sender = Arc::new(Sender { inner: sender, id, username: String::new() });

    let (reader, writer) = stream.into_split();
    tokio::spawn(async move { handle_writer(writer, receiver).await });
    tokio::spawn(async move { handle_reader(reader, sender, id, room_sender).await });
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:5555").await.unwrap();

    // Setup rooms here
    let (room_sender, room_receiver) = mpsc::channel(1_000);
    tokio::spawn(async move { rooms(room_receiver).await });

    loop {
        let (stream, _addr) = listener.accept().await.unwrap();
        handle_connection(stream, room_sender.clone()).await;
    }
}

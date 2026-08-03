use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::str;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, SystemTime};

use protocol::Frame;
use protocol::decode;
use protocol::encode;

const BAN_LIMIT: Duration = Duration::from_secs(10 * 60);
const MESSAGE_RATE: Duration = Duration::from_secs(1);
const STRIKE_LIMIT: u64 = 10;
const IGNORE_ID: u32 = 0;

enum Message {
    ClientConnected {
        author: Sender<Frame>,
        author_addr: SocketAddr,
    },
    ClientDisconnected {
        author_addr: SocketAddr,
    },
    Received {
        author_addr: SocketAddr,
        bytes: Vec<u8>,
        id: u32,
    },
}

struct Client {
    tx: Sender<Frame>,
    last_message: SystemTime,
    strike_count: u64,
    authenticated: bool,
}

struct Server {
    clients: HashMap<SocketAddr, Client>,
    banned_clients: HashMap<IpAddr, SystemTime>,
    token: String,
}

impl Server {
    fn with_token(token: String) -> Self {
        Self {
            clients: HashMap::new(),
            banned_clients: HashMap::new(),
            token,
        }
    }

    fn client_connected(&mut self, tx: Sender<Frame>, author_addr: SocketAddr) {
        let now = SystemTime::now();

        let banned_at_and_diff_time =
            self.banned_clients
                .remove(&author_addr.ip())
                .and_then(|banned_at| {
                    let diff = now.duration_since(banned_at).unwrap_or_else(|err| {
                        eprintln!("The clock might have gone backwards: {err}");
                        Duration::from_secs(0)
                    });
                    if diff >= BAN_LIMIT {
                        None
                    } else {
                        Some((banned_at, diff))
                    }
                });

        if let Some((banned_at, diff)) = banned_at_and_diff_time {
            self.banned_clients.insert(author_addr.ip(), banned_at);
            let secs = (BAN_LIMIT - diff).as_secs_f32();
            println!(
                "INFO: Client {author_addr} tried to connect, but got banned for {secs} more seconds"
            );

            let _ = tx
                .send(Frame::System {
                    text: format!("You are banned! {secs}s left").into_bytes(),
                })
                .map_err(|err| {
                    eprintln!("Could not send ban message for client {author_addr}: {err}");
                });
        } else {
            println!("INFO: Client {author_addr} connected");

            let _ = tx
                .send(Frame::System {
                    text: "Token: ".into(),
                })
                .map_err(|err| {
                    eprintln!(
                        "ERROR: Could not send token prompt to {}: {}",
                        author_addr, err
                    )
                });

            self.clients.insert(
                author_addr,
                Client {
                    tx,
                    last_message: now - 2 * MESSAGE_RATE,
                    strike_count: 0,
                    authenticated: false,
                },
            );
        }
    }

    fn client_disconnected(&mut self, author_addr: SocketAddr) {
        self.clients.remove(&author_addr);
        println!("INFO: Client {author_addr} disconnected");
    }

    fn new_message(&mut self, author_addr: SocketAddr, bytes: &[u8], id: u32) {
        if let Some(author) = self.clients.get_mut(&author_addr) {
            let now = SystemTime::now();

            let diff = now
                .duration_since(author.last_message)
                .expect("TODO: we shouldn't crash if the clock goes backwards");

            if diff >= MESSAGE_RATE
                && let Ok(text) = str::from_utf8(bytes)
            {
                author.last_message = now;

                if author.authenticated {
                    println!("Client {author_addr} sent message {bytes:?}");
                    for (addr, client) in self.clients.iter() {
                        if *addr != author_addr && client.authenticated {
                            let _ = client.tx.send(Frame::Chat {
                                id: IGNORE_ID,
                                text: bytes.to_vec(),
                            });
                        }
                    }
                } else {
                    if text == self.token {
                        author.authenticated = true;
                        let _ = author
                            .tx
                            .send(Frame::System {
                                text: "Welcome to the club, buddy! Now you can send messages."
                                    .into(),
                            })
                            .map_err(|err| {
                                eprintln!(
                                    "Could not send auth succesfull prompt to {}: {}",
                                    author_addr, err
                                )
                            });
                    } else {
                        println!("INFO: User {} failed authentication", author_addr);
                        let _ = author
                            .tx
                            .send(Frame::System {
                                text: "Invalid token!".into(),
                            })
                            .map_err(|err| {
                                eprintln!(
                                    "Could not send auth failed prompt to {}: {}",
                                    author_addr, err
                                )
                            });
                        self.clients.remove(&author_addr);
                    }
                }
            } else {
                let _ = author.tx.send(Frame::Dropped { id });
                author.strike_count += 1;
                if author.strike_count >= STRIKE_LIMIT {
                    self.banned_clients.insert(author_addr.ip(), now);
                    let secs = BAN_LIMIT.as_secs_f32();
                    let _ = author.tx.send(Frame::System {
                        text: format!("You are banned! {secs}s left").into(),
                    });
                    self.clients.remove(&author_addr);
                    println!("INFO: Client {author_addr} banned");
                }
            }
        }
    }
}

fn generate_token() -> Result<String> {
    let mut token_raw = [0; 16];

    getrandom::fill(&mut token_raw).context("Failed to generate random symbols for token")?;

    let mut token = String::new();

    for b in token_raw.iter() {
        let _ = write!(token, "{b:02X}");
    }

    Ok(token)
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = generate_token()?;
    println!("INFO: Auth token is: {token}");

    let addr = "0.0.0.0:6969";
    let listener = TcpListener::bind(addr)?;
    println!("Listening to {}", addr);

    let (message_sender, message_recevier): (Sender<Message>, Receiver<Message>) = channel();
    thread::spawn(|| server(message_recevier, token));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let message_sender = message_sender.clone();

                thread::spawn(move || client(Arc::new(stream), message_sender));
            }
            Err(e) => {
                eprintln!("ERROR: could not accept connection: {e}")
            }
        }
    }

    Ok(())
}

fn server(messages: Receiver<Message>, token: String) -> Result<()> {
    let mut server = Server::with_token(token);

    loop {
        let msg = messages.recv().expect("The server receiver is not hung up");

        match msg {
            Message::ClientConnected {
                author,
                author_addr,
            } => {
                server.client_connected(author, author_addr);
            }
            Message::ClientDisconnected { author_addr } => {
                server.client_disconnected(author_addr);
            }
            Message::Received {
                author_addr,
                bytes,
                id,
            } => {
                server.new_message(author_addr, &bytes, id);
            }
        }
    }
}

fn client(reader_part: Arc<TcpStream>, messages: Sender<Message>) -> Result<()> {
    let author_addr = reader_part.peer_addr()?;

    let mut writer_part = reader_part.try_clone()?;

    let (tx, rx): (Sender<Frame>, Receiver<Frame>) = channel();

    thread::spawn(move || {
        loop {
            match rx.recv() {
                Ok(frame) => {
                    let _ = encode(&frame, &mut writer_part);
                }
                Err(_) => {
                    let _ = writer_part.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        }
    });

    messages.send(Message::ClientConnected {
        author: tx,
        author_addr,
    })?;

    loop {
        let decoded_stream = decode(&mut reader_part.as_ref());
        match decoded_stream {
            Ok(frame) => match frame {
                protocol::Frame::Chat { id, text } => messages.send(Message::Received {
                    author_addr,
                    bytes: text,
                    id,
                })?,
                _ => {
                    eprintln!("Client Cannot send Frame except Chat frame");
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
            },
            Err(e) => match e {
                protocol::ProtocolError::PayloadIsTooLong(len) => {
                    eprintln!("ERROR: The message is too big: {len}");
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
                protocol::ProtocolError::IO(e) => {
                    eprintln!("ERROR: Could not read message from client {e}");
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
                protocol::ProtocolError::Disconnect => {
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
                protocol::ProtocolError::UnknownFrameType(b) => {
                    eprintln!("ERROR: Client sent unknown frame type: {b}");
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
                protocol::ProtocolError::PayloadIsMalformed => {
                    eprintln!("ERROR: Payload recieved from a client is malformed");
                    let _ = messages.send(Message::ClientDisconnected { author_addr });
                    return Ok(());
                }
            },
        }
    }
}

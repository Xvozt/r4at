use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::net::{IpAddr, SocketAddr};
use std::str;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task;

use protocol::{Frame, encode_async};
use protocol::{ProtocolError, decode_async};

const BAN_LIMIT: Duration = Duration::from_secs(10 * 60);
const MESSAGE_RATE: Duration = Duration::from_secs(1);
const STRIKE_LIMIT: u64 = 10;
const IGNORE_ID: u32 = 0;

enum Message {
    ClientConnected {
        author: UnboundedSender<Frame>,
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
    tx: UnboundedSender<Frame>,
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

    fn client_connected(&mut self, tx: UnboundedSender<Frame>, author_addr: SocketAddr) {
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
    let listener = TcpListener::bind(addr).await?;
    println!("Listening to {}", addr);

    let (message_sender, message_recevier): (UnboundedSender<Message>, UnboundedReceiver<Message>) =
        mpsc::unbounded_channel();

    tokio::spawn(server(message_recevier, token));

    loop {
        let (stream, addr) = listener.accept().await?;
        tokio::spawn(client(stream, addr, message_sender.clone()));
    }
}

async fn server(mut messages: UnboundedReceiver<Message>, token: String) -> Result<()> {
    let mut server = Server::with_token(token);

    while let Some(msg) = messages.recv().await {
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
    Ok(())
}

async fn client(
    stream: TcpStream,
    author_addr: SocketAddr,
    messages: UnboundedSender<Message>,
) -> Result<()> {
    let (mut reader_part, mut writer_part) = stream.into_split();

    let (tx, mut rx): (UnboundedSender<Frame>, UnboundedReceiver<Frame>) = unbounded_channel();

    task::spawn(async move {
        loop {
            match rx.recv().await {
                Some(frame) => match encode_async(&frame, &mut writer_part).await {
                    Ok(_) => {}
                    Err(ProtocolError::IO(e)) => {
                        eprintln!("Socket is dead: {e}");
                        break;
                    }
                    Err(e) => {
                        eprintln!("Tried to send bad frame: {e}");
                        continue;
                    }
                },
                None => {
                    let _ = writer_part.shutdown().await;
                    break;
                }
            }
        }
    });

    messages.send(Message::ClientConnected {
        author: tx,
        author_addr,
    })?;

    loop {
        let decoded_stream = decode_async(&mut reader_part).await;
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

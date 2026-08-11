use std::{env, sync::Arc};

use anyhow::Result;
use crossterm::event::{
    Event::{self as CtEvent},
    EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use futures::StreamExt;
use protocol::{Frame, ProtocolError, decode_async, encode_async};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, List, ListState, Paragraph},
};
use tokio::{
    io::{AsyncRead, AsyncWriteExt, split},
    net::TcpStream,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{
        ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName},
    },
};

macro_rules! chat_info {
    ($app:expr, $($arg:tt)*) => {
        $app.push_message(Message::System(format!($($arg)*)))
    };
}

macro_rules! chat_msg {
    ($app:expr, $($arg:tt)*) => {
        $app.push_message(Message::Incoming(format!($($arg)*)))
    };
}

macro_rules! chat_err {
    ($app:expr, $($arg:tt)*) => {
        $app.push_message(Message::Error(format!($($arg)*)))
    };
}

struct Command {
    name: &'static str,
    description: &'static str,
    signature: &'static str,
    to_run: fn(&mut App, &str),
}

fn find_command(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        description: "Prints help",
        signature: "/help <command>",
        to_run: help_command,
    },
    Command {
        name: "connect",
        description: "Connects to the server by <ip> with token auth",
        signature: "/connect <ip>",
        to_run: connect_command,
    },
    Command {
        name: "disconnect",
        description: "Disconnects from the server",
        signature: "/disconnect",
        to_run: disconnect_command,
    },
];

fn disconnect_command(app: &mut App, _arg: &str) {
    let stream = app.stream.take();
    match stream {
        Some(_) => {
            let _ = app.event_tx.send(Event::Disconnect);
        }
        None => {
            chat_info!(app, "You are already disconnected");
        }
    }
}
fn connect_command(app: &mut App, arg: &str) {
    if arg.is_empty() {
        chat_info!(app, "/connect <ip> - connects to a server");
        return;
    }
    if app.stream.is_some() {
        chat_info!(app, "You are already connected. Disconnect first.");
        return;
    }

    app.connect(arg);
}
fn help_command(app: &mut App, arg: &str) {
    let command_name = arg.trim();
    if command_name.is_empty() {
        for c in COMMANDS.iter() {
            chat_info!(app, "{} - {}", c.signature, c.description);
        }
    } else {
        if let Some(c) = find_command(command_name) {
            chat_info!(app, "{} - {}", c.signature, c.description);
        } else {
            chat_err!(app, "Unknown command `/{command_name}`");
        }
    }
}

enum Event {
    Terminal(CtEvent),
    Chat(String),
    System(String),
    Disconnect,
    Connect(UnboundedSender<Frame>),
    Dropped(u32),
    Error(String),
}

enum Message {
    System(String),
    Incoming(String),
    Sent {
        text: String,
        id: u32,
        dropped: bool,
    },
    Error(String),
}

impl Message {
    fn text(&self) -> &str {
        match self {
            Message::System(s) => s.as_str(),
            Message::Incoming(s) => s.as_str(),
            Message::Error(s) => s.as_str(),
            Message::Sent { text, .. } => text.as_str(),
        }
    }

    fn color(&self) -> Color {
        match self {
            Message::System(_) => Color::Yellow,
            Message::Incoming(_) => Color::White,
            Message::Error(_) => Color::LightRed,
            Message::Sent { dropped, .. } => {
                if *dropped {
                    Color::Red
                } else {
                    Color::White
                }
            }
        }
    }
}

struct App {
    exit: bool,
    messages: Vec<Message>,
    user_message: String,
    stream: Option<UnboundedSender<Frame>>,
    connector: TlsConnector,
    event_tx: UnboundedSender<Event>,
    chat_state: ListState,
    next_message_id: u32,
}
impl App {
    async fn run(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
        mut rx: UnboundedReceiver<Event>,
    ) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;

            match rx.recv().await {
                Some(ev) => match ev {
                    Event::Terminal(CtEvent::Key(k)) => self.handle_key_events(k)?,
                    Event::Chat(message) => {
                        chat_msg!(self, "{message}");
                    }
                    Event::System(message) => {
                        chat_info!(self, "{message}")
                    }
                    Event::Error(message) => chat_err!(self, "{message}"),
                    Event::Dropped(id) => self.mark_dropped(id),
                    Event::Disconnect => {
                        self.stream.take();
                    }
                    Event::Connect(tx) => {
                        self.stream = Some(tx);
                    }
                    Event::Terminal(_) => {}
                },
                None => {
                    break;
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::prelude::Frame<'_>) {
        let input_width = frame.area().width.saturating_sub(2).max(1);

        let input_lines = wrap_text(&self.user_message, input_width as usize);

        let input_height = (input_lines.len().max(1) + 2).min(6);

        let vertical_layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(input_height as u16),
        ]);
        let [chat_area, status_area, input_area] = vertical_layout.areas(frame.area());

        let instructions_for_input = Line::from(vec![
            Span::styled(" Clear all ", Style::new().italic()),
            Span::styled(" <ESC> ", Style::new().italic()),
            Span::styled(" Exit ", Style::new().italic()),
            Span::styled(" <Control + Q> ", Style::new().italic()),
        ])
        .centered();

        let input_block = Block::bordered()
            .title_bottom(instructions_for_input)
            .border_set(border::THICK);

        let input = Paragraph::new(Text::from(input_lines)).block(input_block);

        frame.render_widget(input, input_area);

        let (status_text, status_color) = match self.stream {
            Some(_) => ("CONNECTED", Color::LightGreen),
            None => ("DISCONNECTED", Color::Gray),
        };
        let status = Paragraph::new(status_text)
            .centered()
            .style(Style::new().bg(status_color));

        frame.render_widget(status, status_area);

        let chat_block = Block::bordered().border_set(border::THICK);
        let chat_width = chat_area.width.saturating_sub(2).max(1);

        let message_list: Vec<Text> = self
            .messages
            .iter()
            .map(|m| {
                wrap_text(m.text(), chat_width as usize)
                    .into_iter()
                    .map(|l| l.style(Style::new().fg(m.color())))
                    .collect::<Text>()
            })
            .collect::<Vec<Text>>();

        let chat = List::new(message_list).block(chat_block);

        frame.render_stateful_widget(chat, chat_area, &mut self.chat_state);
    }

    fn handle_key_events(&mut self, event: KeyEvent) -> std::io::Result<()> {
        match (event.kind, event.code, event.modifiers) {
            (KeyEventKind::Press, KeyCode::Char('q'), KeyModifiers::CONTROL) => self.exit = true,
            (KeyEventKind::Press, KeyCode::Backspace, KeyModifiers::NONE) => {
                let _ = self.user_message.pop();
            }
            (KeyEventKind::Press, KeyCode::Esc, KeyModifiers::NONE) => {
                self.user_message.clear();
            }
            (KeyEventKind::Press, KeyCode::Enter, KeyModifiers::NONE) => {
                self.submit();
            }
            (KeyEventKind::Press, KeyCode::Char(c), modifier)
                if modifier == KeyModifiers::NONE || modifier == KeyModifiers::SHIFT =>
            {
                self.user_message.push(c);
            }
            (KeyEventKind::Press, KeyCode::Up, KeyModifiers::NONE) => {
                self.chat_state.scroll_up_by(5);
            }
            (KeyEventKind::Press, KeyCode::Down, KeyModifiers::NONE) => {
                self.chat_state.scroll_down_by(5);
            }
            _ => {}
        }
        Ok(())
    }

    fn push_message(&mut self, message: Message) {
        self.messages.push(message);
        self.chat_state.select(Some(self.messages.len() - 1));
    }
    fn submit(&mut self) {
        let message = self.user_message.clone();

        match message.strip_prefix("/") {
            Some(rest) => {
                let (command, argument) = rest.split_once(' ').unwrap_or((rest, ""));
                match find_command(command) {
                    Some(command) => (command.to_run)(self, argument),
                    None => {
                        chat_err!(self, "Command is not supported");
                    }
                }
            }
            None => {
                let stream = self.stream.as_ref();
                if let Some(stream) = stream {
                    let _ = stream.send(Frame::Chat {
                        id: self.next_message_id,
                        text: message.clone().into_bytes(),
                    });
                    self.push_message(Message::Sent {
                        text: message,
                        id: self.next_message_id,
                        dropped: false,
                    });
                    self.next_message_id += 1;
                } else {
                    chat_info!(
                        self,
                        "You are disconnected. Your message wasn't delivered. Try to reconnect"
                    );
                }
            }
        }
        self.user_message.clear();
    }
    fn connect(&mut self, ip: &str) {
        let ip = ip.to_string();
        let event_tx = self.event_tx.clone();
        let connector = self.connector.clone();
        task::spawn(async move {
            let server_name =
                ServerName::try_from("rchat.server").expect("rchat.server is a valid DNS name");

            let Ok(stream) = TcpStream::connect(format!("{ip}:6969")).await else {
                let _ = event_tx.send(Event::Error("Couldn't reach IP".into()));
                return;
            };

            let tls_stream = match connector.connect(server_name, stream).await {
                Ok(s) => s,
                Err(_) => {
                    let _ =
                        event_tx.send(Event::Error("Couldnt establish secure connection".into()));
                    return;
                }
            };

            let (reader_part, mut writer_part) = split(tls_stream);
            let (tx, mut rx): (UnboundedSender<Frame>, UnboundedReceiver<Frame>) =
                unbounded_channel();

            let writer_task_tx = event_tx.clone();
            let reader_task_tx = event_tx.clone();

            task::spawn(handle_chat_events(reader_task_tx.clone(), reader_part));
            task::spawn(async move {
                loop {
                    match rx.recv().await {
                        Some(frame) => match encode_async(&frame, &mut writer_part).await {
                            Ok(_) => {}
                            Err(ProtocolError::IO(e)) => {
                                let _ = writer_task_tx
                                    .send(Event::Error(format!("Socket is dead: {e}")));
                                break;
                            }
                            Err(e) => {
                                let _ = writer_task_tx
                                    .send(Event::Error(format!("Tried to send bad frame: {e}")));
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
            let _ = event_tx.send(Event::Connect(tx));
        });
    }

    fn mark_dropped(&mut self, id: u32) {
        for m in self.messages.iter_mut() {
            if let Message::Sent {
                id: m_id, dropped, ..
            } = m
                && *m_id == id
            {
                *dropped = true;
                break;
            }
        }
    }
}

fn wrap_text(message: &str, width: usize) -> Vec<Line<'static>> {
    message
        .chars()
        .collect::<Vec<char>>()
        .chunks(width)
        .map(|chunk| Line::from(chunk.iter().collect::<String>()))
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cert_bytes = include_bytes!("../certs/server_cert.der");
    let cert = CertificateDer::from(&cert_bytes[..]);
    let mut store = RootCertStore::empty();
    store.add(cert)?;

    let config = ClientConfig::builder()
        .with_root_certificates(store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    let mut terminal = ratatui::init();
    let (tx_input, event_rx) = unbounded_channel::<Event>();

    let mut app = App {
        exit: false,
        messages: vec![],
        user_message: "".to_string(),
        event_tx: tx_input.clone(),
        connector,
        stream: None,
        chat_state: ListState::default(),
        next_message_id: 0,
    };

    let addr = env::args().nth(1);
    if let Some(addr) = addr {
        app.connect(&addr);
    }

    task::spawn(handle_input_events(tx_input));

    app.run(&mut terminal, event_rx).await
}

async fn handle_chat_events<T>(tx_reader: UnboundedSender<Event>, mut stream: T)
where
    T: AsyncRead + Unpin,
{
    loop {
        match decode_async(&mut stream).await {
            Ok(f) => match f {
                protocol::Frame::Chat { text, .. } => tx_reader
                    .send(Event::Chat(String::from_utf8_lossy(&text).into_owned()))
                    .unwrap(),
                protocol::Frame::System { text, .. } => tx_reader
                    .send(Event::System(String::from_utf8_lossy(&text).into_owned()))
                    .unwrap(),
                protocol::Frame::Dropped { id } => tx_reader.send(Event::Dropped(id)).unwrap(),
            },
            Err(_) => {
                tx_reader.send(Event::Disconnect).unwrap();
                break;
            }
        }
    }
}

async fn handle_input_events(tx: UnboundedSender<Event>) {
    let mut reader = EventStream::new();
    loop {
        let event = reader.next().await;

        match event {
            Some(Ok(e)) => {
                if tx.send(Event::Terminal(e)).is_err() {
                    break;
                }
            }
            Some(Err(e)) => {
                if tx
                    .send(Event::Error(format!("Problem with input occurred: {e}")))
                    .is_err()
                {
                    break;
                }
            }
            None => {
                break;
            }
        }
    }
}

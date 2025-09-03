use std::thread;

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{self, WebSocket},
    },
    response::{Html, Response},
    routing::{any, get},
    serve,
};
use futures_util::{SinkExt, StreamExt};
use nvim_oxi::{
    api::{
        self,
        opts::{CreateAugroupOpts, CreateAutocmdOpts, CreateCommandOpts},
    },
    mlua::{self, lua},
    print,
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    net::TcpListener,
    sync::{
        broadcast,
        mpsc::{Receiver, Sender, channel},
        oneshot,
    },
};
use tower_http::services::ServeDir;

const ADDR: &str = "localhost:0";

#[nvim_oxi::plugin]
fn strudel() -> nvim_oxi::Result<()> {
    let opts = CreateAugroupOpts::builder().clear(true).build();
    let strudel_augroup = api::create_augroup("strudel", &opts)?;

    let augroup = strudel_augroup;
    let opts = CreateCommandOpts::builder().desc("starts strudel").build();
    api::create_user_command(
        "StrudelStart",
        move |_args| {
            let running: Result<bool, mlua::Error> = lua().globals().get("strudel_running");
            match running {
                Ok(true) => {
                    print!("strudel already running");
                    return;
                }
                _ => {
                    _ = lua().globals().set("strudel_running", true);
                }
            }

            let strudel = App::new();

            _ = nvim_setup(strudel.get_tx(), strudel.get_broadcast());

            let tx = strudel.get_tx();
            let opts = CreateAutocmdOpts::builder()
                .group(augroup)
                .callback(move |_args| {
                    _ = tx.blocking_send(AppMessage::Quit);
                    _ = lua().globals().set("strudel_running", false);
                    nvim_oxi::Result::Ok(true)
                })
                .build();
            _ = api::create_autocmd(["ExitPre"], &opts);

            _ = thread::spawn(move || strudel.run());
        },
        &opts,
    )?;

    Ok(())
}

/// this will run in the nvim event loop on the main thread
fn nvim_setup(
    tx_prime: Sender<AppMessage>,
    broadcast_tx_prime: broadcast::Sender<SocketMessage>,
) -> nvim_oxi::Result<()> {
    let tx = tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("returns the port of the strudel server")
        .build();
    api::create_user_command(
        "StrudelGetPort",
        move |_args| {
            let (oneshot_tx, rx) = oneshot::channel();
            if tx.blocking_send(AppMessage::GetPort(oneshot_tx)).is_err() {
                print!("couldn't get port");
            } else if let Ok(Some(port)) = rx.blocking_recv() {
                print!("{port}");
            } else {
                print!("couldn't get port");
            }
        },
        &opts,
    )?;

    let tx = tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("quits the strudel server")
        .build();
    api::create_user_command(
        "StrudelQuitServer",
        move |_args| {
            if tx.blocking_send(AppMessage::Quit).is_err() {
                print!("failed to quit server");
            }
            _ = lua().globals().set("strudel_running", false);
        },
        &opts,
    )?;

    let tx = tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("opens the strudel client in the default browser")
        .build();
    api::create_user_command(
        "StrudelOpen",
        move |_args| {
            let (oneshot_tx, rx) = oneshot::channel();
            if tx.blocking_send(AppMessage::GetPort(oneshot_tx)).is_err() {
                print!("strudel server rx dropped");
            } else if let Ok(Some(port)) = rx.blocking_recv() {
                let url = format!("http://localhost:{port}");
                _ = open::that(url);
            }
        },
        &opts,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("starts playback on the strudel client")
        .build();
    api::create_user_command(
        "StrudelPlay",
        move |_args| _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Playing)),
        &opts,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("pauses playback on the strudel client")
        .build();
    api::create_user_command(
        "StrudelPause",
        move |_args| _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Paused)),
        &opts,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("stops playback on the strudel client")
        .build();
    api::create_user_command(
        "StrudelStop",
        move |_args| _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Stopped)),
        &opts,
    )?;

    let current_buffer_as_string = || {
        let current_buffer = api::get_current_buf();
        let lines = current_buffer.get_lines(.., false)?;

        let lines: Vec<String> = lines.into_iter().map(|s| s.to_string()).collect();
        let lines = lines.join("\n");

        anyhow::Ok(lines)
    };

    let broadcast_tx = broadcast_tx_prime.clone();
    let opts = CreateCommandOpts::builder()
        .desc("updates the code strudel is executing")
        .build();
    api::create_user_command(
        "StrudelUpdateCode",
        move |_args| {
            if let Ok(code) = current_buffer_as_string() {
                _ = broadcast_tx.send(SocketMessage::Code(code));
            } else {
                print!("couldn't get the current buffer")
            }
        },
        &opts,
    )?;

    Ok(())
}

struct App {
    port: Option<u16>,
    rx: Receiver<AppMessage>,
    tx: Sender<AppMessage>,
    broadcast_tx: broadcast::Sender<SocketMessage>,
}

struct AppState {
    tx: Sender<AppMessage>,
    rx: broadcast::Receiver<SocketMessage>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: self.rx.resubscribe(),
        }
    }
}

impl App {
    fn new() -> Self {
        let (tx, rx) = channel(16);
        let (broadcast_tx, _) = broadcast::channel(16);
        Self {
            port: None,
            rx,
            tx,
            broadcast_tx,
        }
    }

    #[tokio::main]
    async fn run(mut self) -> anyhow::Result<()> {
        let mut file = File::open("../strudel-frontend/dist/index.html").await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let app = Router::new()
            .route("/", get(|| async move { Html::from(contents) }))
            .route("/ws", any(websocket_handler))
            .with_state(AppState {
                tx: self.tx.clone(),
                rx: self.broadcast_tx.subscribe(),
            })
            .nest_service(
                "/assets/",
                ServeDir::new("../strudel-frontend/dist/assets/"),
            );

        let listener = TcpListener::bind(ADDR).await?;
        self.port = Some(listener.local_addr()?.port());
        let (shutdown_tx, shutdown_rx) = channel(1);

        async fn shutdown(mut shutdown_rx: Receiver<()>) {
            let _ = shutdown_rx.recv().await;
        }

        tokio::spawn(async {
            serve(listener, app)
                .with_graceful_shutdown(shutdown(shutdown_rx))
                .await
        });

        while let Some(message) = self.rx.recv().await {
            match message {
                AppMessage::GetPort(oneshot) => {
                    let _ = oneshot.send(self.port);
                }
                AppMessage::Quit => {
                    let _ = shutdown_tx.send(()).await;
                    break;
                }
            }
        }

        anyhow::Ok(())
    }

    pub fn get_tx(&self) -> Sender<AppMessage> {
        self.tx.clone()
    }

    fn get_broadcast(&self) -> broadcast::Sender<SocketMessage> {
        self.broadcast_tx.clone()
    }
}

async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(
    socket: WebSocket,
    AppState {
        tx: _app_tx,
        rx: mut broadcast_rx,
    }: AppState,
) {
    let (mut socket_tx, mut _socket_rx) = socket.split();
    _ = socket_tx
        .send(ws::Message::Text(
            serde_json::to_string(&SocketMessage::Message(String::from("hello")))
                .unwrap_or_else(|_| String::from("message failed to deserialize"))
                .into(),
        ))
        .await;

    tokio::spawn(async move {
        loop {
            let msg = broadcast_rx.recv().await;
            use broadcast::error::RecvError as e;
            match msg {
                Ok(msg) => {
                    let message = if let Ok(json) = serde_json::to_string(&msg) {
                        ws::Message::Text(json.into())
                    } else {
                        ws::Message::Text("failed to serialize".into())
                    };

                    _ = socket_tx.send(message).await;
                }
                Err(e::Closed) => break,
                Err(e::Lagged(num)) => {
                    let message = if let Ok(json) = serde_json::to_string(&SocketMessage::Error(
                        format!("broadcast lagged by {num} messages"),
                    )) {
                        ws::Message::Text(json.into())
                    } else {
                        ws::Message::Text("failed to serialize".into())
                    };

                    _ = socket_tx.send(message).await;
                }
            }
        }
    });
}

enum AppMessage {
    GetPort(oneshot::Sender<Option<u16>>),
    Quit,
}

#[derive(Clone, Serialize, Deserialize)]
enum SocketMessage {
    Message(String),
    Code(String),
    Playback(PlaybackState),
    Error(String),
}

#[derive(Clone, Serialize, Deserialize)]
enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

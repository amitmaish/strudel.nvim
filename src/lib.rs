use std::{ffi::c_void, thread};

use axum::extract::ws::{self, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::any;
use axum::{Router, response::Html, routing::get, serve};
use futures_util::{SinkExt, StreamExt};
use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::{
    fs::File,
    io::AsyncReadExt,
    net::TcpListener,
    runtime::Runtime,
    sync::{
        mpsc::{Receiver, Sender, channel},
        oneshot,
    },
};
use tower_http::services::ServeDir;

const ADDR: &str = "localhost:0";

fn start_server(lua: &Lua, _: ()) -> LuaResult<LuaTable> {
    let mut app = App::new()?;
    let tx_prime = app.get_tx();
    let broadcast_tx_prime = app.get_broadcast();

    let server_thread = thread::spawn(move || {
        app.run()?;
        anyhow::Ok(())
    });

    let t = lua.create_table()?;

    let server_handle: *mut thread::JoinHandle<Result<(), anyhow::Error>> =
        Box::into_raw(Box::new(server_thread));

    t.set(
        "server_handle",
        LuaValue::LightUserData(LuaLightUserData(server_handle as *mut c_void)),
    )?;

    let tx = tx_prime.clone();
    t.set(
        "quit_server",
        lua.create_function(move |_, server_handle: LuaLightUserData| {
            if tx.blocking_send(AppMessage::Quit).is_err() {
                Err(LuaError::RuntimeError(String::from(
                    "strudel server rx dropped",
                )))
            } else {
                let handle = server_handle.0 as *mut thread::JoinHandle<Result<(), anyhow::Error>>;
                let server_thread = unsafe { Box::from_raw(handle) };
                if let Ok(Ok(_)) = server_thread.join() {
                    Ok(())
                } else {
                    Err(LuaError::RuntimeError(String::from(
                        "strudel server errored",
                    )))
                }
            }
        })?,
    )?;

    let tx = tx_prime.clone();
    t.set(
        "get_port",
        lua.create_function(move |_, _: ()| {
            let (oneshot_tx, rx) = oneshot::channel();
            if tx.blocking_send(AppMessage::GetPort(oneshot_tx)).is_err() {
                Err(LuaError::RuntimeError(String::from(
                    "strudel server rx dropped",
                )))
            } else if let Ok(Some(value)) = rx.blocking_recv() {
                Ok(LuaValue::Integer(value as LuaInteger))
            } else {
                Ok(LuaValue::Nil)
            }
        })?,
    )?;

    let tx = tx_prime.clone();
    t.set(
        "open_site",
        lua.create_function(move |_, _: ()| {
            let (oneshot_tx, rx) = oneshot::channel();
            if tx.blocking_send(AppMessage::GetPort(oneshot_tx)).is_err() {
                return Err(LuaError::RuntimeError(String::from(
                    "strudel server rx dropped",
                )));
            }
            if let Ok(Some(port)) = rx.blocking_recv() {
                let url = format!("http://localhost:{port}");
                let _ = open::that(url);
            }

            Ok(())
        })?,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    t.set(
        "play",
        lua.create_function(move |_, _: ()| {
            let _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Playing));
            Ok(())
        })?,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    t.set(
        "pause",
        lua.create_function(move |_, _: ()| {
            let _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Paused));
            Ok(())
        })?,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    t.set(
        "stop",
        lua.create_function(move |_, _: ()| {
            let _ = broadcast_tx.send(SocketMessage::Playback(PlaybackState::Stopped));
            Ok(())
        })?,
    )?;

    let broadcast_tx = broadcast_tx_prime.clone();
    t.set(
        "update_code",
        lua.create_function(move |_, code: String| {
            let _ = broadcast_tx.send(SocketMessage::Code(code));
            Ok(())
        })?,
    )?;

    Ok(t)
}

#[mlua::lua_module]
fn strudelserver(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("start_server", lua.create_function(start_server)?)?;
    Ok(exports)
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
    fn new() -> anyhow::Result<Self> {
        let (tx, rx) = channel(16);
        let (broadcast_tx, _) = broadcast::channel(16);
        Ok(Self {
            port: None,
            rx,
            tx,
            broadcast_tx,
        })
    }

    fn run(&mut self) -> anyhow::Result<()> {
        let runtime = Runtime::new()?;
        runtime.block_on(async {
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

            runtime.spawn(async {
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
        })?;

        Ok(())
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

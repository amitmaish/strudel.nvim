use std::{ffi::c_void, thread};

use axum::{Router, response::Html, routing::get, serve};
use mlua::chunk;
use mlua::prelude::*;
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

fn hello(lua: &Lua, name: String) -> LuaResult<LuaTable> {
    let t = lua.create_table()?;
    t.set("name", name.clone())?;
    let _globals = lua.globals();
    lua.load(chunk! {
        print("hello, " .. $name)
    })
    .exec()?;
    Ok(t)
}

fn start_server(lua: &Lua, _: ()) -> LuaResult<LuaTable> {
    let mut app = App::new()?;
    let tx = app.get_tx();

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

    {
        let tx = tx.clone();
        t.set(
            "quit_server",
            lua.create_function(move |_, server_handle: LuaLightUserData| {
                if tx.blocking_send(AppMessage::Quit).is_err() {
                    Err(LuaError::RuntimeError(String::from(
                        "strudel server rx dropped",
                    )))
                } else {
                    let handle =
                        server_handle.0 as *mut thread::JoinHandle<Result<(), anyhow::Error>>;
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
    }
    {
        let tx = tx.clone();
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
    }
    {
        let tx = tx.clone();
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
    }
    Ok(t)
}

#[mlua::lua_module]
fn strudelserver(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("hello", lua.create_function(hello)?)?;
    exports.set("start_server", lua.create_function(start_server)?)?;
    Ok(exports)
}

struct App {
    runtime: Runtime,
    port: Option<u16>,
    rx: Receiver<AppMessage>,
    tx: Sender<AppMessage>,
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let runtime = Runtime::new()?;
        let (tx, rx) = channel(16);
        Ok(Self {
            runtime,
            port: None,
            rx,
            tx,
        })
    }

    fn run(&mut self) -> anyhow::Result<()> {
        self.runtime.block_on(async {
            let mut file = File::open("../strudel-frontend/dist/index.html").await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let app = Router::new()
                .route("/", get(|| async move { Html::from(contents) }))
                .nest_service(
                    "/assets/",
                    ServeDir::new("../strudel-frontend/dist/assets/"),
                );

            let listener = TcpListener::bind("localhost:0").await?;
            self.port = Some(listener.local_addr()?.port());
            let (shutdown_tx, shutdown_rx) = channel(1);

            async fn shutdown(mut shutdown_rx: Receiver<()>) {
                let _ = shutdown_rx.recv().await;
            }

            self.runtime.spawn(async {
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
}

enum AppMessage {
    GetPort(oneshot::Sender<Option<u16>>),
    Quit,
}

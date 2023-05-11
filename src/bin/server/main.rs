mod registry;

use anyhow::{anyhow, Result};

use std::{env, io::Error};
use std::sync::Arc;
use tokio_tungstenite::{tungstenite, WebSocketStream, tungstenite::protocol::Message};

use futures_util::{SinkExt, StreamExt};
use futures_util::stream::{SplitStream, SplitSink};
use tokio::net::{TcpListener, TcpStream};

use std::time::Duration;
use tokio::{time};

use registry::{Registry, GameCtx};

use connect4::{WSClientToServer};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let addr = env::args().nth(1).unwrap_or_else(|| "0.0.0.0:7248".to_string());

    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("failed to bind");
    println!("Listening on: {}", addr);

    let r = Arc::new(Registry::new());

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_conn(r.clone(), stream));
    }

    Ok(())
}

async fn handle_conn(r: Arc<Registry>, stream: TcpStream) -> Result<()> {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Err(e) => {
            println!("Error during the websocket handshake: {}", e);
            return Err(anyhow!("{}", e));
        }
        Ok(v) => v,
    };

    println!("New websocket connection: {}", addr);

    let (mut write, mut read) = ws_stream.split();

    // Wait for the hello message.
    let recv = read.next().await.ok_or(anyhow!("failed to read from ws"))??;
    let msg: WSClientToServer = serde_json::from_str(&recv.to_string())?;

    let hello = match msg {
        WSClientToServer::Hello(msg) => msg,
        v => {
            // TODO: format as JSON
            let _ = write.send(tungstenite::Message::Text(format!("expected hello"))).await;
 
            return Err(anyhow!("expected hello, got {:?}", v));
        },
    };

    let player_id = addr.to_string();

    let game_ctx = match r.join_or_create_game(&hello.game_id, &player_id).await {
        Ok(v) => v,
        Err(err) => {
            // TODO: format as JSON
            let _ = write.send(tungstenite::Message::Text(format!("no game for you: {}", err))).await;
            return Err(err);
        }
    };

    let leave_msg = match handle_player(game_ctx.clone(), &player_id, write, read).await {
        Ok(()) => format!("ok"),
        Err(err) => format!("err: {}", err),
    };

    r.leave_game(&hello.game_id, &player_id).await;

    Err(anyhow!("left game: {}", leave_msg))
}

async fn handle_player(
    game_ctx: Arc<GameCtx>,
    player_id: &str,
    mut write: SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>,
    mut read: SplitStream<WebSocketStream<tokio::net::TcpStream>>,
) -> Result<()> {
    println!("handling game {} for {}", game_ctx.id, &player_id);

    let mut interval = time::interval(Duration::from_millis(1000));

    loop {
        tokio::select! {
            Some(v) = read.next() => {
                let recv = v?;

                println!("new msg: {:?}", recv);
                write.send(tungstenite::Message::Text("my message ".to_string() + &recv.to_string())).await?
            }

            _ = interval.tick() => {
                println!("tick");
                write.send(tungstenite::Message::Text("ping".to_string())).await.expect("boo");
            }
        }
    }

    Ok(())
}

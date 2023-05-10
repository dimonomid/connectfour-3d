mod registry;

use std::{env, io::Error};
use std::sync::Arc;
use tokio_tungstenite::tungstenite;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};

use std::time::Duration;
use tokio::{time};

use registry::Registry;

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

async fn handle_conn(r: Arc<Registry>, stream: TcpStream) {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Err(e) => {
            println!("Error during the websocket handshake: {}", e);
            return;
        }
        Ok(v) => v,
    };

    println!("New websocket connection: {}", addr);

    let (mut write, mut read) = ws_stream.split();

    // TODO: get those from the message that we should wait from the client first.
    let game_id = "mygame".to_string();
    let player_id = addr.to_string();

    match r.join_or_create_game(&game_id, &player_id).await {
        Ok(_) => {
            println!("hey got our game");
        },
        Err(err) => {
            println!("failed to create game: {}", err);

            // TODO: format as JSON
            let _ = write.send(tungstenite::Message::Text(format!("no game for you: {}", err))).await;
            return;
        }
    }

    let mut interval = time::interval(Duration::from_millis(1000));

    loop {
        tokio::select! {
            Some(v) = read.next() => {
                println!("new msg: {:?}", v);
                match write.send(tungstenite::Message::Text("my message ".to_string())).await {
                    Ok(_) => {},
                    Err(err) => {
                        println!("failed to read from conn: {}", err);
                        break;
                    }
                }
            }

            _ = interval.tick() => {
                println!("tick");
                write.send(tungstenite::Message::Text("ping".to_string())).await.expect("boo");
            }
        }
    }

    r.leave_game(&game_id, &player_id).await;
}

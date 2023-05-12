mod registry;

use std::{env, io::Error, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use registry::{GameCtx, PlayerToPlayer, Registry};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time;
use tokio_tungstenite::{tungstenite, tungstenite::protocol::Message, WebSocketStream};

use connectfour::game;
use connectfour::game_manager::GameState;
use connectfour::{WSClientToServer, WSFullGameState, WSGameReset, WSServerToClient};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // By default, listen on 0.0.0.0:7248.
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:7248".to_string());

    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("failed to bind");
    println!("Listening on: {}", addr);

    // Create registry to keep all active game data in.
    let r = Arc::new(Registry::new());

    // Listen forever, accepting incoming connections.
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_conn(r.clone(), stream));
    }

    Ok(())
}

/// Takes care of a single connection, until it is broken. Never returns Ok.
async fn handle_conn(r: Arc<Registry>, stream: TcpStream) -> Result<()> {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
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

    // Wait for the hello message first.
    let recv = read
        .next()
        .await
        .ok_or(anyhow!("failed to read from ws"))??;
    let msg: WSClientToServer = serde_json::from_str(&recv.to_string())?;

    let player_info = match msg {
        WSClientToServer::Hello(msg) => msg,
        v => {
            let j = serde_json::to_string(&WSServerToClient::Msg("expected hello".to_string()))?;
            let _ = write.send(tungstenite::Message::Text(j)).await;

            return Err(anyhow!("expected hello, got {:?}", v));
        }
    };

    let (to_player_tx, to_player_rx) = mpsc::channel::<PlayerToPlayer>(8);

    // Use player remote address as an ID. Player IDs must only be unique for a
    // particular game ID, but having them globally unique doesn't hurt.
    let player_id = addr.to_string();

    let game_ctx = match r
        .join_or_create_game(
            &player_info.game_id,
            &player_id,
            to_player_tx.clone(),
            player_info.game_state,
        )
        .await
    {
        Ok(v) => v,
        Err(err) => {
            let j = serde_json::to_string(&WSServerToClient::Msg(err.to_string()))?;
            let _ = write.send(tungstenite::Message::Text(j)).await;
            return Err(err);
        }
    };

    // Now that the player is authenticated and added to the game, defer all the
    // rest of the work on behalf of this player to handle_player.
    let leave_msg =
        match handle_player(game_ctx.clone(), &player_id, to_player_rx, write, read).await {
            Ok(()) => {
                panic!("should never happen");
            }
            Err(err) => format!("err: {}", err),
        };

    // The client has disconnected, remove it from the game (and potentially
    // destroy the game).
    r.leave_game(&player_info.game_id, &player_id).await;

    Err(anyhow!("left game: {}", leave_msg))
}

/// Take care of a single player, until the connection is broken. Never returns Ok.
async fn handle_player(
    game_ctx: Arc<GameCtx>,
    player_id: &str,
    mut from_opponent: mpsc::Receiver<PlayerToPlayer>,
    mut to_ws: SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>,
    mut from_ws: SplitStream<WebSocketStream<tokio::net::TcpStream>>,
) -> Result<()> {
    println!("handling game {} for {}", game_ctx.id, &player_id);

    let mut ping_interval = time::interval(Duration::from_millis(5000));
    let mut maybe_to_opponent: Option<mpsc::Sender<PlayerToPlayer>> = None;
    let mut side = game::Side::White;

    loop {
        tokio::select! {
            // Handle messages from websocket, so from the remote client on
            // behalf of which we're working here.
            Some(v) = from_ws.next() => {
                let recv = v?;

                let msg: WSClientToServer = serde_json::from_str(&recv.to_string())?;
                match msg {
                    WSClientToServer::Hello(_) => { return Err(anyhow!("did not expect hello")); }
                    WSClientToServer::PutToken(tcoords) => {
                        let mut gd = game_ctx.data.lock().await;

                        gd.game.put_token(side.opposite(), tcoords)?;
                        gd.game_state = GameState::WaitingFor(side);
                        drop(gd);

                        if let Some(to_opponent) = &maybe_to_opponent {
                            to_opponent.send(PlayerToPlayer::PutToken(tcoords)).await?;
                        }
                    },
                }
            }

            // Handle messages from the opponent, so another player connected to
            // the same server.
            Some(val) = from_opponent.recv() => {
                println!("player {}: received from another player: {:?}", player_id, val);

                match val {
                    PlayerToPlayer::OpponentIsHere(v) => {
                        maybe_to_opponent = Some(v.to_opponent);
                        side = v.my_side;

                        let gd = game_ctx.data.lock().await;
                        let game_reset = WSServerToClient::GameReset(WSGameReset{
                            opponent_name: "my opponent".to_string(), // TODO: actual name
                            game_state: WSFullGameState{
                                game_state: gd.game_state,
                                ws_player_side: side,
                                board: gd.game.get_board().clone(),
                            },
                        });

                        drop(gd);

                        let j = serde_json::to_string(&game_reset)?;
                        to_ws.send(tungstenite::Message::Text(j)).await?;
                    },
                    PlayerToPlayer::OpponentIsGone => {
                        maybe_to_opponent = None;

                        let j = serde_json::to_string(&WSServerToClient::OpponentIsGone)?;
                        to_ws.send(tungstenite::Message::Text(j)).await?;
                    }

                    PlayerToPlayer::PutToken(tcoords) => {
                        let put_token = WSServerToClient::PutToken(tcoords);
                        let j = serde_json::to_string(&put_token)?;
                        to_ws.send(tungstenite::Message::Text(j)).await?;
                    },
                }
            }

            _ = ping_interval.tick() => {
                let j = serde_json::to_string(&WSServerToClient::Ping)?;
                to_ws.send(tungstenite::Message::Text(j)).await?;
            }
        }
    }
}

mod registry;

use anyhow::{anyhow, Result};

use std::{env, io::Error};
use std::sync::Arc;
use tokio_tungstenite::{tungstenite, WebSocketStream, tungstenite::protocol::Message};

use futures_util::{SinkExt, StreamExt};
use futures_util::stream::{SplitStream, SplitSink};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use std::time::Duration;
use tokio::{time};

use registry::{Registry, GameCtx};

use connect4::{WSClientToServer, WSServerToClient, WSFullGameState, GameReset};
use connect4::game;
use connect4::game_manager::GameState;
use registry::PlayerToPlayer;

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

    let player_info = match msg {
        WSClientToServer::Hello(msg) => msg,
        v => {
            let j = serde_json::to_string(&WSServerToClient::Msg("expected hello".to_string()))?;
            let _ = write.send(tungstenite::Message::Text(j)).await;
 
            return Err(anyhow!("expected hello, got {:?}", v));
        },
    };

    let player_id = addr.to_string();

    let (to_player_tx, to_player_rx) = mpsc::channel::<PlayerToPlayer>(8);

    let game_ctx = match r.join_or_create_game(
        &player_info.game_id, &player_id, to_player_tx.clone(),
        player_info.game_state.clone(),
        ).await {
        Ok(v) => v,
        Err(err) => {
            let j = serde_json::to_string(&WSServerToClient::Msg(format!("no game for you: {}", err)))?;
            let _ = write.send(tungstenite::Message::Text(j)).await;
            return Err(err);
        }
    };

    let leave_msg = match handle_player(game_ctx.clone(), (*player_info.game_state).clone(), &player_id, to_player_rx, write, read).await {
        Ok(()) => format!("ok"),
        Err(err) => format!("err: {}", err),
    };

    r.leave_game(&player_info.game_id, &player_id).await;

    Err(anyhow!("left game: {}", leave_msg))
}

async fn handle_player(
    game_ctx: Arc<GameCtx>,
    game_state: WSFullGameState,
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
            Some(v) = from_ws.next() => {
                let recv = v?;
                println!("new msg: {:?}", recv);
                //to_ws.send(tungstenite::Message::Text("my message ".to_string() + &recv.to_string())).await?

                let msg: WSClientToServer = serde_json::from_str(&recv.to_string())?;
                match msg {
                    WSClientToServer::Hello(_) => { return Err(anyhow!("did not expect hello")); }
                    WSClientToServer::PutToken(coords) => {
                        let to_opponent = match maybe_to_opponent.clone() {
                            Some(sender) => sender,
                            None => {
                                // TODO: stash the value
                                println!("received coords while not having the opponent");
                                continue;
                            },
                        };

                        let _ = to_opponent.send(PlayerToPlayer::PutToken(coords)).await;
                    },
                }
            }

            Some(val) = from_opponent.recv() => {
                println!("player {}: received from another player: {:?}", player_id, val);

                match val {
                    PlayerToPlayer::OpponentIsHere(v) => {
                        maybe_to_opponent = Some(v.to_opponent);
                        side = v.my_side;

                        let mut gd = game_ctx.data.lock().await;
                        let game_reset = WSServerToClient::GameReset(GameReset{
                            opponent_name: Arc::new("my opponent".to_string()), // TODO: actual name
                            game_state: Arc::new(WSFullGameState{
                                game_state: gd.game_state,
                                ws_player_side: side,
                                board: gd.game.get_board().clone(),
                            }),
                        });

                        drop(gd);

                        let j = serde_json::to_string(&game_reset)?;
                        to_ws.send(tungstenite::Message::Text(j)).await?;
                    },
                    PlayerToPlayer::OpponentIsGone => { maybe_to_opponent = None; }

                    PlayerToPlayer::PutToken(coords) => {
                        println!("received PutToken({:?})", coords);

                        let mut gd = game_ctx.data.lock().await;
                        gd.game.put_token(side, coords.x, coords.z);
                        gd.game_state = GameState::WaitingFor(side.opposite());
                        drop(gd);

                        let put_token = WSServerToClient::PutToken(coords);
                        let j = serde_json::to_string(&put_token)?;
                        to_ws.send(tungstenite::Message::Text(j)).await?;
                    },
                }
            }

            _ = ping_interval.tick() => {
                let j = serde_json::to_string(&WSServerToClient::Ping)?;
                to_ws.send(tungstenite::Message::Text(j)).await.expect("boo");
            }
        }
    }

    Ok(())
}

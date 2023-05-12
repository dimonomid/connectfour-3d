use anyhow::{anyhow, Result};

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};

use super::{FullGameState, GameManagerToPlayer, GameState, PlayerState, PlayerToGameManager};
use crate::game;
use crate::{WSClientInfo, WSClientToServer, WSFullGameState, WSServerToClient};

use tokio::sync::mpsc;
use tokio::time;
use tokio::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite;

#[derive(Debug)]
pub enum PlayerLocalToUI {
    // Lets UI know that we're waiting for the input, and when it's done,
    // the resulting coords should be sent via the provided sender.
    RequestInput(game::Side, mpsc::Sender<game::CoordsXZ>),
}

pub struct PlayerWSClient {
    connect_url: url::Url,
    game_id: String,

    side: Option<game::Side>,

    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,

    server_msg: Option<String>,
}

impl PlayerWSClient {
    pub fn new(
        connect_url: url::Url,
        game_id: String,
        from_gm: mpsc::Receiver<GameManagerToPlayer>,
        to_gm: mpsc::Sender<PlayerToGameManager>,
    ) -> PlayerWSClient {
        PlayerWSClient {
            connect_url,
            game_id,
            side: None,
            from_gm,
            to_gm,
            server_msg: None,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.handle_ws_conn().await {
                Ok(()) => {
                    panic!("should never be ok");
                }
                Err(err) => {
                    println!("ws conn error: {}", &err);
                    self.upd_state_not_ready(&err.to_string()).await?;
                }
            }

            time::sleep(Duration::from_millis(1000)).await;
        }
    }

    pub async fn handle_ws_conn(&mut self) -> Result<()> {
        self.upd_state_not_ready("connecting to server...").await?;

        let (ws_stream, _) = connect_async(&self.connect_url).await?;
        println!("WebSocket handshake has been successfully completed");

        self.upd_state_not_ready("authenticating...").await?;

        let (mut to_ws, mut from_ws) = ws_stream.split();

        let hello = WSClientToServer::Hello(WSClientInfo {
            game_id: Arc::new(self.game_id.clone()),
            player_name: Arc::new("me".to_string()), // TODO: OS username (but it's actually not used by the server yet).

            // TODO: send actual current board state. This way, the game can resume if server was
            // rebooted while both clients kept running and eventually reconnected.
            game_state: Arc::new(WSFullGameState {
                game_state: GameState::WaitingFor(game::Side::White),
                ws_player_side: game::Side::White,
                board: game::BoardState::new(),
            }),
        });

        let j = serde_json::to_string(&hello)?;
        to_ws.send(tungstenite::Message::Text(j)).await?;

        self.upd_state_not_ready("connected, waiting for the opponent...")
            .await?;

        loop {
            tokio::select! {
                v = from_ws.next() => {
                    let recv = v.ok_or(anyhow!("failed to read from ws"))??;

                    let msg: WSServerToClient = match serde_json::from_str(&recv.to_string()) {
                        Ok(v) => v,
                        Err(err) => { return Err(anyhow!("failed to parse {:?}: {}", recv, err)); }
                    };

                    println!("received: {:?}", msg);

                    match msg {
                        WSServerToClient::Ping => {},
                        WSServerToClient::Msg(s) => {
                            println!("got message from server: {}", s);
                            self.upd_state_not_ready(&s).await?;
                            self.server_msg = Some(s);
                        }
                        WSServerToClient::GameReset(v) => {
                            self.upd_state_ready().await?;
                            self.to_gm
                                .send(PlayerToGameManager::SetFullGameState(FullGameState{
                                    game_state: v.game_state.game_state,
                                    primary_player_side: v.game_state.ws_player_side,
                                    board: v.game_state.board.clone(),
                                }))
                                .await?;
                        }
                        WSServerToClient::PutToken(coords) => {
                            self.to_gm.send(PlayerToGameManager::PutToken(coords)).await?;
                        }
                        WSServerToClient::OpponentIsGone => {
                            self.upd_state_not_ready("opponent disconnected, waiting...").await?;
                        }
                    }
                },

                Some(val) = self.from_gm.recv() => {
                    println!("ws player {:?}: received from GM: {:?}", self.side, val);

                    match val {
                        GameManagerToPlayer::Reset(_board, new_side) => {
                            self.side = Some(new_side);
                        },
                        GameManagerToPlayer::OpponentPutToken(coords) => {
                            let msg = WSClientToServer::PutToken(coords);
                            let j = serde_json::to_string(&msg)?;
                            to_ws.send(tungstenite::Message::Text(j)).await?;
                        },
                        GameManagerToPlayer::GameState(_) => {},
                    }
                }
            }
        }
    }

    pub async fn upd_state_not_ready(&mut self, mut state: &str) -> Result<()> {
        let mut tmp;
        if let Some(server_msg) = &self.server_msg {
            tmp = server_msg.clone();
            tmp.push_str(": ");
            tmp.push_str(state);
            state = &tmp;
        }

        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::NotReady(
                state.to_string(),
            )))
            .await?;

        Ok(())
    }

    pub async fn upd_state_ready(&mut self) -> Result<()> {
        self.server_msg = None;
        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::Ready))
            .await?;

        Ok(())
    }
}

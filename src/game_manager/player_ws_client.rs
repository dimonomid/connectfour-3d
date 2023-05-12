use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time;
use tokio::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite;

use super::{FullGameState, GameManagerToPlayer, GameState, PlayerState, PlayerToGameManager};
use crate::game;
use crate::{WSClientInfo, WSClientToServer, WSFullGameState, WSServerToClient};

/// WebSocket client player, which will get actual moves from the remote player
/// via the server.
pub struct PlayerWSClient {
    connect_url: url::Url,
    game_id: String,

    /// Current player side, if any.
    side: Option<game::Side>,

    /// Channels for communicating with the GameManager.
    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,

    /// Last error message that we received from the server; all the
    /// PlayerState::NonReady states that this player generates will be
    /// prepended with that message, until the status finally becomes
    /// PlayerState::Ready.
    server_msg: Option<String>,
}

impl PlayerWSClient {
    /// Create a new WS client player.
    ///
    /// Game ID is how the server matches up the players against each other;
    /// obviously exactly two players with the same game id are required for the
    /// game to take place.
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

    /// Event loop, runs forever, should be swapned by the client code as a
    /// separate task.
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

    /// Tries to connect, and maintains this connection until it dies. Never
    /// returns Ok.
    pub async fn handle_ws_conn(&mut self) -> Result<()> {
        self.upd_state_not_ready("connecting to server...").await?;

        let (ws_stream, _) = connect_async(&self.connect_url).await?;
        println!("WebSocket handshake has been successfully completed");

        self.upd_state_not_ready("authenticating...").await?;

        let (mut to_ws, mut from_ws) = ws_stream.split();

        // Now that we connected, authenticate with the server.
        let hello = WSClientToServer::Hello(WSClientInfo {
            game_id: self.game_id.clone(),

            // TODO: OS username (but it's actually not used by the server yet).
            player_name: "me".to_string(),

            // TODO: send actual current board state, instead of generating a
            // brand new one. This way, the game can resume if server was
            // rebooted while both clients kept running and eventually
            // reconnected.
            game_state: WSFullGameState {
                game_state: GameState::WaitingFor(game::Side::White),
                ws_player_side: game::Side::White,
                board: game::BoardState::new(),
            },
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
                            // Server reset the game, it means we're just meeting with the other
                            // player, and so we need to let GameManager know two things: that
                            // we're ready to play, and also send the full game state to it.
                            self.upd_state_ready().await?;

                            self.to_gm
                                .send(PlayerToGameManager::SetFullGameState(FullGameState{
                                    game_state: v.game_state.game_state,
                                    primary_player_side: v.game_state.ws_player_side,
                                    board: v.game_state.board,
                                }))
                                .await?;
                        }
                        WSServerToClient::PutToken(pcoords) => {
                            // The remote player put token, so here we're communicating it to
                            // our local GameManager on their behalf.
                            self.to_gm.send(PlayerToGameManager::PutToken(pcoords)).await?;
                        }
                        WSServerToClient::OpponentIsGone => {
                            // Opponent is gone, so update our status.
                            self.upd_state_not_ready("opponent disconnected, waiting...").await?;
                        }
                    }
                },

                Some(val) = self.from_gm.recv() => {
                    println!("ws player {:?}: received from GM: {:?}", self.side, val);

                    match val {
                        GameManagerToPlayer::Reset(_board, new_side) => {
                            // Game manager lets us know our side. Actually that info originally
                            // came from the server, so we already could have remembered it when we
                            // received WSServerToClient::GameReset, but the protocol is that
                            // GameManager assigns it to the players, so we just play ball.
                            self.side = Some(new_side);
                        },
                        GameManagerToPlayer::OpponentPutToken(pcoords) => {
                            // Our local opponent put token, so send that info to the server.
                            let msg = WSClientToServer::PutToken(pcoords);
                            let j = serde_json::to_string(&msg)?;
                            to_ws.send(tungstenite::Message::Text(j)).await?;
                        },
                        GameManagerToPlayer::GameStateChanged(_) => {},
                    }
                }
            }
        }
    }

    /// Communicate the NotReady state to the GameManager. Other than just
    /// passing the given state string, it also prepends the state with whatever
    /// error message we last received from the server (WSServerToClient::Msg),
    /// if any.
    async fn upd_state_not_ready(&mut self, mut state: &str) -> Result<()> {
        // If we have server_msg stored, then prepend the state with that server_msg.
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

    /// Communicate the Ready state to the GameManager.
    pub async fn upd_state_ready(&mut self) -> Result<()> {
        self.server_msg = None;
        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::Ready))
            .await?;

        Ok(())
    }
}

use anyhow::{anyhow, Result};

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};

use super::{GameManagerToPlayer, PlayerState, GameState, PlayerToGameManager};
use crate::game;
use crate::{WSClientToServer, WSClientInfo, WSFullGameState};

use tokio::sync::mpsc;
use tokio::{time};
use tokio::time::{Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::tungstenite;

#[derive(Debug)]
pub enum PlayerLocalToUI {
    // Lets UI know that we're waiting for the input, and when it's done,
    // the resulting coords should be sent via the provided sender.
    RequestInput(game::Side, mpsc::Sender<game::CoordsXZ>),
}

pub struct PlayerWSClient {
    connect_url: url::Url,

    side: Option<game::Side>,

    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,
}

impl PlayerWSClient {
    pub fn new(
        connect_url: url::Url,
        from_gm: mpsc::Receiver<GameManagerToPlayer>,
        to_gm: mpsc::Sender<PlayerToGameManager>,
    ) -> PlayerWSClient {
        PlayerWSClient {
            connect_url,
            side: None,
            from_gm,
            to_gm,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        println!("ws client {:?}: letting GM know that we're connecting", self.side);
        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::NotReady("Connecting to server...".to_string())))
            .await?;

        loop {
            match self.handle_conn().await {
                Ok(()) => { println!("ws conn ok"); },
                Err(err) => { println!("ws conn error: {}", err); }
            }

            time::sleep(Duration::from_millis(1000)).await;
        }
    }

    pub async fn handle_conn(&mut self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.connect_url).await?;
        println!("WebSocket handshake has been successfully completed");

        let (mut write, mut read) = ws_stream.split();

        let hello = WSClientToServer::Hello(WSClientInfo{
            game_id: Arc::new("mygame".to_string()),
            player_name: Arc::new("me".to_string()), // TODO: OS username
            game_state: Arc::new(WSFullGameState{
                game_state: GameState::WaitingFor(game::Side::White),
                ws_player_side: game::Side::White,
                board: game::BoardState::new(),
            }),
        });

        let j = serde_json::to_string(&hello)?;
        write.send(tungstenite::Message::Text(j)).await?;
        loop {
            let received = read.next().await.ok_or(anyhow!("failed to read from ws"))??;
            println!("received: {:?}", received);
        }
    }

    //async fn handle_game_state(&mut self, _state: GameState) -> Result<()> {
        // TODO

        //match state {
            //PlayerGameState::YourTurn => {
                //self.to_ui
                    //.send(PlayerLocalToUI::RequestInput(
                        //self.side.unwrap(),
                        //self.coords_from_ui_sender.clone(),
                    //))
                    //.await?;
            //}

            //// We don't need to do anything special on any other game state, but still enumerating
            //// them all explicitly so that if the enum changes, we're forced by the compiler to
            //// revisit this logic.
            //PlayerGameState::NoGame => {}
            //PlayerGameState::OpponentsTurn => {}
            //PlayerGameState::OpponentWon => {}
            //PlayerGameState::YouWon => {}
        //};

        //Ok(())
    //}
}

//pub enum ConnState {
    //Waiting,
    //Connecting
//}

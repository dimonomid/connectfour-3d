use anyhow::Result;

use super::{GameManagerToPlayer, PlayerGameState, PlayerState, PlayerToGameManager};
use crate::game;

use tokio::sync::mpsc;

#[derive(Debug)]
pub enum PlayerLocalToUI {
    // Lets UI know that we're waiting for the input, and when it's done,
    // the resulting coords should be sent via the provided sender.
    RequestInput(game::Side, mpsc::Sender<game::CoordsXZ>),
}

pub struct PlayerLocal {
    side: Option<game::Side>,

    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,

    to_ui: mpsc::Sender<PlayerLocalToUI>,

    coords_from_ui_sender: mpsc::Sender<game::CoordsXZ>,
    coords_from_ui_receiver: mpsc::Receiver<game::CoordsXZ>,
}

impl PlayerLocal {
    pub fn new(
        side: Option<game::Side>,
        from_gm: mpsc::Receiver<GameManagerToPlayer>,
        to_gm: mpsc::Sender<PlayerToGameManager>,
        to_ui: mpsc::Sender<PlayerLocalToUI>,
    ) -> PlayerLocal {
        // Create the channel which we'll be asking UI to send the user-picked coords to.
        let (coords_from_ui_sender, coords_from_ui_receiver) = mpsc::channel::<game::CoordsXZ>(1);

        PlayerLocal {
            side: side,
            from_gm,
            to_gm,
            to_ui,
            coords_from_ui_sender,
            coords_from_ui_receiver,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // If we knew our side from the beginning, let GameManager know immediately.
        self.update_state_if_ready().await?;

        loop {
            tokio::select! {
                Some(val) = self.from_gm.recv() => {
                    println!("player {:?}: received from GM: {:?}", self.side, val);

                    match val {
                        GameManagerToPlayer::SetSide(new_side) => {
                            // GameManager lets us know about our side. If we knew that already,
                            // then do nothing; otherwise, update the side, and also let
                            // GameManager know about our updated state.
                            match self.side {
                                Some(side) if side == new_side => { continue; }
                                _ => {
                                    self.side = Some(new_side);
                                    self.update_state_if_ready().await?;
                                },
                            }

                        },
                        GameManagerToPlayer::OpponentPutToken(_) => {},
                        GameManagerToPlayer::GameState(state) => {
                            self.handle_game_state(state).await?;
                        },
                    }
                }

                Some(coords) = self.coords_from_ui_receiver.recv() => {
                    println!("got coords from UI: {:?}", &coords);
                    self.to_gm.send(PlayerToGameManager::PutToken(coords)).await?;
                }
            }
        }
    }

    async fn update_state_if_ready(&mut self) -> Result<()> {
        if let Some(side) = self.side {
            println!("player {:?}: letting GM know that we're ready", self.side);
            let state = PlayerState::Ready(side);
            self.to_gm
                .send(PlayerToGameManager::StateChanged(state))
                .await?;
        }

        Ok(())
    }

    async fn handle_game_state(&mut self, state: PlayerGameState) -> Result<()> {
        match state {
            PlayerGameState::YourTurn => {
                self.to_ui
                    .send(PlayerLocalToUI::RequestInput(
                        self.side.unwrap(),
                        self.coords_from_ui_sender.clone(),
                    ))
                    .await?;
            }

            // We don't need to do anything special on any other game state, but still enumerating
            // them all explicitly so that if the enum changes, we're forced by the compiler to
            // revisit this logic.
            PlayerGameState::NoGame => {}
            PlayerGameState::OpponentsTurn => {}
            PlayerGameState::OpponentWon => {}
            PlayerGameState::YouWon => {}
        };

        Ok(())
    }
}

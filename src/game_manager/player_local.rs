use anyhow::Result;

use super::{FullGameState, GameManagerToPlayer, GameState, PlayerState, PlayerToGameManager};
use crate::game;

use tokio::sync::mpsc;

#[derive(Debug)]
pub enum PlayerLocalToUI {
    // Lets UI know that we're waiting for the input, and when it's done,
    // the resulting coords should be sent via the provided sender.
    RequestInput(game::Side, mpsc::Sender<game::PoleCoords>),
}

pub struct PlayerLocal {
    side: Option<game::Side>,

    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,

    to_ui: mpsc::Sender<PlayerLocalToUI>,

    coords_from_ui_sender: mpsc::Sender<game::PoleCoords>,
    coords_from_ui_receiver: mpsc::Receiver<game::PoleCoords>,
}

impl PlayerLocal {
    /// If side is set, PlayerLocal will assume it's the primary player, and will generate the
    /// initial update to set up an empty board.
    pub fn new(
        side: Option<game::Side>,
        from_gm: mpsc::Receiver<GameManagerToPlayer>,
        to_gm: mpsc::Sender<PlayerToGameManager>,
        to_ui: mpsc::Sender<PlayerLocalToUI>,
    ) -> PlayerLocal {
        // Create the channel which we'll be asking UI to send the user-picked coords to.
        let (coords_from_ui_sender, coords_from_ui_receiver) = mpsc::channel::<game::PoleCoords>(1);

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
        // If the PlayerLocal was constructed with the side right away (which has to be done if the
        // player is a primary one), then set the initial game state to the GameManager,
        // saying that it's our turn, with an empty board.
        if let Some(side) = self.side {
            self.to_gm
                .send(PlayerToGameManager::SetFullGameState(FullGameState {
                    game_state: GameState::WaitingFor(side),
                    primary_player_side: side,
                    board: game::BoardState::new(),
                }))
                .await?;
        }

        println!("player {:?}: letting GM know that we're ready", self.side);
        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::Ready))
            .await?;

        loop {
            tokio::select! {
                Some(val) = self.from_gm.recv() => {
                    println!("player {:?}: received from GM: {:?}", self.side, val);

                    match val {
                        GameManagerToPlayer::Reset(_board, new_side) => {
                            self.side = Some(new_side);
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

    async fn handle_game_state(&mut self, state: GameState) -> Result<()> {
        match state {
            GameState::WaitingFor(next_move_side) => {
                let my_side = match self.side {
                    Some(side) => side,
                    None => {
                        return Ok(());
                    }
                };

                if my_side != next_move_side {
                    return Ok(());
                }

                // It's our turn, so request input from the UI.
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
            GameState::WonBy(_) => {}
        };

        Ok(())
    }
}

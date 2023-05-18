use anyhow::Result;
use tokio::sync::mpsc;

use super::{FullGameState, GameManagerToPlayer, GameState, PlayerState, PlayerToGameManager};
use crate::game;

/// Local player, which will request actual moves from the UI via the to_ui
/// sender.
pub struct PlayerLocal {
    /// Current player side, if any.
    side: Option<game::Side>,

    /// Channels for communicating with the GameManager.
    from_gm: mpsc::Receiver<GameManagerToPlayer>,
    to_gm: mpsc::Sender<PlayerToGameManager>,

    /// Channel for communication with the UI (only to request input from it,
    /// whenever it's our turn).
    to_ui: mpsc::Sender<PlayerLocalToUI>,

    /// Channel to get the response back from the UI. The sender is here as
    /// well, because this player will send that sender to the UI every time.
    coords_from_ui_sender: mpsc::Sender<game::PoleCoords>,
    coords_from_ui_receiver: mpsc::Receiver<game::PoleCoords>,
}

impl PlayerLocal {
    /// Create a new local player. It will request actual moves from the UI via
    /// the to_ui sender.
    ///
    /// If side is set, PlayerLocal will assume it's the primary player, and
    /// will generate the initial update to set up an empty board. Why does a
    /// player have to send messages like that - see
    /// PlayerToGameManager::SetFullGameState.
    pub fn new(
        side: Option<game::Side>,
        from_gm: mpsc::Receiver<GameManagerToPlayer>,
        to_gm: mpsc::Sender<PlayerToGameManager>,
        to_ui: mpsc::Sender<PlayerLocalToUI>,
    ) -> PlayerLocal {
        // Create the channel which we'll be asking UI to send the user-picked coords to.
        let (coords_from_ui_sender, coords_from_ui_receiver) = mpsc::channel::<game::PoleCoords>(1);

        PlayerLocal {
            side,
            from_gm,
            to_gm,
            to_ui,
            coords_from_ui_sender,
            coords_from_ui_receiver,
        }
    }

    /// Event loop, runs forever, should be swapned by the client code as a separate task.
    pub async fn run(&mut self) -> Result<()> {
        // If the PlayerLocal was constructed with the side right away (which
        // has to be done if the player is a primary one), then set the initial
        // game state to the GameManager, saying that it's our turn, with an
        // empty board.
        if let Some(side) = self.side {
            self.to_gm
                .send(PlayerToGameManager::SetFullGameState(FullGameState {
                    game_state: GameState::WaitingFor(side),
                    primary_player_side: side,
                    board: game::BoardState::new(),
                }))
                .await?;
        }

        //println!("player {:?}: letting GM know that we're ready", self.side);
        self.to_gm
            .send(PlayerToGameManager::StateChanged(PlayerState::Ready))
            .await?;

        loop {
            tokio::select! {
                Some(val) = self.from_gm.recv() => {
                    //println!("player {:?}: received from GM: {:?}", self.side, val);

                    match val {
                        GameManagerToPlayer::Reset(_board, new_side) => {
                            self.side = Some(new_side);
                        },
                        GameManagerToPlayer::OpponentPutToken(_) => {},
                        GameManagerToPlayer::GameStateChanged(state) => {
                            self.handle_game_state(state).await?;
                        },
                    }
                }

                Some(pcoords) = self.coords_from_ui_receiver.recv() => {
                    println!("got pole coords from UI: {:?}", &pcoords);
                    self.to_gm.send(PlayerToGameManager::PutToken(pcoords)).await?;
                }
            }
        }
    }

    /// Called whenever game stat changes. Whenever the state changes so that
    /// it's our turn now, it will request input from the UI.
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

                // It's our turn, so request input from the UI, passing it a channel to send the
                // result to.
                self.to_ui
                    .send(PlayerLocalToUI::RequestInput(
                        self.side.unwrap(),
                        self.coords_from_ui_sender.clone(),
                    ))
                    .await?;
            }

            // We don't need to do anything special on any other game state, but
            // still enumerating them all explicitly so that if the enum
            // changes, we're forced by the compiler to revisit this logic.
            GameState::WonBy(_) => {}
        };

        Ok(())
    }
}

#[derive(Debug)]
pub enum PlayerLocalToUI {
    // Lets UI know that we're waiting for the input, and when it's done,
    // the resulting coords should be sent via the provided sender.
    RequestInput(game::Side, mpsc::Sender<game::PoleCoords>),
}

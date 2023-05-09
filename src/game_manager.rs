pub mod player_local;

use super::game;
use anyhow::{Context, Result};

use tokio::sync::mpsc;

pub struct GameManager {
    game: game::Game,
    cur_side: game::Side,

    to_ui: mpsc::Sender<GameManagerToUI>,
    pwhite_chans: PlayerChans,
    pblack_chans: PlayerChans,
}

pub struct PlayerChans {
    pub to: mpsc::Sender<GameManagerToPlayer>,
    pub from: mpsc::Receiver<PlayerToGameManager>,
}

impl GameManager {
    pub fn new(
        to_ui: mpsc::Sender<GameManagerToUI>,
        pwhite_chans: PlayerChans,
        pblack_chans: PlayerChans,
    ) -> GameManager {
        GameManager {
            game: game::Game::new(),
            cur_side: game::Side::White,

            to_ui,
            pwhite_chans,
            pblack_chans,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        self.upd_player_turns().await.context("initial update")?;

        loop {
            tokio::select! {
                Some(val) = self.pwhite_chans.from.recv() => {
                    match val {
                        PlayerToGameManager::PutToken(coords) => {
                            self.handle_player_put_token(
                                game::Side::White, coords,
                            ).await.context("white")?;
                        },
                    }
                }

                Some(val) = self.pblack_chans.from.recv() => {
                    match val {
                        PlayerToGameManager::PutToken(coords) => {
                            self.handle_player_put_token(
                                game::Side::Black, coords,
                            ).await.context("black")?;
                        },
                    }
                }
            }
        }
    }

    pub async fn upd_player_turns(&mut self) -> Result<()> {
        self.player_chans_by_side(self.cur_side)
            .to
            .send(GameManagerToPlayer::GameState(PlayerGameState::YourTurn))
            .await
            .context(format!("player {:?}", self.cur_side))?;

        let opposite_side = self.cur_side.opposite();
        self.player_chans_by_side(opposite_side)
            .to
            .send(GameManagerToPlayer::GameState(
                PlayerGameState::OpponentsTurn,
            ))
            .await
            .context(format!("player {:?}", opposite_side))?;

        Ok(())
    }

    pub fn player_chans_by_side(&self, side: game::Side) -> &PlayerChans {
        return match side {
            game::Side::White => &self.pwhite_chans,
            game::Side::Black => &self.pblack_chans,
        };
    }

    pub async fn handle_player_put_token(
        &mut self,
        side: game::Side,
        coords: game::CoordsXZ,
    ) -> Result<()> {
        println!("GM: player {:?} put token {:?}", side, coords);

        if side != self.cur_side {
            println!("wrong side: {:?}, waiting for {:?}", side, self.cur_side);
            self.upd_player_turns().await?;
            return Ok(());
        }

        let res = match self.game.put_token(side, coords.x, coords.z) {
            Ok(res) => res,
            Err(err) => {
                println!("can't put: {}", err);
                self.upd_player_turns().await?;
                return Ok(());
            }
        };

        self.to_ui
            .send(GameManagerToUI::SetToken(
                side,
                game::CoordsFull {
                    x: coords.x,
                    y: res.y,
                    z: coords.z,
                },
            ))
            .await
            .context("updating UI")?;

        self.cur_side = self.cur_side.opposite();
        self.upd_player_turns().await?;

        Ok(())
    }
}

/// Game state from the point of view of a particular player.
#[derive(Debug, Eq, PartialEq)]
pub enum PlayerGameState {
    OpponentsTurn,
    YourTurn,
    OpponentWon,
    YouWon,
}

/// Message that GameManager can send to a player.
#[derive(Debug)]
pub enum GameManagerToPlayer {
    OpponentPutToken(game::CoordsXZ),
    GameState(PlayerGameState),
}

/// Message that a player can send to GameManager.
#[derive(Debug)]
pub enum PlayerToGameManager {
    PutToken(game::CoordsXZ),
}

/// Message that a GameManager can send to UI.
#[derive(Debug)]
pub enum GameManagerToUI {
    SetToken(game::Side, game::CoordsFull),
}

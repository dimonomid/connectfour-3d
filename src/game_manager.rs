pub mod player_local;

use super::game;
use anyhow::{anyhow, Context, Result};

use tokio::sync::mpsc;

pub struct GameManager {
    game: game::Game,
    cur_side: Option<game::Side>,

    to_ui: mpsc::Sender<GameManagerToUI>,
    players: [PlayerCtx; 2],
}

struct PlayerCtx {
    side: Option<game::Side>,
    state: PlayerState,

    to: mpsc::Sender<GameManagerToPlayer>,
    from: mpsc::Receiver<PlayerToGameManager>,
}

impl GameManager {
    pub fn new(
        to_ui: mpsc::Sender<GameManagerToUI>,

        to_p0: mpsc::Sender<GameManagerToPlayer>,
        from_p0: mpsc::Receiver<PlayerToGameManager>,

        to_p1: mpsc::Sender<GameManagerToPlayer>,
        from_p1: mpsc::Receiver<PlayerToGameManager>,
    ) -> GameManager {
        let p0 = PlayerCtx {
            state: PlayerState::NotReady("unknown".to_string()),
            side: None,
            to: to_p0,
            from: from_p0,
        };

        let p1 = PlayerCtx {
            state: PlayerState::NotReady("unknown".to_string()),
            side: None,
            to: to_p1,
            from: from_p1,
        };

        GameManager {
            game: game::Game::new(),
            cur_side: None,

            to_ui,
            players: [p0, p1],
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            let (p0_mut, p1_mut) = self.both_players_mut();

            tokio::select! {
                Some(val) = p0_mut.from.recv() => {
                    self.handle_player_msg(0, val).await?;
                }

                Some(val) = p1_mut.from.recv() => {
                    self.handle_player_msg(1, val).await?;
                }
            }
        }
    }

    pub async fn upd_player_turns(&mut self) -> Result<()> {
        let cur_side = match self.cur_side {
            Some(side) => side,
            None => {
                return Err(anyhow!("no current side"));
            }
        };

        self.player_by_side(cur_side)?
            .to
            .send(GameManagerToPlayer::GameState(PlayerGameState::YourTurn))
            .await
            .context(format!("player {:?}", cur_side))?;

        let opposite_side = cur_side.opposite();
        self.player_by_side(opposite_side)?
            .to
            .send(GameManagerToPlayer::GameState(
                PlayerGameState::OpponentsTurn,
            ))
            .await
            .context(format!("player {:?}", opposite_side))?;

        Ok(())
    }

    async fn handle_player_state_change(&mut self, i: usize, state: PlayerState) -> Result<()> {
        // Remember state for the player which sent us the update.
        self.players[i].state = state.clone();

        // TODO: update UI about the player state

        Ok(())
    }

    async fn handle_player_set_side(&mut self, i: usize, side: game::Side) -> Result<()> {
        if i != 0 {
            println!("player {} is not primary, so ignoring its side update ({:?})", i, side);
            return Ok(());
        }

        // Remember state for the player which sent us the update.
        self.players[i].side = Some(side);

        // Let opponent know about its side as well.
        let opposite_side = side.opposite();
        let opponent_idx = Self::opponent_idx(i);
        self.players[opponent_idx].side = Some(opposite_side);
        let opponent = &self.players[opponent_idx];
        opponent
            .to
            .send(GameManagerToPlayer::SetSide(opposite_side))
            .await
            .context(format!("setting player {} side to {:?}", opponent_idx, opposite_side))?;

        println!("sides are known: {}: {:?}, {}: {:?}, gonna start", i, side, opponent_idx, opposite_side);
        self.cur_side = Some(game::Side::White);
        self.upd_player_turns().await.context("initial update")?;

        Ok(())
    }

    fn both_players_mut(&mut self) -> (&mut PlayerCtx, &mut PlayerCtx) {
        let (v0, v1) = self.players.split_at_mut(1);
        return (&mut v0[0], &mut v1[0]);
    }

    fn opponent_idx(i: usize) -> usize {
        match i {
            0 => 1,
            1 => 0,
            // TODO: create an enum for player indices, instead if usize
            _ => {
                panic!("invalid player index {}", i)
            }
        }
    }

    fn player_by_side(&self, side: game::Side) -> Result<&PlayerCtx> {
        match self.players[0].side {
            Some(v) => {
                if side == v {
                    return Ok(&self.players[0]);
                }

                return Ok(&self.players[1]);
            }
            None => Err(anyhow!("player 0 doesn't have a side")),
        }
    }

    pub async fn handle_player_msg(&mut self, i: usize, msg: PlayerToGameManager) -> Result<()> {
        match msg {
            PlayerToGameManager::SetSide(side) => {
                self.handle_player_set_side(i, side).await?;
                Ok(())
            }
            PlayerToGameManager::StateChanged(state) => {
                self.handle_player_state_change(i, state).await?;
                Ok(())
            }
            PlayerToGameManager::PutToken(coords) => {
                self.handle_player_put_token(i, coords).await?;
                Ok(())
            }
        }
    }

    pub async fn handle_player_put_token(
        &mut self,
        i: usize,
        coords: game::CoordsXZ,
    ) -> Result<()> {
        let side = self.players[i].side;

        println!("GM: player {:?} put token {:?}", side, coords);

        let cur_side = match self.cur_side {
            None => {
                println!("no current side, but player put token");
                self.upd_player_turns().await?;
                return Ok(());
            }
            Some(side) => side,
        };

        let player_side = match side {
            None => {
                println!("no current player side, but player put token");
                self.upd_player_turns().await?;
                return Ok(());
            }
            Some(side) => side,
        };

        if player_side != cur_side {
            println!("wrong side: {:?}, waiting for {:?}", player_side, cur_side);
            self.upd_player_turns().await?;
            return Ok(());
        }

        let res = match self.game.put_token(player_side, coords.x, coords.z) {
            Ok(res) => res,
            Err(err) => {
                println!("can't put: {}", err);
                self.upd_player_turns().await?;
                return Ok(());
            }
        };

        // Add new token to the UI.
        self.to_ui
            .send(GameManagerToUI::SetToken(
                player_side,
                game::CoordsFull {
                    x: coords.x,
                    y: res.y,
                    z: coords.z,
                },
            ))
            .await
            .context("updating UI")?;

        self.cur_side = Some(cur_side.opposite());
        self.upd_player_turns().await?;

        Ok(())
    }
}

/// Game state from the point of view of a particular player.
#[derive(Debug, Eq, PartialEq)]
pub enum PlayerGameState {
    NoGame,
    OpponentsTurn,
    YourTurn,
    OpponentWon,
    YouWon,
}

/// Player state from the point of view of the GameManager.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PlayerState {
    /// Not-yet-ready, with a human-readable string message explaining the status.
    NotReady(String),
    Ready,
}

/// Message that GameManager can send to a player.
#[derive(Debug)]
pub enum GameManagerToPlayer {
    SetSide(game::Side),
    OpponentPutToken(game::CoordsXZ),
    GameState(PlayerGameState),
}

/// Message that a player can send to GameManager.
#[derive(Debug)]
pub enum PlayerToGameManager {
    /// Set side of this player. GameManager will correspondingly update the side of the opponent
    /// player as well. Only primary player can send SetSide messages; if secondary player does
    /// this, GameManager will just ignore it.
    SetSide(game::Side),
    StateChanged(PlayerState),
    PutToken(game::CoordsXZ),
}

/// Message that a GameManager can send to UI.
#[derive(Debug)]
pub enum GameManagerToUI {
    SetToken(game::Side, game::CoordsFull),
}

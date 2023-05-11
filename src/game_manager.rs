pub mod player_local;
pub mod player_ws_client;

use super::game;
use anyhow::{anyhow, Context, Result};

use tokio::sync::mpsc;

pub struct GameManager {
    game: game::Game,
    game_state: Option<GameState>,

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
            game_state: None,

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
        self.players[0]
            .to
            .send(GameManagerToPlayer::GameState(self.game_state.unwrap()))
            .await
            .context(format!("player 0"))?;

        self.players[1]
            .to
            .send(GameManagerToPlayer::GameState(self.game_state.unwrap()))
            .await
            .context(format!("player 1"))?;

        Ok(())
    }

    async fn handle_player_state_change(&mut self, i: usize, state: PlayerState) -> Result<()> {
        // Remember state for the player which sent us the update.
        self.players[i].state = state.clone();

        // Update UI about the player state
        self.to_ui
            .send(GameManagerToUI::PlayerStateChanged(i, state.clone()))
            .await
            .context("updating UI")?;

        Ok(())
    }

    async fn handle_full_game_state(&mut self, i: usize, fgstate: FullGameState) -> Result<()> {
        if i != 0 {
            println!("player {} is not primary, so ignoring its FullGameState update ({:?})", i, fgstate);
            return Ok(());
        }

        // Update board state.
        self.game.reset_board(&fgstate.board);

        // Remember state for the player which sent us the update.
        self.players[0].side = Some(fgstate.primary_player_side);

        // Update it for the opponent as well.
        let opposite_side = fgstate.primary_player_side.opposite();
        let opponent_idx = Self::opponent_idx(0);
        self.players[opponent_idx].side = Some(opposite_side);

        // Reset the game for the opponent.
        let opponent = &self.players[opponent_idx];
        opponent
            .to
            .send(GameManagerToPlayer::Reset(fgstate.board.clone(), opposite_side))
            .await
            .context(format!("resetting player {}, setting side to {:?}", opponent_idx, opposite_side))?;

        // Update UI.
        self.to_ui
            .send(GameManagerToUI::ResetBoard(fgstate.board.clone()))
            .await
            .context("updating UI")?;

        // Update UI about the player sides.
        self.to_ui
            .send(GameManagerToUI::PlayerSidesChanged(fgstate.primary_player_side, opposite_side))
            .await
            .context("updating UI")?;

        println!("game state is known, gonna let players know: {}: {:?}, {}: {:?}", i, fgstate.primary_player_side, opponent_idx, opposite_side);

        self.game_state = Some(fgstate.game_state);

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
            PlayerToGameManager::SetFullGameState(fgstate) => {
                self.handle_full_game_state(i, fgstate).await?;
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

        let next_move_side = match self.game_state.unwrap() {
            GameState::WaitingFor(side) => side,
            GameState::WonBy(_) => {
                println!("game is won, but player put token");
                self.upd_player_turns().await?;
                return Ok(());
            },
        };

        let player_side = match side {
            None => {
                println!("no current player side, but player put token");
                self.upd_player_turns().await?;
                return Ok(());
            }
            Some(side) => side,
        };

        if player_side != next_move_side {
            println!("wrong side: {:?}, waiting for {:?}", player_side, next_move_side);
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

        let opposite_side = next_move_side.opposite();

        self.player_by_side(opposite_side).unwrap()
            .to
            .send(GameManagerToPlayer::OpponentPutToken(coords))
            .await?;

        self.game_state = Some(GameState::WaitingFor(opposite_side));
        self.upd_player_turns().await?;

        Ok(())
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GameState {
    WaitingFor(game::Side),
    WonBy(game::Side),
}

#[derive(Debug)]
pub struct FullGameState {
    pub game_state: GameState,

    /// Side of the primary player (the one who sends PlayerToGameManager::SetFullGameState with
    /// this full state).
    pub primary_player_side: game::Side,

    /// Full board state.
    pub board: game::BoardState,
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
    Reset(game::BoardState, game::Side),
    OpponentPutToken(game::CoordsXZ),
    GameState(GameState),
}

/// Message that a player can send to GameManager.
#[derive(Debug)]
pub enum PlayerToGameManager {
    /// Overwrite full game state. Only primary player can send SetFullGameState messages; if
    /// secondary player does this, GameManager will just ignore it.
    SetFullGameState(FullGameState),
    StateChanged(PlayerState),
    PutToken(game::CoordsXZ),
}

/// Message that a GameManager can send to UI.
#[derive(Debug)]
pub enum GameManagerToUI {
    SetToken(game::Side, game::CoordsFull),
    ResetBoard(game::BoardState),
    PlayerStateChanged(usize, PlayerState),
    PlayerSidesChanged(game::Side, game::Side),
}

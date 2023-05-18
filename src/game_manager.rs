pub mod player_local;
pub mod player_ws_client;

use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc;

use super::game;

/// Game manager which orchestrates the game between the UI and two players. It
/// communicates with the players and UI via the channels, see
/// GameManagerToPlayer, PlayerToGameManager, GameManagerToUI.
pub struct GameManager {
    /// Current game and its state.
    game: game::Game,
    game_state: Option<GameState>,

    /// Sender to the UI.
    to_ui: mpsc::Sender<GameManagerToUI>,
    /// Contexts of both players.
    players: [PlayerCtx; 2],
}

/// Context of a single player.
struct PlayerCtx {
    /// Player's current side.
    side: Option<game::Side>,
    /// Player's current state.
    state: PlayerState,

    /// Sender to and receiver from the player.
    to: mpsc::Sender<GameManagerToPlayer>,
    from: mpsc::Receiver<PlayerToGameManager>,
}

impl GameManager {
    /// Creates a new GameManager, which will communicate with the UI and
    /// players using the given channels.
    ///
    /// The first player (p0) is considered *primary*, and GameManager will
    /// listen to it when it says to reset the whole game. As such, in a network
    /// game, the network player has to be primary (p0), and local will be
    /// secondary (p1). See more details in PlayerToGameManager::SetFullGameState.
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

    /// Event loop, runs forever, should be swapned by the client code as a
    /// separate task.
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

    /// Propagate current game state to both players and the UI.
    async fn propagate_game_state_change(&mut self) -> Result<()> {
        let gs = self.game_state.unwrap();

        self.players[0]
            .to
            .send(GameManagerToPlayer::GameStateChanged(gs))
            .await
            .context("player 0")?;

        self.players[1]
            .to
            .send(GameManagerToPlayer::GameStateChanged(gs))
            .await
            .context("player 1")?;

        self.to_ui
            .send(GameManagerToUI::GameStateChanged(gs))
            .await
            .context("updating UI")?;

        Ok(())
    }

    async fn handle_player_state_change(&mut self, i: usize, state: PlayerState) -> Result<()> {
        // Remember state for the player which sent us the update.
        self.players[i].state = state.clone();

        // Update UI about the player state
        self.to_ui
            .send(GameManagerToUI::PlayerStateChanged(i, state))
            .await
            .context("updating UI")?;

        Ok(())
    }

    /// Handles full game reset; it happens when e.g. a network player connected
    /// to the server and the server has dumped the current game state to it.
    /// Here we should update internal state, the other player, and the UI.
    async fn handle_full_game_state(&mut self, i: usize, fgstate: FullGameState) -> Result<()> {
        if i != 0 {
            println!(
                "player {} is not primary, so ignoring its FullGameState update ({:?})",
                i, fgstate
            );
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
            .send(GameManagerToPlayer::Reset(
                fgstate.board.clone(),
                opposite_side,
            ))
            .await
            .context(format!(
                "resetting player {}, setting side to {:?}",
                opponent_idx, opposite_side
            ))?;

        // Update UI.
        self.to_ui
            .send(GameManagerToUI::ResetBoard(fgstate.board))
            .await
            .context("updating UI")?;

        // Update UI about the player sides.
        self.to_ui
            .send(GameManagerToUI::PlayerSidesChanged(
                fgstate.primary_player_side,
                opposite_side,
            ))
            .await
            .context("updating UI")?;

        // Update game state and propagate it to everyone.
        self.game_state = Some(fgstate.game_state);
        self.propagate_game_state_change()
            .await
            .context("initial update")?;

        Ok(())
    }

    fn both_players_mut(&mut self) -> (&mut PlayerCtx, &mut PlayerCtx) {
        let (v0, v1) = self.players.split_at_mut(1);
        (&mut v0[0], &mut v1[0])
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

                Ok(&self.players[1])
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
            PlayerToGameManager::PutToken(pcoords) => {
                self.handle_player_put_token(i, pcoords).await?;
                Ok(())
            }
        }
    }

    /// Called when a player puts a token.
    pub async fn handle_player_put_token(
        &mut self,
        i: usize,
        pcoords: game::PoleCoords,
    ) -> Result<()> {
        let maybe_side = self.players[i].side;
        println!("GM: player {:?} put token {:?}", maybe_side, pcoords);

        // Some sanity checks that the game state and the player side are all as
        // expected. If something is off, for now we'll just print to stdout,
        // update everyone about the current game state, and return Ok. We can't
        // return an error here because it'll be interpreted as communication
        // failure and the whole task will exit. It might be tempting to
        // actually do this, or even to panic, since it "shouldn't happen", but
        // since it can actually happen in a network game with a potentially
        // broken server or the remote player, we take it mildly.  TODO: perhaps
        // improve this error handling at some point.

        let expected_move_side = match self.game_state.unwrap() {
            GameState::WaitingFor(s) => s,
            GameState::WonBy(_) => {
                println!("game is won, but player put token");
                self.propagate_game_state_change().await?;
                return Ok(());
            }
        };

        let side = match maybe_side {
            None => {
                println!("no current player side, but player put token");
                self.propagate_game_state_change().await?;
                return Ok(());
            }
            Some(s) => s,
        };

        if side != expected_move_side {
            println!(
                "wrong side: {:?}, waiting for {:?}",
                side, expected_move_side
            );
            self.propagate_game_state_change().await?;
            return Ok(());
        }

        // The side matches, try to actually put the token. This can still fail
        // if the pole is full. Again, we don't give any actual feedback to the
        // player that the pole is full; we simply refuse to put the token and
        // gonna request a move from it again. That's a reasonable UX imo.
        let res = match self.game.put_token(side, pcoords) {
            Ok(res) => res,
            Err(err) => {
                println!("can't put: {}", err);
                self.propagate_game_state_change().await?;
                return Ok(());
            }
        };

        // All good, add new token to the UI.
        self.to_ui
            .send(GameManagerToUI::SetToken(
                side,
                game::TokenCoords {
                    x: pcoords.x,
                    y: res.y,
                    z: pcoords.z,
                },
            ))
            .await
            .context("updating UI")?;

        // Let the other player know.
        let opposite_side = side.opposite();
        self.player_by_side(opposite_side)
            .unwrap()
            .to
            .send(GameManagerToPlayer::OpponentPutToken(pcoords))
            .await?;

        // Update game state, depending on whether the new token won the game.
        if res.won {
            self.game_state = Some(GameState::WonBy(side));

            // Also let the UI know the full winning row.
            self.to_ui
                .send(GameManagerToUI::WinRow(
                    self.game.get_win_row().clone().unwrap(),
                ))
                .await
                .context("updating UI")?;
        } else {
            self.game_state = Some(GameState::WaitingFor(opposite_side));
        }

        // Let everyone know about the current game state.
        self.propagate_game_state_change().await?;

        Ok(())
    }
}

/// Simple state of the game: either waiting for someone's turn, or someone has
/// won already.
#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum GameState {
    WaitingFor(game::Side),
    WonBy(game::Side),
}

/// Full state of the game, containing the board state, and side of the players.
/// See PlayerToGameManager::SetFullGameState, where it is used.
#[derive(Debug)]
pub struct FullGameState {
    /// Either waiting for someone's turn, or someone has won already.
    pub game_state: GameState,

    /// Side of the primary player (the one who sends
    /// PlayerToGameManager::SetFullGameState with this full state). The other
    /// player obviously has the opposite side.
    pub primary_player_side: game::Side,

    /// Full board state.
    pub board: game::BoardState,
}

/// Player state from the point of view of the GameManager.
#[derive(Debug, Clone)]
pub enum PlayerState {
    /// Not-yet-ready, with a human-readable string message explaining the
    /// status. Only used by PlayerWSClient.
    NotReady(String),
    /// Ready to play. Local players are always ready.
    Ready,
}

/// Message that GameManager can send to a player.
#[derive(Debug)]
pub enum GameManagerToPlayer {
    /// Reset the game: the board state, and the side of the receiving player.
    Reset(game::BoardState, game::Side),
    /// Opponent has put token on the given pole.
    OpponentPutToken(game::PoleCoords),
    /// Game state has changed.
    GameStateChanged(GameState),
}

/// Message that a player can send to GameManager.
#[derive(Debug)]
pub enum PlayerToGameManager {
    /// Overwrite full game state. Only primary player can send SetFullGameState
    /// messages; if secondary player does this, GameManager will just ignore
    /// it.
    ///
    /// It might feel weird that a _player_ can call GameManager and reset the
    /// whole game, but with a network game, a "player" is just an interface to
    /// the server, and the server ultimately has to decide what the game status
    /// is. So when the server says that the game is reset, both players
    /// connected to it will communicate that to their GameManager-s, which in
    /// turn will update the other (local) player and the UI.
    SetFullGameState(FullGameState),
    /// Player state has changed.
    StateChanged(PlayerState),
    /// Player put a token on the given pole.
    PutToken(game::PoleCoords),
}

/// Message that a GameManager can send to UI.
#[derive(Debug)]
pub enum GameManagerToUI {
    /// Set token of the given size and coords.
    SetToken(game::Side, game::TokenCoords),
    /// The whole board is reset to this new state.
    ResetBoard(game::BoardState),
    /// Player with the given index has changed its status.  The index can only
    /// be 0 or 1. TODO: create an enum for those primary/secondary players.
    PlayerStateChanged(usize, PlayerState),
    /// Players have changed their sides. The given sides correspond to player 0
    /// and 1.
    PlayerSidesChanged(game::Side, game::Side),
    /// Game state has changed.
    GameStateChanged(GameState),
    /// There is a winner.
    WinRow(game::WinRow),
}

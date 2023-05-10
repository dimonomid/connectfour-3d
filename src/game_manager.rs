pub mod player_local;

use super::game;
use anyhow::{anyhow, Context, Result};

use tokio::sync::mpsc;

pub struct GameManager {
    game: game::Game,
    cur_side: game::Side,

    to_ui: mpsc::Sender<GameManagerToUI>,
    players: [PlayerCtx; 2],
}

struct PlayerCtx {
    side: Option<game::Side>,

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
        let p0 = PlayerCtx{
            side: None,
            to: to_p0,
            from: from_p0,
        };

        let p1 = PlayerCtx{
            side: None,
            to: to_p1,
            from: from_p1,
        };

        GameManager {
            game: game::Game::new(),
            cur_side: game::Side::White,

            to_ui,
            players: [p0, p1],
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // For now, setting player sides right here; but with network players, it'll have to
        // change, since player side info will come from the server, which means, from one of the
        // players.
        self.set_player_side(0, game::Side::White).await?;
        self.set_player_side(1, game::Side::Black).await?;

        self.upd_player_turns().await.context("initial update")?;

        loop {
            let (p0_mut, p1_mut) = self.both_players_mut();

            tokio::select! {
                Some(val) = p0_mut.from.recv() => {
                    self.handle_player_msg(self.players[0].side.unwrap(), val).await?;
                }

                Some(val) = p1_mut.from.recv() => {
                    self.handle_player_msg(self.players[1].side.unwrap(), val).await?;
                }
            }
        }
    }

    pub async fn upd_player_turns(&mut self) -> Result<()> {
        self.player_by_side(self.cur_side)?
            .to
            .send(GameManagerToPlayer::GameState(PlayerGameState::YourTurn))
            .await
            .context(format!("player {:?}", self.cur_side))?;

        let opposite_side = self.cur_side.opposite();
        self.player_by_side(opposite_side)?
            .to
            .send(GameManagerToPlayer::GameState(
                PlayerGameState::OpponentsTurn,
            ))
            .await
            .context(format!("player {:?}", opposite_side))?;

        Ok(())
    }

    async fn set_player_side(&mut self, i: usize, side: game::Side) -> Result<()> {
        self.players[i]
            .to
            .send(GameManagerToPlayer::SetSide(side))
            .await
            .context(format!("setting player {} side to {:?}", i, self.cur_side))?;

        self.players[i].side = Some(side);

        Ok(())
    }

    fn both_players_mut(&mut self) -> (&mut PlayerCtx, &mut PlayerCtx) {
        let (v0, v1) = self.players.split_at_mut(1);
        return (&mut v0[0], &mut v1[0]);
    }

    fn player_by_side(&self, side: game::Side) -> Result<&PlayerCtx> {
        match self.players[0].side {
            Some(v) => {
                if side == v {
                    return Ok(&self.players[0]);
                }

                return Ok(&self.players[1]);
            }
            None => {Err(anyhow!("player 0 doesn't have a side"))}
        }
    }

    pub async fn handle_player_msg(&mut self, side: game::Side, msg: PlayerToGameManager) -> Result<()> {
        match msg {
            PlayerToGameManager::PutToken(coords) => {
                self.handle_player_put_token(side, coords).await?;
                Ok(())
            },
        }
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
    NoGame,
    OpponentsTurn,
    YourTurn,
    OpponentWon,
    YouWon,
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
    PutToken(game::CoordsXZ),
}

/// Message that a GameManager can send to UI.
#[derive(Debug)]
pub enum GameManagerToUI {
    SetToken(game::Side, game::CoordsFull),
}

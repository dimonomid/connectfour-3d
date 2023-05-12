use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use connectfour::game;
use connectfour::game_manager::GameState;
use connectfour::WSFullGameState;

/// Game registry, to match players by game IDs.
///
/// Game context is kept as long as at least player is connected to it. So e.g.
/// when two players were playing, then one of them left (due to connection
/// issues or whatever else), the game state is kept for now, and when that
/// player reconnects later on, the game resumes from where it left off.
///
/// When the last player leaves the game, the game is destroyed as well.
pub struct Registry {
    game_by_name: Mutex<HashMap<String, Arc<GameCtx>>>,
}

pub struct GameCtx {
    /// The game ID, the same as the key in the game_by_name hashmap.
    pub id: String,

    pub data: Mutex<GameData>,
}

pub struct GameData {
    /// Both players, primary and secondary. The primary one is *always* set. If
    /// there were two players, but then primary left, then secondary becomes
    /// primary. Once both players are left, the game is destroyed.
    player_pri: Option<Player>,
    player_sec: Option<Player>,

    /// Game state, it will be dumped to both players whenever we have two of them.
    pub game_state: GameState,
    pub player_pri_side: game::Side,
    pub game: game::Game,
}

/// Player context; contains just the ID and a sender to send messages to this
/// player.
struct Player {
    id: String,

    /// Sender to send messages to this player.
    to: mpsc::Sender<PlayerToPlayer>,
}

/// Message that either player can send to the other one.
#[derive(Debug)]
pub enum PlayerToPlayer {
    /// Sent to both players whenever the second player arrives.
    OpponentIsHere(GameStartOrResume),
    /// Sent to a player whenever the opponent leaves.
    OpponentIsGone,

    /// Opponent has put token on the given pole.
    PutToken(game::PoleCoords),
}

/// Sent to both players whenever the second player arrives.
#[derive(Debug)]
pub struct GameStartOrResume {
    /// Channel to send messages to the opponent.
    pub to_opponent: mpsc::Sender<PlayerToPlayer>,
    /// Side of the receiving player.
    pub my_side: game::Side,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Registry {
        let m = HashMap::<String, Arc<GameCtx>>::new();

        Registry {
            game_by_name: Mutex::<HashMap<String, Arc<GameCtx>>>::new(m),
        }
    }

    /// Either join existing game by game_id, or if it doesn't exist, then
    /// create a new one, and in either case, return the game context. If the
    /// game already has both players, an error is returned.
    ///
    /// Player ID must only be unique for a particular game ID, but in practice,
    /// client code just passes remote addr as player ID, so they end up being
    /// globally unique.
    pub async fn join_or_create_game(
        &self,
        game_id: &str,
        player_id: &str,
        to_player: mpsc::Sender<PlayerToPlayer>,
        game_state: WSFullGameState,
    ) -> Result<Arc<GameCtx>> {
        let mut m = self.game_by_name.lock().await;

        // Try to join existin game, if any.
        match self
            .try_join_game(&mut m, game_id, player_id, &to_player)
            .await
        {
            Some(res) => {
                return res;
            }
            None => {}
        }

        // There's no existing game, so creating a new one.
        println!(
            "game {}: creating with the first player {}",
            game_id, player_id
        );

        let sname = game_id.to_string();

        let gc = GameCtx::new(sname.clone(), player_id.to_string(), to_player, game_state);
        let a = Arc::new(gc);

        m.insert(sname, a.clone());

        Ok(a)
    }

    /// Try joining existing game, if any. Returned None means there is no
    /// suitable existing game for us to join, so the caller should create a new
    /// one; Some(res) means we either found a game, or ran into an error;
    /// regardless, the caller should just return it further.
    async fn try_join_game(
        &self,
        m: &mut HashMap<String, Arc<GameCtx>>,
        game_id: &str,
        player_id: &str,
        to_player: &mpsc::Sender<PlayerToPlayer>,
    ) -> Option<Result<Arc<GameCtx>>> {
        match m.get(game_id) {
            Some(v) => {
                // The game already exists.
                let gc = v.clone();
                let mut gd = gc.data.lock().await;

                // If the game is over already (there's a winner), delete it and
                // pretend it didn't exist. This is needed to make it easier to
                // start a new game: if we kept returning the old one, then
                // after the game has ended, to to restart a game players would
                // have to first *both* disconnect (so the server forgets the
                // game), and then connect back. But it's inconvenient, since it
                // would require some external coordination between the players:
                // "did you disconnect? - no, not yet - now I did - ok then we
                // can connect again now". It's more convenient if each player
                // disconnects/reconnects independently, so we're accommodating
                // this case here.
                //
                // Obviously it would be even more convenient to just have a way
                // to restart the game without quitting the app, but it's a much
                // bigger change than this simple hack (We'd need to implement
                // some UI for collecting the user's willingness to start a new
                // game, and some coordination via the server, so that the game
                // only restarts when both players agreed to. All in all, it's a
                // TODO).
                if !gd.game.get_win_row().is_none() {
                    m.remove(game_id);
                    return None;
                }

                // The game already exists and has not ended yet, check how many
                // players are there.  If both are there, error out; otherwise,
                // add the new player and return the game.
                if let Some(_) = gd.player_sec {
                    println!("game {} already has both players", game_id);
                    return Some(Err(anyhow!("game {} already has both players", game_id)));
                }

                // The game only had a single player, so adding this one as the secondary.
                gd.player_sec = Some(Player {
                    id: player_id.to_string(),
                    to: to_player.clone(),
                });

                let to_pri = gd.player_pri.as_ref().unwrap().to.clone();
                let to_sec = to_player;
                let pri_side = gd.player_pri_side;
                drop(gd);

                // Introduce both players to each other, so they can take it
                // from there. If sending fails, which would mean that some
                // player is gone, we ignore it here, since it'll be handled in
                // the individual connection's loop.

                let _ = to_pri
                    .send(PlayerToPlayer::OpponentIsHere(GameStartOrResume {
                        to_opponent: to_sec.clone(),
                        my_side: pri_side,
                    }))
                    .await;

                let _ = to_sec
                    .send(PlayerToPlayer::OpponentIsHere(GameStartOrResume {
                        to_opponent: to_pri,
                        my_side: pri_side.opposite(),
                    }))
                    .await;

                println!("game {}: added new player {}", game_id, player_id);

                return Some(Ok(gc));
            }

            // If the game doesn't exist yet, let the caller create one.
            None => {
                return None;
            }
        }
    }

    /// Leave the game with the given ID. If it was the last player on this game, the game is
    /// destroyed.
    pub async fn leave_game(&self, game_id: &str, player_id: &str) {
        let mut m = self.game_by_name.lock().await;
        let gc = m[game_id].clone();

        let mut gd = gc.data.lock().await;
        match gd.num_players() {
            1 => {
                // With one player, we just destroy the game, since there are no more players.
                println!(
                    "game {}: removing the last player {}, destroying the game",
                    game_id, player_id
                );
                assert_eq!(gd.player_pri.as_ref().unwrap().id, player_id);

                m.remove(game_id);
            }

            2 => {
                // With two players, we need to make sure that whoever is left,
                // is left as the primary one.

                // If primary player left, move secondary to be the primary.
                let player_pri = gd.player_pri.as_ref().unwrap();
                let player_sec = gd.player_sec.as_ref().unwrap();
                if player_pri.id == player_id {
                    println!(
                        "game {}: primary player {} is left, setting secondary as the primary",
                        game_id, player_id
                    );
                    let _ = player_sec.to.send(PlayerToPlayer::OpponentIsGone).await;
                    gd.sec_to_pri();
                    return;
                }

                // Otherwise, forget the secondary player.
                println!("game {}: secondary player {} is left", game_id, player_id);
                assert_eq!(gd.player_sec.as_ref().unwrap().id, player_id);
                let _ = player_pri.to.send(PlayerToPlayer::OpponentIsGone).await;
                gd.player_sec = None;
            }

            _ => {
                panic!("wrong num players");
            }
        }
    }
}

impl GameCtx {
    fn new(
        game_id: String,
        player_id: String,
        to_player: mpsc::Sender<PlayerToPlayer>,
        game_state: WSFullGameState,
    ) -> GameCtx {
        let player_pri = Player {
            id: player_id,
            to: to_player,
        };

        let mut g = game::Game::new();
        g.reset_board(&game_state.board);

        GameCtx {
            id: game_id,
            data: Mutex::new(GameData {
                player_pri: Some(player_pri),
                player_sec: None,

                game_state: game_state.game_state,
                player_pri_side: game_state.ws_player_side,
                game: g,
            }),
        }
    }
}

impl GameData {
    /// Number of players currently joined the game. Can either be 1 or 2.
    fn num_players(&self) -> usize {
        let mut ret = 0;

        if let Some(_) = self.player_pri {
            ret += 1;
        }

        if let Some(_) = self.player_sec {
            ret += 1;
        }

        ret
    }

    /// Move secondary player to be primary, and forget the primary one.
    fn sec_to_pri(&mut self) {
        std::mem::swap(&mut self.player_pri, &mut self.player_sec);
        self.player_pri_side = self.player_pri_side.opposite();
        self.player_sec = None;
    }
}

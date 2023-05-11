use tokio::sync::{RwLock, Mutex};
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{anyhow, Result};

pub struct Registry {
    game_by_name: RwLock<HashMap<String, Arc<GameCtx>>>,
}

pub struct GameCtx {
    // TODO: make it non-public
    pub id: String,
    data: Mutex<GameData>,
}

struct GameData {
    player_pri: Option<Player>,
    player_sec: Option<Player>,
}

struct Player {
    id: String,
}

impl Registry {
    pub fn new() -> Registry {
        let m = HashMap::<String, Arc<GameCtx>>::new();

        Registry{
            game_by_name: RwLock::<HashMap<String, Arc<GameCtx>>>::new(m),
        }
    }

    pub async fn join_or_create_game(&self, game_id: &str, player_id: &str) -> Result<Arc<GameCtx>> {
        {
            let m = self.game_by_name.read().await;
            match m.get(game_id) {
                Some(v) => {
                    // The game already exists, check how many players are there. If both are
                    // there, error out; otherwise, add the new player and return the game.
                    let gc = v.clone();
                    let mut gd = gc.data.lock().await;

                    if let Some(_) = gd.player_sec {
                        println!("game {} already has both players", game_id);
                        return Err(anyhow!("game {} already has both players: {} and {}", game_id, gd.player_pri.as_ref().unwrap().id, gd.player_sec.as_ref().unwrap().id));
                    }

                    gd.player_sec = Some(Player{
                        id: player_id.to_string(),
                    });
                    drop(gd);

                    println!("game {}: added new player {}", game_id, player_id);

                    return Ok(gc);
                },

                // If the game doesn't exist yet, we'll create one below.
                None => {},
            }
        }

        println!("game {}: creating with the first player {}", game_id, player_id);

        let sname = game_id.to_string();
        let mut m = self.game_by_name.write().await;

        let gc = GameCtx::new(sname.clone(), player_id.to_string());
        let a = Arc::new(gc);

        // Recheck again that the value is still not there after we dropped the reader and
        // reacquired the writer.
        m.entry(sname).or_insert(a.clone());

        Ok(a)
    }

    pub async fn leave_game(&self, game_id: &str, player_id: &str) {
        let m = self.game_by_name.read().await;
        let gc = m[game_id].clone();
        drop(m);

        let mut gd = gc.data.lock().await;
        match gd.num_players() {
            1 => {
                // With one player, we just destroy the game, since there are no more players.
                println!("game {}: removing the last player {}, destroying the game", game_id, player_id);
                assert_eq!(gd.player_pri.as_ref().unwrap().id, player_id);

                let mut m = self.game_by_name.write().await;
                m.remove(game_id);
            }

            2 => {
                // With two players, we need to make sure that whoever is left, is left as the
                // primary one.

                // If primary player left, move secondary to be the primary.
                let player_pri = gd.player_pri.as_ref().unwrap();
                if player_pri.id == player_id {
                    println!("game {}: primary player {} is left, setting secondary as the primary", game_id, player_id);
                    gd.sec_to_pri();
                    return;
                }

                // Otherwise, forget the secondary player.
                println!("game {}: secondary player {} is left", game_id, player_id);
                assert_eq!(gd.player_sec.as_ref().unwrap().id, player_id);
                gd.player_sec = None;
            }

            _ => { panic!("wrong num players"); }
        }
    }
}

impl GameCtx {
    fn new(game_id: String, player_id: String) -> GameCtx {
        let player_pri = Player{
            id: player_id,
        };

        GameCtx{
            id: game_id,
            data: Mutex::new(GameData{
                player_pri: Some(player_pri),
                player_sec: None,
            }),
        }
    }
}

impl GameData {
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

    fn sec_to_pri(&mut self) {
        std::mem::swap(&mut self.player_pri, &mut self.player_sec);
        self.player_sec = None;
    }
}

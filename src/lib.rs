pub mod game;
pub mod game_manager;

use std::sync::Arc;
use crate::game_manager::GameState;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum WSClientToServer {
    Hello(WSClientInfo),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WSClientInfo {
    pub game_id: Arc<String>,
    pub player_name: Arc<String>,

    pub game_state: Arc<WSFullGameState>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WSFullGameState {
    pub game_state: GameState,

    /// Side of the websocket player (the one who sends or receives this update).
    pub ws_player_side: game::Side,

    /// Full board state.
    pub board: game::BoardState,
}

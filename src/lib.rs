pub mod game;
pub mod game_manager;

use crate::game_manager::GameState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WSClientToServer {
    Hello(WSClientInfo),
    PutToken(game::PoleCoords),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WSServerToClient {
    Ping,
    Msg(String),
    GameReset(GameReset),
    PutToken(game::PoleCoords),
    OpponentIsGone,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WSClientInfo {
    pub game_id: String,
    pub player_name: String,

    pub game_state: WSFullGameState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameReset {
    pub opponent_name: String,

    pub game_state: WSFullGameState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WSFullGameState {
    pub game_state: GameState,

    /// Side of the websocket player (the one who sends or receives this update).
    pub ws_player_side: game::Side,

    /// Full board state.
    pub board: game::BoardState,
}

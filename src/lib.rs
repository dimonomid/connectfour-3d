pub mod game;
pub mod game_manager;

use crate::game_manager::GameState;

/// Message that WS client (PlayerWSClient) can send to the server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WSClientToServer {
    /// Authentication message, must be the first one that the client sends.
    Hello(WSClientInfo),
    /// Put token at the given pole.
    PutToken(game::PoleCoords),
}

/// Message that server can send to WS clients (PlayerWSClient).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WSServerToClient {
    /// Ping is sent every few seconds.
    Ping,
    /// Msg is any human readable message that can be useful to show on the UI
    /// as part of the player state.
    Msg(String),
    /// Full game reset; it's sent to both players whenever two of them meet
    /// each other at a game.
    GameReset(WSGameReset),
    /// Opponent put token at the given pole.
    PutToken(game::PoleCoords),
    /// Opponent has disconnected from the server. It might still come back
    /// later though, and the game can continue then.
    OpponentIsGone,
}

/// Authentication message that the client sends right after connecting to the server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WSClientInfo {
    /// ID of the game to play. When two players connect with the same game ID,
    /// the players are introduced to each other, and the game starts. When more
    /// players try to connect with the same game ID, the server responds with
    /// an error message (WSServerToClient::Msg), and disconnects the client.
    /// TODO: would be cool to have a way for people to just watch the game.
    pub game_id: String,
    /// Player name to show to the opponent. As of now, not implemented yet (TODO).
    pub player_name: String,

    /// Full game state that the client currently has. Players send this state
    /// so that if the server restarts, while at least one of the players is
    /// still running and trying to connect, then on the server will pick up the
    /// game state from where it left off.
    pub game_state: WSFullGameState,
}

/// Full game reset, server sends it to both clients whenever two of them meet
/// each other to play a game.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WSGameReset {
    /// Opponent name, currently not implemented (TODO).
    pub opponent_name: String,

    /// Actual state of the game.
    pub game_state: WSFullGameState,
}

/// Full game state, server sends it to both clients whenever two of them meet
/// each other to play a game.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WSFullGameState {
    pub game_state: GameState,

    /// Side of the websocket player (the one who sends or receives this update).
    pub ws_player_side: game::Side,

    /// Full board state.
    pub board: game::BoardState,
}

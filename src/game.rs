use anyhow::{anyhow, Result};

// In "Connect Four", ROW_SIZE is the "Four".
pub const ROW_SIZE: usize = 4;

pub struct Game {
    board: BoardState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoardState {
    tokens: Vec<Option<Side>>,
}

pub struct PutResult {
    // The resulting y where the new token ended up.
    pub y: usize,

    // True if the new token won the game.
    pub won: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Black,
    White,
}

#[derive(Clone, Copy, Debug)]
pub struct CoordsFull {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct CoordsXZ {
    pub x: usize,
    pub z: usize,
}

impl CoordsXZ {
    pub fn new(x: usize, z: usize) -> CoordsXZ {
        CoordsXZ { x, z }
    }
}

impl Game {
    pub fn new() -> Game {
        Game {
            board: BoardState::new(),
        }
    }

    pub fn put_token(&mut self, side: Side, x: usize, z: usize) -> Result<PutResult> {
        panic_if_out_of_bounds(x, 0, z);

        for y in 0..ROW_SIZE {
            match self.board.get(x, y, z) {
                None => {
                    self.board.set(side, x, y, z);
                    return Ok(PutResult {
                        y: y,
                        won: false, // TODO
                    });
                }

                // Token already exists, gonna try next spot (if any).
                Some(_) => continue,
            }
        }

        // The pole is full.
        Err(anyhow!("pole {}, {} is full", x, z))
    }

    pub fn get_token(&self, x: usize, y: usize, z: usize) -> Option<Side> {
        self.board.get(x, y, z)
    }

    pub fn get_board(&self) -> &BoardState {
        &self.board
    }

    pub fn reset_board(&mut self, board: &BoardState) {
        self.board.copy_from(board);
    }
}

impl BoardState {
    pub fn new() -> BoardState {
        BoardState {
            tokens: vec![None; ROW_SIZE * ROW_SIZE * ROW_SIZE],
        }
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> Option<Side> {
        panic_if_out_of_bounds(x, y, z);

        *self.tokens.get(Self::coord_to_idx(x, y, z)).unwrap()
    }

    pub fn set(&mut self, side: Side, x: usize, y: usize, z: usize) {
        panic_if_out_of_bounds(x, y, z);

        self.tokens[Self::coord_to_idx(x, y, z)] = Some(side);
    }

    pub fn copy_from(&mut self, another: &BoardState) {
        self.tokens.copy_from_slice(&another.tokens);
    }

    fn coord_to_idx(x: usize, y: usize, z: usize) -> usize {
        x + y * ROW_SIZE + z * ROW_SIZE * ROW_SIZE
    }
}

impl Side {
    pub fn opposite(&self) -> Side {
        match *self {
            Side::Black => Side::White,
            Side::White => Side::Black,
        }
    }
}

fn panic_if_out_of_bounds(x: usize, y: usize, z: usize) {
    if x >= ROW_SIZE {
        panic!("x is out of bounds: {}", x);
    }

    if y >= ROW_SIZE {
        panic!("y is out of bounds: {}", y);
    }

    if z >= ROW_SIZE {
        panic!("z is out of bounds: {}", z);
    }
}

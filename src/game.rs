use anyhow::{anyhow, Result};

// In "Connect Four", ROW_SIZE is the "Four". It can be changed to any positive number, and the app
// will work. Might even make it a parameter, but I didn't bother.
pub const ROW_SIZE: usize = 4;

pub struct Game {
    board: BoardState,

    win_row: Option<WinRow>,
}

pub struct WinRow {
    pub side: Side,
    pub row: [CoordsFull; ROW_SIZE],
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

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
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
            win_row: None,
        }
    }

    pub fn put_token(&mut self, side: Side, x: usize, z: usize) -> Result<PutResult> {
        panic_if_out_of_bounds(x, 0, z);

        // Make sure there is no winner yet.
        if let Some(win_row) = &self.win_row {
            return Err(anyhow!("there is a winner already"))
        }

        for y in 0..ROW_SIZE {
            match self.board.get(x, y, z) {
                None => {
                    self.board.set(side, x, y, z);
                    self.win_row = self.check_win();

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
        self.win_row = self.check_win();
    }

    fn check_win(&self) -> Option<WinRow> {
        // Vertical rows (constant x, z).
        for x in 0..ROW_SIZE {
            for z in 0..ROW_SIZE {
                let row = self.check_win_row(|y| -> CoordsFull {
                    return CoordsFull{x, y, z};
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Horizontal rows with constant x, y.
        for x in 0..ROW_SIZE {
            for y in 0..ROW_SIZE {
                let row = self.check_win_row(|z| -> CoordsFull {
                    return CoordsFull{x, y, z};
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Horizontal rows with constant z, y.
        for z in 0..ROW_SIZE {
            for y in 0..ROW_SIZE {
                let row = self.check_win_row(|x| -> CoordsFull {
                    return CoordsFull{x, y, z};
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Diagonal rows with constant x.
        for x in 0..ROW_SIZE {
            // Ascending y
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x, y: n, z: n};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending y
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x, y: ROW_SIZE-1-n, z: n};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // Diagonal rows with constant z.
        for z in 0..ROW_SIZE {
            // Ascending y
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x: n, y: n, z};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending y
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x: n, y: ROW_SIZE-1-n, z};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // Diagonal rows with constant y.
        for y in 0..ROW_SIZE {
            // Ascending z
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x: n, y, z: n};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending z
            let row = self.check_win_row(|n| -> CoordsFull {
                return CoordsFull{x: n, y, z: ROW_SIZE-1-n};
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // 3D diagonal with ascending x, y, z
        let row = self.check_win_row(|n| -> CoordsFull {
            return CoordsFull{x: n, y: n, z: n};
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x, z; descending y
        let row = self.check_win_row(|n| -> CoordsFull {
            return CoordsFull{x: n, y: ROW_SIZE-1-n, z: n};
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x, y; descending z
        let row = self.check_win_row(|n| -> CoordsFull {
            return CoordsFull{x: n, y: n, z: ROW_SIZE-1-n};
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x; descending y, z
        let row = self.check_win_row(|n| -> CoordsFull {
            return CoordsFull{x: n, y: ROW_SIZE-1-n, z: ROW_SIZE-1-n};
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        None
    }

    fn check_win_row(&self, getter: impl Fn(usize) -> CoordsFull) -> Option<WinRow> {
        let mut row_side: Option<Side> = None;
        let mut row = [CoordsFull{x: 0, y: 0, z: 0}; ROW_SIZE];

        for i in 0..ROW_SIZE {
            let coords = getter(i);
            match self.get_token(coords.x, coords.y, coords.z) {
                Some(side) => {
                    if i == 0 {
                        row_side = Some(side);
                        continue;
                    }

                    if row_side != Some(side) {
                        return None;
                    }
                },

                None => { return None; }
            }
        }

        // Winning row!
        Some(WinRow{
            side: row_side.unwrap(),
            row: row,
        })
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

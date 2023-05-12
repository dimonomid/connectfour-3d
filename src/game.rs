use anyhow::{anyhow, Result};

/// In "Connect Four", ROW_SIZE is the "Four". It can be changed to any positive number, and the
/// app will work. Might even make it as a parameter, but I didn't bother.
pub const ROW_SIZE: usize = 4;

/// Describes state of the board, a winner (if any), and has useful methods for putting tokens and
/// checking for the winner.
pub struct Game {
    board: BoardState,

    win_row: Option<WinRow>,
}

/// Winning row.
#[derive(Debug, Clone)]
pub struct WinRow {
    /// Side of the winner.
    pub side: Side,
    /// Coords of all winning tokens.
    pub row: [TokenCoords; ROW_SIZE],
}

/// State of the connect-four board, i.e. placement and sides of all tokens. There is no validation
/// done, so technically one can construct an impossible state like hanging tokens.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoardState {
    tokens: Vec<Option<Side>>,
}

/// Successful result of putting a token on a pole.
pub struct PutResult {
    /// The resulting y where the new token ended up.
    pub y: usize,

    /// True if the new token won the game; use get_win_row to get the details.
    pub won: bool,
}

/// Side of the player: either Black or White.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Black,
    White,
}

/// Contains coords of a token: X, Y, Z. All of those must be >= 0 and < ROW_SIZE.
#[derive(Clone, Copy, Debug)]
pub struct TokenCoords {
    pub x: usize,
    pub y: usize,
    pub z: usize,
}

/// Contains coords of a pole: X, Z. Each of those must be >= 0 and < ROW_SIZE.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct PoleCoords {
    pub x: usize,
    pub z: usize,
}

impl PoleCoords {
    pub fn new(x: usize, z: usize) -> PoleCoords {
        PoleCoords { x, z }
    }

    pub fn token_coords(&self, y: usize) -> TokenCoords {
        TokenCoords { x: self.x, y, z: self.z }
    }
}

impl TokenCoords {
    pub fn new(x: usize, y: usize, z: usize) -> TokenCoords {
        TokenCoords { x, y, z }
    }

    pub fn pole_coords(&self) -> PoleCoords {
        PoleCoords { x: self.x, z: self.z }
    }
}

impl Game {
    /// Create a new game with an empty board.
    pub fn new() -> Game {
        Game {
            board: BoardState::new(),
            win_row: None,
        }
    }

    /// Put a new token on the pole with the given coords X, Z. Note that Y is not passed here: it
    /// will be returned in the result, if successful.
    ///
    /// An error is returned if the given pole is full, or if someone won the game already.
    pub fn put_token(&mut self, side: Side, pcoords: PoleCoords) -> Result<PutResult> {
        panic_if_out_of_bounds(pcoords.x, 0, pcoords.z);

        // Make sure there is no winner yet.
        if let Some(win_row) = &self.win_row {
            return Err(anyhow!("there is a winner already: {:?}", win_row.side));
        }

        for y in 0..ROW_SIZE {
            let tcoords = pcoords.token_coords(y);
            match self.board.get(tcoords) {
                None => {
                    self.board.set(side, tcoords);
                    self.win_row = self.check_win();

                    return Ok(PutResult {
                        y: y,
                        won: !self.win_row.is_none(),
                    });
                }

                // Token already exists, gonna try next spot (if any).
                Some(_) => continue,
            }
        }

        // The pole is full.
        Err(anyhow!("pole {}, {} is full", pcoords.x, pcoords.z))
    }

    /// Get the token (if any) with the given coords X, Y, Z.
    pub fn get_token(&self, tcoords: TokenCoords) -> Option<Side> {
        self.board.get(tcoords)
    }

    /// Return current board state.
    pub fn get_board(&self) -> &BoardState {
        &self.board
    }

    /// Reset the board to the data of the provided one.
    pub fn reset_board(&mut self, board: &BoardState) {
        // TODO: sanitize the board: don't allow hanging tokens, multiple wins, and unbalanced
        // sides.

        self.board.copy_from(board);
        self.win_row = self.check_win();
    }

    /// Returns current winning row, if any. Once that function returns Some row, no more tokens
    /// can be put, until reset_board is called.
    pub fn get_win_row(&self) -> &Option<WinRow> {
        &self.win_row
    }

    /// Checks all possible rows and returns the first one that is full of tokens of the same side.
    /// It's called every time a new token is put, or a whole board is imported.
    fn check_win(&self) -> Option<WinRow> {
        // Vertical rows (constant x, z).
        for x in 0..ROW_SIZE {
            for z in 0..ROW_SIZE {
                let row = self.check_win_row(|y| -> TokenCoords {
                    return TokenCoords { x, y, z };
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Horizontal rows with constant x, y.
        for x in 0..ROW_SIZE {
            for y in 0..ROW_SIZE {
                let row = self.check_win_row(|z| -> TokenCoords {
                    return TokenCoords { x, y, z };
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Horizontal rows with constant z, y.
        for z in 0..ROW_SIZE {
            for y in 0..ROW_SIZE {
                let row = self.check_win_row(|x| -> TokenCoords {
                    return TokenCoords { x, y, z };
                });
                if let Some(win_row) = row {
                    return Some(win_row);
                }
            }
        }

        // Diagonal rows with constant x.
        for x in 0..ROW_SIZE {
            // Ascending y
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords { x, y: n, z: n };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending y
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords {
                    x,
                    y: ROW_SIZE - 1 - n,
                    z: n,
                };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // Diagonal rows with constant z.
        for z in 0..ROW_SIZE {
            // Ascending y
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords { x: n, y: n, z };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending y
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords {
                    x: n,
                    y: ROW_SIZE - 1 - n,
                    z,
                };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // Diagonal rows with constant y.
        for y in 0..ROW_SIZE {
            // Ascending z
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords { x: n, y, z: n };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }

            // Descending z
            let row = self.check_win_row(|n| -> TokenCoords {
                return TokenCoords {
                    x: n,
                    y,
                    z: ROW_SIZE - 1 - n,
                };
            });
            if let Some(win_row) = row {
                return Some(win_row);
            }
        }

        // 3D diagonal with ascending x, y, z
        let row = self.check_win_row(|n| -> TokenCoords {
            return TokenCoords { x: n, y: n, z: n };
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x, z; descending y
        let row = self.check_win_row(|n| -> TokenCoords {
            return TokenCoords {
                x: n,
                y: ROW_SIZE - 1 - n,
                z: n,
            };
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x, y; descending z
        let row = self.check_win_row(|n| -> TokenCoords {
            return TokenCoords {
                x: n,
                y: n,
                z: ROW_SIZE - 1 - n,
            };
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        // 3D diagonal with ascending x; descending y, z
        let row = self.check_win_row(|n| -> TokenCoords {
            return TokenCoords {
                x: n,
                y: ROW_SIZE - 1 - n,
                z: ROW_SIZE - 1 - n,
            };
        });
        if let Some(win_row) = row {
            return Some(win_row);
        }

        None
    }

    /// A helper to check if a single row is full of tokens of the same size. The tcoord_getter
    /// callback takes an index from 0 to ROW_SIZE-1, and returns full coords for that token.
    fn check_win_row(&self, tcoord_getter: impl Fn(usize) -> TokenCoords) -> Option<WinRow> {
        let mut row_side: Option<Side> = None;
        let mut row = [TokenCoords { x: 0, y: 0, z: 0 }; ROW_SIZE];

        for i in 0..ROW_SIZE {
            let tcoords = tcoord_getter(i);
            match self.get_token(tcoords) {
                Some(side) => {
                    row[i] = tcoords;

                    // On the first token, just remember the row size.
                    if i == 0 {
                        row_side = Some(side);
                        continue;
                    }

                    // On all other tokens, check that the side remains the same.
                    if row_side.unwrap() != side {
                        return None;
                    }
                }

                None => {
                    // There's no token here, therefore no row.
                    return None;
                }
            }
        }

        // This was a winning row!
        Some(WinRow {
            side: row_side.unwrap(),
            row: row,
        })
    }
}

impl BoardState {
    /// Create a new blank board.
    pub fn new() -> BoardState {
        BoardState {
            tokens: vec![None; ROW_SIZE * ROW_SIZE * ROW_SIZE],
        }
    }

    /// Get a token with the given coords. If coords are outside of the board size, it panics.
    pub fn get(&self, tcoords: TokenCoords) -> Option<Side> {
        panic_if_out_of_bounds(tcoords.x, tcoords.y, tcoords.z);

        *self.tokens.get(Self::coord_to_idx(tcoords)).unwrap()
    }

    /// Set a token of the given side on the given coords. If coords are outside of the board size,
    /// it panics. Other than that, no validation is done, so technically one can set e.g. a
    /// hanging token.
    pub fn set(&mut self, side: Side, tcoords: TokenCoords) {
        panic_if_out_of_bounds(tcoords.x, tcoords.y, tcoords.z);

        self.tokens[Self::coord_to_idx(tcoords)] = Some(side);
    }

    /// Copy data from another board. Existing data is discarded.
    pub fn copy_from(&mut self, another: &BoardState) {
        self.tokens.copy_from_slice(&another.tokens);
    }

    /// A helper to convert token coords X, Y, Z into an index in the slice.
    fn coord_to_idx(tcoords: TokenCoords) -> usize {
        tcoords.x + tcoords.y * ROW_SIZE + tcoords.z * ROW_SIZE * ROW_SIZE
    }
}

impl Side {
    /// Opposite side.
    pub fn opposite(&self) -> Side {
        match *self {
            Side::Black => Side::White,
            Side::White => Side::Black,
        }
    }
}

/// A helper which panics if given coords are outside of the board.
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

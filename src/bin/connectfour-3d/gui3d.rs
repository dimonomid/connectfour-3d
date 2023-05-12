use std::cmp::Ordering;
use std::rc::Rc;
use std::time::{Duration, Instant};
use std::vec::Vec;

use kiss3d::camera::{ArcBall, Camera};
use kiss3d::event::{Action, Event, Key, MouseButton, WindowEvent};
use kiss3d::light::Light;
use kiss3d::nalgebra::{Point2, Point3, Translation3, Vector2, Vector3};
use kiss3d::scene::SceneNode;
use kiss3d::text::Font;
use kiss3d::window::Window;
use ordered_float::OrderedFloat;
use tokio::sync::mpsc;

use super::OpponentKind;
use connectfour::game::{PoleCoords, Side, TokenCoords, WinRow, ROW_SIZE};
use connectfour::game_manager::player_local::PlayerLocalToUI;
use connectfour::game_manager::{GameManagerToUI, GameState, PlayerState};

/// Constants which configure the 3D model.

const POLE_WIDTH: f32 = 1.0;
const TOKEN_D_TO_HEIGHT: f32 = 0.8;
const POLE_RADIUS: f32 = POLE_WIDTH / 2.0;
const TOKEN_RADIUS: f32 = POLE_RADIUS * 2.0;
const TOKEN_HEIGHT: f32 = TOKEN_RADIUS * 2.0 * TOKEN_D_TO_HEIGHT;
const POLE_HEIGHT: f32 = TOKEN_HEIGHT * (ROW_SIZE as f32 + (1.0 - TOKEN_D_TO_HEIGHT));
const POLE_SPACING: f32 = POLE_WIDTH * 3.0; // From center to center, not from edge to edge
const MARGIN: f32 = POLE_WIDTH * 2.0;
const FOUNDATION_WIDTH: f32 = POLE_SPACING * (ROW_SIZE as f32 - 1.0) + MARGIN * 2.0;
const FOUNDATION_HEIGHT: f32 = POLE_WIDTH;
const POINTER_RADIUS: f32 = POLE_RADIUS * 0.7;

/// Y coord for a plane which matches tops of all poles.
const POLES_TOP_Y: f32 = POLE_HEIGHT / 2.0;

/// How often to flash tokens, whenever we need to flash some (we do for the
/// winning row).
const FLASH_DUR_MS: u128 = 200;
const FLASH_DUR: Duration = Duration::from_millis(FLASH_DUR_MS as u64);

/// How many times to flash the last token.
const LAST_TOKEN_NUM_FLASHES: usize = 2;

pub struct Window3D {
    w: Window,
    font: Rc<Font>,
    camera: ArcBall,

    /// A vector of currently added tokens as spheres.
    tokens: Vec<Option<SceneNode>>,
    /// A tiny sphere which shows up on top of poles when mouse hovers them (only
    /// whenever a local player has requested an input from UI, i.e. when
    /// pending_input is not None).
    pole_pointer: SceneNode,

    /// Whenever a PlayerLocal requests an input from UI (where to put a token),
    /// pending_input becomes Some(v). When the user picks a pole, the PoleCoords
    /// are sent via pending_input.coord_sender, and it becomes None again.
    pending_input: Option<PendingInput>,

    /// Last mouse coords are updated whenever the user moves the mouse cursor.
    last_mouse_coords: Point2<f32>,

    /// Whether mouse button (any of them) is down atm.
    mouse_down: bool,
    /// Set to true if the scene is being rotated or moved with the mouse. If
    /// it's true, on the mouse release we will not interpret it as "put token
    /// here".
    rotating: bool,

    /// Last token that was added, if any. Needed because we need to flash it a
    /// little bit.
    last_token: Option<TokenCoords>,
    /// How many flashes are remaining for the last token.
    last_token_num_flash: usize,

    /// Last time when we changed the visibility of flashing tokens.
    last_flash_time: Instant,
    /// Whether flashing tokens are currently visible.
    flash_show: bool,

    from_gm: mpsc::Receiver<GameManagerToUI>,
    from_players: mpsc::Receiver<PlayerLocalToUI>,

    players: [PlayerInfo; 2],
    opponent_kind: OpponentKind,

    game_state: Option<GameState>,

    /// If not None, it means there is a winner, and it's the winning row. We'll
    /// flash the tokens there.
    win_row: Option<WinRow>,
}

impl Window3D {
    pub fn new(
        from_gm: mpsc::Receiver<GameManagerToUI>,
        from_players: mpsc::Receiver<PlayerLocalToUI>,
        opponent_kind: OpponentKind,
    ) -> Window3D {
        let mut w = Window::new("ConnectFour 3D");
        w.set_light(Light::StickToCamera);

        // Set up camera in a meaningful position.
        let eye = Point3::new(18.0, 18.0, 18.0);
        let at = Point3::origin();
        let camera = ArcBall::new(eye, at);

        // Create pole pointer, initially invisible. It'll be visible only when
        // the mouse cursor hovers a pole.
        let mut pole_pointer = w.add_sphere(POINTER_RADIUS);
        pole_pointer.set_visible(false);

        let p0_name;
        let p1_name;

        match opponent_kind {
            OpponentKind::Local => {
                p0_name = "local";
                p1_name = "local";
            }
            OpponentKind::Network => {
                p0_name = "network";
                p1_name = "local (you)";
            }
        }

        let mut window = Window3D {
            w,
            font: Font::default(),
            camera,
            tokens: vec![None; ROW_SIZE * ROW_SIZE * ROW_SIZE],
            pole_pointer,
            pending_input: None,
            mouse_down: false,
            rotating: false,
            last_token: None,
            last_token_num_flash: 0,
            last_flash_time: Instant::now(),
            flash_show: true,
            from_gm,
            from_players,
            last_mouse_coords: Point2::new(0.0f32, 0.0f32),
            players: [
                PlayerInfo {
                    name: p0_name.to_string(),
                    state: PlayerState::NotReady("-".to_string()),
                    side: None,
                },
                PlayerInfo {
                    name: p1_name.to_string(),
                    state: PlayerState::NotReady("-".to_string()),
                    side: None,
                },
            ],
            opponent_kind,
            game_state: None,
            win_row: None,
        };

        window.create_3d_board();

        window
    }

    /// Event loop, runs until the user closes the GUI window. Client code
    /// should run it in a separate OS thread. It might be possible to stick it
    /// to be an async task, but I didn't find a way to figure when it is worth
    /// to re-render. So for now, we do what all examples in kiss3d are doing:
    /// just keeps rendering all the time.
    pub fn run(&mut self) {
        while self.render() {
            // Handle keyboard and mouse events (apart from rotating the model,
            // zooming etc - this one is taken care of automatically).
            for event in self.w.events().iter() {
                self.handle_user_input(&event)
            }

            self.handle_gm_messages();
            self.handle_player_messages();

            // If some tokens need to be flashed, flash them every FLASH_DUR_MS ms.
            let now = Instant::now();
            let dur = now
                .saturating_duration_since(self.last_flash_time)
                .as_millis();
            if dur >= FLASH_DUR_MS {
                self.last_flash_time = self.last_flash_time.checked_add(FLASH_DUR).unwrap();
                self.flash_show = !self.flash_show;

                // Flash last token, if needed
                if self.last_token_num_flash > 0 {
                    if let Some(last_token) = self.last_token {
                        self.tokens[Self::token_coords_to_idx(last_token)]
                            .as_mut()
                            .unwrap()
                            .set_visible(self.flash_show);

                        if self.flash_show {
                            self.last_token_num_flash -= 1;
                        }
                    }
                }

                // Flash win row, if any
                if let Some(win_row) = &self.win_row {
                    for tcoords in win_row.row {
                        self.tokens[Self::token_coords_to_idx(tcoords)]
                            .as_mut()
                            .unwrap()
                            .set_visible(self.flash_show);
                    }
                }
            }
        }
    }

    /// Create a 3D model of an empty game board.
    fn create_3d_board(&mut self) {
        let mut foundation = self
            .w
            .add_cube(FOUNDATION_WIDTH, FOUNDATION_HEIGHT, FOUNDATION_WIDTH);
        foundation.set_color(1.0, 0.8, 0.0);
        foundation.set_local_translation(Translation3::new(
            0.0,
            -(POLE_HEIGHT + FOUNDATION_HEIGHT) / 2.0,
            0.0,
        ));

        for x in 0..ROW_SIZE {
            for z in 0..ROW_SIZE {
                let mut pole = self.w.add_cylinder(POLE_RADIUS, POLE_HEIGHT);

                pole.set_local_translation(Self::pole_translation(PoleCoords::new(x, z)));
                pole.set_color(1.0, 1.0, 0.0);
            }
        }
    }

    fn handle_user_input(&mut self, event: &Event) {
        match event.value {
            WindowEvent::MouseButton(_btn, Action::Press, _modif) => {
                self.mouse_down = true;
            }

            WindowEvent::MouseButton(btn, Action::Release, _modif) => {
                let was_rotating = self.rotating;

                self.mouse_down = false;
                self.rotating = false;

                // If it wasn't the left button, or if were rotating scene, then
                // don't add a token on release.
                if btn != MouseButton::Button1 || was_rotating || !self.waiting_for_input() {
                    // When we release after the rotation, the mouse might again
                    // be pointing at a pole, so update the pole pointer if
                    // that's the case.
                    self.update_pole_pointer();
                    return;
                }

                // Going to try to add a token.

                let pcoords = match self.mouse_coords_to_pole_coords(self.last_mouse_coords) {
                    Some(pcoords) => pcoords,
                    None => return,
                };

                match self
                    .pending_input
                    .as_ref()
                    .expect("no pending_input")
                    .coord_sender
                    .try_send(pcoords)
                {
                    Ok(_) => {
                        self.pending_input = None;
                    }
                    Err(err) => {
                        println!("failed sending coords to the player: {}", err);
                    }
                }

                self.update_pole_pointer();
            }
            WindowEvent::CursorPos(x, y, _modif) => {
                self.last_mouse_coords = Point2::new(x as f32, y as f32);
                if self.mouse_down {
                    self.rotating = true;
                }

                self.update_pole_pointer();
            }

            WindowEvent::Key(Key::L, _action, _modif) => {
                if let Some(last_token) = self.last_token {
                    // Call set_last_token with an already existing token, just to
                    // cause it to flash,
                    self.set_last_token(last_token);

                    // but adjust the last_flash_time to be one flashing period
                    // ago, so that we'll make it invisible right away.
                    self.last_flash_time = self.last_flash_time.checked_sub(FLASH_DUR).unwrap();
                }
            }

            _ => {}
        }
    }

    /// Depending on the current mouse coords and internal state, either hide
    /// the pole pointer, or hide it. We show it when all of those are true:
    ///
    /// - PlayerLocal requested an input from the UI
    /// - The mouse hovers some pole top
    /// - We aren't in the process of rotating or moving 3D view
    fn update_pole_pointer(&mut self) {
        if self.rotating || !self.waiting_for_input() {
            self.pole_pointer.set_visible(false);
            return;
        }

        let pcoords = match self.mouse_coords_to_pole_coords(self.last_mouse_coords) {
            Some(pcoords) => pcoords,
            None => {
                self.pole_pointer.set_visible(false);
                return;
            }
        };

        // We need to show the pointer, so figure the exact coords, and show it.

        let mut pole_top_t = Self::pole_translation(pcoords);
        pole_top_t.y += POLE_HEIGHT / 2.0;

        self.pole_pointer.set_local_translation(pole_top_t);
        self.pole_pointer.set_visible(true);
    }

    /// Handle all pending messages from GameManager.
    fn handle_gm_messages(&mut self) {
        loop {
            let msg = match self.from_gm.try_recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };

            //println!("hey received from GM: {:?}", &msg);

            match msg {
                GameManagerToUI::SetToken(side, tcoords) => {
                    self.add_token(side, tcoords);
                    self.set_last_token(tcoords);
                }
                GameManagerToUI::ResetBoard(board) => {
                    for maybe_token in &mut self.tokens {
                        if let Some(token) = maybe_token {
                            token.unlink();
                            *maybe_token = None;
                        }
                    }

                    self.win_row = None;
                    self.last_token = None;

                    // TODO: reimplement as an iterator exposed by the board.
                    for x in 0..ROW_SIZE {
                        for y in 0..ROW_SIZE {
                            for z in 0..ROW_SIZE {
                                let tcoords = TokenCoords::new(x, y, z);
                                if let Some(side) = board.get(tcoords) {
                                    self.add_token(side, tcoords);
                                }
                            }
                        }
                    }
                }

                GameManagerToUI::PlayerStateChanged(i, state) => {
                    if i >= 2 {
                        // TODO: create a separate enum for player indices
                        panic!("invalid player idx: {}", i)
                    }

                    self.players[i].state = state;
                }

                GameManagerToUI::PlayerSidesChanged(pri_side, sec_side) => {
                    self.players[0].side = Some(pri_side);
                    self.players[1].side = Some(sec_side);
                }

                GameManagerToUI::GameStateChanged(game_state) => {
                    self.game_state = Some(game_state);
                }

                GameManagerToUI::WinRow(win_row) => {
                    self.win_row = Some(win_row);
                }
            }
        }
    }

    /// Handle all pending messages from both players.
    fn handle_player_messages(&mut self) {
        loop {
            let msg = match self.from_players.try_recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };

            //println!("hey received from a player {:?}", &msg);

            match msg {
                PlayerLocalToUI::RequestInput(side, coord_sender) => {
                    // Sanity check that the UI is not busy serving another player.
                    if let Some(v) = &self.pending_input {
                        if v.side != side {
                            panic!("coord_sender already existed for {:?} when a new one for {:?} came in", v.side, side);
                        }
                    }

                    // Remember the channel to send the resulting coords to.
                    self.pending_input = Some(PendingInput { coord_sender, side });

                    // Update the color of the pole pointer to reflect the side.
                    let c = Self::color_by_side(side);
                    self.pole_pointer.set_color(c.0, c.1, c.2);
                }
            }
        }
    }

    /// Call the 3D rendering routine, and put all the overlay texts.
    fn render(&mut self) -> bool {
        if !self.w.render_with_camera(&mut self.camera) {
            return false;
        }

        // Write details about both players.

        self.w.draw_text(
            &self.player_str(0),
            &Point2::new(10.0, 0.0),
            40.0,
            &self.font,
            &Point3::new(0.0, 1.0, 0.0),
        );

        self.w.draw_text(
            &self.player_str(1),
            &Point2::new(10.0, 50.0),
            40.0,
            &self.font,
            &Point3::new(0.0, 1.0, 0.0),
        );

        // If needed, write details about the game status.
        match self.game_state {
            None => {
                self.w.draw_text(
                    "The game did not start yet",
                    &Point2::new(10.0, 100.0),
                    40.0,
                    &self.font,
                    &Point3::new(1.0, 1.0, 1.0),
                );
            }

            Some(GameState::WaitingFor(waiting_for_side)) => {
                match self.opponent_kind {
                    OpponentKind::Local => {
                        // Nothing special to write here in local mode.
                    }
                    OpponentKind::Network => {
                        let player_local = &self.players[1];
                        let text;
                        let color;

                        if player_local.side == Some(waiting_for_side) {
                            text = "Your turn";
                            color = Point3::new(1.0, 1.0, 1.0);
                        } else {
                            text = "Opponent's turn";
                            color = Point3::new(0.5, 0.5, 0.5);
                        }

                        self.w
                            .draw_text(text, &Point2::new(10.0, 100.0), 60.0, &self.font, &color);
                    }
                }
            }

            Some(GameState::WonBy(winning_side)) => {
                let text;

                // Depending on the opponent kind (local or network), we construct the text
                // differently.
                match self.opponent_kind {
                    OpponentKind::Local => {
                        if self.players[0].side == Some(winning_side) {
                            text = "player #1 won";
                        } else {
                            text = "player #2 won";
                        }
                    }
                    OpponentKind::Network => {
                        let player_local = &self.players[1];
                        if player_local.side == Some(winning_side) {
                            text = "you won!";
                        } else {
                            text = "you lost!";
                        }
                    }
                }

                self.w.draw_text(
                    text,
                    &Point2::new(10.0, 100.0),
                    100.0,
                    &self.font,
                    &Point3::new(1.0, 1.0, 1.0),
                );
            }
        }

        // Write some hint about the controls, at the bottom.
        self.w.draw_text(
            "Left mouse btn: rotate, Right mouse btn: move, Enter: center, L: flash last token",
            &Point2::new(10.0, self.w.size()[1] as f32 * 2.0 - 50.0),
            35.0,
            &self.font,
            &Point3::new(0.0, 1.0, 0.0),
        );

        true
    }

    /// Return whether we are currently waiting for the user's input where to
    /// put the token.
    fn waiting_for_input(&self) -> bool {
        if let Some(_) = self.pending_input {
            return true;
        }

        false
    }

    /// Return 3D coords (translation) of the given pole.
    fn pole_translation(pcoords: PoleCoords) -> Translation3<f32> {
        let xcoord = MARGIN + pcoords.x as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;
        let zcoord = MARGIN + pcoords.z as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;

        Translation3::new(xcoord, 0.0, zcoord)
    }

    /// Return 3D coords (translation) of the given token.
    fn token_translation(tcoords: TokenCoords) -> Translation3<f32> {
        let mut t = Self::pole_translation(tcoords.pole_coords());
        t.y = -POLE_HEIGHT / 2.0 + TOKEN_HEIGHT / 2.0 + TOKEN_HEIGHT * (tcoords.y as f32);

        t
    }

    /// returns approximate point where the given ray intersects with the plane
    /// which matches the top of the poles.
    ///
    /// TODO: it's written in extremely dumb way, figure the proper way to do it.
    fn top_plane_intersect(p: &Point3<f32>, v: &Vector3<f32>) -> Option<Point3<f32>> {
        // If the ray doesn't intersect with the plane, cut it short.
        let ord_p = Self::cmpf32(POLES_TOP_Y, p.y);
        let ord_v = Self::cmpf32(0.0, v.y);
        if ord_p == ord_v {
            return None;
        }

        let mut pcur = *p;
        let vcur = Vector3::new(v.x / 10.0, v.y / 10.0, v.z / 10.0);

        let ord_orig = Self::cmpf32(POLES_TOP_Y, pcur.y);

        loop {
            pcur.x += vcur.x;
            pcur.y += vcur.y;
            pcur.z += vcur.z;

            if Self::cmpf32(POLES_TOP_Y, pcur.y) != ord_orig {
                break;
            }
        }

        Some(pcur)
    }

    /// Just a shortcut to compare two f32-s.
    fn cmpf32(n: f32, m: f32) -> Ordering {
        return OrderedFloat(n).cmp(&OrderedFloat(m));
    }

    /// Try to convert mouse pointer coords into approximate 3D coords of a
    /// plane on which all pole top are. If mouse doesn't seem to be pointing to
    /// this plane, returns None.
    fn mouse_coords_to_pole_translation(&self, mouse_pt: Point2<f32>) -> Option<Point3<f32>> {
        let window_size = Vector2::new(self.w.size()[0] as f32, self.w.size()[1] as f32);
        let ray = self.camera.unproject(&mouse_pt, &window_size);

        Self::top_plane_intersect(&ray.0, &ray.1)
    }

    /// Try to convert pole top 3D coords (translation) to the game PoleCoords.
    /// If the given coords don't seem to be pointing to a particular plane,
    /// returns None.
    fn pole_translation_to_pole_coords(t: Point3<f32>) -> Option<PoleCoords> {
        const TOLERANCE: f32 = POLE_RADIUS * 1.5;

        for x in 0..ROW_SIZE {
            for z in 0..ROW_SIZE {
                let cur_t = Self::pole_translation(PoleCoords::new(x, z));
                if t.x >= cur_t.x - TOLERANCE
                    && t.x <= cur_t.x + TOLERANCE
                    && t.z >= cur_t.z - TOLERANCE
                    && t.z <= cur_t.z + TOLERANCE
                {
                    return Some(PoleCoords::new(x, z));
                }
            }
        }

        None
    }

    /// Try to convert mouse pointer coords into game coords of a pole
    /// (PoleCoords). If mouse doesn't seem to be pointing to a particular pole
    /// top, returns None.
    fn mouse_coords_to_pole_coords(&self, mouse_pt: Point2<f32>) -> Option<PoleCoords> {
        let v = self.mouse_coords_to_pole_translation(mouse_pt)?;
        Self::pole_translation_to_pole_coords(v)
    }

    /// Add a new token with the given side and coords.
    fn add_token(&mut self, side: Side, tcoords: TokenCoords) {
        let mut s = self.w.add_sphere(TOKEN_RADIUS);
        let c = Self::color_by_side(side);
        s.set_color(c.0, c.1, c.2);
        s.set_local_translation(Self::token_translation(tcoords));

        self.tokens[Self::token_coords_to_idx(tcoords)] = Some(s);
    }

    /// Remember which token was set last. Needed because we need to flash it a
    /// little bit.
    fn set_last_token(&mut self, tcoords: TokenCoords) {
        self.last_token = Some(tcoords);
        self.last_token_num_flash = LAST_TOKEN_NUM_FLASHES;
        self.last_flash_time = Instant::now();
        self.flash_show = true;
    }

    /// Convert game token coords to the index in the self.tokens vector.
    fn token_coords_to_idx(tcoords: TokenCoords) -> usize {
        tcoords.x + tcoords.y * ROW_SIZE + tcoords.z * ROW_SIZE * ROW_SIZE
    }

    /// Return RGB floats for the given game side.
    fn color_by_side(side: Side) -> (f32, f32, f32) {
        return match side {
            Side::Black => (0.8, 0.5, 0.0),
            Side::White => (1.0, 1.0, 1.0),
        };
    }

    /// Returns player status to show on the screen.
    fn player_str(&self, i: usize) -> String {
        if i >= 2 {
            // TODO: create a separate enum for player indices
            panic!("invalid player idx: {}", i);
        }

        let mut s = format!("player #{}, {}", i + 1, self.players[i].name);

        if let Some(side) = self.players[i].side {
            s.push_str(&format!(" ({:?})", side));
        }

        match &self.players[i].state {
            PlayerState::NotReady(v) => {
                s.push_str(&format!(": {}", v));
            }
            PlayerState::Ready => {
                s.push_str(&format!(": ready"));
            }
        }

        if let Some(pi) = &self.pending_input {
            if Some(pi.side) == self.players[i].side {
                s.push_str(": your turn");
            }
        }

        s
    }
}

/// Context for the input requested from UI by PlayerLocal.
struct PendingInput {
    /// Where to send the resulting pole coords to.
    coord_sender: mpsc::Sender<PoleCoords>,
    /// Side of the token to request. Only affects the color of the pole
    /// pointer.
    side: Side,
}

/// Context of a single player.
struct PlayerInfo {
    /// Name as to show on the screen. Doesn't have to match to any other kind
    /// of name for this player, it's up to UI to construct this name.
    name: String,

    state: PlayerState,
    side: Option<Side>,
}

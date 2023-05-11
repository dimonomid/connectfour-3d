use kiss3d::camera::{ArcBall, Camera};
use kiss3d::event::{Action, Event, MouseButton, WindowEvent};
use kiss3d::light::Light;
use kiss3d::nalgebra::{Point2, Point3, Translation3, Vector2, Vector3};
use kiss3d::scene::SceneNode;
use kiss3d::text::Font;
use kiss3d::window::Window;

use tokio::sync::mpsc;

use std::rc::Rc;
use std::cmp::Ordering;
use std::vec::Vec;
use std::time::{Duration, Instant};

use ordered_float::OrderedFloat;
use connectfour::game::{CoordsXZ, Side, WinRow, ROW_SIZE};
use connectfour::game_manager::{GameManagerToUI, PlayerState, GameState};
use connectfour::game_manager::player_local::{PlayerLocalToUI};
use super::OpponentKind;

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

// Y coord for a plane which matches tops of all poles.
const POLES_TOP_Y: f32 = POLE_HEIGHT / 2.0;

const FLASH_DUR_MS: u128 = 200;
const FLASH_DUR: Duration = Duration::from_millis(FLASH_DUR_MS as u64);

pub struct Window3D {
    w: Window,
    font: Rc<Font>,
    camera: ArcBall,

    tokens: Vec<Option<SceneNode>>,
    pole_pointer: SceneNode,

    pending_input: Option<PendingInput>,

    last_pos: Point2<f32>,

    mouse_down: bool,
    // Set to true if the scene is being rotated or moved with the mouse.
    rotating: bool,

    from_gm: mpsc::Receiver<GameManagerToUI>,
    from_players: mpsc::Receiver<PlayerLocalToUI>,

    players: [PlayerInfo; 2],
    opponent_kind: OpponentKind,

    game_state: Option<GameState>,
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

        // Create pole pointer, initially invisible. It'll be visible only when the mouse cursor
        // hovers a pole.
        let mut pole_pointer = w.add_sphere(POINTER_RADIUS);
        pole_pointer.set_visible(false);

        let p0_name;
        let p1_name;

        match opponent_kind {
            OpponentKind::Local => {
                p0_name = "local";
                p1_name = "local";
            },
            OpponentKind::Network => {
                p0_name = "network";
                p1_name = "local (you)";
            },
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
            from_gm,
            from_players,
            last_pos: Point2::new(0.0f32, 0.0f32),
            players: [
                PlayerInfo{
                    name: p0_name.to_string(),
                    state: PlayerState::NotReady("-".to_string()),
                    side: None,
                },
                PlayerInfo{
                    name: p1_name.to_string(),
                    state: PlayerState::NotReady("-".to_string()),
                    side: None,
                },
            ],
            opponent_kind,
            game_state: None,
            win_row: None,
        };

        window.create_frame();

        window
    }

    fn create_frame(&mut self) {
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

                pole.set_local_translation(Self::pole_translation(x, z));
                pole.set_color(1.0, 1.0, 0.0);
            }
        }
    }

    fn handle_event(&mut self, event: &Event) {
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

                let coords = match self.mouse_coords_to_game_xz(self.last_pos) {
                    Some(coords) => coords,
                    None => return,
                };

                match self
                    .pending_input
                    .as_ref()
                    .expect("no pending_input")
                    .coord_sender
                    .try_send(CoordsXZ {
                        x: coords.0,
                        z: coords.1,
                    }) {
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
                self.last_pos = Point2::new(x as f32, y as f32);
                if self.mouse_down {
                    self.rotating = true;
                }

                self.update_pole_pointer();
            }
            _ => {}
        }
    }

    fn update_pole_pointer(&mut self) {
        if self.rotating || !self.waiting_for_input() {
            self.pole_pointer.set_visible(false);
            return;
        }

        let coords = match self.mouse_coords_to_game_xz(self.last_pos) {
            Some(coords) => coords,
            None => {
                self.pole_pointer.set_visible(false);
                return;
            }
        };
        let mut pole_top_t = Self::pole_translation(coords.0, coords.1);
        pole_top_t.y += POLE_HEIGHT / 2.0;

        //let v = self.mouse_coords_to_pole_translation(self.last_pos);
        self.pole_pointer.set_local_translation(pole_top_t);
        self.pole_pointer.set_visible(true);
    }

    pub fn run(&mut self) {
        let mut last_flash_time = Instant::now();
        let mut flash_show = true;

        while self.render() {
            for event in self.w.events().iter() {
                self.handle_event(&event)
            }

            self.handle_gm_messages();
            self.handle_player_messages();

            // Flash tokens every FLASH_DUR_MS ms.
            let now = Instant::now();
            let dur = now.saturating_duration_since(last_flash_time).as_millis();
            if dur >= FLASH_DUR_MS {
                last_flash_time = last_flash_time.checked_add(FLASH_DUR).unwrap();
                flash_show = !flash_show;

                if let Some(win_row) = &self.win_row {
                    for coords in win_row.row {
                        self.tokens[Self::coord_to_idx(coords.x, coords.y, coords.z)]
                            .as_mut()
                            .unwrap()
                            .set_visible(flash_show);
                    }
                }
            }
        }
    }

    fn handle_gm_messages(&mut self) {
        loop {
            let msg = match self.from_gm.try_recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };

            println!("hey received from GM: {:?}", &msg);

            match msg {
                GameManagerToUI::SetToken(side, coords) => {
                    self.add_token(side, coords.x, coords.y, coords.z);
                },
                GameManagerToUI::ResetBoard(board) => {
                    for maybe_token in &mut self.tokens {
                        if let Some(token) = maybe_token {
                            token.unlink();
                            *maybe_token = None;
                        }
                    }

                    self.win_row = None;

                    // TODO: reimplement as an iterator exposed by the board.
                    for x in 0..ROW_SIZE {
                        for y in 0..ROW_SIZE {
                            for z in 0..ROW_SIZE {
                                if let Some(side) = board.get(x, y, z) {
                                    self.add_token(side, x, y, z);
                                }
                            }
                        }
                    }
                    //self.add_token(side, coords.x, coords.y, coords.z);
                },

                GameManagerToUI::PlayerStateChanged(i, state) => {
                    if i >= 2 {
                        // TODO: create a separate enum for player indices
                        panic!("invalid player idx: {}", i)
                    }

                    self.players[i].state = state;
                },

                GameManagerToUI::PlayerSidesChanged(pri_side, sec_side) => {
                    self.players[0].side = Some(pri_side);
                    self.players[1].side = Some(sec_side);
                },

                GameManagerToUI::GameStateChanged(game_state) => {
                    self.game_state = Some(game_state);
                },

                GameManagerToUI::WinRow(win_row) => {
                    self.win_row = Some(win_row);
                },
            }
        }
    }

    fn handle_player_messages(&mut self) {
        loop {
            let msg = match self.from_players.try_recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };

            println!("hey received from a player {:?}", &msg);

            match msg {
                PlayerLocalToUI::RequestInput(side, coord_sender) => {
                    // Sanity check that the UI is not busy serving another player.
                    if let Some(v) = &self.pending_input {
                        if v.side != side {
                            panic!("coord_sender already existed for {:?} when a new one for {:?} came in", v.side, side);
                        }
                    }

                    // Remember the channel to send the resulting coords to.
                    self.pending_input = Some(PendingInput{
                        coord_sender,
                        side,
                    });

                    // Update the color of the pole pointer to reflect the side.
                    let c = Self::color_by_side(side);
                    self.pole_pointer.set_color(c.0, c.1, c.2);
                }
            }
        }
    }

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
            },

            Some(GameState::WaitingFor(side)) => {
                match self.opponent_kind {
                    OpponentKind::Local => {
                        // Nothing special to write here in local mode.
                    },
                    OpponentKind::Network => {
                        let player_local = &self.players[1];
                        if player_local.side == Some(side) {
                            self.w.draw_text(
                                "Your turn",
                                &Point2::new(10.0, 100.0),
                                60.0,
                                &self.font,
                                &Point3::new(1.0, 1.0, 1.0),
                            );
                        }
                    },
                }
            },

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
                    },
                    OpponentKind::Network => {
                        let player_local = &self.players[1];
                        if player_local.side == Some(winning_side) {
                            text = "you won!";
                        } else {
                            text = "you lost!";
                        }
                    },
                }

                self.w.draw_text(
                    text,
                    &Point2::new(10.0, 100.0),
                    100.0,
                    &self.font,
                    &Point3::new(1.0, 1.0, 1.0),
                );
            },
            Some(GameState::WaitingFor(_)) => {},
        }

        true
    }

    fn waiting_for_input(&self) -> bool {
        if let Some(_) = self.pending_input {
            return true;
        }

        false
    }

    fn pole_translation(x: usize, z: usize) -> Translation3<f32> {
        let xcoord = MARGIN + x as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;
        let zcoord = MARGIN + z as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;

        Translation3::new(xcoord, 0.0, zcoord)
    }

    fn token_translation(x: usize, y: usize, z: usize) -> Translation3<f32> {
        let mut t = Self::pole_translation(x, z);
        t.y = -POLE_HEIGHT / 2.0 + TOKEN_HEIGHT / 2.0 + TOKEN_HEIGHT * (y as f32);

        t
    }

    // returns approximate point where the given ray intersects with the plane which
    // matches the top of the poles.
    //
    // TODO: it's written in extremely dumb way, figure the proper way to do it.
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

    // Just a shortcut to compare two f32-s.
    fn cmpf32(n: f32, m: f32) -> Ordering {
        return OrderedFloat(n).cmp(&OrderedFloat(m));
    }

    fn mouse_coords_to_pole_translation(&self, mouse_pt: Point2<f32>) -> Option<Point3<f32>> {
        let window_size = Vector2::new(self.w.size()[0] as f32, self.w.size()[1] as f32);
        let ray = self.camera.unproject(&mouse_pt, &window_size);

        Self::top_plane_intersect(&ray.0, &ray.1)
    }

    fn pole_translation_to_game_xz(t: Point3<f32>) -> Option<(usize, usize)> {
        const TOLERANCE: f32 = POLE_RADIUS * 1.5;

        for x in 0..ROW_SIZE {
            for z in 0..ROW_SIZE {
                let cur_t = Self::pole_translation(x, z);
                if t.x >= cur_t.x - TOLERANCE
                    && t.x <= cur_t.x + TOLERANCE
                    && t.z >= cur_t.z - TOLERANCE
                    && t.z <= cur_t.z + TOLERANCE
                {
                    return Some((x, z));
                }
            }
        }

        None
    }

    fn mouse_coords_to_game_xz(&self, mouse_pt: Point2<f32>) -> Option<(usize, usize)> {
        let v = self.mouse_coords_to_pole_translation(mouse_pt)?;
        Self::pole_translation_to_game_xz(v)
    }

    fn add_token(&mut self, side: Side, x: usize, y: usize, z: usize) {
        let mut s = self.w.add_sphere(TOKEN_RADIUS);
        let c = Self::color_by_side(side);
        s.set_color(c.0, c.1, c.2);
        s.set_local_translation(Self::token_translation(x, y, z));

        self.tokens[Self::coord_to_idx(x, y, z)] = Some(s);
    }

    fn coord_to_idx(x: usize, y: usize, z: usize) -> usize {
        x + y * ROW_SIZE + z * ROW_SIZE * ROW_SIZE
    }

    fn color_by_side(side: Side) -> (f32, f32, f32) {
        return match side {
            Side::Black => (0.8, 0.5, 0.0),
            Side::White => (1.0, 1.0, 1.0),
        }
    }

    fn player_str(&self, i: usize) -> String {
        if i >= 2 {
            // TODO: create a separate enum for player indices
            panic!("invalid player idx: {}", i);
        }

        let mut s = format!("player #{}, {}", i+1, self.players[i].name);

        if let Some(side) = self.players[i].side {
            s.push_str(&format!(" ({:?})", side));
        }

        match &self.players[i].state {
            PlayerState::NotReady(v) => {
                s.push_str(&format!(": {}", v));
            },
            PlayerState::Ready => {
                s.push_str(&format!(": ready"));
            },
        }

        if let Some(pi) = &self.pending_input {
            if Some(pi.side) == self.players[i].side {
                s.push_str(": your turn");
            }
        }

        s
    }

    fn player_by_side(&self, side: Side) -> Option<&PlayerInfo> {
        for p in &self.players {
            if p.side == Some(side) {
                return Some(p);
            }
        }

        return None
    }
}

struct PendingInput {
    coord_sender: mpsc::Sender<CoordsXZ>,
    side: Side,
}

struct PlayerInfo {
    name: String,
    state: PlayerState,
    side: Option<Side>,
}

use kiss3d::camera::{ArcBall, Camera};
use kiss3d::event::{Action, Event, MouseButton, WindowEvent};
use kiss3d::light::Light;
use kiss3d::nalgebra::{Point2, Point3, Translation3, Vector2, Vector3};
use kiss3d::scene::SceneNode;
use kiss3d::text::Font;
use kiss3d::window::Window;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;
use std::time::Duration;
//use async_std::channel;
//use async_std::task;
use ordered_float::OrderedFloat;

use tokio::sync::mpsc;
use tokio::{task, time};

use connect4::game::{CoordsXZ, Game, Side, ROW_SIZE};
use connect4::game_manager::{
    GameManager, GameManagerToPlayer, GameManagerToUI, PlayerChans, PlayerLocal, PlayerLocalToUI,
    PlayerToGameManager,
};

//async fn send_recv() {
//let (mut tx, mut rx) = channel::bounded(10);

//tx.send(1).await.unwrap();
//tx.send(2).await.unwrap();
//drop(tx);

//assert_eq!(Ok(1), rx.recv().await);
//assert_eq!(Ok(2), rx.recv().await);
////assert_eq!(Err(), rx.recv().await);
//}

//fn main() {
//let mut rt = tokio::runtime::Runtime::new().unwrap();
//rt.block_on(async {
//println!("hello");

//let mut w = Window3D::new();

//let forever = task::spawn(async {
//let mut interval = time::interval(Duration::from_millis(30));

//loop {
//interval.tick().await;
////w.render();
//println!("hello2");
//}
//});

//forever.await;
//})
//}

//fn main() {
//task::block_on(send_recv());

//let mut w = Window3D::new();
//w.run();
//}

fn main() {
    let (gm_to_ui_sender, gm_to_ui_receiver) = mpsc::channel::<GameManagerToUI>(16);

    let (gm_to_pwhite_tx, gm_to_pwhite_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pwhite_to_gm_tx, pwhite_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let (gm_to_pblack_tx, gm_to_pblack_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pblack_to_gm_tx, pblack_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let (player_to_ui_tx, player_to_ui_rx) = mpsc::channel::<PlayerLocalToUI>(1);
    let pwhite_to_ui_tx = player_to_ui_tx.clone();
    let pblack_to_ui_tx = player_to_ui_tx;

    thread::spawn(move || gui(gm_to_ui_receiver, player_to_ui_rx));

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        task::spawn(async {
            let mut pwhite = PlayerLocal::new(
                Side::White,
                gm_to_pwhite_rx,
                pwhite_to_gm_tx,
                pwhite_to_ui_tx,
            );
            pwhite.run().await;

            println!("GameManager exited, TODO exit the program");
        });

        task::spawn(async {
            let mut pblack = PlayerLocal::new(
                Side::Black,
                gm_to_pblack_rx,
                pblack_to_gm_tx,
                pblack_to_ui_tx,
            );
            pblack.run().await;

            println!("GameManager exited, TODO exit the program");
        });

        task::spawn(async {
            let mut gm = GameManager::new(
                gm_to_ui_sender,
                PlayerChans {
                    to: gm_to_pwhite_tx,
                    from: pwhite_to_gm_rx,
                },
                PlayerChans {
                    to: gm_to_pblack_tx,
                    from: pblack_to_gm_rx,
                },
            );
            gm.run().await;

            println!("GameManager exited, TODO exit the program");
        });

        // TODO: remove it
        let forever = task::spawn(async {
            let mut interval = time::interval(Duration::from_millis(100));

            loop {
                interval.tick().await;
                //println!("hello2");
            }
        });

        forever.await;
    })
}

fn gui(from_gm: mpsc::Receiver<GameManagerToUI>, from_players: mpsc::Receiver<PlayerLocalToUI>) {
    let mut w = Window3D::new(from_gm, from_players);
    w.run();
}

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

struct Window3D {
    w: Window,
    font: Rc<Font>,
    camera: ArcBall,

    pole_pointer: SceneNode,

    coord_sender: Option<mpsc::Sender<CoordsXZ>>,

    last_pos: Point2<f32>,

    mouse_down: bool,
    // Set to true if the scene is being rotated or moved with the mouse.
    rotating: bool,

    from_gm: mpsc::Receiver<GameManagerToUI>,
    from_players: mpsc::Receiver<PlayerLocalToUI>,
}

impl Window3D {
    fn new(
        from_gm: mpsc::Receiver<GameManagerToUI>,
        from_players: mpsc::Receiver<PlayerLocalToUI>,
    ) -> Window3D {
        let mut w = Window::new("Connect4 3D");
        let font = Font::default();

        let eye = Point3::new(20.0, 20.0, 20.0);
        let at = Point3::origin();
        let camera = ArcBall::new(eye, at);

        let mut pole_pointer = w.add_sphere(POINTER_RADIUS);
        pole_pointer.set_color(1.0, 0.5, 0.5);
        pole_pointer.set_visible(false);

        w.set_light(Light::StickToCamera);
        w.set_framerate_limit(None);

        let mut window = Window3D {
            w,
            font,
            camera,
            pole_pointer,
            coord_sender: None,
            mouse_down: false,
            rotating: false,
            from_gm,
            from_players,
            last_pos: Point2::new(0.0f32, 0.0f32),
        };

        window.add_frame();

        window
    }

    fn add_frame(&mut self) {
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
            //WindowEvent::FramebufferSize(x, y) => {
            //println!("frame buffer size event {}, {}", x, y);
            //}
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
                    .coord_sender
                    .as_ref()
                    .expect("no coord_sender")
                    .try_send(CoordsXZ {
                        x: coords.0,
                        z: coords.1,
                    }) {
                    Ok(_) => {
                        self.coord_sender = None;
                    }
                    Err(err) => {
                        println!("failed sending coords to the player: {}", err);
                    }
                }
            }
            //WindowEvent::Key(key, action, modif) => {
            //println!("key event {:?} on {:?} with {:?}", key, action, modif);
            //}
            WindowEvent::CursorPos(x, y, _modif) => {
                self.last_pos = Point2::new(x as f32, y as f32);
                if self.mouse_down {
                    self.rotating = true;
                }

                self.update_pole_pointer();

                //println!(
                //"conv {:?},{:?} to {:?}",
                //x, y, v,
                //);
            }
            //WindowEvent::Close => {
            //println!("close event");
            //}
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

    fn run(&mut self) {
        while self.render() {
            for event in self.w.events().iter() {
                self.handle_event(&event)
            }

            self.handle_gm_messages();
            self.handle_player_messages();

            //println!("hey");
        }
    }

    fn handle_gm_messages(&mut self) {
        loop {
            let msg = match self.from_gm.try_recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };

            match msg {
                GameManagerToUI::SetToken(side, coords) => {
                    self.add_token(side, coords.x, coords.y, coords.z);
                }
            }

            println!("hey received from GM: {:?}", &msg);
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
                    self.coord_sender = Some(coord_sender);

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

        if self.waiting_for_input() {
            self.w.draw_text(
                "Waiting for input",
                &Point2::origin(),
                60.0,
                &self.font,
                &Point3::new(0.0, 1.0, 1.0),
            );
        }

        true
    }

    fn waiting_for_input(&self) -> bool {
        if let Some(_) = self.coord_sender {
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
    }

    fn color_by_side(side: Side) -> (f32, f32, f32) {
        return match side {
            Side::Black => (0.8, 0.5, 0.0),
            Side::White => (1.0, 1.0, 1.0),
        }
    }
}

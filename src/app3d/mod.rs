extern crate kiss3d;

use kiss3d::nalgebra::{Translation3, Vector2, Vector3, Point2, Point3};
use kiss3d::event::{Action, WindowEvent};
use kiss3d::window::Window;
use kiss3d::camera::{ArcBall, Camera};
use kiss3d::text::Font;
use kiss3d::light::Light;

use std::cmp::Ordering;
use ordered_float::OrderedFloat;

// In "Connect Four", ROW_SIZE is the "Four". Technically we can change it to 5 or any other
// positive number, and the UI will work just fine.
const ROW_SIZE: usize = 4;

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

pub fn run() {
    let mut window = Window::new("Connect4 3D");
    let font = Font::default();

    let eye = Point3::new(20.0, 20.0, 20.0);
    let at = Point3::origin();
    let mut camera = ArcBall::new(eye, at);

    //let mut camera = kiss3d::planar_camera::FixedView::new();

    add_frame(&mut window);

    window.set_light(Light::StickToCamera);

    //let rot = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 0.014);

    while window.render_with_camera(&mut camera) {
        window.draw_text(
            "Hello birds!",
            &Point2::origin(),
            60.0,
            &font,
            &Point3::new(0.0, 1.0, 1.0),
        );

        for event in window.events().iter() {
            match event.value {
                WindowEvent::FramebufferSize(x, y) => {
                    println!("frame buffer size event {}, {}", x, y);
                }
                WindowEvent::MouseButton(_button, Action::Release, _modif) => {
                    //println!("mouse press event on {:?} with {:?}", button, modif);
                    ////let window_size =
                        ////nalgebra::Vector2::new(window.size()[0] as f32, window.size()[1] as f32);

                    //let window_size = Vector2::new(800.0, 600.0);

                    //let v = (&camera).unproject(&last_pos, &window_size);
                    //println!(
                        //"conv {:?} to {:?} win siz {:?} ",
                        //last_pos, v, window_size
                    //);
                }
                //WindowEvent::Key(key, action, modif) => {
                    //println!("key event {:?} on {:?} with {:?}", key, action, modif);
                //}
                WindowEvent::CursorPos(x, y, _modif) => {
                    let last_pos = Point2::new(x as f32, y as f32);
                    println!("move: {:?}", last_pos);

                    //let window_size = Vector2::new(800.0, 600.0);
                    let window_size =
                        Vector2::new(window.size()[0] as f32, window.size()[1] as f32);
                    let ray = camera.unproject(&last_pos, &window_size);

                    //println!("checking {:?}, ray {:?}", last_pos, ray);

                    let v = plane_intersect(&ray.0, &ray.1);

                    println!(
                        "conv {:?} to {:?}, win size {:?}",
                        last_pos, v, window_size,
                    );
                }
                //WindowEvent::Close => {
                    //println!("close event");
                //}
                _ => {}
            }
        }

        //c.prepend_to_local_rotation(&rot);
    }

}

fn add_frame(window: &mut Window) {
    let mut foundation = window.add_cube(FOUNDATION_WIDTH, FOUNDATION_HEIGHT, FOUNDATION_WIDTH);
    foundation.set_color(1.0, 0.8, 0.0);
    foundation.set_local_translation(Translation3::new(0.0, -(POLE_HEIGHT + FOUNDATION_HEIGHT)/2.0, 0.0));

    for x in 0..ROW_SIZE {
        for z in 0..ROW_SIZE {
            let mut pole = window.add_cylinder(POLE_RADIUS, POLE_HEIGHT);

            pole.set_local_translation(pole_translation(x, z));
            pole.set_color(1.0, 1.0, 0.0);

            //let mut s1 = window.add_sphere(TOKEN_RADIUS);
            //s1.set_color(1.0, 0.0, 0.0);
            //s1.set_local_translation(token_translation(x, 0, z));

            //let mut s2 = window.add_sphere(TOKEN_RADIUS);
            //s2.set_color(1.0, 0.0, 0.0);
            //s2.set_local_translation(token_translation(x, 1, z));

            //let mut s3 = window.add_sphere(TOKEN_RADIUS);
            //s3.set_color(1.0, 0.0, 0.0);
            //s3.set_local_translation(token_translation(x, 2, z));

            //let mut s4 = window.add_sphere(TOKEN_RADIUS);
            //s4.set_color(1.0, 0.0, 0.0);
            //s4.set_local_translation(token_translation(x, 3, z));
        }
    }

    //let mut s1 = window.add_sphere(1.0);
    //s1.set_color(1.0, 0.0, 0.0);
    //s1.set_local_translation(Translation3::new(0.0, -4.0, 0.0));
}

fn pole_translation(x: usize, z: usize) -> Translation3<f32> {
    let xcoord = MARGIN + x as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;
    let zcoord = MARGIN + z as f32 * POLE_SPACING - FOUNDATION_WIDTH / 2.0;

    Translation3::new(xcoord, 0.0, zcoord)
}

fn token_translation(x: usize, y: usize, z: usize) -> Translation3<f32> {
    let mut t = pole_translation(x, z);
    t.y = -POLE_HEIGHT/2.0 + TOKEN_HEIGHT/2.0 + TOKEN_HEIGHT*(y as f32);

    t
}

// returns approximate point where the given ray intersects with the plane which
// matches the top of the poles.
//
// TODO: it's written in extremely dumb way, figure the proper way to do it.
fn plane_intersect(p: &Point3<f32>, v: &Vector3<f32>) -> Option<Point3<f32>> {
    const Y_PLANE: f32 = POLE_HEIGHT / 2.0;

    // If the ray doesn't intersect with the plane, cut it short.
    let ord_p = cmpf32(Y_PLANE, p.y);
    let ord_v = cmpf32(0.0, v.y);
    if ord_p == ord_v {
        return None;
    }

    let mut pcur = *p;
    let vcur = Vector3::new(v.x/10.0, v.y/10.0, v.z/10.0);

    let ord_orig = cmpf32(Y_PLANE, pcur.y);

    loop {
        pcur.x += vcur.x;
        pcur.y += vcur.y;
        pcur.z += vcur.z;

        if cmpf32(Y_PLANE, pcur.y) != ord_orig {
            break
        }
    }

    Some(pcur)
}

// Just a shortcut to compare two f32-s.
fn cmpf32(n: f32, m: f32) -> Ordering {
    return OrderedFloat(n).cmp(&OrderedFloat(m))
}

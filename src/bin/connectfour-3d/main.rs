mod gui3d;

use std::thread;

use tokio::sync::mpsc;
use tokio::{task};

use connect4::game_manager::{
    GameManager, GameManagerToPlayer, GameManagerToUI,
    PlayerToGameManager,
};
use connect4::game_manager::player_local::{PlayerLocal, PlayerLocalToUI};

fn main() {
    let (gm_to_ui_sender, gm_to_ui_receiver) = mpsc::channel::<GameManagerToUI>(16);
    let (player_to_ui_tx, player_to_ui_rx) = mpsc::channel::<PlayerLocalToUI>(1);

    // Setup tokio runtime in another thread.
    thread::spawn(move || async_runtime(gm_to_ui_sender, player_to_ui_tx));

    // Run GUI in the main thread. It's easier since when the user closes the window, the whole
    // thing gets killed (albeit not yet gracefully).
    let mut w = gui3d::Window3D::new(gm_to_ui_receiver, player_to_ui_rx);
    w.run();

    // GUI window was closed by the user.
    //
    // TODO: would be nice to implement some graceful shutdown, but not bothering for now.
}

fn async_runtime(
    gm_to_ui_sender: mpsc::Sender<GameManagerToUI>,
    player_to_ui_tx: mpsc::Sender<PlayerLocalToUI>,
) {
    // Every player will need a copy of the sender, so clone it.
    let pwhite_to_ui_tx = player_to_ui_tx.clone();
    let pblack_to_ui_tx = player_to_ui_tx;

    // For both players, create channels for bidirectional communication with the GameManager.
    let (gm_to_pwhite_tx, gm_to_pwhite_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pwhite_to_gm_tx, pwhite_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let (gm_to_pblack_tx, gm_to_pblack_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pblack_to_gm_tx, pblack_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut set = task::JoinSet::new();

        set.spawn(async {
            let mut pwhite = PlayerLocal::new(
                gm_to_pwhite_rx,
                pwhite_to_gm_tx,
                pwhite_to_ui_tx,
            );
            pwhite.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        set.spawn(async {
            let mut pblack = PlayerLocal::new(
                gm_to_pblack_rx,
                pblack_to_gm_tx,
                pblack_to_ui_tx,
            );
            pblack.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        set.spawn(async {
            let mut gm = GameManager::new(
                gm_to_ui_sender,

                gm_to_pwhite_tx,
                pwhite_to_gm_rx,

                gm_to_pblack_tx,
                pblack_to_gm_rx,
            );
            gm.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        // Normally the tasks should run indefinitely, but if some of them error out,
        // print the errors.
        while let Some(v) = set.join_next().await {
            let res = match v {
                Err(err) => { println!("task panicked {:?}", err); continue; },
                Ok(res) => res,
            };

            match res {
                Ok(_) => { println!("task returned ok"); },
                Err(err) => { println!("task returned error {:?}", err); },
            }
        }
    })
}

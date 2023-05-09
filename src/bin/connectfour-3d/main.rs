mod gui3d;

use std::thread;

use tokio::sync::mpsc;
use tokio::{task};

use connect4::game::{Side};
use connect4::game_manager::{
    GameManager, GameManagerToPlayer, GameManagerToUI, PlayerChans, PlayerLocal, PlayerLocalToUI,
    PlayerToGameManager,
};

fn main() {
    let (gm_to_ui_sender, gm_to_ui_receiver) = mpsc::channel::<GameManagerToUI>(16);
    let (player_to_ui_tx, player_to_ui_rx) = mpsc::channel::<PlayerLocalToUI>(1);

    // Setup tokio stuff in another thread.
    thread::spawn(move || tokio_thread(gm_to_ui_sender, player_to_ui_tx));

    // Run GUI in the main thread. It's easier since when the user closes the window, the whole
    // thing gets killed (albeit not yet gracefully).
    let mut w = gui3d::Window3D::new(gm_to_ui_receiver, player_to_ui_rx);
    w.run();

    // GUI window was closed by the user.
    //
    // TODO: would be nice to implement some graceful shutdown, but not bothering for now.
}

fn tokio_thread(
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
        let handle_pwhite = task::spawn(async {
            let mut pwhite = PlayerLocal::new(
                Side::White,
                gm_to_pwhite_rx,
                pwhite_to_gm_tx,
                pwhite_to_ui_tx,
            );
            pwhite.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        let handle_pblack = task::spawn(async {
            let mut pblack = PlayerLocal::new(
                Side::Black,
                gm_to_pblack_rx,
                pblack_to_gm_tx,
                pblack_to_ui_tx,
            );
            pblack.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        let handle_gm = task::spawn(async {
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
            gm.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        let handles = vec![handle_pwhite, handle_pblack, handle_gm];
        for h in handles {
            let res = match h.await {
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
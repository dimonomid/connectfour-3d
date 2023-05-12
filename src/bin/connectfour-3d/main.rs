mod gui3d;

use std::env;
use std::thread;

use tokio::sync::mpsc;
use tokio::task;

use connectfour::game::Side;
use connectfour::game_manager::player_local::{PlayerLocal, PlayerLocalToUI};
use connectfour::game_manager::player_ws_client::PlayerWSClient;
use connectfour::game_manager::{
    GameManager, GameManagerToPlayer, GameManagerToUI, PlayerToGameManager,
};

fn main() {
    let opponent_kind_str = env::args().nth(1).unwrap_or_else(|| "network".to_string());

    let opponent_kind = match opponent_kind_str.as_str() {
        "local" => OpponentKind::Local,
        "network" => OpponentKind::Network,
        _ => {
            println!("Wrong opponent kind, try local or remote");
            std::process::exit(1);
        }
    };

    let (gm_to_ui_sender, gm_to_ui_receiver) = mpsc::channel::<GameManagerToUI>(16);
    let (player_to_ui_tx, player_to_ui_rx) = mpsc::channel::<PlayerLocalToUI>(1);

    // Setup tokio runtime in another thread.
    thread::spawn(move || async_runtime(gm_to_ui_sender, player_to_ui_tx, opponent_kind));

    // Run GUI in the main thread. It's easier since when the user closes the window, the whole
    // thing gets killed (albeit not yet gracefully).
    let mut w = gui3d::Window3D::new(gm_to_ui_receiver, player_to_ui_rx, opponent_kind);
    w.run();

    // GUI window was closed by the user.
    //
    // TODO: would be nice to implement some graceful shutdown, but not bothering for now.
}

fn async_runtime(
    gm_to_ui_sender: mpsc::Sender<GameManagerToUI>,
    player_to_ui_tx: mpsc::Sender<PlayerLocalToUI>,
    opponent_kind: OpponentKind,
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

        set.spawn(async move {
            match opponent_kind {
                OpponentKind::Local => {
                    let mut p0 = PlayerLocal::new(
                        Some(Side::White),
                        gm_to_pwhite_rx,
                        pwhite_to_gm_tx,
                        pwhite_to_ui_tx,
                    );
                    p0.run().await?;
                }
                OpponentKind::Network => {
                    let connect_addr = "ws://127.0.0.1:7248";
                    let conn_url = url::Url::parse(&connect_addr).unwrap();
                    let mut p0 = PlayerWSClient::new(conn_url, gm_to_pwhite_rx, pwhite_to_gm_tx);
                    p0.run().await?;
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        set.spawn(async {
            let mut p1 = PlayerLocal::new(None, gm_to_pblack_rx, pblack_to_gm_tx, pblack_to_ui_tx);
            p1.run().await?;

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
                Err(err) => {
                    println!("task panicked {:?}", err);
                    continue;
                }
                Ok(res) => res,
            };

            match res {
                Ok(_) => {
                    println!("task returned ok");
                }
                Err(err) => {
                    println!("task returned error {:?}", err);
                }
            }
        }
    })
}

#[derive(Debug, Copy, Clone)]
pub enum OpponentKind {
    Local,
    Network,
    // TODO: AI
}

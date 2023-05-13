mod gui3d;
mod sounds;

use std::fmt;
use std::str::FromStr;
use std::thread;

use anyhow::{anyhow, Result};
use clap::Parser;
use tokio::sync::mpsc;
use tokio::task;

use connectfour::game::Side;
use connectfour::game_manager::player_local::{PlayerLocal, PlayerLocalToUI};
use connectfour::game_manager::player_ws_client::PlayerWSClient;
use connectfour::game_manager::{
    GameManager, GameManagerToPlayer, GameManagerToUI, PlayerToGameManager,
};

#[derive(Debug, clap::Parser)]
struct CliArgs {
    #[clap(short = 'o', long = "opponent", default_value_t = OpponentKind::Network)]
    opponent_kind: OpponentKind,

    /// URL to use for the network game.
    #[clap(short = 'u', long = "url", default_value_t = String::from("ws://64.226.98.150:7248"))]
    url: String,

    /// Game name to use for the network game.
    #[clap(short = 'g', long = "game", default_value_t = String::from("mygame1"))]
    game_id: String,
}

fn main() -> Result<()> {
    let cli_args = CliArgs::parse();
    let opponent_kind = cli_args.opponent_kind;

    let (gm_to_ui_sender, gm_to_ui_receiver) = mpsc::channel::<GameManagerToUI>(16);
    let (player_to_ui_tx, player_to_ui_rx) = mpsc::channel::<PlayerLocalToUI>(1);

    // Setup tokio runtime in another thread.
    thread::spawn(move || async_runtime(gm_to_ui_sender, player_to_ui_tx, cli_args));

    let sound_player = sounds::Player::new()?;

    // Run GUI in the main thread. It's easier since when the user closes the
    // window, the whole thing gets killed (albeit not yet gracefully).
    let mut w = gui3d::Window3D::new(
        sound_player,
        gm_to_ui_receiver,
        player_to_ui_rx,
        opponent_kind,
    );
    w.run();

    // GUI window was closed by the user.
    //
    // TODO: would be nice to implement some graceful shutdown, but not
    // bothering for now.

    Ok(())
}

/// Should be called in a separate OS thread, it'll handle all the tokio runtime.
fn async_runtime(
    gm_to_ui_sender: mpsc::Sender<GameManagerToUI>,
    player_to_ui_tx: mpsc::Sender<PlayerLocalToUI>,
    cli_args: CliArgs,
) {
    // Every player will need a copy of the sender, so clone it.
    let pwhite_to_ui_tx = player_to_ui_tx.clone();
    let pblack_to_ui_tx = player_to_ui_tx;

    // For both players, create channels for bidirectional communication with
    // the GameManager.
    let (gm_to_pwhite_tx, gm_to_pwhite_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pwhite_to_gm_tx, pwhite_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let (gm_to_pblack_tx, gm_to_pblack_rx) = mpsc::channel::<GameManagerToPlayer>(16);
    let (pblack_to_gm_tx, pblack_to_gm_rx) = mpsc::channel::<PlayerToGameManager>(16);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut set = task::JoinSet::new();

        // Create the primary player, depending on the opponent_kind: either the
        // network or local player. Network player *has* to be the primary one,
        // since it will receive info from the server which has the big picture.
        set.spawn(async move {
            match cli_args.opponent_kind {
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
                    let conn_url = url::Url::parse(&cli_args.url).unwrap();
                    let mut p0 = PlayerWSClient::new(
                        conn_url,
                        cli_args.game_id,
                        gm_to_pwhite_rx,
                        pwhite_to_gm_tx,
                    );
                    p0.run().await?;
                }
            }

            Ok::<(), anyhow::Error>(())
        });

        // Create the secondary player, always local.
        set.spawn(async {
            let mut p1 = PlayerLocal::new(None, gm_to_pblack_rx, pblack_to_gm_tx, pblack_to_ui_tx);
            p1.run().await?;

            Ok::<(), anyhow::Error>(())
        });

        // Create the GameManager.
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

/// Kind of the opponent: local or network.
#[derive(Debug, Copy, Clone)]
pub enum OpponentKind {
    Local,
    Network,
    // TODO: AI
}

impl FromStr for OpponentKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "" => Ok(OpponentKind::Local),
            "local" => Ok(OpponentKind::Local),
            "network" => Ok(OpponentKind::Network),
            _ => Err(anyhow!("invalid opponent kind; try 'local' or 'network'")),
        }
    }
}

impl fmt::Display for OpponentKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OpponentKind::Local => write!(f, "local"),
            OpponentKind::Network => write!(f, "network"),
        }
    }
}

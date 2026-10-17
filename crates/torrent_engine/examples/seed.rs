//! A seeder for trying the BitTorrent screens by hand.
//!
//! ```text
//! cargo run -p fetchbox-torrent-engine --example seed -- <dir> <peer ip:port> [KiB/s] [MiB]
//! ```
//!
//! Makes a random file `<dir>/files/demo.bin` (default 64 MiB) and writes `<dir>/demo.torrent`. Add
//! that file to a running FetchBox (`--torrent-port` is the `<peer>` port), then create the file
//! `<dir>/go`: the seeder starts and connects to the peer. (It connects once, so the torrent has to
//! be there first.) Upload is limited to the given speed (default 300 KiB/s) so the transfer can be
//! watched. Stop it with Ctrl-C.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use fetchbox_torrent_engine::{Config, TorrentEngine};

#[tokio::main]
async fn main() -> anyhow_free::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .ok_or("usage: seed <dir> <peer ip:port> [KiB/s] [MiB]")?,
    );
    let peer: SocketAddr = args.next().ok_or("missing <peer ip:port>")?.parse()?;
    let kib: u64 = args.next().map_or(Ok(300), |v| v.parse())?;
    let mib: usize = args.next().map_or(Ok(64), |v| v.parse())?;

    let files = dir.join("files");
    std::fs::create_dir_all(&files)?;
    let data = files.join("demo.bin");
    if !data.exists() {
        // Not all zeros: a repeating pattern that still differs from piece to piece.
        let mut x: u32 = 0x9e37_79b9;
        let bytes: Vec<u8> = (0..mib * 1024 * 1024)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 24) as u8
            })
            .collect();
        std::fs::write(&data, bytes)?;
    }
    let made = librqbit::create_torrent(
        &data,
        librqbit::CreateTorrentOptions {
            piece_length: Some(256 * 1024),
            ..Default::default()
        },
        &librqbit::spawn_utils::BlockingSpawner::new(2),
    )
    .await?;
    let torrent = made.as_bytes()?;
    std::fs::write(dir.join("demo.torrent"), &torrent)?;
    println!("wrote {}", dir.join("demo.torrent").display());

    let go = dir.join("go");
    println!(
        "add the .torrent to FetchBox, then create {} to start seeding",
        go.display()
    );
    while !go.exists() {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    let engine = TorrentEngine::start(Config {
        output_dir: files,
        state_dir: dir.join("state"),
        listen_port: 0,
        down_limit: 0,
        up_limit: kib * 1024,
        dht: false,
        client: "FetchBox-demo-seeder/0".into(),
    })
    .await?;
    let hash = engine.add_file(torrent, false, vec![peer]).await?.info_hash;
    println!("seeding {hash}, connecting to {peer} (Ctrl-C to stop)");
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if let Some(s) = engine.snapshot(&hash) {
            println!("peers: {} connected, up {} B/s", s.connected, s.up_speed);
        }
    }
}

/// `main` returns any error that can be printed.
mod anyhow_free {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

pub mod errors;
pub mod protocol;
pub mod api_key;
pub mod permissions;
pub mod kv_store;
pub mod broker;
pub mod frame_io;
pub mod client;

pub use errors::*;
pub use protocol::*;
pub use api_key::*;
pub use permissions::*;
pub use kv_store::*;
pub use broker::*;
pub use frame_io::*;
pub use client::*;

use sled::Db;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

pub async fn run_server(addr: &str, db: Db, root_token: String) -> Result<(), FoguError> {
    let kv = Arc::new(KvStore::new(db.clone()));
    let apikeys = Arc::new(ApiKeyStore::new(&db)?);
    let (broker_tx, broker_rx) = tokio::sync::mpsc::unbounded_channel::<BrokerEvent>();

    tokio::spawn(broker_loop(broker_rx));

    // TTL purge task
    {
        let kv2 = kv.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                tick.tick().await;
                let n = kv2.purge_expired();
                if n > 0 {
                    info!("TTL purge: {n} keys removed");
                }
            }
        });
    }

    let listener = TcpListener::bind(addr).await?;
    info!("🚀 FOGU listening on {addr}");
    info!("🔑 Root token configured — keep it secret!");

    loop {
        let (stream, peer) = listener.accept().await?;
        info!("→ {peer}");
        let (kv, apikeys, broker_tx, root_token) = (
            kv.clone(),
            apikeys.clone(),
            broker_tx.clone(),
            root_token.clone(),
        );
        tokio::spawn(async move {
            Client::new(stream, kv, apikeys, broker_tx, root_token)
                .run()
                .await;
        });
    }
}
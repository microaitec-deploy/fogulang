use fogu::{run_server, FoguError};
use sled::Config;
use std::env;
use tracing::info;
use dotenvy::dotenv;

#[tokio::main]
async fn main() -> Result<(), FoguError> {
    tracing_subscriber::fmt::init();
      dotenv().ok();

    let addr = env::var("FOGU_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let db_path = env::var("FOGU_DB_PATH").unwrap_or_else(|_| "./db".to_string());
    let root_token = env::var("FOGU_ROOT_TOKEN").unwrap_or_else(|_| "root_token_secret".to_string());

    info!("Initializing database at {}", db_path);
    let db_config = Config::new().path(db_path);
    let db = db_config.open()?;

    run_server(&addr, db, root_token).await
}
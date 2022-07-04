use std::sync::Arc;

use axum::{
    response::{IntoResponse, Response},
    routing::post,
    Extension, Server,
};
use clap::Parser;
use color_eyre::Result;
use hyper::StatusCode;
use speedtester_rs::api::PubTestReport;
use tokio_postgres::Client;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Config {
    #[clap(env = "DB_NAME")]
    db_name: String,
    #[clap(env = "DB_HOST")]
    db_host: String,
    #[clap(env = "DB_USER")]
    db_user: String,
    #[clap(env = "DB_PASS")]
    db_pass: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    setup()?;

    let args = Config::parse();
    let mut cfg = tokio_postgres::Config::new();

    cfg.application_name(module_path!())
        .dbname(&args.db_name)
        .host(&args.db_host)
        .user(&args.db_user)
        .password(&args.db_pass);

    let (db_client, db_conn) = cfg.connect(tokio_postgres::NoTls).await?;
    let db_client = Arc::new(db_client);

    let api = axum::Router::new()
        .route("/api/v1/post_report", post(post_report))
        .layer(
            ServiceBuilder::new()
                .concurrency_limit(50)
                .layer(TraceLayer::new_for_http())
                .layer(Extension(db_client)),
        );

    info!("Creating future for server...");

    Server::bind(&"0.0.0.0:9000".parse().unwrap())
        .serve(api.into_make_service())
        .await
        .unwrap();
    Ok(())
}

#[axum_macros::debug_handler]
async fn post_report(
    Json(report): Json<PubTestReport>,
    Extension(db): Extension<Arc<Client>>,
) -> impl IntoResponse {
    StatusCode::OK
}

fn setup() -> Result<()> {
    // Load environment from .env if present for dev convenience
    dotenv::dotenv().ok();

    // if std::env::var("RUST_LIB_BACKTRACE").is_err() {
    //     std::env::set_var("RUST_LIB_BACKTRACE", "1")
    // }
    color_eyre::install()?;

    // For now, debug at top level and info for all other modules and crates. Will change to warning later
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}

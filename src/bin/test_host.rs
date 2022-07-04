use axum::{
    extract::{Form, Path},
    http::{Response, StatusCode},
    routing::post,
    Extension, Json, Router, Server,
};
use clap::Parser;
use color_eyre::{Report, Result};
use rand::prelude::*;
use speedtester_rs::api::{TestRequest, TestReservation};
use std::{
    collections::HashSet,
    error::Error,
    future::Future,
    net::TcpListener,
    process::{ExitStatus, Output},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, SemaphorePermit};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, trace};
use tracing_subscriber::EnvFilter;

use speedtester_rs::auth::authenticate;
// new test
//- spawn thread
//- create iperf server ctx for one off
//- reply to req with test info
// ensure test is cleaned up

// Constants
const MAX_CONCURRENT_TESTS: usize = 5;

#[derive(Parser)]
struct Config {
    #[clap(env = "VALID_TOKENS", short = 'v')]
    valid_tokens: Vec<String>,
}

struct State {
    active_ports: Arc<Mutex<HashSet<u16>>>,
    test_counter: Arc<Semaphore>,
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    setup()?;

    // Setup application state
    let state = Arc::new(State {
        active_ports: Arc::new(Mutex::new(HashSet::new())),
        test_counter: Arc::new(Semaphore::new(MAX_CONCURRENT_TESTS)),
    });

    //TODO: Auth layer
    let app = Router::new()
        .route("/api/v1/newtest", post(new_test))
        .layer(
            ServiceBuilder::new()
                // First layer of rate limiting, only this many inflight requests
                .concurrency_limit(50)
                .layer(TraceLayer::new_for_http())
                .layer(Extension(state)),
        );

    info!("Listening on 0.0.0.0:8000");
    Server::bind(&"0.0.0.0:8000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

#[axum_macros::debug_handler]
async fn new_test(
    Json(request): Json<TestRequest>,
    Extension(state): Extension<Arc<State>>,
) -> axum::response::Result<Json<TestReservation>, &'static str> {
    info!("Client info: {request:#?}");

    // Axum has an inflight request limit, however iperf instances are more expensive and my linode is small
    // Hold clients here in a queue until enough other tests expire
    let test_permit = state.test_counter.clone().acquire_owned().await.unwrap();

    // Small loop to generate a new, free port for iperf to bind to
    let port: u16 = {
        // Lock and borrow while finding a new port
        let mut port_set = state.active_ports.lock().unwrap();

        let mut rng = thread_rng();
        loop {
            let port = rng.gen_range(5000..6000);
            if !port_set.contains(&port) || !port_in_use(port) {
                port_set.insert(port);
                break port;
            }
        }
    };

    // Spawn iperf server into a thread
    let res = tokio::process::Command::new("iperf3")
        .args(&["-s", "-p", &port.to_string(), "-1", "--idle-timeout", "10"])
        .output();

    // Asynchronously wait for the test to finish away from here
    tokio::spawn(wait_for_iperf(port, state, test_permit, res));

    // Give server some time to initialize...
    tokio::time::sleep(Duration::from_millis(500)).await;

    // This assumes that the server launch passed, and further that the server runs correctly
    // given the commands listed above, there will be issues if any of those fail to hold true
    // If the port is in use, which _shouldn't_ happen but still technically could, then this will break

    // when test server has been started reply to request with the port number
    Ok(Json(TestReservation { port_number: port }))
}

fn port_in_use(port: u16) -> bool {
    TcpListener::bind(("0.0.0.0", port)).is_ok()
}

// Consume an iperf server process handle and wait for it to finish
async fn wait_for_iperf<T, E>(port: u16, state: Arc<State>, _permit: OwnedSemaphorePermit, task: T)
where
    T: Future<Output = Result<Output, E>>,
    E: Error,
{
    debug!("Waiting for iperf server on port {port}");
    // Start as server, one-off, with port, JSON output (in case client wants it maybe), and timeout if no one dials in in 10 seconds

    let res = task.await.unwrap();

    if !res.status.success() {
        let out_str = String::from_utf8_lossy(&res.stdout);
        error!("Iperf server exited '{}', output follows", res.status);
        error!("{out_str}");
    } else {
        debug!("Iperf server exited successfully!");
    }

    let mut ports = state.active_ports.lock().unwrap();
    ports.remove(&port);
    // Test permit is dropped here
}

fn setup() -> Result<(), Report> {
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

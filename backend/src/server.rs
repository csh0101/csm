use std::{
    future::Future,
    net::{SocketAddr, TcpListener as StdTcpListener},
};

use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{api, state::SharedState};

pub async fn serve(state: SharedState, bind_addr: SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app(state)).await?;

    Ok(())
}

pub async fn serve_std_listener(
    state: SharedState,
    listener: StdTcpListener,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(listener)?;
    axum::serve(listener, app(state)).await?;

    Ok(())
}

pub async fn serve_with_shutdown(
    state: SharedState,
    bind_addr: SocketAddr,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}

fn app(state: SharedState) -> axum::Router {
    api::router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

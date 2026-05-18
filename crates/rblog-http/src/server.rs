//! Boot path: bind, serve, shut down on Ctrl-C / SIGTERM.

use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;

/// Start serving `router` on `bind`, returning once the shutdown signal
/// fires. Logs the chosen address via `tracing::info!`.
pub async fn serve(router: Router, bind: SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "rblog listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut term) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            term.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("received Ctrl-C, shutting down"),
        () = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}

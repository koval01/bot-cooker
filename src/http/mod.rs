//! The HTTP surface: an accept loop and one task per connection. The wire
//! format comes from `crate::wire`, the content from the engine.

mod admin;
mod conn;
mod handler;
mod request;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing::{debug, warn};

use crate::config::Config;
use crate::engine::Engine;
use crate::routing::AllowList;
use crate::stats::Stats;

/// How long open connections may drain after a stop request: tarpit
/// streams would otherwise hold a shutdown for minutes.
const DRAIN_GRACE: Duration = Duration::from_secs(3);

/// A client connection as the wire code sees it: a plain socket today, a TLS
/// session over one as well.
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}

/// Shared application state handed to every connection.
#[derive(Clone)]
pub struct AppState {
    cfg: Arc<Config>,
    engine: Arc<Engine>,
    allow: Arc<AllowList>,
    stats: Arc<Stats>,
    admin_token: Arc<String>,
}

impl AppState {
    pub fn new(cfg: Config, engine: Arc<Engine>, stats: Arc<Stats>, admin_token: String) -> Self {
        Self { allow: Arc::new(AllowList::new(&cfg.allowlist)), cfg: Arc::new(cfg), engine, stats, admin_token: Arc::new(admin_token) }
    }
}

/// Accept connections until `shutdown` resolves, then give open ones a short
/// grace period and drop the rest.
pub async fn serve(listener: TcpListener, app: AppState, shutdown: impl Future<Output = ()>) {
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((sock, peer)) => {
                    let _ = sock.set_nodelay(true);
                    tasks.spawn(conn::connection(app.clone(), sock, peer));
                }
                // Out of file descriptors and the like: back off instead of
                // spinning; the client retries.
                Err(e) => {
                    debug!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
    drop(listener);
    let drained = tokio::time::timeout(DRAIN_GRACE, async { while tasks.join_next().await.is_some() {} }).await;
    if drained.is_err() {
        warn!("dropping {} connections still open after {}s", tasks.len(), DRAIN_GRACE.as_secs());
        tasks.abort_all();
    }
}

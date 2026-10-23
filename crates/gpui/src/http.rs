use std::future::Future;

use gpui_util::defer;
pub use reqwest;

/// The HTTP client GPUI uses to fetch remote resources, such as images.
///
/// `reqwest` futures must run inside a Tokio runtime, so requests are spawned
/// onto the runtime this client was created with.
#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    runtime: tokio::runtime::Handle,
}

impl HttpClient {
    /// Creates a client that runs its requests on the given Tokio runtime.
    pub const fn new(client: reqwest::Client, runtime: tokio::runtime::Handle) -> Self {
        Self { client, runtime }
    }

    /// The underlying `reqwest` client.
    pub const fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Runs `f` on the Tokio runtime. The spawned task is aborted if the
    /// returned future is dropped before it completes.
    pub fn spawn<F, R>(&self, f: F) -> impl Future<Output = anyhow::Result<R>> + Send + 'static
    where
        F: Future<Output = anyhow::Result<R>> + Send + 'static,
        R: Send + 'static,
    {
        let join_handle = self.runtime.spawn(f);
        let abort_handle = join_handle.abort_handle();
        async move {
            let _abort_on_drop = defer(move || abort_handle.abort());
            join_handle.await?
        }
    }
}

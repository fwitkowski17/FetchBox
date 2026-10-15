//! The operations the HTTP layer needs. Implemented by the server's download manager,
//! so `api` depends on neither the database nor the engine.

use async_trait::async_trait;
use dlm_core::{AddRequest, Category, DiskSpace, Download, Event, Id, Result, Settings};
use tokio::sync::broadcast;

/// Download manager as seen by the API.
#[async_trait]
pub trait Service: Send + Sync + 'static {
    /// All downloads, newest first.
    async fn list(&self) -> Result<Vec<Download>>;
    /// Add a download. A collision returns `Error::Conflict` unless the request says how to resolve it.
    async fn add(&self, req: AddRequest) -> Result<Download>;
    async fn pause(&self, id: Id) -> Result<Download>;
    async fn resume(&self, id: Id) -> Result<Download>;
    async fn cancel(&self, id: Id) -> Result<Download>;
    /// Remove from the list (cancels first when still active). Finished files stay on disk.
    async fn remove(&self, id: Id) -> Result<()>;
    async fn set_category(&self, id: Id, category: Category) -> Result<Download>;
    async fn settings(&self) -> Result<Settings>;
    async fn update_settings(&self, settings: Settings) -> Result<Settings>;
    /// Free space where downloads are written.
    async fn disk_space(&self) -> Result<DiskSpace>;
    /// Live updates for SSE.
    fn subscribe(&self) -> broadcast::Receiver<Event>;
}

use std::sync::Arc;

use anyhow::Result;
use graft::protocol::CapabilitySet;
use graft::worker::{SemanticDispatcher, UnsupportedDispatcher};

mod graft_worker_common;

#[tokio::main]
async fn main() -> Result<()> {
    graft_worker_common::run(
        CapabilitySet::new([])?,
        Arc::new(UnsupportedDispatcher) as Arc<dyn SemanticDispatcher>,
    )
    .await
}

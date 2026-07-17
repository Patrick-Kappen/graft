use std::sync::Arc;

use anyhow::Result;
use graft::protocol::{Capability, CapabilitySet};
use graft::worker::{MockDispatcher, SemanticDispatcher};

mod graft_worker_common;

#[tokio::main]
async fn main() -> Result<()> {
    graft_worker_common::run(
        CapabilitySet::new([Capability::Observe])?,
        Arc::new(MockDispatcher) as Arc<dyn SemanticDispatcher>,
    )
    .await
}

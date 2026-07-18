use anyhow::Result;
use graft::protocol::{Capability, CapabilitySet};

mod graft_worker_common;

fn main() -> Result<()> {
    let prepared = graft_worker_common::prepare()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(graft_worker_common::run(
            prepared,
            CapabilitySet::new([Capability::Observe, Capability::Inspect])?,
            None,
        ))
}

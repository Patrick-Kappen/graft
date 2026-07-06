#![deny(clippy::all)]
#![deny(clippy::pedantic)]

use anyhow::Result;

fn main() -> Result<()> {
    graft::cli::run()
}

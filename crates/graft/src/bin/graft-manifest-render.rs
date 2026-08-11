#![deny(clippy::all)]
#![deny(clippy::pedantic)]

//! Internal build helper for canonical worker discovery documents.

use std::{
    fs::{self, File},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context as _, Result};
use clap::Parser;
use graft::manifest::{render, RenderInput, MAX_MANIFEST_BYTES};

#[derive(Debug, Parser)]
#[command(name = "graft-manifest-render")]
struct Arguments {
    /// Read-only JSON preimage produced by Nix materialisation.
    preimage: PathBuf,
    /// New directory that will contain manifest.json and endpoint.json.
    output: PathBuf,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let input = read_input(&arguments.preimage)?;
    let rendered = render(input).context("manifest renderer preimage is invalid")?;

    fs::create_dir(&arguments.output).with_context(|| {
        format!(
            "cannot create manifest renderer output directory {}",
            arguments.output.display()
        )
    })?;
    write_new(
        &arguments.output.join("manifest.json"),
        rendered.manifest_json(),
    )?;
    write_new(
        &arguments.output.join("endpoint.json"),
        rendered.endpoint_json(),
    )?;
    Ok(())
}

fn read_input(path: &Path) -> Result<RenderInput> {
    let file = File::open(path)
        .with_context(|| format!("cannot open manifest renderer preimage {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read manifest renderer preimage {}", path.display()))?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > MAX_MANIFEST_BYTES) {
        bail!("manifest renderer preimage exceeds its size limit");
    }
    serde_json::from_slice(&bytes).context("manifest renderer preimage JSON is invalid")
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create renderer output {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("cannot write renderer output {}", path.display()))
}

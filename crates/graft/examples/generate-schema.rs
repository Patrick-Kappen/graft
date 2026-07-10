use std::error::Error;
use std::io::{self, Write};

use graft::config::schema::ContainerConfig;

fn main() -> Result<(), Box<dyn Error>> {
    let schema = schemars::schema_for!(ContainerConfig);
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());

    serde_json::to_writer_pretty(&mut output, &schema)?;
    writeln!(output)?;

    Ok(())
}

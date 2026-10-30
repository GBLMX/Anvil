use std::{error::Error, fs, path::Path};

use anvil_api::{EditorView, SetModeRequest};
use schemars::schema_for;

fn main() -> Result<(), Box<dyn Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("bindings");
    fs::create_dir_all(&directory)?;

    fs::write(
        directory.join("editor-view.schema.json"),
        serde_json::to_string_pretty(&schema_for!(EditorView))?,
    )?;
    fs::write(
        directory.join("set-mode.schema.json"),
        serde_json::to_string_pretty(&schema_for!(SetModeRequest))?,
    )?;

    Ok(())
}

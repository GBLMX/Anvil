//! Sandboxed Luau runtime primitives.

use mlua::{Lua, Result};

/// Initializes Luau with its sandbox enabled and runs a deterministic check.
pub(crate) fn smoke() -> Result<i32> {
    let lua = Lua::new();
    lua.globals().set("require", mlua::Value::Nil)?;
    lua.sandbox(true)?;
    let value: i32 = lua.load("return 40 + 2").eval()?;
    let io_kind: String = lua.load("return type(io)").eval()?;
    let require_kind: String = lua.load("return type(require)").eval()?;
    if value != 42 || io_kind != "nil" || require_kind != "nil" {
        return Err(mlua::Error::RuntimeError(format!(
            "Luau sandbox smoke check failed: value={value}, io={io_kind}, require={require_kind}"
        )));
    }
    Ok(value)
}

/// Loads and executes one validated Luau entry inside the sandbox.
pub(crate) fn run_file(path: &std::path::Path) -> Result<()> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| mlua::Error::RuntimeError(format!("read plugin: {error}")))?;
    let lua = Lua::new();
    lua.globals().set("require", mlua::Value::Nil)?;
    lua.sandbox(true)?;
    lua.load(source).exec()
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn runs_validated_luau_entry() {
        let path = std::env::temp_dir().join(format!(
            "anvil-plugin-{}-{}.luau",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(&path, "return 42").expect("write");
        let result = run_file(&path);
        let _ = std::fs::remove_file(path);
        result.expect("run plugin");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn luau_sandbox_runs_without_ambient_io() {
        assert_eq!(super::smoke().expect("sandboxed Luau"), 42);
    }
}

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

#[cfg(test)]
mod tests {
    #[test]
    fn luau_sandbox_runs_without_ambient_io() {
        assert_eq!(super::smoke().expect("sandboxed Luau"), 42);
    }
}

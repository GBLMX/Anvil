-- luacheck configuration for Anvil's Lua artifacts.
--
-- The only .lua files in the tree are the generated LuaLS stubs in
-- crates/anvil-api/bindings; plugin runtime code is Luau (.luau) and is not
-- checked by luacheck. This file also marks the project root for omp's
-- `cleanse` checker discovery and lua-language-server's root detection.
std = "lua54"
max_line_length = 120

-- Keep `luacheck .` cheap: skip build output and scratch trees.
exclude_files = {
	".git/**",
	".anvil-tmp/**",
	"target/**",
	"dist/**",
}

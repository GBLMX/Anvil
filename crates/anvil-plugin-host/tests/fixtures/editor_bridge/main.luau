local view = editor.get_view()
local cursor = view.selections[1].head
print("cursor line=" .. cursor.line .. " column=" .. cursor.grapheme_column)

local result = editor.apply_edit({
    buffer_id = 1,
    base_revision = 0,
    edits = {
        { range = { ["start"] = 0, ["end"] = 0 }, text = "x" },
    },
})
print("revision=" .. result.revision)

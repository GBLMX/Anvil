if plugin.command ~= "anvil.trim-trailing-whitespace" then
    return
end

local snapshot = editor.get_snapshot()
local text = snapshot.text
local edits = {}
local line_start = 1

while line_start <= #text + 1 do
    local newline = string.find(text, "\n", line_start, true)
    local line_end = newline and newline - 1 or #text
    local trimmed = line_end

    while trimmed >= line_start do
        local character = string.sub(text, trimmed, trimmed)
        if character ~= " " and character ~= "\t" then
            break
        end
        trimmed = trimmed - 1
    end

    if trimmed < line_end then
        table.insert(edits, {
            range = { start = trimmed, ["end"] = line_end },
            text = "",
        })
    end

    if not newline then
        break
    end
    line_start = newline + 1
end

if #edits == 0 then
    return
end

editor.apply_edit({
    buffer_id = snapshot.buffer_id,
    base_revision = snapshot.revision,
    edits = edits,
})

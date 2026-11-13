function provide(request)
    return {
        request_id = request.request_id,
        items = {
            { label = "alpha", value = "alpha", detail = "fixture", insert_text = "alpha" }
        }
    }
end

function act(request)
    return { request_id = request.request_id, value = request.value }
end

editor.register_provider({
    id = "fixture.completion",
    kind = "completion",
    title = "Fixture completion",
    handler = "provide",
    action_handler = "act"
})

editor.register_provider({
    id = "fixture.picker",
    kind = "picker",
    title = "Fixture picker",
    handler = "provide",
    action_handler = "act"
})

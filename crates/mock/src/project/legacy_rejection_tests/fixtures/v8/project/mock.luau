--!strict
-- External SDL paths are adoption locators; do not overwrite those files.
local config: ChigiriMockProjectConfig = {
    format = "chigiri-mock-project/v8",
    schema = { sources = {
        "../schema.graphql",
    } },
    storage = { sources = { "storage.graphql" } },
    clock = { mode = "system" },
    server = { listen = "127.0.0.1:4000", allowed_origins = {} },
    hooks = {
        request = { before = false, after = false },
        operation = { before = false, after = false },
        field = { before = false, after = false },
    },
    state = { fields = {}, initial = {} },
}
return config

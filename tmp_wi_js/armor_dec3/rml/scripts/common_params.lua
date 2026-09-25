local factors = require("rml.scripts.dynamic_params")

local params = {}

params.MaxHealth = {
    type = "transform",
    name = "UI_hitpoints",
    items = {
        { type = "int", value = "/hull/0/maxHealth" },
        { type = "component", component = "turret", item = { type = "int", value = "/maxHealth" } },
        factors.HealthFactor,
        { type = "float", value = "/miscAttrs/descrAttrs/hull/maxHealth", default = 0.0 },
    },
    units = "UI_hp_format",
    fn = function(v)
        local health = (v[1] + v[4] + v[2]) * v[3]
        return math.floor(health / 10 + 0.5) * 10
    end,
    format = { maximumFractionDigits = 0 },
}

return params

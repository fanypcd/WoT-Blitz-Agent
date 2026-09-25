local battle_count_threshold = 100.0
local typeLabelsLong = {"LT", "MT", "HT", "TD", "SPG"}

local factors = require("rml.scripts.dynamic_params")
local CommonParams = require("rml.scripts.common_params")

-- Parameter definitions
local params = {}

local function calcAimingFactor(
    shot_factor, speed, turret_rotation_speed, hull_rotation_speed, 
    dispersion_factor_vehicle_movement, dispersion_factor_turret_rotation, dispersion_factor_vehicle_rotation
)
    return math.sqrt(1 
        + (speed * dispersion_factor_vehicle_movement) ^ 2 
        + (turret_rotation_speed * dispersion_factor_turret_rotation) ^ 2
        + (hull_rotation_speed * dispersion_factor_vehicle_rotation) ^ 2
        + shot_factor ^ 2
    )
end

local function calcAimingTime(
    aiming_time,
    shot_factor, speed, turret_rotation_speed, hull_rotation_speed, 
    dispersion_factor_vehicle_movement, dispersion_factor_turret_rotation, dispersion_factor_vehicle_rotation
)
    return aiming_time * math.log(calcAimingFactor(
        shot_factor, speed, turret_rotation_speed, hull_rotation_speed,
        dispersion_factor_vehicle_movement, dispersion_factor_turret_rotation, dispersion_factor_vehicle_rotation
    ))
end

local function formatList(values, fmt, fn)
    local parts = {}
    for word in string.gmatch(values, "[^%s]+") do
        local num = tonumber(word) or word
        table.insert(parts, params.format(fn and fn(num) or num, nil, fmt))
    end
    return table.concat(parts, " | ")
end


params.VehicleType = {
    type = "int",
    value = "/type"
}

params.MaxHealth = CommonParams.MaxHealth

params.Weight = {
    type = "transform",
    name = "UI_weight",
    allowUndefined = true,
    items = {
        { type = "int", value = "/hull/0/weight" },
        { type = "component", component = "turret", item = { type = "int", value = "/weight" } },
        { type = "component", component = "gun", item = { type = "int", value = "/weight" } },
        { type = "component", component = "chassis", item = { type = "int", value = "/weight" } },
        { type = "component", component = "engine", item = { type = "int", value = "/weight" } },
        { type = "component", component = "radio", item = { type = "int", value = "/weight" } },
        { type = "component", component = "fueltank", item = { type = "int", value = "/weight" } },
    },
    units = "UI_weight_t_format",
    fn = function(v) 
        local sum = 0
        for i, val in ipairs(v) do
            sum = sum + (val or 0)
        end
        return sum * 0.001
    end,
    className = "mt-2"
}

params.MaxLoad = {
    type = "transform",
    name = "UI_max_load",
    items = {{ type = "component", component = "chassis", item = { type = "float", value = "/maxLoad" } }},
    units = "UI_weight_t_format",
    fn = function(v) return v[1] * 0.001 end,
    filter = function() return platformCode ~= "mirtankov" and platformCode ~= "pc" end
}

params.HullArmorFront = {
    type = "int", name = "UI_front", value = "/hull/0/primaryArmor/front", units = "UI_millimeters_format",
    format = { maximumFractionDigits = 0 },
}

params.HullArmorSide = {
    type = "int", name = "UI_side", value = "/hull/0/primaryArmor/side", weight = 0.9, units = "UI_millimeters_format",
    format = { maximumFractionDigits = 0 },
}

params.HullArmorBack = {
    type = "int", name = "UI_back", value = "/hull/0/primaryArmor/back", weight = 0.25, units = "UI_millimeters_format",
    format = { maximumFractionDigits = 0 },
}

params.HullArmorComposite = {
    type = "composite",
    name = "UI_hull_armor",
    items = {params.HullArmorFront, params.HullArmorSide, params.HullArmorBack}
}

params.HullArmorGroup = {
    type = "group",
    value = { name = "UI_hull_armor", className = "mt-2" },
    items = {params.HullArmorFront, params.HullArmorSide, params.HullArmorBack}
}

params.TurretArmorFront = {
    type = "component", name = "UI_front", component = "turret", item = { type = "int", value = "/primaryArmor/front", units = "UI_millimeters_format", format = { maximumFractionDigits = 0 } } }

params.TurretArmorSide = { type = "component", name = "UI_side", component = "turret", item = { type = "int", value = "/primaryArmor/side", weight = 0.9, units = "UI_millimeters_format", format = { maximumFractionDigits = 0 } } }

params.TurretArmorBack = { type = "component", name = "UI_back", component = "turret", item = { type = "int", value = "/primaryArmor/back", weight = 0.25, units = "UI_millimeters_format", format = { maximumFractionDigits = 0 } } }

params.TurretArmorComposite = {
    type = "composite",
    name = "UI_turret_armor",
    items = {params.TurretArmorFront, params.TurretArmorSide, params.TurretArmorBack},
}

params.TurretArmorGroup = {
    type = "group",
    value = { name = "UI_turret_armor", className = "mt-2" },
    items = {params.TurretArmorFront, params.TurretArmorSide, params.TurretArmorBack}
}

params.TurretRotationSpeed = { 
    type = "transform",
    name = "UI_turret_traverse", 
    items = {
        {
            type = "component", 
            component = "turret", 
            item = { type = "float", value = "/rotationSpeed" }
        },
        factors.SkillFactor_gunner,
        factors.TurretRotationSpeedFactor,
        factors.SkillFactor_quickAiming,
        factors.TurretRotationSpeedIncrease,
        { type = "float", value = "/miscAttrs/descrAttrs/turret/rotationSpeedDegrees", default = 0.0 },
    },
    fn = function (v)
        return (v[1] + v[6]) * v[2] * v[3] * (1.0 + v[4] * 0.025) * (1.0 + v[5] * 0.01)
    end,
    units = "UI_degrees_per_second_format"
}

params.SpeedLimitsForward = {
    type = "transform",
    name = "UI_forward_speed",
    items = {
        { type = "component", component = "chassis", item = { type = "float", value = "/speedLimits/forward" } },
        factors.ForwardMaxSpeedKMHTerm,
        factors.SkillFactor_motorExpert,
        factors.FwdSpeedLimitBias,
        factors.ForwardMaxSpeedFactor,
    },
    fn = function(v) return (v[1] + v[2] + v[3] + v[4]) * v[5] end,
    units = "UI_kmh_format", 
    format = { maximumFractionDigits = 1 }
}

params.SpeedLimitsBackward = {
    type = "transform",
    name = "UI_backward_speed",
    items = {
        { type = "component", component = "chassis", item = { type = "float", value = "/speedLimits/backward" } },
        factors.BackwardMaxSpeedKMHTerm,
        factors.SkillFactor_motorExpert,
        factors.BkwdSpeedLimitBias,
        factors.BackwardMaxSpeedFactor,
    },
    fn = function(v) return (v[1] + v[2] + v[3] + v[4]) * v[5] end,
    units = "UI_kmh_format", 
    format = { maximumFractionDigits = 1 }
}

params.SpeedLimitsComposite = {
    type = "composite",
    name = "UI_speedlimits",
    items = {params.SpeedLimitsForward, params.SpeedLimitsBackward}
}

params.EnginePowerBase = { type = "component", component = "engine", item = { type = "float", value = "/power" } }
params.EnginePower = { 
    type = "transform",
    name = "UI_engine_power", 
    items = {
        params.EnginePowerBase,
        params.VehicleType,
        factors.EnginePowerFactors_lightTank,
        factors.EnginePowerFactors_mediumTank,
        factors.EnginePowerFactors_heavyTank,
        factors.EnginePowerFactors_ATSPG,
        factors.EnginePowerFactors_SPG,
        factors.EnginePowerFactor,
        factors.EnginePowerIncrease,
        { type = "float", value = "/miscAttrs/descrAttrs/engine/power", default = 0.0 },
    },
    fn = function(v) return (v[1] + v[10]) * v[3 + v[2]] * v[8] * (1.0 + v[9] * 0.01) end,
    units = "UI_hoursepower_format", 
    format = { maximumFractionDigits = 0 }
}


params.PowerToWeight = {
    type = "transform",
    name = "UI_engine_power_weight",
    items = {params.EnginePower, params.Weight},
    units = "UI_hoursepower_t_format",
    fn = function(v) return v[1] / v[2] end,
    format = { maximumFractionDigits = 1 }
}

params.RammingPotential = {
    type = "transform",
    name = "UI_ramming_potential",
    items = {params.SpeedLimitsForward, params.Weight, factors.RammingFactorMisc},
    fn = function(v)
        return v[1] * v[2] * v[3]
    end,
    format = { maximumFractionDigits = 0 },
}

params.RollingFrictionHardBase = { type = "component", component = "chassis", item = { type = "float", value = "/rollingFriction/hard", weight = -1.0 } }
params.RollingFrictionHard = {
    type = "transform",
    items = {
        params.RollingFrictionHardBase,
        factors.SkillFactor_driver,
        factors.FirmGroundPassabilityIncrease,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        return v[1] / (v[2] * v[3]) * v[4]
    end,
}

params.RollingFrictionMediumBase = { type = "component", component = "chassis", item = { type = "float", value = "/rollingFriction/medium", weight = -1.0 } }
params.RollingFrictionMedium = {
    type = "transform",
    items = {
        params.RollingFrictionMediumBase,
        factors.SkillFactor_driver,
        factors.MediumGroundPassabilityIncrease,
        factors.SkillFactor_badRoadsKing,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        return v[1] / (v[2] * v[3]) * (1.0 - v[4] * (platformCode == "pc" and 0.05 or 0.025)) * v[5]
    end,
}

params.RollingFrictionSoftBase = { type = "component", component = "chassis", item = { type = "float", value = "/rollingFriction/soft", weight = -1.0 } }
params.RollingFrictionSoft = {
    type = "transform",
    items = {
        params.RollingFrictionSoftBase,
        factors.SkillFactor_driver,
        factors.SoftGroundPassabilityIncrease,
        factors.SkillFactor_badRoadsKing,
        params.RollingFrictionMedium,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        if platformCode == "pc" and v[4] > 0.0 then return v[5] end
        return v[1] / (v[2] * v[3]) * (1.0 - v[4] * 0.1) * v[6]
    end,
}

params.TerrainResistanceHardBase = { type = "component", name = "UI_specs_on_terrain_hard", component = "chassis", item = { type = "float", value = "/terrainResistance/hard", weight = -1.0 } }
params.TerrainResistanceHard = {
    type = "transform",
    name = "UI_specs_on_terrain_hard",
    items = {
        params.TerrainResistanceHardBase,
        factors.SkillFactor_driver,
        factors.FirmGroundPassabilityIncrease,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        return v[1] / (v[2] * v[3]) * v[4]
    end,
}

params.TerrainResistanceMediumBase = { type = "component", component = "chassis", item = { type = "float", value = "/terrainResistance/medium", weight = -1.0 } }
params.TerrainResistanceMedium = {
    type = "transform",
    name = "UI_specs_on_terrain_medium",
    items = {
        params.TerrainResistanceMediumBase,
        factors.SkillFactor_driver,
        factors.MediumGroundPassabilityIncrease,
        factors.SkillFactor_badRoadsKing,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        return v[1] / (v[2] * v[3]) * (1.0 - v[4] * (platformCode == "pc" and 0.05 or 0.025)) * v[5]
    end,
}

params.TerrainResistanceSoftBase = { type = "component", component = "chassis", item = { type = "float", value = "/terrainResistance/soft", weight = -1.0 } }
params.TerrainResistanceSoft = {
    type = "transform",
    name = "UI_specs_on_terrain_soft",
    items = {
        params.TerrainResistanceSoftBase,
        factors.SkillFactor_driver,
        factors.SoftGroundPassabilityIncrease,
        factors.SkillFactor_badRoadsKing,
        params.TerrainResistanceMedium,
        factors.RollingFrictionFactor,
    },
    fn = function(v)
        if platformCode == "pc" and v[4] > 0.0 then return v[5] end
        return v[1] / (v[2] * v[3]) * (1.0 - v[4] * 0.1) * v[6]
    end,
}

params.TerrainResistanceComposite = {
    type = "composite",
    name = "UI_terrain_resistance",
    items = {params.TerrainResistanceHard, params.TerrainResistanceMedium, params.TerrainResistanceSoft},
    className = 'mt-2',
}

params.RollingFrictionComposite = {
    type = "transform",
    name = "UI_rolling_friction",
    items = {
        params.TerrainResistanceHard, params.TerrainResistanceMedium, params.TerrainResistanceSoft,
        params.RollingFrictionHard, params.RollingFrictionMedium, params.RollingFrictionSoft,
        {
          type = "composite",
          items = {params.RollingFrictionHard, params.RollingFrictionMedium, params.RollingFrictionSoft},
        },
    },
    fn = function(v) return (math.abs(v[1] - v[4]) > 0.01 or math.abs(v[2] - v[5]) > 0.01 or math.abs(v[3] - v[6]) > 0.01) and v[7] end
}

params.HullTraverseFactorTerrain = {
    type = "transform",
    items = {
        params.RollingFrictionHardBase,
        params.RollingFrictionMediumBase,
        params.RollingFrictionSoftBase,
        params.RollingFrictionHard,
        params.RollingFrictionMedium,
        params.RollingFrictionSoft,
    },
    default = 1.0,
    fn = function(v)
        return 3.0 / (v[4] / v[1] + v[5] / v[2] + v[6] / v[3])
    end,
}

params.ChassisRotationSpeedBase = {
    type = "component",
    component = "chassis",
    item = {
        type = "float",
        value = "/rotationSpeed",
    }
}

params.HullTraverse = {
    type = "transform",
    name = "UI_hull_traverse",
    items = {
        params.ChassisRotationSpeedBase,
        factors.RotationSpeedFactor,
        factors.SkillFactor_virtuoso,
        factors.ChassisRotationFactor,
        { type = "float", value = "/miscAttrs/descrAttrs/chassis/rotationSpeedDegrees", default = 0.0 },
        { type = "float", value = "/miscAttrs/rechargeableNitro/addRotationSpeedBonus", default = 0.0 },
    },
    units = "UI_degrees_per_second_format",
    format = { maximumFractionDigits = 1 },
    fn = function(v)
        return (v[1] + v[5] + v[6]) * v[2] * (1.0 + v[3]) / v[4]
    end,
    className = 'mt-2',
}

params.AverageHullTraverse = {
    type = "transform",
    name = "UI_hull_traverse",
    items = {
        params.HullTraverse,
        params.HullTraverseFactorTerrain,
        params.EnginePower,
        params.EnginePowerBase,
    },
    units = "UI_degrees_per_second_format",
    format = { maximumFractionDigits = 1 },
    fn = function(v)
        return v[1] * v[2] * v[3] / v[4]
    end,
    className = 'mt-2',
}

params.EffectiveSpeedLimitHard = {
    type = "transform",
    name = "UI_specs_on_terrain_hard",
    items = {
        params.SpeedLimitsForward,
        params.PowerToWeight,
        params.RollingFrictionHard,
    },
    units = "UI_kmh_format",
    fn = function(v) return math.min(v[1], v[2] / (12.25 * math.abs(v[3]) * 0.0805) * 3.6) end,
    format = { maximumFractionDigits = 1 },
}

params.EffectiveSpeedLimitMedium = {
    type = "transform",
    name = "UI_specs_on_terrain_medium",
    items = {
        params.SpeedLimitsForward,
        params.PowerToWeight,
        params.RollingFrictionMedium,
    },
    units = "UI_kmh_format",
    fn = function(v) return math.min(v[1], v[2] / (12.25 * math.abs(v[3]) * 0.0805) * 3.6) end,
    format = { maximumFractionDigits = 1 },
}

params.EffectiveSpeedLimitSoft = {
    type = "transform",
    name = "UI_specs_on_terrain_soft",
    items = {
        params.SpeedLimitsForward,
        params.PowerToWeight,
        params.RollingFrictionSoft,
    },
    units = "UI_kmh_format",
    fn = function(v) return math.min(v[1], v[2] / (12.25 * math.abs(v[3]) * 0.0805) * 3.6) end,
    format = { maximumFractionDigits = 1 },
}

params.EffectiveTraverseHard = {
    type = "transform",
    name = "UI_specs_on_terrain_hard",
    items = {
        params.HullTraverse,
        { type = "float", value = "/stock/weight" },
        params.Weight,
        params.EnginePower,
        { type = "float", value = "/engine/stock/power" },
        params.RollingFrictionHardBase,
        params.RollingFrictionHard,
    },
    fn = function(v)
        local wr = v[2] / (1000.0 * v[3]);
        local ep = v[4] / v[5];
        local tr = v[6] / v[7];
        return v[1] * wr * ep * tr * 0.9714
    end,
    units = "UI_degrees_per_second_format",
    format = { maximumFractionDigits = 1 },
}

params.EffectiveTraverseMedium = {
    type = "transform",
    name = "UI_specs_on_terrain_medium",
    items = {
        params.HullTraverse,
        { type = "float", value = "/stock/weight" },
        params.Weight,
        params.EnginePower,
        { type = "float", value = "/engine/stock/power" },
        params.RollingFrictionHardBase,
        params.RollingFrictionMedium,
    },
    fn = function(v)
        local wr = v[2] / (1000.0 * v[3]);
        local ep = v[4] / v[5];
        local tr = v[6] / v[7];
        return v[1] * wr * ep * tr * 0.9714
    end,
    units = "UI_degrees_per_second_format",
    format = { maximumFractionDigits = 1 },
}

params.EffectiveTraverseSoft = {
    type = "transform",
    name = "UI_specs_on_terrain_soft",
    items = {
        params.HullTraverse,
        { type = "float", value = "/stock/weight" },
        params.Weight,
        params.EnginePower,
        { type = "float", value = "/engine/stock/power" },
        params.RollingFrictionHardBase,
        params.RollingFrictionSoft,
    },
    fn = function(v)
        local wr = v[2] / (1000.0 * v[3]);
        local ep = v[4] / v[5];
        local tr = v[6] / v[7];
        return v[1] * wr * ep * tr * 0.9714
    end,
    units = "UI_degrees_per_second_format",
    format = { maximumFractionDigits = 1 },
}

params.InvisibilityAtShot = {
    type = "transform",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/invisibility/atShot" } },
        factors.InvisibilityFactorAtShot,
        { type = "float", value = "/miscAttrs/descrAttrs/gun/invisibilityFactorAtShot", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] * v[3] end,
    units = "UI_percent_format",
}

params.InvisibilityStill = {
    type = "transform",
    name = "UI_camo_still",
    items = {
        {
            type = "float",
            value = "/invisibility/still"
        },
        params.VehicleType,
        factors.InvisibilityAdditiveTerm_lightTank,
        factors.InvisibilityAdditiveTerm_mediumTank,
        factors.InvisibilityAdditiveTerm_heavyTank,
        factors.InvisibilityAdditiveTerm_ATSPG,
        factors.InvisibilityAdditiveTerm_SPG,
        factors.InvisibilityAdditiveTermStillFactor,
        factors.InvisibilityAdditiveTerm,
        factors.InvisibilityAdditiveTermStill,
        factors.SkillFactor_camouflage,
        factors.InvisibilityBaseMultiplicativeTerm,
        factors.InvisibilityCamoPaint,
    },
    fn = function(v) return (v[1] * v[11] * v[12] + v[3 + v[2]] * v[8] + math.max(v[9], v[10]) + v[13]) * 100.0 end,
    units = "UI_percent_format"
}

params.InvisibilityMoving = {
    type = "transform",
    name = "UI_camo_moving",
    items = {
        {
            type = "float",
            value = "/invisibility/moving"
        },
        params.VehicleType,
        factors.InvisibilityAdditiveTerm_lightTank,
        factors.InvisibilityAdditiveTerm_mediumTank,
        factors.InvisibilityAdditiveTerm_heavyTank,
        factors.InvisibilityAdditiveTerm_ATSPG,
        factors.InvisibilityAdditiveTerm_SPG,
        factors.InvisibilityAdditiveTerm,
        factors.SkillFactor_camouflage,
        factors.InvisibilityBaseMultiplicativeTerm,
        factors.InvisibilityCamoPaint,
    },
    fn = function(v) return (v[1] * v[9] * v[10] + v[3 + v[2]] + v[8] + v[11]) * 100.0 end,
    units = "UI_percent_format"
}

params.InvisibilityStillAtShot = {
    type = "transform",
    name = "UI_camo_still_upon_firing",
    items = {params.InvisibilityStill, params.InvisibilityAtShot},
    fn = function(v) return v[1] * v[2] end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.InvisibilityMovingAtShot = {
    type = "transform",
    name = "UI_camo_moving_upon_firing",
    items = {params.InvisibilityMoving, params.InvisibilityAtShot},
    fn = function(v) return v[1] * v[2] end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.InvisibilityStillComposite = {
    type = "composite",
    name = "UI_camo_still_composite",
    items = {params.InvisibilityStill, params.InvisibilityStillAtShot},
}

params.ReloadTimeRegularBase = {
    type = "transform",
    name = "UI_reload_time",
    items = {
        {
            type = "component",
            component = "gun",
            item = {
                type = "float",
                value = "/reloadTime",
                weight = -1.0,
            }
        },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/reloadTime", default = 0.0, weight = -1.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format"
}

params.ReloadTimeRegular = {
    type = "transform",
    items = {
        params.ReloadTimeRegularBase,
        factors.ReloadTimeFactor,
        factors.SkillFactor_desperado,
        factors.SkillFactor_melee,
        factors.GunReloadSpeedIncrease,
        factors.SkillFactor_secondChance,
        { type = "component", component = "gun", item = { type = "int", value = "/clipCount" } },
        factors.SkillFactor_magMastery,
    },
    fn = function(v) 
      local clipReloadFactor = v[7] > 1 and 1.0 - v[8] * 0.025 or 1.0
      local base = v[1] * v[2] * (1.0 - v[3] * (platformCode == "pc" and 0.05 or 0.1)) * (1.0 - v[4] * 0.025) * (1.0 - v[5] * 0.01) * (1.0 - v[6] * 0.025) * clipReloadFactor
      return base
    end
}

params.ReloadTimeAutoloader = {
    type = "transform",
    name = "UI_reload_times",
    allowUndefined = true,
    items = {
        {
            type = "component",
            component = "gun",
            item = {
                type = "string",
                value = "/autoreload/reloadTime",
                weight = function(v)
                    local sum = 0
                    for part in string.gmatch(v, "[^%s]+") do
                        sum = sum + tonumber(part)
                    end
                    return -sum
                end
            }
        },
        factors.ReloadTimeFactor,
        factors.SkillFactor_desperado,
        factors.SkillFactor_melee,
        factors.GunReloadSpeedIncrease,
        {
            type = "component",
            component = "gun",
            item = {
                type = "string",
                value = "/dualGun/reloadTimes",
                weight = function(v)
                    local sum = 0
                    for part in string.gmatch(v, "[^%s]+") do
                        sum = sum + tonumber(part)
                    end
                    return -sum
                end
            }
        },
    },
    fn = function(v) 
        local values = v[1] or v[6]
        if not values then
            return
        end

        if type(values) ~= 'string' then
            return values * v[2] * (1.0 - v[3] * (platformCode == "pc" and 0.05 or 0.1)) * (1.0 - v[4] * 0.025) * (1.0 - v[5] * 0.01)
        end
    
        local parts = {}
        for word in string.gmatch(values, "[^%s]+") do
            local num = tonumber(word)
            table.insert(parts, params.format(num * v[2] * (1.0 - v[3] * (platformCode == "pc" and 0.05 or 0.1)) * (1.0 - v[4] * 0.025) * (1.0 - v[5] * 0.01)))
        end
    
        return table.concat(parts, " | ")
    end,
    units = "UI_seconds_format"
}

params.ReloadTimeAutoloaderMin = {
    type = "transform",
    items = {
        {
            type = "component",
            component = "gun",
            item = {
                type = "string",
                value = "/autoreload/reloadTime",
                weight = function(v)
                    local minValue = math.huge
                    for p in string.gmatch(v, "[^%s]+") do
                        local num = tonumber(p)
                        if num and num < minValue then
                            minValue = num
                        end
                    end
                    return minValue ~= math.huge and -minValue or nil
                end
            }
        },
        factors.ReloadTimeFactor,
        factors.SkillFactor_desperado,
        factors.SkillFactor_melee,
        factors.GunReloadSpeedIncrease,
    },
    fn = function(v)
        if type(v[1]) == "string" then
            local minValue = math.huge
            for p in string.gmatch(v[1], "[^%s]+") do
                local num = tonumber(p)
                if num and num < minValue then
                    minValue = num
                end
            end
            return minValue ~= math.huge and minValue * v[2] * (1.0 - v[3] * (platformCode == "pc" and 0.05 or 0.1)) * (1.0 - v[4] * 0.025) * (1.0 - v[5] * 0.01)
        end
        return v[1]
    end,
    units = "UI_seconds_format"
}

params.ReloadTime = {
    type = "transform",
    name = "UI_reload_time",
    allowUndefined = true,
    items = {
        params.ReloadTimeRegular,
        params.ReloadTimeAutoloader,
    },
    fn = function(v) return v[2] and v[2] or v[1] end,
    units = "UI_seconds_format"
}

params.ShellChangeTime = {
    type = "transform",
    name = "UI_shell_change_time",
    items = {
        params.ReloadTimeRegular,
        factors.SkillFactor_intuition,
    },
    fn = function(v) return v[2] ~= 0 and v[1] * (1.0 + v[2]) end,
    units = "UI_seconds_format"
}

params.ClipCount = {
    type = "transform",
    name = "UI_clip_count",
    items = {
        { type = "component", component = "gun", item = { type = "int", value = "/clipCount" } },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/clip/0", default = 0.0 },
    },
    fn = function(v) return v[1] > 1 and v[1] + v[2] or nil end
}

params.InterClipReload = {
    type = "transform",
    name = "UI_clip_rate",
    allowUndefined = true,
    items = {
        { type = "component", component = "gun", item = { type = "int", value = "/clipRate", weight = -1.0 } },
        factors.ClipReloadTimeFactor,
        { type = "component", component = "gun", item = { type = "float", value = "/dualGun/rateTime", weight = -1.0 } },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/clip/1", default = 0.0, weight = -1.0 },
    },
    units = "UI_seconds_format",
    fn = function(v) 
        local time = v[1] and math.abs(v[1]) > 1 and 60.0 / (v[1] + v[4]) or v[3]
        return time and time * v[2]
    end
}

params.Burst = {
    type = "transform",
    name = "UI_burst",
    items = {
        { type = "component", component = "gun", item = { type = "int", value = "/burstCount" } },
    },
    fn = function(v) return v[1] > 1 and v[1] or nil end
}

params.ClipBurstTime = {
    type = "transform",
    name = "UI_clip_time",
    items = {
        params.ClipCount,
        params.InterClipReload,
        -- params.Burst,
    },
    units = "UI_seconds_format",
    fn = function(v) return v[2] * (v[1] - 1) end
}

params.InterBurstReload = {
    type = "transform",
    name = "UI_burst_rate",
    items = {
        { type = "component", component = "gun", item = { type = "int", value = "/burstRate", weight = -1.0 } },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/burst/1", default = 0.0, weight = -1.0 },
    },
    units = "UI_seconds_format",
    fn = function(v) return v[1] and math.abs(v[1]) > 1 and 60.0 / (v[1] + v[2]) or nil end
}

params.AutoShootInterval = {
    type = "transform",
    allowUndefined = true,
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/autoShoot/shotInterval", weight = -1.0 } },
        params.InterClipReload,
    },
    fn = function(v) return v[1] and v[1] ~= 0.0 and v[1] or v[2] end
}
params.AutoShootMaxDispersion = { type = "component", component = "gun", item = { type = "float", value = "/autoShoot/maxShotDispersion", weight = -1.0 } }
params.AutoShootShotDispersionPerSec = { type = "component", component = "gun", item = { type = "float", value = "/autoShoot/shotDispersionPerSec", weight = -1.0 } }
params.AutoShootShotDispersionPerShot = {
    type = "transform",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/autoShoot/shotDispersionPerShot", weight = -1.0 } },
        factors.AutoShootShotDispersionPerShot
    },
    fn = function(v) return v[1] * v[2] end
}
params.AutoShootAimingDelay = { type = "component", name = "UI_aiming_delay", component = "gun", item = { type = "float", value = "/autoShoot/aimingDelay", weight = -1.0 } }

params.TemperatureMechanicsTotalHeatingTime = { 
    type = "component", 
    name = "UI_heating_time",
    component = "gun", 
    item = { type = "float", value = "/temperature/totalHeatingTime", units = "UI_seconds_format" },
}
params.TemperatureMechanicsCoolingDelay = {
    type = "transform",
    name = "UI_cooling_delay",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/temperature/coolingDelay", weight = -1 } },
        { type = "float", value = "/miscAttrs/temperatureGun/coolingDelay", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
}
params.TemperatureMechanicsCoolingTime = { 
    type = "transform",
    name = "UI_cooling_time",
    items = {
        {
            type = "component",
            component = "gun", 
            item = { type = "float", value = "/temperature/coolingTime", units = "UI_seconds_format", weight = -1 },
        },
        factors.TemperatureGunCoolingPerSec,
        { type = "float", value = "/miscAttrs/temperatureGun/coolingPerSec", default = 1.0 },
    },
    fn = function(v) return v[1] / (v[2] * v[3]) end,
}
params.TemperatureMechanicsAvgDamageModifier = { 
    type = "component", 
    component = "gun", 
    item = { type = "float", value = "/temperature/avgDamageModifier" }
}
params.TemperatureMechanicsCoolingTimeOverheat = {
    type = "transform",
    name = "UI_cooling_overheat_time",
    items = {
        {
            type = "component",
            component = "gun", 
            item = { type = "float", value = "/temperature/coolingTimeOverheat", units = "UI_seconds_format", weight = -1 },
        },
        factors.TemperatureGunCoolingPerSec,
        { type = "float", value = "/miscAttrs/temperatureGun/coolingPerSec", default = 1.0 },
    },
    fn = function(v) return v[1] / (v[2] * v[3]) end,
}
params.TemperatureMechanicsRateOfFire = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.AutoShootInterval,
        params.TemperatureMechanicsTotalHeatingTime,
        params.TemperatureMechanicsCoolingTime,
        params.TemperatureMechanicsCoolingDelay,
        params.TemperatureMechanicsAvgDamageModifier,
        params.TemperatureMechanicsCoolingTimeOverheat,
    },
    fn = function(v)
        local shotInterval = v[1]
        if not v[6] or not shotInterval then
            return
        end
        local shotsPerCycle = math.abs(v[2] / shotInterval)
        local cyclesPerMinute = 60.0 / (v[2] + math.abs(v[6] + v[4]))
        return shotsPerCycle * v[5] * cyclesPerMinute
    end,
}

params.RateOfFire = {
    type = "transform",
    name = "UI_rate_of_fire",
    allowUndefined = true,
    items = {
        params.ReloadTimeRegular,
        params.ReloadTimeAutoloaderMin,
        params.ClipCount,
        params.InterClipReload,
        params.Burst,
        params.InterBurstReload,
        params.TemperatureMechanicsRateOfFire,
    },
    fn = function(v)
        if v[7] then
            return v[7]
        end

        local reloadTime = math.abs(v[2] and v[2] or v[1])  -- abs to properly calculate weight
        local shotsMade = 1
        local shotsTime = 0.0
        local interClipReload = v[4] and math.abs(v[4])

        if v[3] and v[3] > 1 and v[4] and interClipReload > 0.0 then
            if not v[2] then
                shotsMade = shotsMade + (v[3] - 1)
                local interBurstReload = v[6] and math.abs(v[6])
                if v[5] and v[5] > 1 and v[6] and interBurstReload > 0.0 then
                    local burstGroup = v[3] / v[5]
                    shotsTime = shotsTime + interBurstReload * (v[5] - 1) * burstGroup + (burstGroup - 1) * interClipReload
                else
                    shotsTime = shotsTime + (v[3] - 1) * interClipReload
                end
            elseif platformCode == "blitz" or platformCode == "tanksblitz" then
                reloadTime = reloadTime + interClipReload
            end
        end

        shotsTime = shotsTime + reloadTime
        return shotsMade / shotsTime * 60.0
    end,
    units = "UI_per_minute_format"
}

params.AimingTime = {
    type = "transform",
    name = "UI_aiming_time",
    items = {
        {
            type = "component",
            component = "gun",
            item = { type = "float", value = "/aimingTime", weight = -1.0 }
        },
        factors.AimingTimeFactor,
        factors.SkillFactor_coordination,
        factors.SkillFactor_quickAiming,
        { type = "float", value = "/miscAttrs/descrAttrs/gun/aimingTime", default = 0.0, weight = -1.0 },
    },
    fn = function(v) return (v[1] + v[5]) * v[2] * (1.0 + v[3]) * (1.0 - v[4] * 0.025) end,
    units = "UI_seconds_format",
    className = "mt-2"
}

params.ShotDispersionRadius = {
    type = "transform",
    name = "UI_dispersion",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/shotDispersionRadius", weight = -1.0 } },
        factors.ShotDispersionAngleFactor,
        factors.MultShotDispersionFactor,
        factors.SkillFactor_armorer,
        factors.SkillFactor_focus,
        { type = "float", value = "/miscAttrs/descrAttrs/gun/shotDispersionRadius", default = 0.0, weight = -1.0 },
    },
    fn = function(v) return (v[1] + v[6]) * v[2] * v[3] * (1.0 - v[4] * 0.015) * (1.0 + v[5]) end,
    units = "UI_meters_format"
}

params.ShotDispersionFactorsAfterShotBase = { type = "component", component = "gun", item = { type = "float", value = "/shotDispersionFactors/afterShot", weight = -1.0 } }
params.ShotDispersionFactorsVehicleRotationBase = {
    type = "transform",
    items = {
        { type = "component", component = "chassis", item = { type = "float", value = "/shotDispersionFactors/vehicleRotation", weight = -1.0 } },
        { type = "float", value = "/miscAttrs/descrAttrs/chassis/shotDispersionFactors", default = 1.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/chassis/shotDispersionFactors/1", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] * v[3] end,
}
params.ShotDispersionFactorsVehicleMovementBase = {
    type = "transform",
    items = {
        { type = "component", component = "chassis", item = { type = "float", value = "/shotDispersionFactors/vehicleMovement", weight = -1.0 } },
        { type = "float", value = "/miscAttrs/descrAttrs/chassis/shotDispersionFactors", default = 1.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/chassis/shotDispersionFactors/0", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] * v[3] end,
}
params.ShotDispersionFactorsTurretRotationBase = { type = "component", component = "gun", item = { type = "float", value = "/shotDispersionFactors/turretRotation", units = "UI_meters_format", weight = -1.0 } }

params.ShotDispersionFactorsAfterShot = {
    type = "transform",
    name = "UI_dispersion_after_shot",
    allowUndefined = true,
    items = {
        params.ShotDispersionFactorsAfterShotBase,
        factors.AdditiveShotDispersionFactor,
        factors.ShotDispersionFactorAfterShot,
        params.AutoShootShotDispersionPerShot,
    },
    fn = function(v)
        return (v[4] and v[4] or v[1]) * v[2] * v[3]
    end,
}

params.ShotDispersionFactorsVehicleRotation = {
    type = "transform",
    name = "UI_hull_traverse",
    items = {
        params.ShotDispersionFactorsVehicleRotationBase,
        factors.AdditiveShotDispersionFactor,
        factors.ShotDispersionFactorVehicleRotation,
    },
    fn = function(v)
        return v[1] * v[2] * v[3]
    end,
}

params.ShotDispersionFactorsVehicleMovement = {
    type = "transform",
    name = "UI_dispersion_movement",
    items = {
        params.ShotDispersionFactorsVehicleMovementBase,
        factors.AdditiveShotDispersionFactor,
        factors.ShotDispersionFactorMovement,
        factors.SkillFactor_smoothDriving,
    },
    fn = function(v)
        return v[1] * v[2] * v[3] * (1.0 + v[4])
    end,
}

params.ShotDispersionFactorsTurretRotation = {
    type = "transform",
    name = "UI_dispersion_turret_rotation",
    items = {
        params.ShotDispersionFactorsTurretRotationBase,
        factors.AdditiveShotDispersionFactor,
        factors.ShotDispersionFactorTurretRotation,
        factors.SkillFactor_smoothTurret,
    },
    fn = function(v)
        return v[1] * v[2] * v[3] * (1.0 + v[4])
    end,
}

params.ShotDispersionFactorsWhileGunDamagedBase = { 
    type = "component", 
    component = "gun", 
    item = { 
        type = "float", 
        value = "/shotDispersionFactors/whileGunDamaged", 
        units = "UI_factor_format", 
        weight = -1.0 
    } 
}

params.ShotDispersionFactorsWhileGunDamaged = {
    type = "transform",
    name = "UI_dispersion_gun_damaged", 
    items = {
        params.ShotDispersionFactorsWhileGunDamagedBase,
        factors.SkillFactor_gunsmith,
        factors.ShotDispersionWhileGunDamagedFactor,
    },
    fn = function(v) return v[1] * (1.0 + v[2]) * v[3] end
}

params.AimingTimeAfterShot = {
    type = "transform",
    name = "UI_dispersion_after_shot", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsAfterShot,
    },
    fn = function(v) return calcAimingTime(v[1], v[2], 0, 0, 0, 0, 0, 0) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format"
}

params.AimingTimeMaxSpeed = {
    type = "transform",
    name = "UI_dispersion_movement", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleMovement,
        params.SpeedLimitsForward,
    },
    fn = function(v) return calcAimingTime(v[1], 0, v[3], 0, 0, v[2], 0, 0) end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 2 },
    className = "mt-2"
}

params.AimingTimeVehicleRotation = {
    type = "transform",
    name = "UI_dispersion_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleRotation,
        params.EffectiveTraverseMedium,
    },
    fn = function(v) return calcAimingTime(v[1], 0, 0, 0, v[3], 0, 0, v[2]) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format",
    className = "mt-2"
}

params.AimingTimeTurretRotation = {
    type = "transform",
    name = "UI_dispersion_turret_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsTurretRotation,
        params.TurretRotationSpeed,
    },
    fn = function(v) return calcAimingTime(v[1], 0, 0, v[3], 0, 0, v[2], 0) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format"
}

params.AimingTimeMaxSpeedTurretRotation = {
    type = "transform",
    name = "UI_dispersion_turret_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleMovement,
        params.SpeedLimitsForward,
        params.ShotDispersionFactorsTurretRotation,
        params.TurretRotationSpeed,
    },
    fn = function(v) return calcAimingTime(v[1], 0, v[3], v[5], 0, v[2], v[4], 0) end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 2 },
}

params.AimingTimeVehicleTurretRotation = {
    type = "transform",
    name = "UI_hull_turret_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleRotation,
        params.EffectiveTraverseMedium,
        params.ShotDispersionFactorsTurretRotation,
        params.TurretRotationSpeed,
    },
    fn = function(v) return calcAimingTime(v[1], 0, 0, v[5], v[3], 0, v[4], v[2]) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format"
}

params.AimingTimeMaxSpeedVehicleTurretRotation = {
    type = "transform",
    name = "UI_hull_turret_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleMovement,
        params.SpeedLimitsForward,
        params.ShotDispersionFactorsVehicleRotation,
        params.EffectiveTraverseMedium,
        params.ShotDispersionFactorsTurretRotation,
        params.TurretRotationSpeed,
    },
    fn = function(v) return calcAimingTime(v[1], 0, v[3], v[7], v[5], v[2], v[6], v[4]) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format"
}

params.AimingTimeMaxSpeedVehicleRotation = {
    type = "transform",
    name = "UI_dispersion_rotation", 
    items = {
        params.AimingTime,
        params.ShotDispersionFactorsVehicleMovement,
        params.SpeedLimitsForward,
        params.ShotDispersionFactorsVehicleRotation,
        params.EffectiveTraverseMedium,
    },
    fn = function(v) return calcAimingTime(v[1], 0, v[3], 0, v[5], v[2], 0, v[4]) end,
    format = { maximumFractionDigits = 2 },
    units = "UI_seconds_format"
}


params.ShellName = { type = "shell", item = { type = "string", value = "/localizedName" } }
params.ShellKind = { type = "shell", item = { type = "int", value = "/kind" } }

params.PiercingPower100Base = { type = "shell", item = { type = "int", value = "/piercingPower/100" } }
params.PiercingPower500Base = { type = "shell", item = { type = "int", value = "/piercingPower/500" } }
params.PiercingPower100 = {
    type = "transform",
    name = "UI_pen100m",
    items = {
        params.PiercingPower100Base,
        params.ShellKind,
        factors.PiercingPowerFactor_ARMOR_PIERCING,
        factors.PiercingPowerFactor_HOLLOW_CHARGE,
        factors.PiercingPowerFactor_HIGH_EXPLOSIVE,
        factors.PiercingPowerFactor_ARMOR_PIERCING_HE,
        factors.PiercingPowerFactor_ARMOR_PIERCING_CR,
        factors.ArmourPiercingFactor,
        factors.SkillFactor_pointBlast,
        { type = "float", value = "/miscAttrs/descrAttrs/shot0/piercingPower", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot1/piercingPower", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot2/piercingPower", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
    },
    fn = function(v) 
        local si = v[13]
        local dpen = si == 0 and v[10] or (si == 1 and v[11] or v[12])
        local pp = (v[1] + dpen) * v[8] * (1.0 + v[9] * 0.05)
        if v[2] == 0 then
            return pp * v[3]
        elseif v[2] == 1 then
            return pp * v[6]
        elseif v[2] == 2 then
            return pp * v[7]
        elseif v[2] == 3 then
            return pp * v[5]
        elseif v[2] == 5 then
            return pp * v[4]
        end
        return pp
    end,
    units = "UI_millimeters_format",
    format = { maximumFractionDigits = 0 }
}
params.PiercingPower500 = {
    type = "transform",
    name = "UI_pen500m",
    items = {
        params.PiercingPower500Base,
        params.ShellKind,
        factors.PiercingPowerFactor_ARMOR_PIERCING,
        factors.PiercingPowerFactor_HOLLOW_CHARGE,
        factors.PiercingPowerFactor_HIGH_EXPLOSIVE,
        factors.PiercingPowerFactor_ARMOR_PIERCING_HE,
        factors.PiercingPowerFactor_ARMOR_PIERCING_CR,
        params.PiercingPower100Base,
        factors.PiercingPenaltyFactor500m,
        factors.ArmourPiercingFactor,
        { type = "float", value = "/miscAttrs/descrAttrs/shot0/piercingPower", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot1/piercingPower", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot2/piercingPower", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
    },
    fn = function(v) 
        local si = v[15]
        local dpen = si == 0 and v[11] or (si == 1 and v[12] or v[13])
        local pp500 = (v[8] + (v[1] - v[8]) * v[9] + dpen) * v[10]
        if v[2] == 0 then
            return pp500 * v[3]
        elseif v[2] == 1 then
            return pp500 * v[6]
        elseif v[2] == 2 then
            return pp500 * v[7]
        elseif v[2] == 3 then
            return pp500 * v[5]
        elseif v[2] == 5 then
            return pp500 * v[4]
        end
        return pp500
    end,
    units = "UI_millimeters_format",
    format = { maximumFractionDigits = 0 }
}

-- gunner_armorer shrinks the default ±distribution by 5% (e.g. ±25% → ±20%)
-- loader_ammunitionImprove raises the lower bound by +0.02
local function calcPiercingLowerBound(v)
    local baseLower = 1.0 - v[3]
    return baseLower + 0.05 * v[1] + 0.02 * v[2]
end

local function calcPiercingUpperBound(v)
    local baseUpper = 1.0 + v[2]
    return baseUpper - 0.05 * v[1]
end

local function calcDamageLowerBound(v)
    local baseLower = 1.0 - v[3]
    return baseLower + 0.05 * v[1] + 0.02 * v[2]
end

local function calcDamageUpperBound(v)
    local baseUpper = 1.0 + v[2]
    return baseUpper - 0.05 * v[1]
end

params.PiercingLowerBoundFactor = {
    type = "transform",
    items = {
        factors.SkillFactor_armorer,
        factors.SkillFactor_ammunitionImprove,
        { type = "float", value = "/miscAttrs/piercingPowerDistribution", default = 0.25 },
    },
    fn = calcPiercingLowerBound,
}

params.PiercingUpperBoundFactor = {
    type = "transform",
    items = {
        factors.SkillFactor_armorer,
        { type = "float", value = "/miscAttrs/piercingPowerDistribution", default = 0.25 },
    },
    fn = calcPiercingUpperBound,
}

params.DamageLowerBoundFactor = {
    type = "transform",
    items = {
        factors.SkillFactor_armorer,
        factors.SkillFactor_ammunitionImprove,
        { type = "float", value = "/miscAttrs/damageDistribution", default = 0.25 },
    },
    fn = calcDamageLowerBound,
}

params.DamageUpperBoundFactor = {
    type = "transform",
    items = {
        factors.SkillFactor_armorer,
        { type = "float", value = "/miscAttrs/damageDistribution", default = 0.25 },
    },
    fn = calcDamageUpperBound,
}

params.DamageArmor = {
    type = "transform",
    name = "UI_damage_armor",
    items = {
        { type = "shell", item = { type = "int", value = "/damageArmor" } },
        factors.DamageFactor,
        { type = "float", value = "/miscAttrs/descrAttrs/shell0/armorDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell1/armorDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell2/armorDamage", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
    },
    fn = function(v)
        local si = v[6]
        local da = si == 0 and v[3] or (si == 1 and v[4] or v[5])
        return (v[1] + da) * v[2]
    end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 }
}

params.DamageArmorSplashHp = {
    type = "transform",
    name = "UI_splash_damage",
    items = {
        { type = "shell", item = { type = "int", value = "/armorSpallsDamageArmor" } },
        factors.DamageFactor
    },
    fn = function(v) return v[1] * v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 }
}

params.DamageArmor500 = {
    type = "transform",
    name = "UI_damage_armor_500",
    items = {
        { type = "shell", item = { type = "int", value = "/damageArmor/500" } },
        factors.DamageFactor,
        { type = "float", value = "/miscAttrs/descrAttrs/shell0/armorDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell1/armorDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell2/armorDamage", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
    },
    fn = function(v)
        local si = v[6]
        local da = si == 0 and v[3] or (si == 1 and v[4] or v[5])
        return (v[1] + da) * v[2]
    end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 }
}

params.DamageArmorSplash = {
    type = "transform",
    name = "UI_splash_damage",
    items = {
        { type = "shell", item = { type = "int", value = "/damageArmor" } },
        { type = "shell", item = { type = "int", value = "/armorSpallsDamageArmor" } },
    },
    fn = function(v) return v[1] > 0 and v[2] / v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 }
}

params.DamageModules = {
    type = "transform",
    name = "UI_damage_modules",
    items = {
        { type = "shell", item = { type = "int", value = "/damageDevices" } },
        { type = "float", value = "/miscAttrs/descrAttrs/shell0/deviceDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell1/deviceDamage", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shell2/deviceDamage", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
    },
    fn = function(v)
        local si = v[5]
        local dd = si == 0 and v[2] or (si == 1 and v[3] or v[4])
        return v[1] + dd
    end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.Mode = { type = "string", value = "/mode", weight = function() return 0.0 end }
params.Tags = { type = "string", value = "/tags", weight = function() return 0.0 end }

params.DPM = { 
    type = "transform", 
    name = "UI_dpm", 
    items = {
        params.RateOfFire,
        params.DamageArmor,
        params.Mode,
        params.Tags,
    },
    fn = function(v) 
        local is_twin_gun = v[3] == "siege" and string.find(v[4], "twinGun")
        return v[1] * v[2] * (is_twin_gun and 2.0 or 1.0)
    end,
    units = "UI_hp_format", 
    format = { maximumFractionDigits = 0 }
}

params.MaxExplosionRadius = {
    type = "transform",
    name = "UI_explosion_radius",
    allowUndefined = true,
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/shell0/explosionRadius" } },
        { type = "component", component = "gun", item = { type = "float", value = "/shell1/explosionRadius" } },
        { type = "component", component = "gun", item = { type = "float", value = "/shell2/explosionRadius" } },
    },
    fn = function(v) return math.max(v[1] or 0, v[2] or 0, v[3] or 0) end,
    units = "UI_meters_format"
}

params.ShellVelocityBase = {
    type = "shell", item = { type = "float", value = "/speed" }
}

params.ShellVelocity = {
    type = "transform",
    name = "UI_velocity",
    items = {
        params.ShellVelocityBase,
        factors.ProjectileSpeedFactor,
        factors.SkillFactor_perfectCharge,
        { type = "float", value = "/miscAttrs/descrAttrs/shot0/speed", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot1/speed", default = 0.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/shot2/speed", default = 0.0 },
        { type = "int", value = "/shellIdx", default = 0 },
        { type = "float", value = "/miscAttrs/lowChargeShot/shot0/shotSpeedValue", default = 0.0 },
        { type = "float", value = "/miscAttrs/lowChargeShot/shot1/shotSpeedValue", default = 0.0 },
        { type = "float", value = "/miscAttrs/lowChargeShot/shot2/shotSpeedValue", default = 0.0 },
    },
    fn = function(v)
        local si = v[7]
        local ds = si == 0 and v[4] or (si == 1 and v[5] or v[6])
        local lq = si == 0 and v[8] or (si == 1 and v[9] or v[10])
        return (v[1] + ds + lq) * v[2] * (1.0 + v[3] * 0.1)
    end,
    units = "UI_ms_format",
    format = { maximumFractionDigits = 0 }
}

params.ShellMaxDistance = { 
    type = "shell", 
    name = "UI_distance", 
    item = { 
        type = "float", 
        value = "/maxDistance", 
        units = "UI_meters_format"
    } 
}

params.ShellAccelerationBase = { 
    type = "shell", 
    name = "UI_acceleration", 
    item = { 
        type = "float", 
        value = "/acceleration", 
        units = "UI_meters_per_second2_format"
    } 
}

params.ShellAcceleration = {
    type = "transform",
    name = "UI_acceleration", 
    items = { params.ShellAccelerationBase },
    fn = function(v) return v[1] > 0.0 and v[1] end,
    units = "UI_meters_per_second2_format",
    format = { maximumFractionDigits = 0 }
}

params.PiercingPowerLossFactorByDistance = {
    type = "transform",
    name = "UI_piercing_power_loss_factor_by_distance",
    items = {
        { type = "shell", item = { type = "float", value = "/piercingPowerLossFactorByDistance" } },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 }
}

params.ShellNormalizationAngle = {
    type = "shell",
    name = "UI_normalization_angle",
    item = {
        type = "float",
        value = "/normalizationAngle",
        units = "UI_degrees_format",
        format = { maximumFractionDigits = 0 }
    }
}

params.ShellRicochetAngle = {
    type = "shell",
    name = "UI_ricochet_angle",
    item = {
        type = "float",
        value = "/ricochetAngle",
        units = "UI_degrees_format",
        format = { maximumFractionDigits = 0 }
    }
}


params.Viewrange = { 
    type = "transform",
    name = "UI_viewrange", 
    items = {
        {
            type = "component", 
            component = "turret", 
            item = { type = "float", value = "/circularVisionRadius" }
        },
        factors.ViewRangeFactor,
        params.VehicleType,
        factors.CircularVisionRadiusFactors_lightTank,
        factors.CircularVisionRadiusFactors_mediumTank,
        factors.CircularVisionRadiusFactors_heavyTank,
        factors.CircularVisionRadiusFactors_ATSPG,
        factors.CircularVisionRadiusFactors_SPG,
        factors.CircularVisionRadiusFactor,
        factors.SkillFactor_eagleEye,
        factors.SkillFactor_finder,
        factors.SkillFactor_threatSearch,
        { type = "float", value = "/miscAttrs/descrAttrs/turret/circularVisionRadius", default = 0.0 },
    },
    fn = function(v) 
        return (v[1] + v[13]) * v[2] * v[4 + v[3]] * v[9] * (1.0 + v[10]) * (1.0 + v[11]) * (1.0 + v[12])
    end,
    units = "UI_meters_format", 
    format = { maximumFractionDigits = 0 }
}

params.ViewrangeStill = { 
    type = "transform",
    name = "UI_viewrange_still", 
    items = {
        {
            type = "component", 
            component = "turret", 
            item = { type = "float", value = "/circularVisionRadius" }
        },
        factors.ViewRangeFactor,
        factors.CircularVisionRadiusStillFactor,
    },
    fn = function(v) 
        return v[3] ~= 1.0 and v[1] * v[2] * v[3]
    end,
    units = "UI_meters_format", 
    format = { maximumFractionDigits = 0 }
}



params.GunDepressionBase = { type = "component", component = "gun", item = { type = "float", value = "/pitchLimits/min", weight = -1.0 } }
params.GunElevationBase = { type = "component", component = "gun", item = { type = "float", value = "/pitchLimits/max", weight = 0.3 } }

params.HullAimingPitchMinBase = {
    type = "float",
    value = "/hullAiming/pitchMin",
    name = "UI_hull_aiming_pitch_min",
    units = "UI_degrees_format",
    format = { maximumFractionDigits = 1 },
}

params.HullAimingPitchMaxBase = {
    type = "float",
    value = "/hullAiming/pitchMax",
    name = "UI_hull_aiming_pitch_max",
    units = "UI_degrees_format",
    format = { maximumFractionDigits = 1 },
}

params.GunDepressionIncrease = {
    type = "transform",
    items = {
        factors.LowerPitchLimitIncrease,
        { type = "float", value = "/miscAttrs/descrAttrs/gunPitchLimits/maxPitchDegrees/0", default = 0.0, weight = -1.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/gunPitchLimits/maxPitchDegrees", default = 0.0, weight = -1.0 },
    },
    fn = function(v) return v[1] + v[2] + v[3] end,
}

params.GunElevationIncrease = {
    type = "transform",
    items = {
        factors.UpperPitchLimitIncrease,
        { type = "float", value = "/miscAttrs/descrAttrs/gunPitchLimits/minPitchDegrees", default = 0.0 },
    },
    fn = function(v) return v[1] - v[2] end,
}

params.GunDepression = { 
    type = "transform",
    name = "UI_gun_depression", 
    items = {
        params.GunDepressionBase,
        params.GunDepressionIncrease,
    },
    fn = function(v) 
        return v[1] - v[2]
    end,
    units = "UI_degrees_format", 
    format = { maximumFractionDigits = 1 },
    className = "mt-2"
}

params.GunElevation = { 
    type = "transform",
    name = "UI_gun_elevation", 
    items = {
        params.GunElevationBase,
        params.GunElevationIncrease,
    },
    fn = function(v) 
        return v[1] + v[2]
    end,
    units = "UI_degrees_format", 
    format = { maximumFractionDigits = 1 },
    className = "mt-2"
}


params.GunAnglesComposite = {
    type = "composite",
    name = "UI_gun_angles",
    items = {
        params.GunDepression,
        params.GunElevation
    },
    className = "mt-2"
}

params.HullAimingPitchComposite = {
    type = "composite",
    name = "UI_hull_aiming_pitch",
    items = {
        params.HullAimingPitchMinBase,
        params.HullAimingPitchMaxBase
    }
}

params.TurretYawLimitsMin = {
    type = "transform",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/yawLimits/min", weight = -1.0 } },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/turretYawLimitsDegrees", default = 0.0, weight = -1.0 },
    },
    fn = function(v) return v[1] - v[2] end,
    units = "UI_degrees_format",
}

params.TurretYawLimitsMax = {
    type = "transform",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/yawLimits/max" } },
        { type = "float", value = "/miscAttrs/descrAttrs/gun/turretYawLimitsDegrees", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_degrees_format",
}

params.TurretAnglesComposite = {
    type = "composite",
    name = "UI_turret_angles",
    items = {
        params.TurretYawLimitsMin,
        params.TurretYawLimitsMax,
    },
}

params.TurretPitch = {
    type = "float",
    name = "UI_turret_pitch",
    value = "/hull/0/turretPitch",
    units = "UI_degrees_format", 
}

params.ShellPrice = {
    type = "shell",
    name = "UI_price",
    item = {
        type = "price",
        value = "/price",
        currency = {
            type = "shell",
            item = {
                type = "string",
                value = "/priceCurrency"
            }
        },
        format = { maximumFractionDigits = 0 }
    },
    filter = function() return platformCode ~= "blitz" and platformCode ~= "tanksblitz" end,
}

params.ShellEffectiveDistance = {
    type = "transform",
    name = "UI_effective_range",
    items = {
        params.GunElevation,
        params.ShellVelocity,
        {
            type = "shell", 
            item = { type = "float", value = "/gravity" },
        },
        params.ShellMaxDistance,
    },
    fn = function(v)
        return math.min(v[2] * v[2] * math.sin(math.rad(2.0 * math.min(45, v[1]))) / v[3], v[4])
    end,
    units = "UI_meters_format",
    format = { maximumFractionDigits = 0 }
}

params.MaxSteeringLockAngle = { 
    type = "component",     
    name = "UI_max_wheel_angle",
    component = "chassis", 
    item = { type = "float", value = "/maxSteeringLockAngle" },
    units = "UI_degrees_format",
    format = { maximumFractionDigits = 0},
}


-- Modules
params.CrewMembers = { type = "int", name = "UI_crew_members", value = "/crewMembers" }

params.TracksHealthBase = { type = "component", component = "chassis", item = { type = "float", value = "/maxHealth" } }
params.TracksHealthRegenBase = { type = "component", component = "chassis", item = { type = "float", value = "/maxRegenHealth" } }

params.TracksHealth = {
    type = "transform",
    name = "UI_tracks_health",
    items = {
        params.TracksHealthBase,
        factors.ChassisHealthFactor,
    },
    fn = function(v) 
        return v[1] * v[2]
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
    className = "mt-2"
}

params.TracksHealthRegen = {
    type = "transform",
    name = "UI_tracks_health",
    items = {
        params.TracksHealthRegenBase,
        factors.ChassisHealthFactor,
        params.TracksHealth,
        factors.ChassisRegenerationPenaltyFactor,
    },
    fn = function(v) 
        local regen_health = v[1] * v[2]
        return v[3] + (regen_health - v[3]) * v[4]
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.TracksHealthRegenPerSec = {
    type = "component",
    component = "chassis",
    item = { type = "float", value = "/healthRegenPerSec", default = 0 },
}

params.TracksRepairTime = {
    type = "transform",
    name = "UI_tracks_repair_time",
    items = {
        {
            type = "component",
            component = "chassis",
            item = { type = "float", value = "/repairTime", weight = -1.0, default = 0 },
        },
        params.TracksHealthRegen,
        params.TracksHealthRegenPerSec,
        factors.RepairSpeedFactor,
        factors.ChassisRepairSpeedFactor,
    },
    fn = function(v)
        local repairTime = v[1]
        if repairTime <= 0 and v[3] > 0 then
            repairTime = v[2] / (v[3] * 3.0) -- bulk health factor
        end
        return repairTime / (v[4] * v[5])
    end,
    weight = function(v)
        local repairTime = v[1]
        if repairTime <= 0 and v[3] > 0 then
            repairTime = -v[2] / v[3]
        end
        return repairTime / (v[4] * v[5])
    end,
    filter = function() return platformCode == "pc" or platformCode == "mirtankov" or platformCode == "blitz" or platformCode == "reforged" end,
    units = "UI_seconds_format"
}

params.TracksHealthComposite = {
    type = "composite",
    name = "UI_tracks_health",
    items = { params.TracksHealth, params.TracksHealthRegen },
    className = "mt-2"
}

params.GunHealth = { type = "component", component = "gun", item = { type = "float", value = "/maxHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }
params.GunHealthRegen = { type = "component", component = "gun", item = { type = "float", value = "/maxRegenHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }

params.TurretHealth = { type = "component", component = "turret", item = { type = "float", value = "/turretRotatorHealth/maxHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }
params.TurretHealthRegen = { type = "component", component = "turret", item = { type = "float", value = "/turretRotatorHealth/maxRegenHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }

params.SurveyingDeviceHealth = { type = "component", component = "turret", item = { type = "float", value = "/surveyingDeviceHealth/maxHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }
params.SurveyingDeviceHealthRegen = { type = "component", component = "turret", item = { type = "float", value = "/surveyingDeviceHealth/maxRegenHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }

params.EngineHealthBase = { type = "component", component = "engine", name = "UI_engine_health", item = { type = "float", value = "/maxHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }
params.EngineHealthRegenBase = { type = "component", component = "engine", name = "UI_engine_health", item = { type = "float", value = "/maxRegenHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } } }

params.EngineHealth = {
    type = "transform",
    name = "UI_engine_health",
    items = {
        params.EngineHealthBase,
        factors.EngineHealthFactor,
    },
    fn = function(v) 
        return v[1] * v[2]
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.EngineHealthRegen = {
    type = "transform",
    name = "UI_engine_health",
    items = {
        params.EngineHealthRegenBase,
        factors.EngineHealthFactor,
    },
    fn = function(v) 
        return v[1] * v[2]
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.EngineHealthComposite = {
    type = "composite",
    name = "UI_engine_health",
    items = { params.EngineHealth, params.EngineHealthRegen },
}

params.EngineFireChance = {
    type = "transform",
    name = "UI_engine_fire_chance",
    items = {
        {
            type = "component",
            component = "engine",
            item = {
                type = "float",
                value = "/fireStartingChance",
                weight = -1.0
            }
        },
        factors.FireStartingChanceFactor
    },
    units = "UI_percent_format",
    fn = function(v) return v[1] * v[2] * 100.0 end,
    format = { maximumFractionDigits = 0 },
    className = "mt-2"
}

params.RadioHealth = { 
    type = "component", 
    component = "radio", 
    name = "UI_radio_health", 
    item = { type = "float", value = "/maxHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } },
    filter = function() return platformCode ~= "blitz" and platformCode ~= "tanksblitz" end
}
params.RadioHealthRegen = { 
    type = "component", 
    component = "radio", 
    name = "UI_radio_health", 
    item = { type = "float", value = "/maxRegenHealth", units = "UI_hp_format", format = { maximumFractionDigits = 0 } },
    filter = function() return platformCode ~= "blitz" and platformCode ~= "tanksblitz" end
}

params.RadioHealthComposite = {
    type = "composite",
    name = "UI_radio_health",
    items = { params.RadioHealth, params.RadioHealthRegen },
    filter = function() return platformCode ~= "blitz" and platformCode ~= "tanksblitz" end
}

params.RadioRange = { 
    type = "transform",
    name = "UI_radio_range", 
    items = {
        {
            type = "component", 
            component = "radio", 
            item = { type = "float", value = "/distance" },
        },
        factors.SkillFactor_radioman,
        factors.SkillFactor_inventor
    },
    fn = function(v) return v[1] * v[2] * (1.0 + v[3]) end,
    units = "UI_meters_format", 
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode ~= "blitz" and platformCode ~= "tanksblitz" end
}
params.FuelTankHealthBase = { type = "component", component = "fueltank", item = { type = "float", value = "/maxHealth" } }
params.FuelTankHealthRegenBase = { type = "component", component = "fueltank", item = { type = "float", value = "/maxRegenHealth" } }

params.FuelTankHealth = {
    type = "transform",
    name = "UI_fueltank_health",
    allowUndefined = true,
    items = {
        params.FuelTankHealthBase,
        factors.FuelTankHealthFactor,
    },
    fn = function(v) 
        return v[1] and v[1] * v[2] or 1
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.FuelTankHealthRegen = {
    type = "transform",
    name = "UI_fueltank_health",
    allowUndefined = true,
    items = {
        params.FuelTankHealthRegenBase,
        factors.FuelTankHealthFactor,
    },
    fn = function(v) 
        return v[1] and v[1] * v[2] or 1
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.FuelTankHealthComposite = {
    type = "composite",
    name = "UI_fueltank_health",
    items = { params.FuelTankHealth, params.FuelTankHealthRegen },
    allowUndefined = true,
}

params.AmmorackHealthBase = { type = "float", value = "/hull/0/ammoBay/maxHealth" }
params.AmmorackHealthRegenBase = { type = "float", value = "/hull/0/ammoBay/maxRegenHealth" }

params.AmmorackHealth = {
    type = "transform",
    name = "UI_ammorack_health",
    allowUndefined = true,
    items = {
        params.AmmorackHealthBase,
        factors.AmmoBayHealthFactor,
        factors.SkillFactor_pedant,
    },
    fn = function(v) 
        return v[1] * v[2] * (1.0 + v[3] * (platformCode == "pc" and 0.25 or 0.125))
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.AmmorackHealthRegen = {
    type = "transform",
    name = "UI_ammorack_health",
    allowUndefined = true,
    items = {
        params.AmmorackHealthRegenBase,
        factors.AmmoBayHealthFactor,
        factors.SkillFactor_pedant,
    },
    fn = function(v) 
        return v[1] * v[2] * (1.0 + v[3] * (platformCode == "pc" and 0.25 or 0.125))
    end,
    format = { maximumFractionDigits = 0 },
    units = "UI_hp_format",
}

params.AmmorackHealthComposite = {
    type = "composite",
    name = "UI_ammorack_health",
    items = { params.AmmorackHealth, params.AmmorackHealthRegen },
    allowUndefined = true,
}

params.RepairSpeedIncrease = {
    type = "transform",
    name = "UI_repair_speed",
    items = {
        factors.RepairSpeedFactor
    },
    fn = function(v) 
        local base_speed = (platformCode == "blitz" or platformCode == "tanksblitz") and v[1] or v[1] / 0.57
        return base_speed ~= 1.0 and (base_speed - 1.0) * 100.0
    end,
    units = "UI_percent_format",
    format = { signDisplay = "always" },
}

params.ChassisRepairSpeedIncrease = {
    type = "transform",
    name = "UI_tracks_repair_speed",
    items = {
        factors.ChassisRepairSpeedFactor
    },
    fn = function(v) return v[1] ~= 1.0 and (v[1] - 1.0) * 100.0 end,
    units = "UI_percent_format",
    format = { signDisplay = "always" },
}

params.HEDamageReduction = {
    type = "transform",
    name = "UI_he_damage_reduction",
    items = {
        factors.AntifragmentationLiningFactor,
    },
    fn = function(v)
        local x = v[1]
        if x == nil or x == 1.0 then
            return nil
        end
        return (1.0 - x) * 100.0
    end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.RepairCost = {
    type = "transform",
    name = "UI_repair_cost",
    items = {
        params.MaxHealth,
        { type = "float", value = "/repairCost", weight = -1.0 },
        params.TracksHealth,
        { type = "component", component = "chassis", item = { type = "float", value = "/maxRegenHealth" } },
        { type = "component", component = "chassis", item = { type = "float", value = "/repairCost", weight = -1.0 } },
    },
    fn = function(v)
        return v[1] * v[2] + (v[3] - v[4]) * v[5]
    end,
    filter = function() 
        return platformCode ~= "blitz" and platformCode ~= "tanksblitz"
    end,
    format = { maximumFractionDigits = 0 }
}

-- Dual gun
params.DualGunChargeTime = { type = "component", component = "gun", item = { type = "float", value = "/dualGun/chargeTime", weight = -1.0 } }
params.DualGunLockTime = { type = "component", component = "gun", item = { type = "float", value = "/dualGun/reloadLockTime", weight = -1.0 } }
params.DualGunChargeLockTime = { 
    type = "composite",
    name = "UI_charge_lock_time",
    items = {
        params.DualGunChargeTime,
        params.DualGunLockTime 
    },
    units = "UI_seconds_format",
}


-- Temperature Mechanics
params.TemperatureMechanicsStates = { type = "component", component = "gun", item = { type = "int", value = "/temperature/states" } }
params.TemperatureMechanicsHeatingTimes = {
    type = "transform",
    name = "UI_heating_time",
    items = {
        { type = "component", component = "gun", item = { type = "string", value = "/temperature/heatingTimes" } }
    },
    fn = function(v) return formatList(v[1], { maximumFractionDigits = 1 }) end,
    className = "mt-2"
}
params.TemperatureMechanicsCoolingTimes = {
    type = "transform",
    name = "UI_cooling_time",
    items = {
        { type = "component", component = "gun", item = { type = "string", value = "/temperature/coolingTimes" } },
        factors.TemperatureGunCoolingPerSec,
    },
    fn = function(v) return formatList(v[1], { maximumFractionDigits = 1 }, function(vv) return vv / v[2] end) end
}
params.TemperatureMechanicsDamageModifiers = {
    type = "transform",
    name = "UI_damages_percent",
    items = {
        { type = "component", component = "gun", item = { type = "string", value = "/temperature/damageModifiers" } }
    },
    fn = function(v) return formatList(v[1], { maximumFractionDigits = 2 }) end
}
params.TemperatureMechanicsDispersionModifiers = {
    type = "transform",
    name = "UI_dispersion",
    items = {
        { type = "component", component = "gun", item = { type = "string", value = "/temperature/dispersionModifiers" } }
    },
    fn = function(v) return formatList(v[1], { maximumFractionDigits = 2 }) end
}
params.TemperatureMechanicsAvgDamage = { 
    type = "transform", 
    name = "UI_average_damage",
    items = {
        params.DamageArmor,
        params.TemperatureMechanicsAvgDamageModifier,
    },
    fn = function(v) return v[1] * v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}


-- Relative power
params.RelativeArmor = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.HullArmorFront,
        params.HullArmorSide,
        params.HullArmorBack,
        params.TurretArmorFront,
        params.TurretArmorSide,
        params.TurretArmorBack,
        params.MaxHealth,
        { type = "float", value = "/adjustment/armor" },
    },
    fn = function(v)
        local hullArmor = v[1] * 1.0 + v[2] * 0.9 + v[3] * 0.25
        local turretArmor = v[4] and v[4] > 0.0 and (v[4] * 1.0 + v[5] * 0.9 + v[6] * 0.25) or hullArmor
        return math.max(1.0, (hullArmor + turretArmor) * v[7] * 0.00037 * v[8])
    end,
}

params.RelativePower = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.TurretArmorFront,
        { type = "int", value = "/type" },
        { type = "shell", item = { type = "int", value = "/kind" } },
        { type = "component", component = "gun", item = { type = "float", value = "/caliberCorrectionAdjustment" } },
        params.DPM,
        params.PiercingPower100,
        params.ShotDispersionRadius,
        params.TurretRotationSpeed,
        { type = "float", value = "/adjustment/power" },
    },
    fn = function(v)
        local turretCoeff = v[1] and v[1] > 0.0 and 1.0 or 0.8
        local heCorrection = 1.0
        local spgCorrection = 1.0
        if v[2] == 4 then
            spgCorrection = 6.0
        else
            if v[3] == 3 or v[3] == 9 then
                heCorrection = 1.35
            end
        end
        local gunCorrection = v[4] or 1.0
        local shotDispersionAngle = math.atan(v[7] * 0.01) * 100.0
        return math.max(1.0, v[5] * v[6] / shotDispersionAngle * (0.97 + 0.001 * v[8]) * turretCoeff * 0.0002692 * v[9] * spgCorrection * gunCorrection * heCorrection)
    end,
}

params.RelativeMobility = {
    type = "transform",
    allowUndefined = true,
    items = {
        { type = "string", value = "/tags" },
        { type = "component", component = "chassis", item = { type = "int", value = "/isWheeledOnSpotRotation" } },
        params.MaxSteeringLockAngle,
        params.HullTraverse,
        params.SpeedLimitsForward,
        params.EffectiveSpeedLimitMedium,
        { type = "float", value = "/adjustment/mobility" },
    },
    fn = function(v)
        local suspensionInfluence = 1.0
        local isWheeledVehicle = v[1] and string.find(v[1], "wheeledVehicle") ~= nil
        if isWheeledVehicle and v[2] == 0 then
            suspensionInfluence = v[3] * 0.35
        else
            suspensionInfluence = v[4] * 0.25
        end
        return (suspensionInfluence + v[5] * 4.0 + v[6] * 0.25) * 2.497 * v[7]
    end,
    showRating = true
}

params.RelativeVisibility = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.Viewrange,
        { type = "float", value = "/adjustment/visibility" },
    },
    fn = function(v)
        local minVisionRadius = (platformCode == "blitz" or platformCode == "tanksblitz") and 50.0 or 150.0
        local maxVisionRadius = (platformCode == "blitz" or platformCode == "tanksblitz") and 300.0 or 500.0
        return (v[1] - minVisionRadius) / (maxVisionRadius - minVisionRadius) * 837.0 * v[2]
    end,
    showRating = true
}

params.RelativeCamouflage = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.InvisibilityMoving,
        params.InvisibilityStill,
        params.InvisibilityStillAtShot,
        { type = "float", value = "/adjustment/camouflage" },
    },
    fn = function(v)
        return (v[1] + v[2] + v[3]) / 3.0 * 27.0 * v[4]
    end,
}


--
-- Tier XI mechanics
--

-- ChargeableBursr
params.ChargeableBurstDispersionFactor = {
    type = "transform",
    name = "UI_burst_dispersion",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/chargeableBurst/burstDispersionFactor" } },
        { type = "float", value = "/miscAttrs/chargeableBurst/burstDispersionFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
}

-- StationaryReload (for gun component)
params.StationaryReloadPreparingDelay = {
    type = "transform",
    name = "UI_activation",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/stationaryReload/preparingDelay", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } } },
        { type = "float", value = "/miscAttrs/stationaryReload/preparingDelayFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.StationaryReloadFinishingDelay = {
    type = "transform",
    name = "UI_interruption",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/stationaryReload/finishingDelay", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } } },
        { type = "float", value = "/miscAttrs/stationaryReload/finishingDelayFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

-- ExtraShotClip (for gun component)
params.ExtraShotClipExtraReloadTime = {
    type = "transform",
    name = "UI_reload_penalty",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/extraShotClip/extraReloadTime", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } } },
        { type = "float", value = "/miscAttrs/gun/extraShotClip/extraReloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

-- LowChargeShot (gun)
params.LowChargeShotAlmostFinishedTime = {
    type = "component",
    name = "UI_reload_finishing_time",
    component = "gun",
    item = { type = "float", value = "/lowChargeShot/almostFinishedTime", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } },
}

params.LowChargeShotReloadTime = {
    type = "transform",
    name = "UI_low_charge_reload_time",
    items = {
      params.ReloadTimeRegular,
      params.LowChargeShotAlmostFinishedTime,
      {
        component = "gun",
        type = "component",
        item = { type = "float", value = "/lowChargeShot/reloadTimeCoefficient" },
      },
      { type = "float", value = "/miscAttrs/lowChargeShot/reloadTimeCoefficient", default = 1.0 },
    },
    fn = function(v) return (v[1] - v[2]) * v[3] * v[4] end,
    units = "UI_seconds_format",
}

-- PropellantAfterburnerGun (gun)
params.PropellantAfterburnerGunChargingPerSec = {
    type = "transform",
    name = "UI_charge_rate",
    items = {
        { type = "component", component = "gun", item = { type = "float", value = "/propellantAfterburnerGun/chargingPerSec" } },
        { type = "float", value = "/miscAttrs/propellantGun/chargingPerSec", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
}

params.PropellantAfterburnerGunChargeDelay = {
    type = "component",
    name = "UI_charge_delay",
    component = "gun",
    item = { type = "float", value = "/propellantAfterburnerGun/chargeDelay", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } },
}

params.PropellantAfterburnerGunDischargingPerSec = {
    type = "component",
    name = "UI_discharge_rate",
    component = "gun",
    item = { type = "float", value = "/propellantAfterburnerGun/dischargingPerSec" },
}

params.PropellantAfterburnerGunOverchargeSwitchCooldown = {
    type = "component",
    name = "UI_time_to_switch",
    component = "gun",
    item = { type = "float", value = "/propellantAfterburnerGun/overchargeSwitchCooldown", units = "UI_seconds_format", format = { maximumFractionDigits = 1 } },
}

params.PropellantAfterburnerGunMinDamage = {
    type = "transform",
    name = "UI_min_damage",
    allowUndefined = true,
    items = {
      params.DamageArmor,
      { type = "component", component = "gun", item = { type = "float", value = "/propellantAfterburnerGun/chargeStage0/damageFactorLimits/minFactor" } },
      { type = "component", component = "gun", item = { type = "float", value = "/propellantAfterburnerGun/chargeStage1/damageFactorLimits/minFactor" } },
      params.Mode,
    },
    fn = function(v) return v[4] == "special" and v[1] * v[3] or v[1] * v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.PropellantAfterburnerGunMaxDamage = {
    type = "transform",
    name = "UI_max_damage",
    items = {
      params.DamageArmor,
      { type = "component", component = "gun", item = { type = "float", value = "/propellantAfterburnerGun/chargeStage0/damageFactorLimits/maxFactor" } },
      { type = "component", component = "gun", item = { type = "float", value = "/propellantAfterburnerGun/chargeStage1/damageFactorLimits/maxFactor" } },
      params.Mode,
    },
    fn = function(v) return v[4] == "special" and v[1] * v[3] or v[1] * v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

-- WheeledDash (vehicle)
params.WheeledDashDeployTime = {
    type = "float",
    name = "UI_deploy_time",
    value = "/wheeledDash/deployTime",
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.WheeledDashReloadTime = {
    type = "transform",
    name = "UI_reload_time",
    items = {
        { type = "float", value = "/wheeledDash/reloadTime" },
        { type = "float", value = "/miscAttrs/wheeledDash/reloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.WheeledDashDuration = {
    type = "transform",
    name = "UI_duration",
    items = {
        { type = "float", value = "/wheeledDash/duration" },
        { type = "float", value = "/miscAttrs/wheeledDash/duration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

-- ChargeShot
params.ChargeShotMaxLevels = {
    type = "int",
    value = "/chargeShot/maxLevels",
}

params.ChargeShotTimePerLevel = {
    type = "transform",
    name = "UI_time_per_level",
    items = {
        { type = "string", value = "/chargeShot/timePerLevel" }
    },
    fn = function(v) return formatList(v[1]) end,
}

params.ChargeShotDamageFactorsPerLevel = {
    type = "transform",
    name = "UI_damages_percent",
    items = {
        { type = "string", value = "/chargeShot/damageFactorsPerLevel" }
    },
    fn = function(v) return formatList(v[1], nil, function(v) return fmt("x{:.3Lg}", v) end) end,
}

params.ChargeShotShotBlockTime = {
    type = "transform",
    name = "UI_block_time",
    items = {
        { type = "float", value = "/chargeShot/shotBlockTime" },
        { type = "float", value = "/miscAttrs/chargeShot/shotBlockTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
}

-- TargetDesignator
params.TargetDesignatorDeployTime = {
    type = "transform",
    name = "UI_deploy_time",
    items = {
        { type = "float", value = "/targetDesignator/deployTime" },
        { type = "float", value = "/miscAttrs/targetDesignator/deployTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.TargetDesignatorCooldownTime = {
    type = "transform",
    name = "UI_cooldown_time",
    items = {
        { type = "float", value = "/targetDesignator/cooldownTime" },
        { type = "float", value = "/miscAttrs/targetDesignator/cooldownTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.TargetDesignatorSpottedMarkedTime = {
    type = "transform",
    name = "UI_mark_duration",
    items = {
        { type = "float", value = "/targetDesignator/spottedMarkedTime" },
        { type = "float", value = "/miscAttrs/targetDesignator/spottedMarkedTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.TargetDesignatorUnspottedMarkedTime = {
    type = "float",
    name = "UI_unspotted",
    value = "/targetDesignator/unspottedMarkedTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.TargetDesignatorDamageIncomeFactor = {
    type = "transform",
    name = "UI_damage_factor",
    items = {
      { type = "float", value = "/targetDesignator/damageIncomeFactor", },
      { type = "float", value = "/miscAttrs/targetDesignator/damageIncome", default = 0.0 },
    },
    fn = function(v) return (v[1] + v[2]) * 100 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format",
}

-- AccuracyStacks
params.AccuracyStacksLevelMax = {
    type = "int",
    value = "/accuracyStacks/levelMax",
}

params.AccuracyStacksAimLevelBonus = {
    type = "transform",
    name = "UI_aim_bonus_per_level",
    items = {
        { type = "float", value = "/accuracyStacks/aimLevelBonus" },
        { type = "float", value = "/miscAttrs/accuracyStacks/aimLevelBonus", default = 0.0 },
    },
    fn = function(v) return (v[1] + v[2]) * 100 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format",
}

params.AccuracyStacksGainMaxSpd = {
    type = "transform",
    name = "UI_speed_limit",
    items = {
        { type = "float", value = "/accuracyStacks/gainMaxSpd" },
        { type = "float", value = "/miscAttrs/accuracyStacks/gainMaxSpd", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_kmh_format",
}

params.AccuracyStacksGainTime = {
    type = "transform",
    name = "UI_time",
    items = {
        { type = "float", value = "/accuracyStacks/gainTime" },
        { type = "float", value = "/miscAttrs/accuracyStacks/gainTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.AccuracyStacksStabilizeBonus = {
    type = "transform",
    name = "UI_stabilization_bonus",
    items = {
        { type = "float", value = "/accuracyStacks/stabilizeBonus" },
        { type = "float", value = "/miscAttrs/accuracyStacks/stabilizeBonus", default = 0.0 },
    },
    fn = function(v) return (1.0 - v[1] - v[2]) * 100 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format",
}

-- OverheatStacks
params.OverheatStacksHeatingTime = {
    type = "transform",
    name = "UI_heating_time",
    items = {
        { type = "float", value = "/overheatStacks/heatingTime" },
        { type = "float", value = "/miscAttrs/overheatStacks/totalTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.OverheatStacksCoolingTime = {
    type = "float",
    name = "UI_cooling_time",
    value = "/overheatStacks/coolingTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.OverheatStacksDmgBonus = {
    type = "transform",
    name = "UI_damage_bonus",
    items = {
        { type = "float", value = "/overheatStacks/dmgBonus" },
        { type = "float", value = "/miscAttrs/overheatStacks/dmgBonusFactor", default = 1.0 },
    },
    fn = function(v) return (v[1] * v[2] - 1) * 100 end,
    format = { maximumFractionDigits = 2 },
    units = "UI_percent_format"
}

params.OverheatStacksAimBonus = {
    type = "transform",
    name = "UI_aim_bonus",
    items = {
        { type = "float", value = "/overheatStacks/aimBonus" },
        { type = "float", value = "/miscAttrs/overheatStacks/aimBonusFactor", default = 1.0 },
    },
    fn = function(v) return (v[1] * v[2] - 1) * 100 end,
    format = { maximumFractionDigits = 2 },
    units = "UI_percent_format"
}

params.OverheatStacksGainMaxSpd = {
    type = "transform",
    name = "UI_speed_limit",
    items = {
        { type = "float", value = "/overheatStacks/gainMaxSpd" },
        { type = "float", value = "/miscAttrs/overheatStacks/gainMaxSpd", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_kmh_format",
}

params.OverheatStacksDelayTimerDuration = {
    type = "float",
    name = "UI_heating_delay",
    value = "/overheatStacks/delayTimerDuration",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

-- ConcentrationMode
params.ConcentrationModeDeployTime = {
    type = "transform",
    name = "UI_deploy_time",
    items = {
        { type = "float", value = "/concentrationMode/deployTime" },
        { type = "float", value = "/miscAttrs/concentrationModeDeployTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.ConcentrationModeDuration = {
    type = "transform",
    name = "UI_duration",
    items = {
        { type = "float", value = "/concentrationMode/duration" },
        { type = "float", value = "/miscAttrs/concentrationModeDuration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.ConcentrationModeReloadTime = {
    type = "transform",
    name = "UI_cooldown_time",
    items = {
        { type = "float", value = "/concentrationMode/reloadTime" },
        { type = "float", value = "/miscAttrs/concentrationModeReloadTime", default = 0.0 },
    },
    fn = function(v) return v[1]+ v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

-- BattleFury
params.BattleFuryMaxLevel = {
    type = "int",
    value = "/battleFury/maxLevel",
}

params.BattleFuryDuration = {
    type = "transform",
    name = "UI_duration",
    items = {
        { type = "float", value = "/battleFury/duration" },
        { type = "float", value = "/miscAttrs/battleFury/duration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.BattleFuryReloadSpdBonus = {
    type = "transform",
    name = "UI_reload_bonus_per_level",
    items = {
        { type = "float", value = "/battleFury/reloadSpdBonus" },
        { type = "float", value = "/miscAttrs/battleFury/reloadSpdBonus", default = 0.0 },
    },
    fn = function(v) return (v[1] + v[2]) * 100 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format",
}

-- RechargeableNitro
params.RechargeableNitroReloadTime = {
    type = "transform",
    name = "UI_reload_time",
    items = {
        { type = "float", value = "/rechargeableNitro/reloadTime" },
        { type = "float", value = "/miscAttrs/rechargeableNitro/reloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.RechargeableNitroDuration = {
    type = "transform",
    name = "UI_duration",
    items = {
        { type = "float", value = "/rechargeableNitro/duration" },
        { type = "float", value = "/miscAttrs/rechargeableNitro/duration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.RechargeableNitroCooldown = {
    type = "float",
    name = "UI_cooldown_time",
    value = "/rechargeableNitro/cooldown",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

-- StagedJetBoosters
params.StagedJetBoostersDeployTime = {
    type = "float",
    name = "UI_deploy_time",
    value = "/stagedJetBoosters/deployTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.StagedJetBoostersReloadTime = {
    type = "transform",
    name = "UI_cooldown_time",
    items = {
        { type = "float", value = "/stagedJetBoosters/reloadTime" },
        { type = "float", value = "/miscAttrs/stagedJetBoosters/reloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.StagedJetBoostersReuseCount = {
    type = "transform",
    name = "UI_reuse_count",
    items = {
        { type = "int", value = "/stagedJetBoosters/reuseCount" },
        { type = "int", value = "/miscAttrs/stagedJetBoosters/reuseCount", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
}
params.StagedJetBoostersDuration = {
    type = "float",
    name = "UI_duration",
    value = "/stagedJetBoosters/duration",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

-- RocketAcceleration
params.RocketAccelerationDeployTime = {
    type = "float",
    name = "UI_deploy_time",
    value = "/rocketAcceleration/deployTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.RocketAccelerationReloadTime = {
    type = "float",
    name = "UI_cooldown_time",
    value = "/rocketAcceleration/reloadTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.RocketAccelerationReuseCount = {
    type = "int",
    name = "UI_reuse_count",
    value = "/rocketAcceleration/reuseCount",
}

params.RocketAccelerationDuration = {
    type = "float",
    name = "UI_duration",
    value = "/rocketAcceleration/duration",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

-- PowerMode
params.PowerModeSpeedThreshold = {
    type = "float",
    name = "UI_speed_limit",
    value = "/powerMode/speedThreshold",
    format = { maximumFractionDigits = 1 },
    units = "UI_kmh_format",
}

params.PowerModeModeThreshold = {
    type = "transform",
    name = "UI_charge_time",
    items = {
        { type = "float", value = "/powerMode/modeThreshold" },
        { type = "float", value = "/miscAttrs/powerMode/modeThreshold", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.PowerModeModeDuration = {
    type = "transform",
    name = "UI_delay",
    items = {
        { type = "float", value = "/powerMode/modeDuration" },
        { type = "float", value = "/miscAttrs/powerMode/modeDuration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.PowerModeEnginePowerStat = {
    type = "transform",
    name = "UI_engine_power",
    items = {
        { type = "float", value = "/powerMode/enginePower" },
        { type = "float", value = "/miscAttrs/powerMode/enginePower", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] * 100.0 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format", 
}

params.PowerModeVehicleSpeedStat = {
    type = "transform",
    name = "UI_speed",
    allowUndefined = true,
    items = {
        { type = "float", value = "/powerMode/vehicleSpeed" },
        { type = "float", value = "/miscAttrs/powerMode/vehicleSpeed", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] * 100.0 end,
    format = { maximumFractionDigits = 0 },
    units = "UI_percent_format",
}

-- PillboxSiegeMode
params.PillboxSiegeModeSwitchDriveToPillboxTime = {
    type = "float",
    name = "UI_drive_to_pillbox_time",
    value = "/pillboxSiegeMode/switchDriveToPillboxTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.PillboxSiegeModeSwitchSiegeToPillboxTime = {
    type = "float",
    name = "UI_siege_to_pillbox_time",
    value = "/pillboxSiegeMode/switchSiegeToPillboxTime",
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
    className = "mt-2"
}

params.PillboxSiegeModeSwitchPillboxToSiegeTime = {
    type = "transform",
    name = "UI_pillbox_to_siege_time",
    items = {
        { type = "float", value = "/pillboxSiegeMode/switchPillboxToSiegeTime" },
        { type = "float", value = "/miscAttrs/pillboxSiegeMode/switchPillboxToSiegeTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.PillboxSiegeModeSwitchPillboxToDriveTime = {
    type = "transform",
    name = "UI_pillbox_to_drive_time",
    items = {
        { type = "float", value = "/pillboxSiegeMode/switchPillboxToDriveTime" },
        { type = "float", value = "/miscAttrs/pillboxSiegeMode/switchPillboxToDriveTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}


-- StanceDance
params.StanceDanceTimeSwitchStance = {
    type = "transform",
    name = "UI_time_to_switch",
    items = {
        { type = "float", value = "/stanceDance/timeSwitchStance" },
        { type = "float", value = "/miscAttrs/stanceDance/timeSwitchStance", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
}

params.StanceDanceGainFightEnergyPoints = {
    type = "transform",
    name = "UI_passive_charge",
    items = {
        { type = "float", value = "/stanceDance/gainFightEnergyPoints" },
        { type = "float", value = "/miscAttrs/stanceDance/gainFightEnergyPointsFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_percent_per_second_format",
}

params.StanceDanceGainTurboEnergyPoints = {
    type = "transform",
    name = "UI_passive_charge",
    items = {
        { type = "float", value = "/stanceDance/gainTurboEnergyPoints" },
        { type = "float", value = "/miscAttrs/stanceDance/gainTurboEnergyPointsFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
    format = { maximumFractionDigits = 2 },
    units = "UI_percent_per_second_format",
}
params.StanceDanceGainTurboEnergyBonusPoints = {
    type = "float",
    name = "UI_charge_rate",
    value = "/stanceDance/gainTurboEnergyBonusPoints",
    format = { maximumFractionDigits = 1 },
    units = "UI_percent_format",
}

params.StanceDanceGainTurboEnergySpdLimitKmh = {
    type = "float",
    name = "UI_speed_limit",
    value = "/stanceDance/gainTurboEnergySpdLimitKmh",
    format = { maximumFractionDigits = 0 },
    units = "UI_kmh_format",
}

params.StanceDanceActiveTurboDuration = {
    type = "transform",
    name = "UI_turbo_mode",
    items = {
        { type = "float", value = "/stanceDance/activeTurboDuration" },
        { type = "float", value = "/miscAttrs/stanceDance/activeTurboDuration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
    className = "mt-2"
}

params.StanceDancePassiveFightEnergyBonusPerHit = {
    type = "float",
    name = "UI_charge_per_hit",
    value = "/stanceDance/passiveFightEnergyBonusPerHit",
    format = { maximumFractionDigits = 1 },
    units = "UI_percent_format",
}

params.StanceDanceActiveFightDuration = {
    type = "transform",
    name = "UI_engagement_mode",
    items = {
        { type = "float", value = "/stanceDance/activeFightDuration" },
        { type = "float", value = "/miscAttrs/stanceDance/activeFightDuration", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 1 },
    units = "UI_seconds_format",
    className = "mt-2"
}



-- Group configurations
params.GeneralParams = {
    icon = "Tank-ViewRight-1",
    title = "UI_general",
    column = 2,
    items = {
        params.CrewMembers,
        params.MaxHealth,
        params.HullArmorComposite,
        params.TurretArmorComposite,
        params.Weight,
        params.MaxLoad,
        params.TracksHealthComposite,
        params.EngineHealthComposite,
        params.RadioHealthComposite,
        params.FuelTankHealthComposite,
        params.AmmorackHealthComposite,
        params.EngineFireChance,
        params.RadioRange,
        params.RepairSpeedIncrease,
        params.ChassisRepairSpeedIncrease,
        params.HEDamageReduction,
    },
}


params.EconomicsParams = {
    icon = "credits",
    title = "UI_economics",
    column = 2,
    items = {   
        { 
            type = "price", 
            name = "UI_price", 
            value = "/price", 
            currency = { type = "string", value = "/priceCurrency" }, 
            format = { maximumFractionDigits = 0 } 
        },
        params.RepairCost,
        { 
            type = "float", 
            name = "UI_credits_factor", 
            value = "/creditsFactor", 
            filter = function() 
                return platformCode == "console" 
            end 
        },
        { 
            type = "float", 
            name = "UI_xp_factor", 
            value = "/xpFactor", 
            filter = function() 
                return platformCode == "console" 
            end 
        },
        { 
            type = "float", 
            name = "UI_crew_xp_factor", 
            value = "/crewXpFactor" 
        },
        { 
            type = "float", 
            name = "UI_free_xp_factor", 
            value = "/freeXpFactor", 
            filter = function() 
                return platformCode == "console" 
            end 
        },
    }
}


params.AimingTimeParams = {
    icon = "heatmap-view-focus",
    title = "UI_aiming_time",
    column = 3,
    items = {   
        params.AimingTimeAfterShot,
        params.AimingTimeVehicleRotation,
        params.AimingTimeTurretRotation,
        params.AimingTimeVehicleTurretRotation,
        {
            type = "group",
            value = params.AimingTimeMaxSpeed,
            items = {
                params.AimingTimeMaxSpeedVehicleRotation,
                params.AimingTimeMaxSpeedTurretRotation,
                params.AimingTimeMaxSpeedVehicleTurretRotation,
            }
        },
    }
}


params.WeaponryParams = {
    icon = "modules-turret-3",
    title = "UI_weaponry",
    column = 3,
    items = {
        params.DPM,
        params.RateOfFire,
        {
            type = "group",
            value = params.ReloadTime,
            items = { 
                params.ShellChangeTime,
            }
        },
        params.DualGunChargeLockTime,
        params.AimingTime,
        params.AutoShootAimingDelay,
        {
            type = "group",
            value = params.ShotDispersionRadius,
            items = {
                params.ShotDispersionFactorsVehicleMovement,
                params.ShotDispersionFactorsVehicleRotation,
                params.ShotDispersionFactorsTurretRotation,
                params.ShotDispersionFactorsAfterShot,
                params.ShotDispersionFactorsWhileGunDamaged,
            }
        }
    }
}

params.MobilityParams = {
    icon = "Modules-Tracks-",
    title = "UI_mobility",
    column = 2,
    items = {
        params.SpeedLimitsComposite,
        params.TracksRepairTime,
        params.EnginePower,
        params.PowerToWeight,
        params.RammingPotential,
        params.MaxSteeringLockAngle,
        params.TerrainResistanceComposite,
        params.RollingFrictionComposite,
        params.AverageHullTraverse,

        -- params.HullTraverse,
        -- params.TerrainResistanceHardBase,
        -- { type = "transform", name = "weight ratio", items = {params.Weight, { type = "float", value = "/stock/weight" }}, fn = function(v) return 1000.0 * v[1] / v[2] end },
        -- { type = "transform", name = "power ratio", items = {params.EnginePower, { type = "float", value = "/engine/stock/power" }}, fn = function(v) return v[1] / v[2] end },

        params.TurretRotationSpeed,
        params.GunAnglesComposite,
        params.HullAimingPitchComposite,
        params.TurretAnglesComposite,
        params.TurretPitch,
        { 
            type = "group", 
            value = { 
                name = "UI_effective_speed", 
                className = "mt-2" 
            }, 
            items = {params.EffectiveSpeedLimitHard, params.EffectiveSpeedLimitMedium, params.EffectiveSpeedLimitSoft} 
        },
        { 
            type = "group", 
            value = { 
                name = "UI_effective_hull_traverse", 
                className = "mt-2" 
            }, 
            items = {params.EffectiveTraverseHard, params.EffectiveTraverseMedium, params.EffectiveTraverseSoft} 
        },
    }
}

params.CamouflageParams = {
    icon = "Tank-Camo-1",
    title = "UI_camouflage",
    column = 3,
    items = {
        params.Viewrange,
        params.ViewrangeStill,
        {
            type = "group",
            value = { 
                name = "UI_concealment", 
                className = "mt-2" 
            },
            items = {
                params.InvisibilityStill, 
                params.InvisibilityStillAtShot,
                params.InvisibilityMoving, 
                params.InvisibilityMovingAtShot,
                --[[
                {
                    type = "composite",
                    name = "UI_camo_moving_composite",
                    items = {params.InvisibilityMoving, params.InvisibilityMovingAtShot},
                },
                { 
                    type = "float", 
                    name = "UI_camo_fire_penalty", 
                    value = "/invisibility/firePenalty", 
                    units = "UI_percent_format", 
                    weight = -1.0 
                },
                { 
                    type = "float", 
                    name = "UI_camo_bonus", 
                    value = "/invisibility/camouflageBonus", 
                    units = "UI_percent_format", 
                    className = "mt-2", 
                    filter = function() 
                        return platformCode ~= "blitz" and platformCode ~= "tanksblitz"
                    end 
                },
                { 
                    type = "float", 
                    name = "UI_camo_net_bonus", 
                    value = "/invisibility/camouflageNetBonus", 
                    units = "UI_percent_format", 
                    filter = function() 
                        return platformCode ~= "pc" and platformCode ~= "mirtankov"
                    end 
                },
                ]]
            }
        },
    },
}

params.MaxAmmoGun = {
    type = "transform",
    name = "UI_max_ammo",
    items = {
        { type = "component", component = "gun", item = { type = "int", value = "/maxAmmo" } },
        { type = "int", value = "/miscAttrs/descrAttrs/gun/maxAmmo", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 0 },
}

params.AmmoParams = {
    icon = "Specs-Ammo-AP-",
    title = "UI_ammo",
    column = 3,
    tag = "ammo",
    items = {
        params.DPM,
        {
            type = "shell", 
            name = "UI_caliber", 
            item = { 
                type = "float", 
                value = "/caliber", 
                units = "UI_millimeters_format"
            } 
        },
        params.MaxAmmoGun,
        {
            type = "group",
            value = params.ClipCount,
            allowEmpty = false,
            items = {
                params.InterClipReload,
                params.ExtraShotClipExtraReloadTime,
                params.ClipBurstTime,
            }
        },
        {
            type = "group",
            value = params.Burst,
            allowEmpty = false,
            items = {
                params.InterBurstReload,
            }
        },
        {
            type = "composite",
            name = "UI_pen_composite",
            items = {params.PiercingPower100, params.PiercingPower500},
            className = "mt-2"
        },
        {
            type = "composite",
            name = "UI_damage_composite",
            items = {params.DamageArmor, params.DamageModules}
        },
        params.DamageArmor500,
        params.DamageArmorSplash,
        { 
            type = "shell", 
            name = "UI_explosion_radius", 
            item = { 
                type = "float", 
                value = "/explosionRadius", 
                units = "UI_meters_format" 
            } 
        },
        params.ShellPrice,
        params.ShellVelocity,
        params.ShellAcceleration,
        params.PiercingPowerLossFactorByDistance,
        params.ShellNormalizationAngle,
        params.ShellRicochetAngle,
        params.ShellEffectiveDistance,
    }
}


params.CrewParams = {
    icon = "Modules-Crew",
    title = "UI_crew_members",
    column = 2,
    tag = "crew",
    items = {
    }
}


params.TemperatureMechanicsParams = {
    icon = "Modules-Gun-",
    title = "UI_overheat",
    column = 3,
    items = {
        params.TemperatureMechanicsTotalHeatingTime,
        params.TemperatureMechanicsCoolingTimeOverheat,
        params.TemperatureMechanicsCoolingTime,
        params.TemperatureMechanicsCoolingDelay,
        params.TemperatureMechanicsAvgDamage,
        params.TemperatureMechanicsHeatingTimes,
        params.TemperatureMechanicsCoolingTimes,
        params.TemperatureMechanicsDamageModifiers,
        params.TemperatureMechanicsDispersionModifiers,
    },
    filter = function(specs) return params.getItemValue(params.TemperatureMechanicsStates, specs, false)[1] end
}


-- Group all Tier XI mechanics into appropriate categories

-- StationaryReload Group
params.StationaryReloadParams = {
    icon = "Modules-Gun-",
    title = "UI_stationary_reload",
    column = 3,
    items = {
        params.StationaryReloadPreparingDelay,
        params.StationaryReloadFinishingDelay,
    },
    filter = function(specs)  return params.getItemValue(params.StationaryReloadPreparingDelay, specs, false)[1] end
}

-- ChargeShot Group
params.ChargeShotParams = {
    icon = "Modules-Gun-",  -- Adjust icon as needed
    title = "UI_charge_shot",
    column = 3,
    items = {
        params.ChargeShotShotBlockTime,
        params.ChargeShotTimePerLevel,
        params.ChargeShotDamageFactorsPerLevel,
    },
    filter = function(specs) return params.getItemValue(params.ChargeShotTimePerLevel, specs, false)[1] end
}

-- TargetDesignator Group
params.TargetDesignatorParams = {
    icon = "Event-Chat-ArtyAttack",
    title = "UI_target_designator",
    column = 3,
    items = {
        params.TargetDesignatorDeployTime,
        params.TargetDesignatorCooldownTime,
        {
            type = "group",
            value = params.TargetDesignatorSpottedMarkedTime,
            items = {params.TargetDesignatorUnspottedMarkedTime}
        },
        params.TargetDesignatorDamageIncomeFactor,
    },
    filter = function(specs) return params.getItemValue(params.TargetDesignatorDeployTime, specs, false)[1] end
}

-- AccuracyStacks Group
params.AccuracyStacksParams = {
    icon = "Modules-Gun-",
    title = "UI_accuracy_stacks",
    column = 3,
    items = {
        params.AccuracyStacksGainMaxSpd,
        params.AccuracyStacksGainTime,
        params.AccuracyStacksAimLevelBonus,
        params.AccuracyStacksStabilizeBonus,
    },
    filter = function(specs) return params.getItemValue(params.AccuracyStacksLevelMax, specs, false)[1] end
}

-- OverheatStacks Group
params.OverheatStacksParams = {
    icon = "Modules-Gun-",
    title = "UI_overheat_stacks",
    column = 3,
    items = {
        params.OverheatStacksHeatingTime,
        params.OverheatStacksCoolingTime,
        params.OverheatStacksDelayTimerDuration,
        params.OverheatStacksDmgBonus,
        params.OverheatStacksAimBonus,
        params.OverheatStacksGainMaxSpd,
    },
    filter = function(specs) return params.getItemValue(params.OverheatStacksHeatingTime, specs, false)[1] end
}

-- ConcentrationMode Group
params.ConcentrationModeParams = {
    icon = "Modules-Gun-",  -- Adjust icon as needed
    title = "UI_concentration_mode",
    column = 3,
    items = {
        params.ConcentrationModeDeployTime,
        params.ConcentrationModeDuration,
        params.ConcentrationModeReloadTime,
    },
    filter = function(specs) return params.getItemValue(params.ConcentrationModeDeployTime, specs, false)[1] end
}

-- BattleFury Group
params.BattleFuryParams = {
    icon = "Modules-Gun-",  -- Adjust icon as needed
    title = "UI_battle_fury",
    column = 3,
    items = {
        params.BattleFuryDuration,
        params.BattleFuryReloadSpdBonus,
    },
    filter = function(specs) return params.getItemValue(params.BattleFuryMaxLevel, specs, false)[1] end
}

-- RechargeableNitro Group
params.RechargeableNitroParams = {
    icon = "Modules-Engine-1",  -- Adjust icon as needed
    title = "UI_rechargeable_nitro",
    column = 3,
    items = {
        params.RechargeableNitroReloadTime,
        params.RechargeableNitroDuration,
        params.RechargeableNitroCooldown,
    },
    filter = function(specs) return params.getItemValue(params.RechargeableNitroReloadTime, specs, false)[1] end
}

-- StagedJetBoosters Group
params.StagedJetBoostersParams = {
    icon = "Modules-Engine-1",  -- Adjust icon as needed
    title = "UI_staged_jet_boosters",
    column = 3,
    items = {
        params.StagedJetBoostersDeployTime,
        params.StagedJetBoostersReloadTime,
        params.StagedJetBoostersDuration,
        params.StagedJetBoostersReuseCount,
    },
    filter = function(specs) return params.getItemValue(params.StagedJetBoostersDeployTime, specs, false)[1] end
}

-- RocketAcceleration Group
params.RocketAccelerationParams = {
    icon = "Modules-Engine-1",
    title = "UI_rocket_acceleration",
    column = 3,
    items = {
        params.RocketAccelerationDeployTime,
        params.RocketAccelerationReloadTime,
        params.RocketAccelerationDuration,
        params.RocketAccelerationReuseCount,
    },
    filter = function(specs) return params.getItemValue(params.RocketAccelerationDeployTime, specs, false)[1] end
}

-- PowerMode Group
params.PowerModeParams = {
    icon = "Modules-Engine-1",  -- Adjust icon as needed
    title = "UI_power_mode",
    column = 3,
    items = {
        params.PowerModeSpeedThreshold,
        params.PowerModeModeThreshold,
        params.PowerModeModeDuration,
        params.PowerModeEnginePowerStat,
        params.PowerModeVehicleSpeedStat,
    },
    filter = function(specs) return params.getItemValue(params.PowerModeSpeedThreshold, specs, false)[1] end
}

-- PillboxSiegeMode Group
params.PillboxSiegeModeParams = {
    icon = "Modules-Tracks-",  -- Adjust icon as needed
    title = "UI_pillbox_siege_mode",
    column = 3,
    items = {
        params.PillboxSiegeModeSwitchDriveToPillboxTime,
        params.PillboxSiegeModeSwitchPillboxToDriveTime,
        params.PillboxSiegeModeSwitchSiegeToPillboxTime,
        params.PillboxSiegeModeSwitchPillboxToSiegeTime,
    },
    filter = function(specs) return params.getItemValue(params.PillboxSiegeModeSwitchDriveToPillboxTime, specs, false)[1] end
}

-- StanceDance Group
params.StanceDanceParams = {
    icon = "Modules-Gun-",
    title = "UI_stance_dance",
    column = 3,
    items = {
        -- Stance switching
        params.StanceDanceTimeSwitchStance,
        {
            type = "group",
            value = params.StanceDanceActiveTurboDuration,
            items = {
                params.StanceDanceGainTurboEnergyPoints,
                params.StanceDanceGainTurboEnergyBonusPoints,
                params.StanceDanceGainTurboEnergySpdLimitKmh,
            },
        },
        {
            type = "group",
            value = params.StanceDanceActiveFightDuration,
            items = {
                params.StanceDanceGainFightEnergyPoints,
                params.StanceDancePassiveFightEnergyBonusPerHit,
            },
        },
    },
    filter = function(specs) return params.getItemValue(params.StanceDanceTimeSwitchStance, specs, false)[1] end
}

params.LowChargeShotParams = {
    icon = "Modules-Gun-",
    title = "UI_low_charge_shot",
    column = 3,
    items = {
        params.LowChargeShotAlmostFinishedTime,
        params.LowChargeShotReloadTime,
    },
    filter = function(specs) return params.getItemValue(params.LowChargeShotAlmostFinishedTime, specs, false)[1] end
}

params.PropellantAfterburnerGunParams = {
    icon = "Modules-Gun-",
    title = "UI_propellant_afterburner_gun",
    column = 3,
    items = {
        params.PropellantAfterburnerGunChargingPerSec,
        params.PropellantAfterburnerGunChargeDelay,
        params.PropellantAfterburnerGunDischargingPerSec,
        params.PropellantAfterburnerGunOverchargeSwitchCooldown,
        params.PropellantAfterburnerGunMinDamage,
        params.PropellantAfterburnerGunMaxDamage,
    },
    filter = function(specs) return params.getItemValue(params.PropellantAfterburnerGunChargingPerSec, specs, false)[1] end
}

params.WheeledDashParams = {
    icon = "Modules-Engine-1",
    title = "UI_wheeled_dash",
    column = 3,
    items = {
        params.WheeledDashDeployTime,
        params.WheeledDashReloadTime,
        params.WheeledDashDuration,
    },
    filter = function(specs) return params.getItemValue(params.WheeledDashDeployTime, specs, false)[1] end
}

params.SightPointerLevelMax = {
    type = "int",
    value = "/sightPointer/levelMax",
}

params.SightPointerInitialDeployTime = {
    type = "transform",
    name = "UI_deploy_time",
    items = {
        { type = "float", value = "/sightPointer/initialDeployTime" },
        { type = "float", value = "/miscAttrs/sightPointer/initialDeployTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.SightPointerReloadTime = {
    type = "transform",
    name = "UI_reload_time",
    items = {
        { type = "float", value = "/sightPointer/reloadTime" },
        { type = "float", value = "/miscAttrs/sightPointer/reloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.SightPointerSelfRevealVisionTime = {
    type = "transform",
    name = "UI_self_reveal_time",
    items = {
        { type = "float", value = "/sightPointer/selfRevealVisionTime" },
        { type = "float", value = "/miscAttrs/sightPointer/selfRevealVisionTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.SightPointerDuration = {
    type = "float",
    name = "UI_duration",
    value = "/sightPointer/duration",
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
    className = "mt-2",
}

params.SightPointerAngle = {
    type = "float",
    name = "UI_angle",
    value = "/sightPointer/angle",
    units = "UI_degrees_format",
    format = { maximumFractionDigits = 1 },
}

params.SightPointerInConeVisionRadiusFactor = {
    type = "float",
    name = "UI_cone_view_range",
    value = "/miscAttrs/inConeVision/circularVisionRadiusFactor",
    units = "UI_factor_format",
    format = { maximumFractionDigits = 2 },
}

params.SightPointerInConeDemaskFoliageFactor = {
    type = "float",
    name = "UI_foliage_demask",
    value = "/miscAttrs/inConeVision/demaskFoliageFactor",
    units = "UI_factor_format",
    format = { maximumFractionDigits = 2 },
}

params.SightPointerInConeDemaskMovingFactor = {
    type = "float",
    name = "UI_moving_demask",
    value = "/miscAttrs/inConeVision/demaskMovingFactor",
    units = "UI_factor_format",
    format = { maximumFractionDigits = 2 },
}

params.SightPointerParams = {
    icon = "TankSelector-Equipment-CoatedOptics",
    title = "UI_sight_pointer",
    column = 3,
    items = {
        params.SightPointerInitialDeployTime,
        params.SightPointerReloadTime,
        params.SightPointerSelfRevealVisionTime,
        params.SightPointerDuration,
        params.SightPointerAngle,
        params.SightPointerInConeVisionRadiusFactor,
        params.SightPointerInConeDemaskFoliageFactor,
        params.SightPointerInConeDemaskMovingFactor,
    },
    filter = function(specs) return params.getItemValue(params.SightPointerInitialDeployTime, specs, false)[1] end
}

params.BustleFeedActivationTime = {
    type = "float",
    name = "UI_activation",
    value = "/bustleFeed/activationTime",
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.BustleFeedDeactivationTime = {
    type = "float",
    name = "UI_deactivation",
    value = "/bustleFeed/deactivationTime",
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.BustleFeedShotReloadFactor = {
    type = "transform",
    name = "UI_reload_factor",
    items = {
        { type = "float", value = "/bustleFeed/bustleShotReloadFactor" },
        { type = "float", value = "/miscAttrs/bustleFeed/bustleShotReloadFactor", default = 1.0 },
    },
    fn = function(v) return v[1] * v[2] end,
    format = { maximumFractionDigits = 2 },
}

params.BustleFeedShotDamageBonusShell0 = {
    type = "transform",
    name = "UI_shell_0_damage_bonus",
    items = {
        { type = "float", value = "/bustleFeed/bustleShotDamageBonusShell0" },
        { type = "float", value = "/miscAttrs/bustleFeed/bustleShotDamageBonusShell0", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.BustleFeedShotDamageBonusShell1 = {
    type = "transform",
    name = "UI_shell_1_damage_bonus",
    items = {
        { type = "float", value = "/bustleFeed/bustleShotDamageBonusShell1" },
        { type = "float", value = "/miscAttrs/bustleFeed/bustleShotDamageBonusShell1", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.BustleFeedParams = {
    icon = "Modules-Gun-",
    title = "UI_bustle_feed",
    column = 3,
    items = {
        params.BustleFeedActivationTime,
        params.BustleFeedDeactivationTime,
        params.BustleFeedShotReloadFactor,
        params.BustleFeedShotDamageBonusShell0,
        params.BustleFeedShotDamageBonusShell1,
    },
    filter = function(specs) return params.getItemValue(params.BustleFeedActivationTime, specs, false)[1] end
}

params.AutoreloaderSurgeMaxCharges = {
    type = "int",
    name = "UI_max_charges",
    value = "/autoreloaderSurge/maxCharges",
}

params.AutoreloaderSurgeStartCharges = {
    type = "transform",
    name = "UI_start_charges",
    items = {
        { type = "int", value = "/autoreloaderSurge/startCharges" },
        { type = "float", value = "/miscAttrs/autoreloaderSurge/startCharges", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
}

params.AutoreloaderSurgeChargeTimeSRegular = {
    type = "transform",
    name = "UI_charge_time_regular",
    items = {
        { type = "float", value = "/autoreloaderSurge/chargeTimeSRegular" },
        { type = "float", value = "/miscAttrs/autoreloaderSurge/chargeTimeSRegular", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.AutoreloaderSurgeChargeTimeSFullClip = {
    type = "transform",
    name = "UI_charge_time_full_clip",
    items = {
        { type = "float", value = "/autoreloaderSurge/chargeTimeSFullClip" },
        { type = "float", value = "/miscAttrs/autoreloaderSurge/chargeTimeSFullClip", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.AutoreloaderSurgeReloadTime = {
    type = "transform",
    name = "UI_reload_time",
    items = {
        { type = "float", value = "/autoreloaderSurge/reloadTime" },
        { type = "float", value = "/miscAttrs/autoreloaderSurge/reloadTime", default = 0.0 },
    },
    fn = function(v) return v[1] + v[2] end,
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 1 },
}

params.AutoreloaderSurgeParams = {
    icon = "Modules-Gun-",
    title = "UI_autoreloader_surge",
    column = 3,
    items = {
        params.AutoreloaderSurgeMaxCharges,
        params.AutoreloaderSurgeStartCharges,
        params.AutoreloaderSurgeChargeTimeSRegular,
        params.AutoreloaderSurgeChargeTimeSFullClip,
        params.AutoreloaderSurgeReloadTime,
    },
    filter = function(specs) return params.getItemValue(params.AutoreloaderSurgeMaxCharges, specs, false)[1] end
}


-- Merge battle/stat params
do
    local StatParams = require("rml.scripts.stat_params")
    for k, v in pairs(StatParams) do
        params[k] = v
    end
end

-- Configuration arrays
params.SpecsConfiguration = {
    params.GeneralParams,
    params.AimingTimeParams,
    params.WeaponryParams,
    params.MobilityParams,
    params.EconomicsParams,
    params.AmmoParams,

    params.TemperatureMechanicsParams,
    params.ChargeShotParams,
    params.TargetDesignatorParams,
    params.AccuracyStacksParams,
    params.OverheatStacksParams,
    params.ConcentrationModeParams,
    params.BattleFuryParams,
    params.RechargeableNitroParams,
    params.StagedJetBoostersParams,
    params.RocketAccelerationParams,
    params.PowerModeParams,
    params.PillboxSiegeModeParams,
    params.StanceDanceParams,
    params.StationaryReloadParams,
    params.LowChargeShotParams,
    params.PropellantAfterburnerGunParams,
    params.WheeledDashParams,
    params.SightPointerParams,
    params.BustleFeedParams,
    params.AutoreloaderSurgeParams,

    params.CrewParams,
    params.CamouflageParams,
}

-- Format function
function params.format(num, units, format)
    if type(num) == "string" then
        return num
    elseif type(num) == "table" then
        local result = {}
        for i, v in ipairs(num) do
            if i > 1 then
                table.insert(result, " | ")
            end
            table.insert(result, params.format(v[1], v[2], v[3]))
        end
        return table.concat(result)
    elseif not num or num ~= num then -- Check for NaN
        return "--"
    else
        local value
        local sign = format and format.signDisplay == "always" and "+" or ""
        if num % 1 ~= 0 then
            value = format and format.maximumFractionDigits and fmt("{:" .. sign .. "." .. format.maximumFractionDigits .. "Lf}", num) or fmt("{:" .. sign .. ".3Lg}", num)
        else
            value = fmt("{:" .. sign .. "L}", num)
        end
        if units then
            return fmt(units, value)
        else
            return value
        end
    end
end


function params.getItemValue(item, specs, effectiveValues)
    if item.type == "string" then
        return {specs[item.value]}
    elseif item.type == "int" or item.type == "float" then
        local v = not effectiveValues and item.default or specs[item.value]
        return {v and tonumber(v) or item.default, item.units, item.format}
    elseif item.type == "price" then
        if not specs[item.value] then
            return {nil}
        end
        local v = tonumber(specs[item.value])
        local currency = specs[item.currency.value]
        return {v, currency == '1' and "UI_gold_price_format" or nil, item.format}
    elseif item.type == "component" then
        local componentId = specs["/" .. item.component .. "/id"]
        if not componentId then return {nil} end
        local prefix = item.component == "gun" and "/turret/" .. specs["/turret/id"] or ""
        local componentItem = {
            type = item.item.type,
            value = prefix .. "/" .. item.component .. "/" .. componentId .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
        }
        return params.getItemValue(componentItem, specs, effectiveValues)
    elseif item.type == "shell" then
        local shellIdx = specs["/shellIdx"] or "0"
        local gunId = specs["/gun/id"]
        local turretId = specs["/turret/id"]
        if not gunId or not turretId then return {nil} end
        local shellItem = {
            type = item.item.type,
            value = "/turret/" .. turretId .. "/gun/" .. gunId .. "/shell" .. shellIdx .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
        }
        return params.getItemValue(shellItem, specs, effectiveValues)
    elseif item.type == "transform" then
        if not effectiveValues and item.default then
            return {item.default, item.units, item.format}
        end

        local transformParams = {}
        for i = 1, #item.items do
            local v = params.getItemValue(item.items[i], specs, effectiveValues)
            if not v[1] and not item.allowUndefined then
                return {nil}
            end
            transformParams[i] = v[1]
        end
        return {item.fn(transformParams), item.units, item.format}
    elseif item.type == "composite" then
        local compositeParams = {}
        for i = 1, #item.items do
            local v = params.getItemValue(item.items[i], specs, effectiveValues)
            if not v[1] then
                return {nil}
            end
            compositeParams[i] = v
        end
        return {compositeParams, item.units}
    elseif item.type == "economics" then
        local econType = specs["/economics/type"] or "all"
        local econItem = {
            type = item.item.type,
            value = "/stats/economics/" .. econType .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
            allowUndefined = item.item.allowUndefined,
        }
        return params.getItemValue(econItem, specs, effectiveValues)
    else
        return {nil}
    end
end

function params.getItemWeight(item, specs, effectiveValues)
    if item.type == "string" then
        if item.weight then
            local value = params.getItemValue(item, specs, effectiveValues)[1]
            return value and item.weight(value) or nil
        end
        return nil
    elseif item.type == "int" or item.type == "float" then
        local v = params.getItemValue(item, specs, effectiveValues)[1]
        if not v then return nil end
        return v * (item.weight or 1.0)
    elseif item.type == "price" then
        local v = params.getItemValue(item, specs, effectiveValues)[1]
        if not v then return nil end
        local currency = specs[item.currency.value]
        if currency == '1' then
            return -v * 400
        end
        return -v
    elseif item.type == "component" then
        local componentId = specs["/" .. item.component .. "/id"]
        if not componentId then return nil end
        local prefix = item.component == "gun" and "/turret/" .. specs["/turret/id"] or ""
        local componentItem = {
            type = item.item.type,
            value = prefix .. "/" .. item.component .. "/" .. componentId .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
        }
        return params.getItemWeight(componentItem, specs, effectiveValues)
    elseif item.type == "shell" then
        local shellIdx = specs["/shellIdx"] or "0"
        local gunId = specs["/gun/id"]
        local turretId = specs["/turret/id"]
        if not gunId or not turretId then return nil end
        local shellItem = {
            type = item.item.type,
            value = "/turret/" .. turretId .. "/gun/" .. gunId .. "/shell" .. shellIdx .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
        }
        return params.getItemWeight(shellItem, specs, effectiveValues)
    elseif item.type == "transform" then
        if not effectiveValues and item.default then
            return item.default
        end

        local weights = {}
        for i = 1, #item.items do
            local v = params.getItemWeight(item.items[i], specs, effectiveValues)
            if not v and not item.allowUndefined then
                return nil
            end
            weights[i] = v
        end
        if item.weight then
            return item.weight(weights)
        else
            return item.fn(weights)
        end
    elseif item.type == "composite" then
        local weights = {}
        for i = 1, #item.items do
            local v = params.getItemWeight(item.items[i], specs, effectiveValues)
            if not v then
                return nil
            end
            weights[i] = v
        end
        local sum = 0
        for i, weight in ipairs(weights) do
            sum = sum + weight
        end
        return sum / #weights
    elseif item.type == "economics" then
        local econType = specs["/economics/type"] or "all"
        local econItem = {
            type = item.item.type,
            value = "/stats/economics/" .. econType .. item.item.value,
            units = item.item.units,
            format = item.item.format,
            weight = item.item.weight,
            currency = item.item.currency,
            default = item.item.default,
            allowUndefined = item.item.allowUndefined,
        }
        return params.getItemWeight(econItem, specs, effectiveValues)
    else
        return nil
    end
end


function params.getItemDifference(item, specs1, specs2, effective_values)
    local value1 = params.getItemValue(item, specs1, effective_values)[1]
    local value2 = params.getItemValue(item, specs2, effective_values)[1]

    local weight1 = params.getItemWeight(item, specs1, effective_values)
    local weight2 = params.getItemWeight(item, specs2, effective_values)

    return type(value2) == "number" and type(value1) == "number" and value2 - value1, weight1 and (weight2 or 0) - weight1 or math.abs(weight2 or 0)
end


-- Util functions
function params.getModuleHealth(device_type, specs, effective_values)
    if not specs or device_type == PlatformDB.DeviceType.ARMOR then
        return
    end

    if device_type == PlatformDB.DeviceType.ENGINE or device_type == PlatformDB.DeviceType.TRANSMISSION then
        return params.getItemValue(params.EngineHealth, specs, effective_values)[1], params.getItemValue(params.EngineHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.AMMO_BAY then
        return params.getItemValue(params.AmmorackHealth, specs, effective_values)[1], params.getItemValue(params.AmmorackHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.FUEL_TANK then
        return params.getItemValue(params.FuelTankHealth, specs, effective_values)[1], params.getItemValue(params.FuelTankHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.RADIO then
        return params.getItemValue(params.RadioHealth, specs, effective_values)[1], params.getItemValue(params.RadioHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.WHEEL or device_type == PlatformDB.DeviceType.TRACK then
        return params.getItemValue(params.TracksHealth, specs, effective_values)[1], params.getItemValue(params.TracksHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.GUN then
        return params.getItemValue(params.GunHealth, specs, effective_values)[1], params.getItemValue(params.GunHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.TURRET_ROTATOR then
        return params.getItemValue(params.TurretHealth, specs, effective_values)[1], params.getItemValue(params.TurretHealthRegen, specs, effective_values)[1]
    end
    if device_type == PlatformDB.DeviceType.SURVEYING_DEVICE then
        return params.getItemValue(params.SurveyingDeviceHealth, specs, effective_values)[1], params.getItemValue(params.SurveyingDeviceHealthRegen, specs, effective_values)[1]
    end

    return 25.0, 25.0
end

function params.getEffectiveChanceToHit(material, specs, effective_values)
    local base_chance = material.chance_to_hit_by_projectile

    if material.device_type >= PlatformDB.DeviceType.COMMANDER then
        base_chance = base_chance * params.getItemValue(factors.CrewChanceToHitFactor, specs, effective_values)[1]
    elseif material.device_type == PlatformDB.DeviceType.ENGINE then
        base_chance = base_chance * params.getItemValue(factors.EngineChanceToHitFactor, specs, effective_values)[1]
    elseif material.device_type == PlatformDB.DeviceType.AMMO_BAY then
        base_chance = base_chance * params.getItemValue(factors.AmmoBayChanceToHitFactor, specs, effective_values)[1]
    end

    return base_chance
end


return params
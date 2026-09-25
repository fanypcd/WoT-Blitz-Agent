local params = {}

local CommonParams = require("rml.scripts.common_params")

local function platformIsPcFamily()
    return platformCode == "pc" or platformCode == "mirtankov"
end

local function platformIsBlitzFamily()
    return platformCode == "blitz" or platformCode == "tanksblitz"
end

local function notConsole()
    return platformCode ~= "console"
end

params.StatsBattleCount = {
    type = "int",
    name = "UI_battle_count",
    value = "/stats/battleCount",
    format = { maximumFractionDigits = 0 },
}

params.StatsPlayerWtr = {
    type = "int",
    name = "UI_player_wtr",
    value = "/stats/playerWtr",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "pc" or platformCode == "mirtankov" end,
}

params.StatsHitpointsLeft = {
    type = "float",
    name = "UI_hitpoints_left",
    value = "/stats/hitpointsLeft",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsCredits = {
    type = "float",
    name = "UI_credits_base",
    value = "/stats/credits",
    format = { maximumFractionDigits = 0 },
}

params.StatsExperience = {
    type = "float",
    name = "UI_experience_base",
    value = "/stats/experience",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsExpOther = {
    type = "float",
    name = "UI_other_experience",
    value = "/stats/expOther",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsShotsMade = {
    type = "float",
    name = "UI_shots_made",
    value = "/stats/shotsMade",
    format = { maximumFractionDigits = 1 },
    className = "mt-2",
}

params.StatsShotsHit = {
    type = "float",
    name = "UI_shots_hit",
    value = "/stats/shotsHit",
    format = { maximumFractionDigits = 1 },
}

params.StatsShotsSplash = {
    type = "float",
    name = "UI_splash_shots",
    value = "/stats/shotsSplash",
    format = { maximumFractionDigits = 1 },
}

params.StatsShotsPen = {
    type = "float",
    name = "UI_penetrating_shots",
    value = "/stats/shotsPen",
    format = { maximumFractionDigits = 1 },
}

params.StatsDamageMade = {
    type = "float",
    name = "UI_damage_per_game",
    value = "/stats/damageMade",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageReceived = {
    type = "float",
    name = "UI_damage_received_per_game",
    value = "/stats/damageReceived",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    weight = -1.0,
}

params.StatsDamageAssisted = {
    type = "float",
    name = "UI_damage_assisted",
    value = "/stats/damageAssisted",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageAssistedTrack = {
    type = "float",
    name = "UI_track_damage_assisted",
    value = "/stats/damageAssistedTrack",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageAssistedStun = {
    type = "float",
    name = "UI_stun_damage_assisted",
    value = "/stats/damageAssistedStun",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "pc" end,
}

params.StatsDamageAssistedSmoke = {
    type = "float",
    name = "UI_smoke_damage_assisted",
    value = "/stats/damageAssistedSmoke",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "mirtankov" or platformCode == "pc" end,
}

params.StatsDamageBlocked = {
    type = "float",
    name = "UI_damage_blocked_per_game",
    value = "/stats/damageBlocked",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageSniper = {
    type = "float",
    name = "UI_sniper_damage",
    value = "/stats/damageSniper",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "mirtankov" or platformCode == "pc" end,
    className = "mt-2",
}

params.StatsHitsReceived = {
    type = "float",
    name = "UI_hits_received",
    value = "/stats/hitsReceived",
    format = { maximumFractionDigits = 1 },
    className = "mt-2",
}

params.StatsHitsBounced = {
    type = "float",
    name = "UI_hits_bounced",
    value = "/stats/hitsBounced",
    format = { maximumFractionDigits = 1 },
}

params.StatsHitsSplash = {
    type = "float",
    name = "UI_splash_hits",
    value = "/stats/hitsSplash",
    format = { maximumFractionDigits = 1 },
}

params.StatsHitsPen = {
    type = "float",
    name = "UI_penetrating_hits",
    value = "/stats/hitsPen",
    format = { maximumFractionDigits = 1 },
}

params.StatsEnemiesSpotted = {
    type = "float",
    name = "UI_enemies_spotted",
    value = "/stats/enemiesSpotted",
    className = "mt-2",
}

params.StatsEnemiesDamaged = {
    type = "float",
    name = "UI_enemies_damaged",
    value = "/stats/enemiesDamaged",
    className = "mt-2",
}

params.StatsEnemiesDestroyed = {
    type = "float",
    name = "UI_enemies_destroyed",
    value = "/stats/enemiesDestroyed",
}

params.StatsEnemiesStunned = {
    type = "float",
    name = "UI_enemies_stunned",
    value = "/stats/enemiesStunned",
    filter = function() return platformCode == "pc" end,
}

params.StatsTimeAlive = {
    type = "float",
    name = "UI_time_alive",
    value = "/stats/timeAlive",
    units = "UI_seconds_format",
    className = "mt-2",
}

params.StatsDistanceTravelled = {
    type = "float",
    name = "UI_distance_travelled",
    value = "/stats/distanceTravelled",
    units = "UI_meters_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsBaseCapturePoints = {
    type = "float",
    name = "UI_base_capture_points",
    value = "/stats/baseCapturePoints",
    className = "mt-2",
}

params.StatsBaseDefendPoints = {
    type = "float",
    name = "UI_base_defend_points",
    value = "/stats/baseDefendPoints",
}

params.StatsStunDuration = {
    type = "float",
    name = "UI_stun_duration",
    value = "/stats/stunDuration",
    units = "UI_seconds_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "pc" end,
}

params.StatsGunMarkBattleExp = {
    type = "float",
    name = "UI_avg_battle_xp",
    value = "/stats/expBattle",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "blitz" end,
}

params.StatsGunMarkPercentile65 = {
    type = "float",
    name = "UI_gun_mark_i",
    value = "/stats/gunMarkPercentile65",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode ~= "mirtankov" and platformCode ~= "pc" end,
}

params.StatsGunMarkPercentile85 = {
    type = "float",
    name = "UI_gun_mark_ii",
    value = "/stats/gunMarkPercentile85",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode ~= "mirtankov" and platformCode ~= "pc" end,
}

params.StatsGunMarkPercentile95 = {
    type = "float",
    name = "UI_gun_mark_iii",
    value = "/stats/gunMarkPercentile95",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode ~= "mirtankov" and platformCode ~= "pc" end,
    className = "mt-2",
}

params.StatsGunMarkPercentile65Pc = {
    type = "float",
    name = "UI_gun_mark_i",
    value = "/stats/gunMarkPercentile65",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "mirtankov" or platformCode == "pc" end,
}

params.StatsGunMarkPercentile85Pc = {
    type = "float",
    name = "UI_gun_mark_ii",
    value = "/stats/gunMarkPercentile85",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "mirtankov" or platformCode == "pc" end,
}

params.StatsGunMarkPercentile95Pc = {
    type = "float",
    name = "UI_gun_mark_iii",
    value = "/stats/gunMarkPercentile95",
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    filter = function() return platformCode == "mirtankov" or platformCode == "pc" end,
    className = "mt-2",
}

params.StatsBadgeMastery = {
    type = "float",
    name = "UI_ace_tanker",
    value = "/stats/expBadgeMastery",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsBadge1stClass = {
    type = "float",
    name = "UI_1st_class",
    value = "/stats/expBadge1st",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsBadge2ndClass = {
    type = "float",
    name = "UI_2nd_class",
    value = "/stats/expBadge2nd",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsBadge3rdClass = {
    type = "float",
    name = "UI_3rd_class",
    value = "/stats/expBadge3rd",
    units = "UI_xp_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsCreditsFactor = {
    type = "float",
    name = "UI_credits_factor",
    value = "/stats/creditsFactor",
    format = { maximumFractionDigits = 2 },
    filter = function() return platformCode ~= "console" end,
    className = "mt-2",
}

params.StatsAvgDistanceOfDamageDealt = {
    type = "int",
    name = "UI_avg_firing_range",
    value = "/stats/shooter/avgFiringRange",
    units = "UI_meters_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsAvgDistanceOfDamageTaken = {
    type = "int",
    name = "UI_avg_firing_range",
    value = "/stats/target/avgFiringRange",
    units = "UI_meters_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsTotalShotsReceived = {
    type = "int",
    name = "UI_total_shots",
    value = "/stats/target/totalShots",
    format = { maximumFractionDigits = 0 },
}

params.StatsTotalShotsFired = {
    type = "int",
    name = "UI_total_shots",
    value = "/stats/shooter/totalShots",
    format = { maximumFractionDigits = 0 },
}

params.StatsProcessedShotsCount = {
    type = "transform",
    name = "UI_processed_shots",
    items = {
        params.StatsTotalShotsReceived,
        params.StatsTotalShotsFired,
    },
    fn = function(v) return v[1] + v[2] end,
    format = { maximumFractionDigits = 0 },
    className = "mb-2",
}

params.StatsDeathReasonTotal = {
    type = "int",
    name = "UI_death_factors",
    value = "/stats/deathReasonTotal",
    format = { maximumFractionDigits = 0 },
}

params.StatsKillReasonTotal = {
    type = "int",
    name = "UI_kill_factors",
    value = "/stats/killReasonTotal",
    format = { maximumFractionDigits = 0 },
}

params.BattleTotalShotsForModuleStatsAsShooter = {
    type = "float",
    name = "UI_total_penetrated_shots",
    value = "/battle/shooter/type/25/shots",
}

params.BattleTotalShotsForModuleStatsAsTarget = {
    type = "float",
    name = "UI_total_penetrated_shots",
    value = "/battle/target/type/25/shots",
}

params.StatsHitpointsLeftPercent = {
    type = "transform",
    name = "UI_hitpoints_left",
    items = {
        params.StatsHitpointsLeft,
        CommonParams.MaxHealth,
    },
    fn = function(v) return (v[1]  and  v[2] > 0.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsAvgDamageDealtPerGamePercent = {
    type = "transform",
    name = "UI_dmg_dealt_of_max_hp",
    items = {
        params.StatsDamageMade,
        CommonParams.MaxHealth,
    },
    fn = function(v) return (v[1]  and  v[2] > 0.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageReceivedPerGamePercent = {
    type = "transform",
    name = "UI_dmg_received_of_max_hp",
    items = {
        params.StatsDamageReceived,
        CommonParams.MaxHealth,
    },
    fn = function(v) return (v[1]  and  v[2] > 0.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsAvgDamageBlockedPerGamePercent = {
    type = "transform",
    name = "UI_dmg_blocked_of_max_hp",
    items = {
        params.StatsDamageBlocked,
        CommonParams.MaxHealth,
    },
    fn = function(v) return (v[1]  and  v[2] > 0.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsWinrate = {
    type = "transform",
    name = "UI_winrate",
    items = {
        { type = "float", value = "/stats/winrate" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsAccuracy = {
    type = "transform",
    name = "UI_accuracy",
    items = {
        params.StatsShotsHit,
        params.StatsShotsMade,
    },
    fn = function(v) return (v[2] > 0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDpm = {
    type = "transform",
    name = "UI_real_dpm",
    items = {
        params.StatsDamageMade,
        params.StatsTimeAlive,
    },
    fn = function(v) return (v[2] > 0) and (v[1] / v[2] * 60.0) or (nil) end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    className = "mt-2",
}

params.StatsFragsPerMinute = {
    type = "transform",
    name = "UI_frags_per_minute",
    items = {
        params.StatsEnemiesDestroyed,
        params.StatsTimeAlive,
    },
    fn = function(v) return (v[2] > 0) and (v[1] / v[2] * 60.0) or (nil) end,
    units = "UI_per_minute_format",
}

params.StatsAssistRatio = {
    type = "transform",
    name = "UI_assist_ratio",
    items = {
        params.StatsDamageMade,
        params.StatsDamageAssisted,
    },
    fn = function(v) return (v[1] > 0.0  and  v[2] > 0.0) and (v[2] / (v[1] + v[2]) * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsAssistRatioPercent = {
    type = "transform",
    name = "UI_assist_ratio_of_max_hp",
    items = {
        params.StatsDamageAssisted,
        CommonParams.MaxHealth,
    },
    fn = function(v) return (v[1]  and  v[2] > 0.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsEconomicsTotalCredits = {
    type = "economics",
    name = "UI_total_credits",
    item = {
        type = "float",
        value = "/totalCredits",
        format = { maximumFractionDigits = 0 },
    },
}

params.StatsEconomicsRepairCost = {
    type = "economics",
    name = "UI_repair_cost",
    item = {
        type = "float",
        value = "/repairCost",
        format = { maximumFractionDigits = 0 },
    },
}

params.StatsEconomicsAmmoCost = {
    type = "economics",
    name = "UI_ammo_cost",
    item = {
        type = "float",
        value = "/ammoCost",
        format = { maximumFractionDigits = 0 },
    },
}

params.StatsEconomicsEquipmentCost = {
    type = "economics",
    name = "UI_equipment_cost",
    item = {
        type = "float",
        value = "/equipmentCost",
        format = { maximumFractionDigits = 0 },
    },
}

params.StatsEconomicsProfit = {
    type = "transform",
    name = "UI_profit",
    items = {
        params.StatsEconomicsTotalCredits,
        params.StatsEconomicsRepairCost,
        params.StatsEconomicsAmmoCost,
        params.StatsEconomicsEquipmentCost,
    },
    allowUndefined = true,
    fn = function(v) 
        if v[1] == nil then
            return nil
        end
        return v[1] + (v[2]  or  0) + (v[3]  or  0) + (v[4]  or  0)
    end,
    format = { maximumFractionDigits = 0 },
}

params.StatsEconomicsProfitPerMinute = {
    type = "transform",
    name = "UI_profit_min",
    items = {
        params.StatsEconomicsProfit,
        params.StatsTimeAlive,
    },
    fn = function(v) return v[1] / v[2] * 60.0 end,
    units = "UI_per_minute_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDamageBouncedPercentage = {
    type = "transform",
    name = "UI_total_damage_bounced",
    items = {
        { type = "float", value = "/stats/target/damageBounced" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsPremiumAmmoHitPercentage = {
    type = "transform",
    name = "UI_premium_ammo_hit",
    items = {
        { type = "float", value = "/stats/target/premiumAmmoUsed" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsHeAmmoHitPercentage = {
    type = "transform",
    name = "UI_he_ammo_used",
    longName = "UI_hits_with_he_ammo",
    items = {
        { type = "float", value = "/stats/target/heAmmoUsed" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsPremiumAmmoUsedPercentage = {
    type = "transform",
    name = "UI_premium_ammo_used",
    items = {
        { type = "float", value = "/stats/shooter/premiumAmmoUsed" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsHeAmmoUsedPercentage = {
    type = "transform",
    name = "UI_he_ammo_used",
    items = {
        { type = "float", value = "/stats/shooter/heAmmoUsed" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 0 },
}

params.StatsDeathReasonFire = {
    type = "transform",
    name = "UI_deaths_by_fire",
    items = {
        { type = "float", value = "/stats/deathReasonFire" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsDeathReasonRamming = {
    type = "transform",
    name = "UI_deaths_by_ramming",
    items = {
        { type = "float", value = "/stats/deathReasonRamming" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsDeathReasonFall = {
    type = "transform",
    name = "UI_deaths_by_fall",
    items = {
        { type = "float", value = "/stats/deathReasonFall" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsDeathReasonDrowning = {
    type = "transform",
    name = "UI_deaths_by_drowning",
    items = {
        { type = "float", value = "/stats/deathReasonDrowning" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsKillReasonFire = {
    type = "transform",
    name = "UI_kills_by_fire",
    items = {
        { type = "float", value = "/stats/killReasonFire" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
    className = "mt-2",
}

params.StatsKillReasonRamming = {
    type = "transform",
    name = "UI_kills_by_ramming",
    items = {
        { type = "float", value = "/stats/killReasonRamming" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsKillReasonFall = {
    type = "transform",
    name = "UI_kills_by_fall",
    items = {
        { type = "float", value = "/stats/killReasonFall" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsKillReasonDrowning = {
    type = "transform",
    name = "UI_kills_by_drowning",
    items = {
        { type = "float", value = "/stats/killReasonDrowning" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    format = { maximumFractionDigits = 1 },
}

params.StatsAvgDamageReceivedPerShot = {
    type = "transform",
    name = "UI_damage_received_per_shot",
    items = {
        params.StatsDamageReceived,
        params.StatsHitsReceived,
    },
    fn = function(v) return (v[2] > 0.0) and (v[1] / v[2]) or (nil) end,
    weight = function(v) return (v[2] > 0.0) and (-v[1] / v[2]) or (nil) end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    className = "mt-2",
}

params.StatsAvgDamageDealtPerShot = {
    type = "transform",
    name = "UI_damage_per_shot",
    items = {
        params.StatsDamageMade,
        params.StatsShotsHit,
    },
    fn = function(v) return (v[2] > 0.0) and (v[1] / v[2]) or (nil) end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    className = "mt-2",
}

params.StatsAvgDamageBlockedPerShot = {
    type = "transform",
    name = "UI_damage_blocked_per_shot",
    items = {
        params.StatsDamageBlocked,
        params.StatsHitsReceived,
    },
    fn = function(v) return (v[2] > 0.0) and (v[1] / v[2]) or (nil) end,
    units = "UI_hp_format",
    format = { maximumFractionDigits = 0 },
    className = "mt-2",
}

params.StatsChanceToSetOnFirePerShot = {
    type = "transform",
    name = "UI_chance_to_set_on_fire",
    items = {
        { type = "float", value = "/stats/shooter/setOnFire" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsChanceToSetOnFirePerGame = {
    type = "transform",
    name = "UI_fires_per_game",
    items = {
        { type = "float", value = "/stats/shooter/setOnFire" },
        params.StatsShotsHit,
    },
    fn = function(v) return v[1] * v[2] end,
}

params.StatsShooterInternalCritsPerShot = {
    type = "transform",
    name = "UI_internal_module_crits",
    longName = "UI_internal_module_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitInternalModule" },
    },
    fn = function(v) return (v[1] > 0) and (v[1] * 100.0) or (nil) end,
    units = "UI_percent_format",
}

params.StatsShooterInternalCritsPerGame = {
    type = "transform",
    name = "UI_internal_crits_per_game",
    longName = "UI_internal_crits_per_game_dealt",
    items = {
        { type = "float", value = "/stats/shooter/avgHitInternalModule" },
        params.StatsShotsHit,
    },
    fn = function(v) return (v[1] > 0) and (v[1] * v[2]) or (nil) end,
    className = "mb-2",
}

params.StatsShooterAmmoRackCritsPerShot = {
    type = "transform",
    name = "UI_ammorack",
    longName = "UI_ammorack_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitAmmorack" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    className = "mt-2",
}

params.StatsShooterEngineCritsPerShot = {
    type = "transform",
    name = "UI_engine",
    longName = "UI_engine_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitEngine" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterFuelTankCritsPerShot = {
    type = "transform",
    name = "UI_fueltank",
    longName = "UI_fueltank_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitFueltank" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterRadioCritsPerShot = {
    type = "transform",
    name = "UI_radio",
    longName = "UI_radio_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitRadio" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    filter = function() return platformCode == "pc" end,
}

params.StatsShooterTracksCritsPerShot = {
    type = "transform",
    name = "UI_tracks",
    longName = "UI_tracks_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitTrack" },
    },
    fn = function(v) return (v[1] > 0.0) and (v[1] * 100.0) or (nil) end,
    units = "UI_percent_format",
}

params.StatsShooterGunCritsPerShot = {
    type = "transform",
    name = "UI_gun",
    longName = "UI_gun_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitGun" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterTurretCritsPerShot = {
    type = "transform",
    name = "UI_turret",
    longName = "UI_turret_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitTurret" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterSurveyingDeviceCritsPerShot = {
    type = "transform",
    name = "UI_surveying_device",
    longName = "UI_surveying_device_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitSurveyingDevice" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterCommanderCritsPerShot = {
    type = "transform",
    name = "UI_commander",
    longName = "UI_commander_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitCommander" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    className = "mt-2",
}

params.StatsShooterDriverCritsPerShot = {
    type = "transform",
    name = "UI_driver",
    longName = "UI_driver_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitDriver" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterRadioOperatorCritsPerShot = {
    type = "transform",
    name = "UI_radio_operator",
    longName = "UI_radio_operator_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitRadioOperator" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    filter = function() return platformCode == "pc" end,
}

params.StatsShooterGunnerCritsPerShot = {
    type = "transform",
    name = "UI_gunner",
    longName = "UI_gunner_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitGunner" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterLoaderCritsPerShot = {
    type = "transform",
    name = "UI_loader",
    longName = "UI_loader_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitLoader" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsShooterWheelCritsPerShot = {
    type = "transform",
    name = "UI_wheel",
    longName = "UI_wheel_crits_per_shot",
    items = {
        { type = "float", value = "/stats/shooter/avgHitWheel" },
    },
    fn = function(v) return (v[1] > 0) and (v[1] * 100.0) or (nil) end,
    units = "UI_percent_format",
}

params.StatsTargetInternalCritsPerShot = {
    type = "transform",
    name = "UI_internal_module_crits",
    longName = "UI_internal_module_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitInternalModule", weight = -1.0 },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetInternalCritsPerGame = {
    type = "transform",
    name = "UI_internal_crits_per_game",
    longName = "UI_internal_crits_received_per_game",
    items = {
        { type = "float", value = "/stats/target/avgHitInternalModule", weight = -1.0 },
        params.StatsHitsReceived,
    },
    fn = function(v) return (v[1]) and (v[1] * v[2]) or (nil) end,
    className = "mb-2",
}

params.StatsTargetAmmoRackCritsPerShot = {
    type = "transform",
    name = "UI_ammorack",
    longName = "UI_ammorack_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitAmmorack" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    className = "mt-2",
}

params.StatsTargetEngineCritsPerShot = {
    type = "transform",
    name = "UI_engine",
    longName = "UI_engine_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitEngine" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetFuelTankCritsPerShot = {
    type = "transform",
    name = "UI_fueltank",
    longName = "UI_fueltank_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitFueltank" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetRadioCritsPerShot = {
    type = "transform",
    name = "UI_radio",
    longName = "UI_radio_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitRadio" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    filter = function() return platformCode == "pc" end,
}

params.StatsTargetTracksCritsPerShot = {
    type = "transform",
    name = "UI_track",
    longName = "UI_tracks_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitTrack" },
    },
    fn = function(v) return (v[1] > 0) and (v[1] * 100.0) or (nil) end,
    units = "UI_percent_format",
}

params.StatsTargetGunCritsPerShot = {
    type = "transform",
    name = "UI_gun",
    longName = "UI_gun_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitGun" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetTurretCritsPerShot = {
    type = "transform",
    name = "UI_turret",
    longName = "UI_turret_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitTurret" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetSurveyingDeviceCritsPerShot = {
    type = "transform",
    name = "UI_surveying_device",
    longName = "UI_surveying_device_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitSurveyingDevice" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetCommanderCritsPerShot = {
    type = "transform",
    name = "UI_commander",
    longName = "UI_commander_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitCommander" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    className = "mt-2",
}

params.StatsTargetDriverCritsPerShot = {
    type = "transform",
    name = "UI_driver",
    longName = "UI_driver_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitDriver" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetRadioOperatorCritsPerShot = {
    type = "transform",
    name = "UI_radio_operator",
    longName = "UI_radio_operator_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitRadioOperator" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
    filter = function() return platformCode == "pc" end,
}

params.StatsTargetGunnerCritsPerShot = {
    type = "transform",
    name = "UI_gunner",
    longName = "UI_gunner_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitGunner" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetLoaderCritsPerShot = {
    type = "transform",
    name = "UI_loader",
    longName = "UI_loader_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitLoader" },
    },
    fn = function(v) return v[1] * 100.0 end,
    units = "UI_percent_format",
}

params.StatsTargetWheelCritsPerShot = {
    type = "transform",
    name = "UI_wheel",
    longName = "UI_wheel_crits_per_hit_received",
    items = {
        { type = "float", value = "/stats/target/avgHitWheel" },
    },
    fn = function(v) return (v[1] > 0.0) and (v[1] * 100.0) or (nil) end,
    units = "UI_percent_format",
}

params.BattleAmmoRacksPerShot = {
    type = "transform",
    name = "UI_ammoracks_per_hit",
    items = {
        { type = "float", value = "/battle/shooter/type/10/weight" },
        params.BattleTotalShotsForModuleStatsAsShooter,
    },
    fn = function(v) return (v[2] > 1000.0) and (v[1] / v[2] * 100.0) or (nil) end,
    units = "UI_percent_format",
    filter = function() return false end,
}

params.BattleAmmoRackedPerShot = {
    type = "transform",
    name = "UI_ammoracked_per_hit",
    items = {
        { type = "float", value = "/battle/target/type/10/weight" },
        params.BattleTotalShotsForModuleStatsAsTarget,
    },
    fn = function(v) return (v[2] > 1000.0) and (v[1] / v[2] * 100.0) or (nil) end,
    weight = function(v) return (v[2] > 0.0) and (-v[1] / v[2]) or (nil) end,
    units = "UI_percent_format",
    filter = function() return false end,
}

params.BattleTakingFirePerShot = {
    type = "transform",
    name = "UI_fire_chance_per_hit",
    items = {
        { type = "float", value = "/battle/target/type/30/shots" },
        params.BattleTotalShotsForModuleStatsAsTarget,
    },
    fn = function(v) return (v[2] > 1000.0) and (v[1] / v[2] * 100.0) or (nil) end,
    weight = function(v) return (v[2] > 0.0) and (-v[1] / v[2]) or (0.0) end,
    units = "UI_percent_format",
    filter = function() return false end,
}


local function hasBattleCount(specs)
    return notConsole() and (not specs or specs["/stats/battleCount"])
end

params.StatBattleEfficiency = {
    icon = "Specs-Stats-1",
    title = "UI_battle_efficiency",
    column = 2,
    items = {
        params.StatsBattleCount,
        params.StatsProcessedShotsCount,
        params.StatsPlayerWtr,
        params.StatsWinrate,
        params.StatsCredits,
        params.StatsExperience,
        params.StatsHitpointsLeftPercent,
        params.StatsTimeAlive,
        params.StatsDistanceTravelled,
        params.StatsBaseCapturePoints,
        params.StatsBaseDefendPoints,
    },
    filter = function(specs) return hasBattleCount(specs) end,
}

params.StatEconomics = {
    icon = "credits",
    title = "UI_economics",
    column = 2,
    tag = "economics",
    items = {
        params.StatsEconomicsTotalCredits,
        params.StatsEconomicsRepairCost,
        params.StatsEconomicsAmmoCost,
        params.StatsEconomicsEquipmentCost,
        params.StatsEconomicsProfit,
        params.StatsEconomicsProfitPerMinute,
        params.StatsCreditsFactor,
    },
    filter = function(specs)
        return notConsole() and (not specs or specs["/stats/economics/all/totalCredits"])
    end,
}

params.StatMastery = {
    icon = "Flag-China-",
    title = "UI_mastery_requirements",
    column = 2,
    items = {
        params.StatsBadgeMastery,
        params.StatsBadge1stClass,
        params.StatsBadge2ndClass,
        params.StatsBadge3rdClass,
        params.StatsGunMarkPercentile95,
        params.StatsGunMarkPercentile85,
        params.StatsGunMarkPercentile65,
        params.StatsGunMarkPercentile95Pc,
        params.StatsGunMarkPercentile85Pc,
        params.StatsGunMarkPercentile65Pc,
        params.StatsGunMarkBattleExp,
    },
    filter = function(specs)
        return notConsole() and (not specs or specs["/stats/gunMarkPercentile65"] or specs["/stats/expBadge3rd"])
    end,
}

params.StatAssistEfficiency = {
    icon = "TankSelector-Equipment-CoatedOptics",
    title = "UI_assistance_efficiency",
    column = 2,
    items = {
        params.StatsDamageAssisted,
        params.StatsDamageAssistedTrack,
        params.StatsDamageAssistedStun,
        params.StatsAssistRatio,
        params.StatsAssistRatioPercent,
        params.StatsEnemiesSpotted,
        params.StatsEnemiesStunned,
        params.StatsStunDuration,
    },
    filter = function(specs) return hasBattleCount(specs) end,
}

params.StatDeathReasons = {
    icon = "Tank-TankDead-",
    title = "UI_cause_of_death",
    column = 2,
    items = {
        params.StatsDeathReasonFire,
        params.StatsDeathReasonRamming,
        params.StatsDeathReasonFall,
        params.StatsDeathReasonDrowning,
        params.StatsKillReasonFire,
        params.StatsKillReasonRamming,
    },
    filter = function(specs)
        return notConsole() and (not specs or tonumber(specs["/stats/deathReasonTotal"] or 0) > 0)
    end,
}

params.BattleStatsTargetParams = {
    icon = "AI-Confrontation-Target",
    title = "UI_armor_efficiency",
    column = 3,
    items = {
        params.StatsAvgDistanceOfDamageTaken,
        params.StatsPremiumAmmoHitPercentage,
        params.StatsHeAmmoHitPercentage,
        params.StatsAvgDamageReceivedPerShot,
        params.StatsDamageReceived,
        params.StatsDamageReceivedPerGamePercent,
        params.StatsAvgDamageBlockedPerShot,
        params.StatsDamageBlocked,
        params.StatsAvgDamageBlockedPerGamePercent,
        params.BattleAmmoRackedPerShot,
        params.BattleTakingFirePerShot,
    },
    filter = function(specs) return hasBattleCount(specs) end,
}

params.BattleStatsShooterParams = {
    icon = "AI-Confrontation-Mode",
    title = "UI_gun_efficiency",
    column = 3,
    items = {
        params.StatsAvgDistanceOfDamageDealt,
        params.StatsPremiumAmmoUsedPercentage,
        params.StatsHeAmmoUsedPercentage,
        params.StatsDamageBouncedPercentage,
        params.StatsDpm,
        params.StatsAccuracy,
        params.StatsFragsPerMinute,
        params.StatsAvgDamageDealtPerShot,
        params.StatsDamageMade,
        params.StatsAvgDamageDealtPerGamePercent,
        params.StatsDamageSniper,
        params.BattleAmmoRacksPerShot,
    },
    filter = function(specs) return hasBattleCount(specs) end,
}

params.BattleStatsModuleShooterParamsBlitz = {
    icon = "Modules-",
    title = "UI_damage_to_modules",
    column = 3,
    items = {
        params.StatsChanceToSetOnFirePerShot,
        params.StatsChanceToSetOnFirePerGame,
        params.StatsShooterInternalCritsPerShot,
        params.StatsShooterInternalCritsPerGame,
        params.StatsShooterTracksCritsPerShot,
        params.StatsShooterWheelCritsPerShot,
        params.StatsShooterGunCritsPerShot,
        params.StatsShooterSurveyingDeviceCritsPerShot,
        params.StatsShooterAmmoRackCritsPerShot,
        params.StatsShooterEngineCritsPerShot,
        params.StatsShooterFuelTankCritsPerShot,
        params.StatsShooterRadioCritsPerShot,
        params.StatsShooterTurretCritsPerShot,
        params.StatsShooterCommanderCritsPerShot,
        params.StatsShooterDriverCritsPerShot,
        params.StatsShooterRadioOperatorCritsPerShot,
        params.StatsShooterGunnerCritsPerShot,
        params.StatsShooterLoaderCritsPerShot,
    },
    filter = function(specs)
        return platformIsBlitzFamily() and (not specs or tonumber(specs["/stats/shooter/totalShots"] or 0) > 0)
    end,
}

params.BattleStatsModuleShooterParamsPc = {
    icon = "Modules-",
    title = "UI_damage_to_modules",
    column = 3,
    items = {
        params.StatsChanceToSetOnFirePerShot,
        params.StatsChanceToSetOnFirePerGame,
        params.StatsShooterInternalCritsPerShot,
        params.StatsShooterInternalCritsPerGame,
        params.StatsShooterTracksCritsPerShot,
        params.StatsShooterGunCritsPerShot,
        params.StatsShooterSurveyingDeviceCritsPerShot,
    },
    filter = function(specs)
        return platformIsPcFamily() and (not specs or tonumber(specs["/stats/shooter/totalShots"] or 0) > 0)
    end,
}

params.BattleStatsModuleTargetParams = {
    icon = "Modules-",
    title = "UI_module_vulnerability",
    column = 3,
    items = {
        params.StatsTargetInternalCritsPerShot,
        params.StatsTargetInternalCritsPerGame,
        params.StatsTargetTracksCritsPerShot,
        params.StatsTargetWheelCritsPerShot,
        params.StatsTargetGunCritsPerShot,
        params.StatsTargetSurveyingDeviceCritsPerShot,
        params.StatsTargetAmmoRackCritsPerShot,
        params.StatsTargetEngineCritsPerShot,
        params.StatsTargetFuelTankCritsPerShot,
        params.StatsTargetRadioCritsPerShot,
        params.StatsTargetTurretCritsPerShot,
        params.StatsTargetCommanderCritsPerShot,
        params.StatsTargetDriverCritsPerShot,
        params.StatsTargetRadioOperatorCritsPerShot,
        params.StatsTargetGunnerCritsPerShot,
        params.StatsTargetLoaderCritsPerShot,
    },
    filter = function(specs)
        return notConsole() and (not specs or tonumber(specs["/stats/target/totalShots"] or 0) > 0)
    end,
}

params.BattleStatsConfiguration = {
    params.StatBattleEfficiency,
    params.StatEconomics,
    params.StatMastery,
    params.StatAssistEfficiency,
    params.StatDeathReasons,
    params.BattleStatsTargetParams,
    params.BattleStatsShooterParams,
    params.BattleStatsModuleShooterParamsBlitz,
    params.BattleStatsModuleShooterParamsPc,
    params.BattleStatsModuleTargetParams,
}

return params

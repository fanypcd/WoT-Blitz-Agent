local params = {}

-- Factors
params.CrewSkill_commander = {
    type = "transform",
    items = {
        {
            type = "float",
            value = "/crewSkill",
        },
        {
            type = "float",
            value = "/miscAttrs/crewLevelIncrease",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/commander_emergency",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/radioman_expert",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/radioman_sideBySide",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/commander_holdLine",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/commander_staySharp",
            default = 0.0,
        },
        {
            type = "float",
            value = "/miscAttrs/skill/driver_bulletproof",
            default = 0.0,
        },
    },
    default = 100.0,
    fn = function(v) return v[1] + v[2] + (v[3] + v[6] + v[7] + v[8]) * 5.0 + v[4] * 2.5 + v[5] * 2.5 end
}

params.SkillFactor_commander = {
    type = "transform",
    items = {
        params.CrewSkill_commander
    },
    default = 1.0,
    fn = function(v) return 0.57 + 0.43 * v[1] * 0.01 end
}

params.CrewSkill_driver = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.CrewSkill_commander,
        {
            type = "float",
            value = "/commanderRole/driver",
        },
    },
    default = 100.0,
    fn = function(v) return v[2] and v[1] or v[1] * 1.1 end
}

params.SkillFactor_driver = {
    type = "transform",
    items = {
        params.CrewSkill_driver
    },
    default = 1.0,
    fn = function(v) return 0.57 + 0.43 * v[1] * 0.01 end
}

params.CrewSkill_loader = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.CrewSkill_commander,
        {
            type = "float",
            value = "/commanderRole/loader",
        },
    },
    default = 100.0,
    fn = function(v) return v[2] and v[1] or v[1] * 1.1 end
}

params.SkillFactor_loader = {
    type = "transform",
    items = {
        params.CrewSkill_loader
    },
    default = 1.0,
    fn = function(v) return 0.57 + 0.43 * v[1] * 0.01 end
}

params.CrewSkill_gunner = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.CrewSkill_commander,
        {
            type = "float",
            value = "/commanderRole/gunner",
        },
    },
    default = 100.0,
    fn = function(v) return v[2] and v[1] or v[1] * 1.1 end
}

params.SkillFactor_gunner = {
    type = "transform",
    items = {
        params.CrewSkill_gunner
    },
    default = 1.0,
    fn = function(v) return 0.57 + 0.43 * v[1] * 0.01 end
}

params.CrewSkill_radioman = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.CrewSkill_commander,
        {
            type = "float",
            value = "/commanderRole/radioman",
        },
    },
    default = 100.0,
    fn = function(v) return v[2] and v[1] or v[1] * 1.1 end
}

params.SkillFactor_radioman = {
    type = "transform",
    items = {
        params.CrewSkill_radioman
    },
    default = 1.0,
    fn = function(v) return 0.57 + 0.43 * v[1] * 0.01 end
}

params.ChassisRotationFactor = {
    type = "float",
    value = "/miscAttrs/rotationFactor",
    default = 1.0,
}

params.RollingFrictionFactor = {
    type = "float",
    value = "/miscAttrs/rollingFrictionFactor",
    default = 1.0,
}


params.ReloadTimeFactor = {
    type = "transform",
    items = {
        params.SkillFactor_loader,
        { type = "float", value = "/miscAttrs/gunReloadTimeFactor", default = 1.0, },
        { type = "float", value = "/miscAttrs/gun/reloadTime", default = 1.0, },
    },
    default = 1.0,
    fn = function(v) 
        return v[2] / v[1] * v[3]
    end
}

params.AimingTimeFactor = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.SkillFactor_gunner,
        { type = "float", value = "/miscAttrs/gunAimingTimeFactor", default = 1.0, },
        { type = "float", value = "/miscAttrs/gun/aimingTime", default = 1.0, },
    },
    default = 1.0,
    fn = function(v) 
        return v[2] / v[1] * v[3]
    end
}

params.ShotDispersionAngleFactor = {
    type = "transform",
    items = {
        params.SkillFactor_gunner,
        {
            type = "float",
            value = "/miscAttrs/shotDispersionAngleFactor",
            default = 1.0
        }
    },
    default = 1.0,
    fn = function(v) 
        return v[2] / v[1]
    end
}

params.AdditiveShotDispersionFactor = {
    type = "float",
    value = "/miscAttrs/additiveShotDispersionFactor",
    default = 1.0
}

params.ShotDispersionFactorAfterShot = {
    type = "float",
    value = "/miscAttrs/gun/shotDispersionFactors/afterShot",
    default = 1.0
}

params.PiercingPenaltyFactor500m = {
    type = "float",
    value = "/miscAttrs/piercingPenaltyFactor500m",
    default = 1.0
}

params.ProjectileSpeedFactor = {
    type = "float",
    value = "/miscAttrs/projectileSpeedFactor",
    default = 1.0
}

params.ViewRangeFactor = {
    type = "transform",
    allowUndefined = true,
    items = {
        params.SkillFactor_commander,
    },
    default = 1.0,
    fn = function(v)
        return v[1]
    end
}

params.ChassisHealthFactor = {
    type = "float",
    value = "/miscAttrs/chassisHealthFactor",
    default = 1.0
}

params.FuelTankHealthFactor = {
    type = "float",
    value = "/miscAttrs/fuelTankHealthFactor",
    default = 1.0
}

params.AmmoBayHealthFactor = {
    type = "float",
    value = "/miscAttrs/ammoBayHealthFactor",
    default = 1.0
}

params.EngineHealthFactor = {
    type = "float",
    value = "/miscAttrs/engineHealthFactor",
    default = 1.0
}

params.CrewChanceToHitFactor = {
    type = "float",
    value = "/miscAttrs/crewChanceToHitFactor",
    default = 1.0
}
params.EngineChanceToHitFactor = {
    type = "float",
    value = "/miscAttrs/hitFactors/engineHealth",
    default = 1.0
}
params.AmmoBayChanceToHitFactor = {
    type = "float",
    value = "/miscAttrs/hitFactors/ammoBayHealth",
    default = 1.0
}

params.HealthFactor = {
    type = "float",
    value = "/miscAttrs/healthFactor",
    default = 1.0
}

params.ChassisRegenerationPenaltyFactor = {
    type = "float",
    value = "/miscAttrs/chassisRegenerationPenaltyFactor",
    default = 1.0
}

params.CircularVisionRadiusFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/circularVisionRadiusFactor", default = 1.0 },
        { type = "float", value = "/miscAttrs/circularVisionRadius", default = 1.0 },
        { type = "float", value = "/miscAttrs/circularVisionRadiusBaseFactor", default = 1.0 },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] * v[3] end,
}

params.CircularVisionRadiusFactors_lightTank = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusFactors/lightTank",
    default = 1.0
}

params.CircularVisionRadiusFactors_mediumTank = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusFactors/mediumTank",
    default = 1.0
}

params.CircularVisionRadiusFactors_heavyTank = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusFactors/heavyTank",
    default = 1.0
}

params.CircularVisionRadiusFactors_ATSPG = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusFactors/AT-SPG",
    default = 1.0
}

params.CircularVisionRadiusFactors_SPG = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusFactors/SPG",
    default = 1.0
}

params.InvisibilityAdditiveTerm = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/invisibilityAdditiveTerm", default = 0.0},
        { type = "float", value = "/miscAttrs/invisibilityBaseAdditive", default = 0.0},
    },
    default = 0.0,
    fn = function(v) return v[1] + v[2] end,
}

params.InvisibilityAdditiveTermStill = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTermStill",
    default = 0.0
}

params.InvisibilityBaseMultiplicativeTerm = {
    type = "float",
    value = "/miscAttrs/invisibilityBaseMultiplicativeTerm",
    default = 1.0
}

params.InvisibilityAdditiveTerm_lightTank = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/lightTank",
    default = 0.0
}

params.InvisibilityAdditiveTerm_mediumTank = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/mediumTank",
    default = 0.0
}

params.InvisibilityAdditiveTerm_heavyTank = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/heavyTank",
    default = 0.0
}

params.InvisibilityAdditiveTerm_ATSPG = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/AT-SPG",
    default = 0.0
}

params.InvisibilityAdditiveTerm_SPG = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/SPG",
    default = 0.0
}

params.InvisibilityAdditiveTermStillFactor = {
    type = "float",
    value = "/miscAttrs/invisibilityAdditiveTerm/stillFactor",
    default = 2.0
}

params.InvisibilityCamoPaint = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/camoPaint", default = 0.0 },
        { type = "float", value = "/invisibility/camouflageBonus", default = 0.0 },
    },
    default = 0.0,
    fn = function(v) return v[1] * v[2] end,
}

params.InvisibilityFactorAtShot = {
    type = "float",
    value = "/miscAttrs/invisibilityFactorAtShot",
    default = 1.0
}

params.RotationSpeedFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/rotationSpeedFactor", },
        { type = "float", value = "/miscAttrs/onStillRotationSpeedFactor", },
        { type = "float", value = "/miscAttrs/vehicle/rotationSpeed", },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] * v[3] end
}

params.EnginePowerFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/enginePowerFactor", default = 1.0 },
        { type = "float", value = "/miscAttrs/engine/power", default = 1.0 },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] end,
}

params.EnginePowerFactors_lightTank = {
    type = "float",
    value = "/miscAttrs/enginePowerFactors/lightTank",
    default = 1.0
}

params.EnginePowerFactors_mediumTank = {
    type = "float",
    value = "/miscAttrs/enginePowerFactors/mediumTank",
    default = 1.0
}

params.EnginePowerFactors_heavyTank = {
    type = "float",
    value = "/miscAttrs/enginePowerFactors/heavyTank",
    default = 1.0
}

params.EnginePowerFactors_ATSPG = {
    type = "float",
    value = "/miscAttrs/enginePowerFactors/AT-SPG",
    default = 1.0
}

params.EnginePowerFactors_SPG = {
    type = "float",
    value = "/miscAttrs/enginePowerFactors/SPG",
    default = 1.0
}

params.PiercingPowerFactor_ARMOR_PIERCING = {
    type = "float",
    value = "/miscAttrs/piercingPowerFactor/ARMOR_PIERCING",
    default = 1.0,
}
params.PiercingPowerFactor_HOLLOW_CHARGE = {
    type = "float",
    value = "/miscAttrs/piercingPowerFactor/HOLLOW_CHARGE",
    default = 1.0,
}
params.PiercingPowerFactor_HIGH_EXPLOSIVE = {
    type = "float",
    value = "/miscAttrs/piercingPowerFactor/HIGH_EXPLOSIVE",
    default = 1.0,
}
params.PiercingPowerFactor_ARMOR_PIERCING_HE = {
    type = "float",
    value = "/miscAttrs/piercingPowerFactor/ARMOR_PIERCING_HE",
    default = 1.0,
}
params.PiercingPowerFactor_ARMOR_PIERCING_CR = {
    type = "float",
    value = "/miscAttrs/piercingPowerFactor/ARMOR_PIERCING_CR",
    default = 1.0,
}

params.UpperPitchLimitIncrease = {
    type = "float",
    value = "/miscAttrs/upperPitchLimitIncrease",
    default = 0.0
}

params.LowerPitchLimitIncrease = {
    type = "float",
    value = "/miscAttrs/lowerPitchLimitIncrease",
    default = 0.0
}

params.FirmGroundPassabilityIncrease = {
    type = "float",
    value = "/miscAttrs/firmGroundPassabilityIncrease",
    default = 1.0
}

params.SoftGroundPassabilityIncrease = {
    type = "float",
    value = "/miscAttrs/softGroundPassabilityIncrease",
    default = 1.0
}

params.MediumGroundPassabilityIncrease = {
    type = "float",
    value = "/miscAttrs/mediumGroundPassabilityIncrease",
    default = 1.0
}

params.FireStartingChanceFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/fireStartingChanceFactor", default = 1.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/engine/fireStartingChance", default = 1.0 },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] end
}

params.ForwardMaxSpeedKMHTerm = {
    type = "transform",
    items = {
      { type = "float", value = "/miscAttrs/forwardMaxSpeedKMHTerm", default = 0.0 },
      { type = "float", value = "/miscAttrs/vehicle/fwMaxSpeedBonus", default = 0.0 },
      { type = "float", value = "/miscAttrs/descrAttrs/engine/maxSpeedForward", default = 0.0 },
      { type = "float", value = "/miscAttrs/rechargeableNitro/addMaxSpeedForwardBonus", default = 0.0 },
    },
    default = 0.0,
    fn = function (v) return v[1] + v[2] + v[3] + v[4] end,
}

params.ForwardMaxSpeedFactor = {
    type = "float",
    value = "/miscAttrs/vehicle/maxSpeed/forward",
    default = 1.0
}

params.BackwardMaxSpeedKMHTerm = {
    type = "transform",
    items = {
      { type = "float", value = "/miscAttrs/backwardMaxSpeedKMHTerm", default = 0.0 },
      { type = "float", value = "/miscAttrs/vehicle/bkMaxSpeedBonus", default = 0.0 },
      { type = "float", value = "/miscAttrs/descrAttrs/engine/maxSpeedBack", default = 0.0 },
    },
    default = 0.0,
    fn = function (v) return v[1] + v[2] + v[3] end,
}

params.BackwardMaxSpeedFactor = {
    type = "float",
    value = "/miscAttrs/vehicle/maxSpeed/backward",
    default = 1.0
}

params.TurretRotationSpeedFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/turretRotationSpeed", default = 1.0 },
        { type = "float", value = "/miscAttrs/turretRotationSpeedFactor", default = 1.0 },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] end
}

params.MultShotDispersionFactor = {
    type = "float",
    value = "/miscAttrs/multShotDispersionFactor",
    default = 1.0,
}

params.ShotDispersionWhileGunDamagedFactor = {
    type = "float",
    value = "/miscAttrs/gun/shotDispersionFactors/whileGunDamaged",
    default = 1.0
}

params.DemaskFoliageFactor = {
    type = "float",
    value = "/miscAttrs/demaskFoliageFactor",
    default = 1.0
}

params.DemaskMovingFactor = {
    type = "float",
    value = "/miscAttrs/demaskMovingFactor",
    default = 1.0
}

params.RammingFactorMisc = {
    type = "float",
    value = "/miscAttrs/rammingFactor",
    default = 1.0
}

params.SecondaryGunReloadTimeFactor = {
    type = "float",
    value = "/miscAttrs/secondaryGunReloadTimeFactor",
    default = 1.0
}

params.CircularVisionRadiusStillFactor = {
    type = "float",
    value = "/miscAttrs/circularVisionRadiusStillFactor",
    default = 1.0
}

params.DamageFactor = {
    type = "float",
    value = "/miscAttrs/damageFactor",
    default = 1.0
}

params.ArmourPiercingFactor = {
    type = "float",
    value = "/miscAttrs/armourPiercingFactor",
    default = 1.0
}

params.AntifragmentationLiningFactor = {
    type = "float",
    value = "/miscAttrs/antifragmentationLiningFactor",
    default = 1.0,
}

params.ShotDispersionFactorMovement = {
    type = "float",
    value = "/miscAttrs/chassis/shotDispersionFactors/movement",
    default = 1.0
}

params.ShotDispersionFactorVehicleRotation = {
    type = "float",
    value = "/miscAttrs/chassis/shotDispersionFactors/rotation",
    default = 1.0
}

params.ShotDispersionFactorTurretRotation = {
    type = "float",
    value = "/miscAttrs/gun/shotDispersionFactors/turretRotation",
    default = 1.0
}

params.CommonSkillLevel = {
    type = "transform",
    items = {
        { type = "int", value = "/crewMembers" },
        params.CrewSkill_commander,
    },
    default = 100.0,
    fn = function(v)
        local non_commander_skill = v[2] * 1.1
        return ((v[1] - 1) * non_commander_skill + v[2]) / v[1]
    end
}

params.SkillFactor_camouflage = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/camouflage" },
        params.CommonSkillLevel,
    },
    default = 1.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 1.0
        end
        return 0.57 + 0.43 * v[1] * v[2] * 0.01
    end
}

params.SkillFactor_repair = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/repair" },
        params.CommonSkillLevel,
    },
    default = 1.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 1.0
        end
        return 0.57 + 0.43 * v[1] * v[2] * 0.01
    end
}

params.RepairSpeedFactor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/repairSpeedFactor" },
        params.SkillFactor_repair,
        { type = "float", value = "/miscAttrs/repairSpeed" },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] * v[3] end
}

params.SkillFactor_eagleEye = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/commander_eagleEye" },
        params.CrewSkill_commander,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01 * 0.02
    end
}

params.SkillFactor_smoothDriving = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/driver_smoothDriving" },
        params.CrewSkill_driver,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.04
    end
}

params.SkillFactor_suspensionRepair = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/driver_suspensionRepair" },
        params.CrewSkill_driver,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_virtuoso = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/driver_virtuoso" },
        params.CrewSkill_driver,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01 * 0.05
    end
}

params.SkillFactor_badRoadsKing = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/driver_badRoadsKing" },
        params.CrewSkill_driver,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_smoothTurret = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_smoothTurret" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.075
    end
}

params.SkillFactor_gunsmith = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_gunsmith" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.2
    end
}

params.SkillFactor_pointBlast = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_pointBlast" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_pedant = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_pedant" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_desperado = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_desperado" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_intuition = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_intuition" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.6
    end
}

params.SkillFactor_finder = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/radioman_finder" },
        params.CrewSkill_radioman,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01 * 0.03
    end
}

params.SkillFactor_threatSearch = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/radioman_threatSearch" },
        params.CrewSkill_radioman,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01 * 0.02
    end
}

params.SkillFactor_inventor = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/radioman_inventor" },
        params.CrewSkill_radioman,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01 * 0.2
    end
}

params.SkillFactor_coordination = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/commander_coordination" },
        params.CrewSkill_commander,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.125
    end
}

params.SkillFactor_practical = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/commander_practical" },
        params.CrewSkill_commander,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.1
    end
}

params.SkillFactor_motorExpert = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/driver_motorExpert" },
        params.CrewSkill_driver,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_armorer = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_armorer" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_ammunitionImprove = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_ammunitionImprove" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_focus = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_focus" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return -v[1] * v[2] * 0.01 * 0.035
    end
}

params.SkillFactor_quickAiming = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/gunner_quickAiming" },
        params.CrewSkill_gunner,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_perfectCharge = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_perfectCharge" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_melee = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_melee" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_secondChance = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_secondChance" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.SkillFactor_magMastery = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/skill/loader_magMastery" },
        params.CrewSkill_loader,
    },
    default = 0.0,
    fn = function(v)
        if platformCode == "blitz" or platformCode == "tanksblitz" then
            return 0.0
        end
        return v[1] * v[2] * 0.01
    end
}

params.EnginePowerIncrease = {
    type = "float",
    value = "/miscAttrs/enginePowerIncrease",
    default = 0.0
}

params.TurretRotationSpeedIncrease = {
    type = "float",
    value = "/miscAttrs/turretRotationSpeedIncrease",
    default = 0.0
}

params.GunReloadSpeedIncrease = {
    type = "float",
    value = "/miscAttrs/gunReloadSpeedIncrease",
    default = 0.0
}

params.FwdSpeedLimitBias = {
    type = "float",
    value = "/miscAttrs/fwdSpeedLimitBias",
    default = 0.0
}

params.BkwdSpeedLimitBias = {
    type = "float",
    value = "/miscAttrs/bkwdSpeedLimitBias",
    default = 0.0
}

params.ClipReloadTimeFactor = {
    type = "float",
    value = "/miscAttrs/clipReloadTimeFactor",
    default = 1.0
}

params.EquipmentDurationFactor = {
    type = "float",
    value = "/miscAttrs/equipmentDurationFactor",
    default = 1.0
}

params.EquipmentReloadBoost = {
    type = "float",
    value = "/miscAttrs/equipmentReloadBoost",
    default = 0.0
}

params.AutoShootShotDispersionPerShot = {
    type = "transform",
    items = {
        { type = "float", value = "/miscAttrs/autoShoot/shotDispersionPerShot", default = 1.0 },
        { type = "float", value = "/miscAttrs/descrAttrs/autoShoot/shotDispersionPerShot", default = 1.0 },
    },
    default = 1.0,
    fn = function(v) return v[1] * v[2] end,
}

params.TemperatureGunCoolingPerSec = {
    type = "float",
    value = "/miscAttrs/temperatureGun/coolingPerSec",
    default = 1.0
}

params.ChassisRepairSpeedFactor = {
    type = "transform",
    items = {
      {
        type = "float",
        value = "/miscAttrs/chassisRepairSpeedFactor",
        default = 1.0
      },
      params.SkillFactor_suspensionRepair,
    },
    default = 1.0,
    fn = function(v) return v[1] * (1.0 + v[2] * 0.15) end
}







return params
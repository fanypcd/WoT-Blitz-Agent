local utils = {
    tier_labels = {[0] = "", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"}
}

local moduleIcons0Based = {
    [0] = nil,
    [1] = "Modules-Engine-1.svg",
    [2] = "Modules-Ammorack-.svg",
    [3] = "Modules-GasTank-1.svg",
    [4] = "Modules-Radio-1.svg",
    [5] = "Modules-Tracks-.svg",
    [6] = "Modules-Gun-.svg",
    [7] = "modules-turret-3.svg",
    [8] = "TankSelector-Equipment-CoatedOptics.svg",
    [9] = "modules-Crew-Commander-1.svg",
    [10] = "modules-Crew-Driver-2.svg",
    [11] = "modules-Crew-Radioman-1.svg",
    [12] = "modules-Crew-Gunner-1.svg",
    [13] = "modules-Crew-LoaderAmmo-1.svg",
    [14] = "Modules-tracks-wheeled.svg",
    [15] = "Modules-Engine-1.svg",
}

local ammoIcons0Based = {
    [0] = "Specs-Ammo-AP-.svg",
    [1] = "Specs-Ammo-HE-.svg",
    [2] = "Specs-Ammo-APCR-.svg",
    [3] = "Specs-Ammo-HE-.svg",
    [4] = "Specs-Ammo-HE-.svg",
    [5] = "Specs-Ammo-HEAT-.svg",
    [6] = "Specs-Ammo-AP-.svg",
    [7] = "Specs-Ammo-Type-APFSDS-.svg",
    [8] = "Specs-Ammo-AP-.svg",
    [9] = "Specs-Ammo-HE-.svg",
    [10] = "Specs-Ammo-HE-.svg",
    [11] = "Specs-Ammo-HE-.svg",
    [12] = "Specs-Ammo-Type-APFSDS-.svg",
}

local typeIcons0Based = {
    [0] = "TankSelector-ClassLight-big.svg",
    [1] = "TankSelector-ClassMedium-big.svg",
    [2] = "TankSelector-ClassHeavy-big.svg",
    [3] = "TankSelector-ClassTankDestroyer-big.svg",
    [4] = "TankSelector-ClassArtillery-big.svg",
}

local nationIcons0Based = {
    [0] = "Flag-China-.svg",
    [1] = "Flag-France-.svg",
    [2] = "Flag-Germany-.svg",
    [3] = "flag-japan.svg",
    [4] = "flag-british.svg",
    [5] = "Flag-SovietUnion-.svg",
    [6] = "flag-usa.svg",
    [7] = "flag-czech.svg",
    [8] = "flag-sweden.svg",
    [9] = "flag-poland.svg",
    [10] = "flag-italy.svg",
    [11] = "AI-platform-Blitz-.svg",
    [12] = "Flag-Mercenaries.svg",
    [13] = "flag-euro.svg",
    [14] = "Flag-International-world-1.svg",
}

local reforgedRoundFlagIcons = {
    [1] = "T_UI_RoundFlag_USSR_XS.png",
    [2] = "T_UI_RoundFlag_Germany_XS.png",
    [3] = "T_UI_RoundFlag_USA_XS.png",
    [4] = "T_UI_RoundFlag_China_XS.png",
    [5] = "T_UI_RoundFlag_France_XS.png",
    [6] = "T_UI_RoundFlag_England_XS.png",
    [7] = "T_UI_RoundFlag_Japan_XS.png",
    [8] = "T_UI_RoundFlag_EU_XS.png",
    [9] = "T_UI_RoundFlag_Poland_XS.png",
    [10] = "T_UI_RoundFlag_Australia_XS.png",
    [11] = "T_UI_RoundFlag_Canada_XS.png",
    [12] = "T_UI_RoundFlag_Italy_XS.png",
    [13] = "T_UI_RoundFlag_Finland_XS.png",
    [14] = "T_UI_RoundFlag_Czech_XS.png",
    [15] = "T_UI_RoundFlag_TBD_XS.png",
    [16] = "T_UI_RoundFlag_GDR_XS.png",
    [17] = "T_UI_RoundFlag_Ireland_XS.png",
    [18] = "T_UI_RoundFlag_Scotland_XS.png",
    [19] = "T_UI_RoundFlag_Sweden_XS.png",
    [20] = "T_UI_RoundFlag_Argo_XS.png",
    [21] = "T_UI_RoundFlag_TBD_XS.png",
    [22] = "T_UI_RoundFlag_Defenders_XS.png",
    [23] = "T_UI_RoundFlag_Progresston_XS.png",
    [24] = "T_UI_RoundFlag_Wasteland_XS.png",
    [25] = "T_UI_RoundFlag_HuntersMoon_XS.png",
    [26] = "T_UI_RoundFlag_ColdFront_XS.png",
    [27] = "T_UI_RoundFlag_Explorers_XS.png",
    [28] = "T_UI_RoundFlag_FableGear_XS.png",
    [29] = "T_UI_RoundFlag_Outlaws_XS.png",
}

local reforgedCarouselFlagImages = {
    [1] = "T_UI_Flag_USSR_S.png",
    [2] = "T_UI_Flag_Germany_S.png",
    [3] = "T_UI_Flag_USA_S.png",
    [4] = "T_UI_Flag_China_S.png",
    [5] = "T_UI_Flag_France_S.png",
    [6] = "T_UI_Flag_UK_S.png",
    [7] = "T_UI_Flag_Japan_S.png",
    [9] = "T_UI_Flag_Poland_S.png",
    [10] = "T_UI_Flag_Australia_S.png",
    [11] = "T_UI_Flag_Canada_S.png",
    [12] = "T_UI_Flag_Italy_S.png",
    [13] = "T_UI_Flag_Finland_S.png",
    [14] = "T_UI_Flag_Czech_S.png",
    [15] = "T_UI_Flag_TBD_S.png",
    [16] = "T_UI_Flag_GDR_S.png",
    [17] = "T_UI_Flag_Ireland_S.png",
    [18] = "T_UI_Flag_Scotland_S.png",
    [19] = "T_UI_Flag_Sweden_S.png",
    [20] = "T_UI_Flag-Argo_S.png",
    [21] = "T_UI_Flag-Titan-lab_S.png",
    [22] = "T_UI_Flag-Flag-Defender_S.png",
    [23] = "T_UI_Flag-Progresston_S.png",
    [24] = "T_UI_Flag-Wasteland_S.png",
    [25] = "T_UI_Flag-Hunter_s-moon_S.png",
    [26] = "T_UI_Flag-Cold-Front_S.png",
    [27] = "T_UI_Flag-The-Explorers_S.png",
    [28] = "T_UI_Flag-Fablegear_S.png",
    [29] = "T_UI_Flag-Outlaws_S.png",
}

local reforgedCarouselFlagDefault = "T_UI_Flag_TBD_S.png"

-- 0-based indexing versions of the functions
function utils.getModuleIcon(deviceType)
    if deviceType == 0 then  -- ARMOR type
        return nil
    end
    return moduleIcons0Based[deviceType]
end

function utils.getAmmoIcon(shellType)
    return ammoIcons0Based[shellType]
end

function utils.getNationIcon(platform, nation)
    if platform and platform.game_code == "reforged" then
        local file = reforgedRoundFlagIcons[nation] or 'T_UI_RoundFlag_TBD_XS.png'
        return utils.resolveUiIcon("/ui/icons/reforged/RoundFlagsXS/" .. file)
    end

    local icon = nationIcons0Based[nation] or nationIcons0Based[1]
    if icon then
        return "/rml/icons/" .. icon
    end
    return nil
end

function utils.isRasterIcon(icon_path)
    if not icon_path then
        return false
    end
    return icon_path:find("^/ui/") ~= nil or icon_path:find("^https?://") ~= nil or icon_path:find("%.png$") ~= nil
end

function utils.useRemoteUiIcons()
    return PLATFORM_EMSCRIPTEN and not VERSION_MOD
end

function utils.resolveUiIcon(path)
    if not path or not utils.useRemoteUiIcons() then
        return path
    end
    local prefix = "/ui/icons/"
    if path:sub(1, #prefix) == prefix then
        return "https://wotinspector-static.s3.eu-central-003.backblazeb2.com/wi/static/icons/" .. path:sub(#prefix + 1)
    end
    return path
end

function utils.getTypeIcon(type)
    return typeIcons0Based[type] or typeIcons0Based[1]
end

function utils.getVehicleStatusStyle(vehicle_info)
    if vehicle_info.is_hidden then
        return "text-muted"
    elseif vehicle_info.is_collectible then
        return "text-reward"
    elseif vehicle_info.is_premium then
        return "text-premium"
    end
    return "text-primary"
end

function utils.toWebGameCode(game_code)
    if game_code == "pcmod" then
        return "pc"
    elseif game_code == "mtmod" then
        return "mirtankov"
    end
    return game_code
end

function utils.getPlatformCodes()
    if VERSION_MOD then
        return { "pcmod", "mtmod", "pc", "mirtankov" }
    end
    return { "pc", "blitz", "console", "mirtankov", "tanksblitz", "reforged" }
end

function utils.platformCodeToEnum(game_code)
    local map = { 
        pc = 0, pcmod = 0, 
        blitz = 1, 
        console = 2, 
        mirtankov = 3, mtmod = 3, 
        tanksblitz = 4, reforged = 5
    }
    return map[game_code]
end

function utils.hasUnseenNewerVersion(platform)
    if not platform or not db_service.are_versions_loaded or not db_service.versions then
        return false
    end

    local platform_enum = utils.platformCodeToEnum(platform.game_code)
    if platform_enum == nil then
        return false
    end

    local bundled_short_name = platform.short_name
    local latest_unix = 0
    for _, version in ipairs(db_service.versions) do
        if version.platform == platform_enum and version.package ~= bundled_short_name then
            latest_unix = version.date_created_unix or 0
            break
        end
    end

    local builtin = platform.creation_time or 0
    if latest_unix == 0 or builtin == 0 then
        return false
    end
    return latest_unix > builtin and latest_unix > settings:getInt("ui.versionsOpened")
end

function utils.getVersionDisplayName(game_code, package)
    local web_code = utils.toWebGameCode(game_code)
    if package:sub(1, #web_code) == web_code then
        return package:sub(#web_code + 1)
    end
    return package
end

function utils.getPlatformIcon(game_code)
    local icon_map = {
		pc = "AI-platform-WOT-PC",
		pcmod = "AI-platform-WOT-PC",
		blitz = "AI-platform-Blitz-",
		console = "AI-platform-WOT-Console",
		mirtankov = "AI-platform-mirtankov",
		mtmod = "AI-platform-mirtankov",
		tanksblitz = "AI-platform-TankBlitz-",
		reforged = "AI-platform-Blitz-",
    }

    return (icon_map[game_code] or "AI-platform-WOT-PC") .. ".svg"
end

function utils.getVehicleFlagImage(vehicle_info)
    local flagTexturesPc = {
        "pc/flagsTank/china.png",
        "pc/flagsTank/france.png",
        "pc/flagsTank/germany.png",
        "pc/flagsTank/japan.png",
        "pc/flagsTank/uk.png",
        "pc/flagsTank/ussr.png",
        "pc/flagsTank/usa.png",
        "pc/flagsTank/czech.png",
        "pc/flagsTank/sweden.png",
        "pc/flagsTank/poland.png",
        "pc/flagsTank/italy.png",
        "",
        "",
        "",
        "pc/flagsTank/intunion.png"
    }

    local flagTexturesBlitz = {
        "blitz/flag_carousel_china@2x.packed.png",
        "blitz/flag_carousel_france@2x.packed.png",
        "blitz/flag_carousel_germany@2x.packed.png",
        "blitz/flag_carousel_japan@2x.packed.png",
        "blitz/flag_carousel_uk@2x.packed.png",
        "blitz/flag_carousel_ussr@2x.packed.png",
        "blitz/flag_carousel_usa@2x.packed.png",
        "blitz/flag_carousel_czech@2x.packed.png",
        "blitz/flag_carousel_china@2x.packed.png",
        "blitz/flag_carousel_china@2x.packed.png",
        "blitz/flag_carousel_italy@2x.packed.png",
        "blitz/flag_carousel_other@2x.packed.png",
        "blitz/flag_carousel_china@2x.packed.png",
        "blitz/flag_carousel_european@2x.packed.png"
    }

    local game_code = vehicle_info.platform.game_code
    if game_code == "console" then
        return nil
    end

    local nation = vehicle_info.platform:getNationById(vehicle_info.id)
    if game_code == "reforged" then
        local file = reforgedCarouselFlagImages[nation] or reforgedCarouselFlagDefault
        return "reforged/" .. file
    end
    if game_code == "blitz" or game_code == "tanksblitz" then
        return flagTexturesBlitz[nation + 1]
    end
    return flagTexturesPc[nation + 1]
end

function utils.getOptionalDeviceIcon(platform, device)
    local obj = platform:getOptionalDevice(device)
    if not obj then
        return "/materials/system/white.png"
    end

    local game_code = platform.game_code

    local code = (game_code == "tanksblitz" or game_code == "blitz") and "blitz" or "pc"
    return utils.resolveUiIcon("/ui/icons/" .. code .. "/equipment/" .. (string.sub(obj.icon, 1, 2) == ".." and obj.archetype or obj.icon) .. ".png")
end

function utils.getProvisionIcon(platform, device)
    local obj = platform:getProvision(device)
    if not obj then
        return "/materials/system/white.png"
    end

    local game_code = platform.game_code
    local icon = obj.icon
    if icon == "anti-high-explosive" then
        icon = "antiFragmentationLining"
    elseif icon == "ration_other" then
        icon = "other_big-food"
    elseif icon == "regular_ration_other" then
        icon = "other_small-food"
    elseif icon == "provision_salmon" then
        icon = "provision_salmon_m"
    elseif icon == "provision_crispbread" then
        icon = "provision_crispbread_m"
    elseif icon == "provision_czech-hq" then
        icon = "provision_czech-hq_m"
    elseif icon == "provision_czech-regular" then
        icon = "provision_czech-regular_m"
    elseif icon == "ration_polish_hq" then
        icon = "provision_polish-hq_m"
    elseif icon == "ration_polish_regular" then
        icon = "provision_polish-regular_m"
    elseif icon == "small-hp-stock" then
        icon = "add_HP_normal"
    elseif icon == "large-hp-stock" then
        icon = "add_HP_big"
    elseif icon == "provision_gear-oil" then
        icon = "provision_gear-oil_m"
    elseif icon == "provision_improved-gear-oil" then
        icon = "provision_improved-gear-oil_m"
    elseif icon == "gunpowder" then
        icon = "provision_improved-gunpowder_m"
    elseif icon == "provision_desert_power" then
        return "/materials/system/white.png"
    end

    local code = (game_code == "tanksblitz" or game_code == "blitz") and "blitz" or "pc"
    return utils.resolveUiIcon("/ui/icons/" .. code .. "/provisions/" .. icon .. ".png")
end

function utils.getConsumableIcon(platform, device)
    local obj = platform:getConsumable(device)
    if not obj then
        return "/materials/system/white.png"
    end

    local game_code = platform.game_code
    local icon = obj.icon
    if icon == "shield-kit" then
        icon = "DynamicShield"
    end

    local code = (game_code == "tanksblitz" or game_code == "blitz") and "blitz" or "pc"
    return utils.resolveUiIcon("/ui/icons/" .. code .. "/consumables/" .. icon .. ".png")
end

function utils.getOptionalDeviceStatusOverlayImage(status)
    if status == PlatformDB.OptionalDevice.Status.TROPHY_BASIC then
        return utils.resolveUiIcon("/ui/icons/common/equipmentTrophyBasic_overlay.png")
    elseif status == PlatformDB.OptionalDevice.Status.TROPHY_UPGRADED then
        return utils.resolveUiIcon("/ui/icons/common/equipmentTrophyUpgraded_overlay.png")
    elseif status == PlatformDB.OptionalDevice.Status.BOND then
        return utils.resolveUiIcon("/ui/icons/common/equipmentPlus_overlay.png")
    elseif status == PlatformDB.OptionalDevice.Status.MODERNIZED1 then
        return utils.resolveUiIcon("/ui/icons/common/equipmentModernized_1_overlay.png")
    elseif status == PlatformDB.OptionalDevice.Status.MODERNIZED2 then
        return utils.resolveUiIcon("/ui/icons/common/equipmentModernized_2_overlay.png")
    elseif status == PlatformDB.OptionalDevice.Status.MODERNIZED3 then
        return utils.resolveUiIcon("/ui/icons/common/equipmentModernized_3_overlay.png")
    end
end

function utils.getOptionalDeviceStatusCategoryImage(regular_category)
    return regular_category and utils.resolveUiIcon("/ui/icons/common/" .. regular_category .. "_on.png")
end

function utils.copyTable(t)
    local copy = {}
    for k, v in pairs(t) do
        copy[k] = v
    end
    return copy
end

--- Append GA campaign params before any #fragment.
function utils.addUtm(url, source, medium, campaign, content)
    local fragment = ""
    local hash = string.find(url, "#", 1, true)
    if hash then
        fragment = string.sub(url, hash)
        url = string.sub(url, 1, hash - 1)
    end

    local params = "utm_source=" .. source .. "&utm_medium=" .. medium .. "&utm_campaign=" .. campaign
    if content and content ~= "" then
        params = params .. "&utm_content=" .. content
    end

    if string.find(url, "?", 1, true) then
        return url .. "&" .. params .. fragment
    end
    return url .. "?" .. params .. fragment
end

--- Armor Inspector outbound links: medium is app, or mod when built as VERSION_MOD.
function utils.addAppUtm(url, campaign, content)
    local medium = (VERSION_MOD ~= nil) and "mod" or "app"
    return utils.addUtm(url, "armor_inspector", medium, campaign, content)
end

return utils
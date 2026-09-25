UIBindings = UIBindings or {}

local function resolveContext(path)
    if string.find(path, "^rml/scene/") then
        return scene_context
    end
    return main_context
end

-- Required before first SceneService::onTick / Environment::update
UIBindings.critical = {
    "rml/scene/gun_pitch.rml",
    "rml/scene/turret_yaw.rml",
    "rml/wait_icon.rml",
}

-- Loaded across subsequent main-thread frames after UIService is RUNNING
UIBindings.deferred = {
    "rml/root_platform.rml",
    "rml/root_menu.rml",
    "rml/toolbar.rml",
    "rml/scene_controls.rml",
    "rml/shot_list.rml",

    "rml/xray_menu.rml",
    "rml/confrontation_menu.rml",
    "rml/collision_menu.rml",
    "rml/heatmaps_menu.rml",
    "rml/options_menu.rml",
    "rml/tank_selector.rml",
    "rml/platform_selector.rml",

    "rml/photo_menu.rml",
    "rml/views_menu.rml",

    "rml/scene/tooltip_heatmaps_damages.rml",
    "rml/scene/tooltip_heatmaps_modules.rml",
    "rml/scene/tooltip_xray.rml",
    "rml/scene/tooltip_confrontation.rml",
    "rml/scene/tooltip_collision.rml",

    "rml/shop_tab.rml",
    "rml/purchase_tab.rml",
    "rml/version_list_tab.rml",
    "rml/account_tab.rml",
    "rml/contacts_tab.rml",
    "rml/about_tab.rml",

    "rml/heatmaps_damages_panel.rml",
    "rml/heatmaps_modules.rml",
    "rml/heatmaps_modules_mt.rml",
    "rml/heatmaps_locked.rml",
    "rml/hd_model_dialog.rml",
    "rml/in_development_tab.rml",
    "rml/early_access_tab.rml",
    "rml/collision_info_panel.rml",
    "rml/collision_info_panel_help.rml",
    "rml/collision_info_panel_locked.rml",
    "rml/xray_textures.rml",
    "rml/xray_info_panel.rml",
    "rml/xray_cutaway_locked.rml",
    "rml/xray_specs.rml",
    "rml/xray_stats.rml",
    "rml/confrontation_info_panel.rml",
    "rml/confrontation_info_panel_help.rml",
    "rml/confrontation_info_panel_locked.rml",
    "rml/tank_setup.rml",
    "rml/hd_settings_panel.rml",
    "rml/languages.rml",
    "rml/unlock_dialog.rml",
}

UIBindings._deferredIndex = 1

function UIBindings.loadCritical()
    for _, path in ipairs(UIBindings.critical) do
        resolveContext(path):LoadDocument(path)
    end
end

-- Loads up to `count` deferred documents. Returns true if more remain.
function UIBindings.loadNextBatch(count)
    local loaded = 0
    while loaded < count and UIBindings._deferredIndex <= #UIBindings.deferred do
        local path = UIBindings.deferred[UIBindings._deferredIndex]
        resolveContext(path):LoadDocument(path)
        UIBindings._deferredIndex = UIBindings._deferredIndex + 1
        loaded = loaded + 1
    end
    return UIBindings._deferredIndex <= #UIBindings.deferred
end

rmlui:LoadFontFace("font/HelveticaNeue-Light.otf")
rmlui:LoadFontFace("font/HelveticaNeue-Roman.otf")
rmlui:LoadFontFace("font/HelveticaNeue-Bold.otf")
rmlui:LoadFontFace("font/NotoSansSC-Light.subset.ttf", true)

main_context = rmlui:CreateContext("main", Vector2i.new(960, 640))
main_context:LoadDocument("rml/popup.rml")

scene_context = rmlui:CreateContext("scene", Vector2i.new(480, 320))
scene_context.dp_ratio = 2.0

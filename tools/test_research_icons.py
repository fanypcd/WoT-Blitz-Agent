"""配置器 96×96 图标居中护栏（standalone，无 pytest 依赖）。

导出器的 `_normalize_icon` 必须把 alpha 内容框居中放进固定画布。曾用
`out.paste(resized, pos, resized)`，把 RGBA 图自己当 mask，边缘 alpha 被二次相乘
（alpha²）后消失，内容框偏到一侧——2026-10-27 实测 102 张里 32 张偏差 >1px。
本测试检查最终资产几何：非技能图标 alpha 内容框四边距差 ≤1px；技能图标使用客户端
64×64 帧，只要求帧缩放居中（内容不越过 6px 边距），不按帧内 alpha 重新构图。

用法：python tools/test_research_icons.py
"""
import pathlib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
ICON_ROOT = ROOT / "data/research/icons"


def test_icon_canvas_and_alpha_bbox_centered():
    files = sorted(ICON_ROOT.rglob("*.webp"))
    assert len(files) == 102, f"expected 102 configurator icons, got {len(files)}"
    bad = []
    for path in files:
        image = Image.open(path).convert("RGBA")
        if image.size != (96, 96):
            bad.append(f"{path.relative_to(ICON_ROOT)}: canvas {image.size}")
            continue
        bbox = image.getchannel("A").getbbox()
        if bbox is None:
            bad.append(f"{path.relative_to(ICON_ROOT)}: empty alpha")
            continue
        left, top, right, bottom = bbox
        gaps = (left, top, image.width - right, image.height - bottom)
        if path.parent.name == "skills":
            # 技能帧是客户端 64×64 画布，帧内构图由美术决定；导出器只负责把
            # 这个帧缩放居中。断言内容不越过居中帧的 6px 边距，而不是要求
            # 帧内 alpha 包围框对称（那会把作者构图重新裁一遍）。
            if min(gaps) < 6:
                bad.append(f"{path.relative_to(ICON_ROOT)}: content outside 64px frame {gaps}")
        elif abs(gaps[0] - gaps[2]) > 1 or abs(gaps[1] - gaps[3]) > 1:
            bad.append(f"{path.relative_to(ICON_ROOT)}: margins {gaps}")
    assert not bad, "off-center icons:\n" + "\n".join(bad)


if __name__ == "__main__":
    test_icon_canvas_and_alpha_bbox_centered()
    print("[ok] test_icon_canvas_and_alpha_bbox_centered")
    print("全部通过")

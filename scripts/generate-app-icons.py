#!/usr/bin/env python3

from pathlib import Path
import shutil

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[1]
ICONS_DIR = ROOT / "src-tauri" / "icons"
ICONSET_DIR = ICONS_DIR / "icon.iconset"


def rounded_rect(draw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


def create_base_icon(size: int) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    pad = int(size * 0.06)
    radius = int(size * 0.22)

    shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow)
    rounded_rect(
        shadow_draw,
        (pad, pad + int(size * 0.02), size - pad, size - pad + int(size * 0.02)),
        radius,
        (0, 0, 0, 180),
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(radius=int(size * 0.035)))
    image.alpha_composite(shadow)

    bg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    bg_draw = ImageDraw.Draw(bg)
    rounded_rect(bg_draw, (pad, pad, size - pad, size - pad), radius, (233, 138, 12, 255))
    image.alpha_composite(bg)

    overlay = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    overlay_draw = ImageDraw.Draw(overlay)
    overlay_draw.rounded_rectangle(
        (pad, pad, size - pad, size - pad),
        radius=radius,
        fill=(0, 0, 0, 0),
        outline=(120, 72, 14, 160),
        width=max(1, size // 64),
    )
    image.alpha_composite(overlay)

    term_left = int(size * 0.18)
    term_top = int(size * 0.18)
    term_right = int(size * 0.82)
    term_bottom = int(size * 0.78)
    term_radius = int(size * 0.1)

    term = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    term_draw = ImageDraw.Draw(term)
    term_draw.rounded_rectangle(
        (term_left, term_top, term_right, term_bottom),
        radius=term_radius,
        fill=(17, 23, 31, 255),
    )
    term_draw.rounded_rectangle(
        (term_left, term_top, term_right, term_bottom),
        radius=term_radius,
        outline=(255, 255, 255, 54),
        width=max(1, size // 80),
    )

    header_h = int(size * 0.11)
    term_draw.rounded_rectangle(
        (term_left, term_top, term_right, term_top + header_h),
        radius=term_radius,
        fill=(255, 243, 232, 255),
    )
    term_draw.rectangle(
        (term_left, term_top + header_h // 2, term_right, term_top + header_h),
        fill=(255, 243, 232, 255),
    )

    light_r = max(2, size // 42)
    light_gap = int(size * 0.045)
    light_y = term_top + header_h // 2
    light_x = term_left + int(size * 0.06)
    colors = [(255, 95, 86, 255), (255, 189, 46, 255), (39, 201, 63, 255)]
    for idx, color in enumerate(colors):
        cx = light_x + idx * light_gap
        term_draw.ellipse((cx - light_r, light_y - light_r, cx + light_r, light_y + light_r), fill=color)

    prompt_color = (244, 247, 250, 255)
    stroke = max(2, size // 30)
    px = term_left + int(size * 0.12)
    py = term_top + header_h + int(size * 0.11)
    p2x = term_left + int(size * 0.23)
    p2y = term_top + header_h + int(size * 0.21)
    p3x = term_left + int(size * 0.12)
    p3y = term_top + header_h + int(size * 0.31)
    term_draw.line((px, py, p2x, p2y), fill=prompt_color, width=stroke, joint="curve")
    term_draw.line((p2x, p2y, p3x, p3y), fill=prompt_color, width=stroke, joint="curve")

    line_y = term_top + header_h + int(size * 0.33)
    term_draw.line(
        (term_left + int(size * 0.31), line_y, term_right - int(size * 0.12), line_y),
        fill=(111, 255, 122, 255),
        width=stroke,
    )

    cursor_w = max(4, size // 20)
    cursor_h = max(8, size // 11)
    cursor_x = term_left + int(size * 0.31)
    cursor_y = line_y + int(size * 0.08)
    term_draw.rounded_rectangle(
        (cursor_x, cursor_y, cursor_x + cursor_w, cursor_y + cursor_h),
        radius=max(2, size // 80),
        fill=(111, 255, 122, 255),
    )

    image.alpha_composite(term)
    return image


def write_png(image: Image.Image, path: Path, size: int) -> None:
    image.resize((size, size), Image.LANCZOS).save(path)


def main() -> None:
    ICONS_DIR.mkdir(parents=True, exist_ok=True)
    if ICONSET_DIR.exists():
        shutil.rmtree(ICONSET_DIR)
    ICONSET_DIR.mkdir()

    base = create_base_icon(1024)
    write_png(base, ICONS_DIR / "32x32.png", 32)
    write_png(base, ICONS_DIR / "128x128.png", 128)
    write_png(base, ICONS_DIR / "128x128@2x.png", 256)
    write_png(base, ICONS_DIR / "icon.ico", 256)

    iconset_sizes = {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }
    for filename, size in iconset_sizes.items():
        write_png(base, ICONSET_DIR / filename, size)

    print(f"Generated icons in {ICONS_DIR}")


if __name__ == "__main__":
    main()

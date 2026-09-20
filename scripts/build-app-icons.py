"""Build the application icon set from one geometry definition.

The mark is the Dashboard's gains waterfall reduced to three bars: cost basis,
market value, and the total return in the up-green. Colours are dark-theme
tokens from docs/VisualDesign.DarkTheme.md.

Every artefact below is generated from the BARS table, so the vector master and
the rasters cannot drift apart. Re-run after changing the geometry:

    python scripts/build-app-icons.py

Requires Pillow. Outputs (all committed):

    frontend/public/icon.svg            scalable favicon, the vector master
    frontend/public/favicon.ico         legacy/bookmark fallback
    frontend/public/apple-touch-icon.png  home-screen icon, squared for iOS masking
    desktop/icons/icon.ico              Windows executable, window, and taskbar icon

Small sizes are not simply downscaled. Each raster size snaps the bars to whole
output pixels before supersampling, because at 16px a half-pixel bar edge turns
the three bars into grey mush.
"""

from pathlib import Path

from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parent.parent

# --- the mark, on a 64-unit design grid -------------------------------------

GRID = 64
TILE_RADIUS = 14
BAR_HEIGHT = 8
BAR_RADIUS = 4

CANVAS = "#0a0b0d"  # --canvas
HAIRLINE = "#20242b"  # --hairline

# (x, y, width, fill) — the waterfall reads top to bottom.
#
# The two neutrals are one step lighter than the Dashboard's own waterfall bars
# (--wf-bar-subtotal/--wf-bar-total). At 16px the darker pair disappears into
# the tile, leaving only the green bar legible.
BARS = (
    (12, 16, 26, "#4a525d"),  # cost basis      (--wf-bar-total)
    (21, 28, 30, "#6b7280"),  # market value    (--text-muted)
    (30, 40, 22, "#16c784"),  # total return    (--up)
)

# Windows uses the small frames for the taskbar and title bar, the large ones
# for Explorer and Alt+Tab.
DESKTOP_ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
WEB_ICO_SIZES = (16, 32, 48)
APPLE_TOUCH_SIZE = 180

# iOS applies its own rounded mask and drops transparency, so that icon is a
# full square with the mark inset instead of a pre-rounded tile.
APPLE_TOUCH_INSET = 0.80

SUPERSAMPLE = 8


def _rgba(value):
    value = value.lstrip("#")
    return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4)) + (255,)


def render(size, *, squared=False, inset=1.0):
    """Render the mark at an exact pixel size, snapped to that size's grid."""
    supersampled = size * SUPERSAMPLE
    image = Image.new("RGBA", (supersampled, supersampled), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    scale = supersampled / GRID

    if squared:
        draw.rectangle([0, 0, supersampled - 1, supersampled - 1], fill=_rgba(CANVAS))
    else:
        box = [0, 0, supersampled - 1, supersampled - 1]
        draw.rounded_rectangle(box, radius=TILE_RADIUS * scale, fill=_rgba(CANVAS))
        draw.rounded_rectangle(
            box,
            radius=TILE_RADIUS * scale,
            outline=_rgba(HAIRLINE),
            width=max(1, round(scale)),
        )

    unit = size / GRID  # one design unit, in output pixels
    offset = (GRID * (1 - inset) / 2) * unit * SUPERSAMPLE
    for x, y, width, fill in BARS:
        left = round(x * unit * inset) * SUPERSAMPLE + offset
        top = round(y * unit * inset) * SUPERSAMPLE + offset
        right = left + max(SUPERSAMPLE, round(width * unit * inset) * SUPERSAMPLE)
        bottom = top + max(SUPERSAMPLE, round(BAR_HEIGHT * unit * inset) * SUPERSAMPLE)
        draw.rounded_rectangle(
            [left, top, right - 1, bottom - 1],
            radius=max(SUPERSAMPLE / 2, BAR_RADIUS * unit * inset * SUPERSAMPLE),
            fill=_rgba(fill),
        )

    return image.resize((size, size), Image.LANCZOS)


def svg_master():
    bars = "\n".join(
        f'  <rect x="{x}" y="{y}" width="{w}" height="{BAR_HEIGHT}" '
        f'rx="{BAR_RADIUS}" fill="{fill}"/>'
        for x, y, w, fill in BARS
    )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {GRID} {GRID}" \
width="{GRID}" height="{GRID}" role="img" aria-label="TickerTapeTallyBoard">
  <title>TickerTapeTallyBoard</title>
  <rect width="{GRID}" height="{GRID}" rx="{TILE_RADIUS}" fill="{CANVAS}"/>
  <rect x="0.5" y="0.5" width="{GRID - 1}" height="{GRID - 1}" \
rx="{TILE_RADIUS - 0.5}" fill="none" stroke="{HAIRLINE}"/>
{bars}
</svg>
"""


def write_ico(path, sizes):
    frames = [render(size) for size in sizes]
    largest, *rest = sorted(frames, key=lambda frame: frame.width, reverse=True)
    largest.save(
        path,
        format="ICO",
        sizes=[(frame.width, frame.width) for frame in frames],
        append_images=rest,
    )


def main():
    public = REPO / "frontend" / "public"
    public.mkdir(parents=True, exist_ok=True)
    (REPO / "desktop" / "icons").mkdir(parents=True, exist_ok=True)

    (public / "icon.svg").write_text(svg_master(), encoding="utf-8", newline="\n")
    write_ico(public / "favicon.ico", WEB_ICO_SIZES)
    render(APPLE_TOUCH_SIZE, squared=True, inset=APPLE_TOUCH_INSET).convert("RGB").save(
        public / "apple-touch-icon.png"
    )
    write_ico(REPO / "desktop" / "icons" / "icon.ico", DESKTOP_ICO_SIZES)

    for name in ("frontend/public/icon.svg", "frontend/public/favicon.ico",
                 "frontend/public/apple-touch-icon.png", "desktop/icons/icon.ico"):
        print(f"wrote {name} ({(REPO / name).stat().st_size} bytes)")


if __name__ == "__main__":
    main()

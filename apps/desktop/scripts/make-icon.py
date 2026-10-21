#!/usr/bin/env python3
"""Generate the app icon.

Run from `apps/desktop`:

    python scripts/make-icon.py && npm run tauri icon app-icon.png

# The mark

Two white windows, offset diagonally, on a near-black rounded tile. It says
"several windows, one place" without drawing a screen, an arrow, or a letter.

Monochrome on purpose: a coloured tile competes with the semantic colours the UI
uses for state (green/amber/red), and neutral reads as a tool rather than a
consumer app. Solid white shapes also survive 16 px in a taskbar, where outline
strokes and pane grids dissolve into grey mush — the 2x2 grid version was
unreadable at that size.

The front window is separated from the back one by a band of tile colour, so the
overlap reads as depth instead of two merged blobs.

Drawn at 4x and downsampled: PIL's own anti-aliasing on arcs leaves visible
stair-stepping.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

SIZE = 1024
SUPERSAMPLE = 4
CANVAS = SIZE * SUPERSAMPLE

# Off-black rather than #000: pure black looks like a missing asset beside other
# dock icons, and the faint lift keeps the tile edge visible on dark desktops.
INK = (17, 18, 20, 255)
PAPER = (255, 255, 255, 255)

# Close to the Windows 11 and macOS tile proportions.
TILE_RADIUS = 0.225

# Window geometry, in fractions of the canvas.
#
# The *union* of the two windows is what has to be centred, not the back one: with
# a diagonal offset the composition is visually heavier at the bottom-right, and
# centring the back window pushes the whole mark off-centre.
WINDOW = 0.455
OFFSET = 0.150
WINDOW_RADIUS = 0.058
# Gap between the two windows. Wide enough to stay open after downsampling to
# 16 px, where anything thinner collapses into grey.
SEPARATION = 0.042
BACK_ORIGIN = (
    (1.0 - (OFFSET + WINDOW)) / 2.0,
    (1.0 - (OFFSET + WINDOW)) / 2.0,
)
FRONT_ORIGIN = (
    BACK_ORIGIN[0] + OFFSET,
    BACK_ORIGIN[1] + OFFSET,
)


def main() -> None:
    image = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    draw.rounded_rectangle(
        (0, 0, CANVAS - 1, CANVAS - 1),
        radius=int(CANVAS * TILE_RADIUS),
        fill=INK,
    )

    side = int(CANVAS * WINDOW)
    radius = int(CANVAS * WINDOW_RADIUS)

    def window(origin: tuple[float, float]) -> tuple[int, int, int, int]:
        left = int(CANVAS * origin[0])
        top = int(CANVAS * origin[1])
        return (left, top, left + side, top + side)

    back = window(BACK_ORIGIN)
    front = window(FRONT_ORIGIN)

    draw.rounded_rectangle(back, radius=radius, fill=PAPER)

    # The front window is drawn as a tile-coloured frame plus a white body: the
    # frame is what separates it from the back window, so no transparency or
    # stroke-width trickery is needed.
    separation = int(CANVAS * SEPARATION)
    draw.rounded_rectangle(
        (
            front[0] - separation,
            front[1] - separation,
            front[2] + separation,
            front[3] + separation,
        ),
        radius=radius + separation,
        fill=INK,
    )
    draw.rounded_rectangle(front, radius=radius, fill=PAPER)

    image.resize((SIZE, SIZE), Image.LANCZOS).save("app-icon.png")
    print("wrote app-icon.png")


if __name__ == "__main__":
    main()

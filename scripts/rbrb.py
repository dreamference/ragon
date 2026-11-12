"""Render the RBRB chart (Ragon-Builds-Ragon benchmark) from docs/rbrb.json to images/rbrb.svg.

python scripts/rbrb.py            # write images/rbrb.svg
python scripts/rbrb.py --check    # exit 1 if the committed svg differs from the regenerated one
"""

import argparse
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "rbrb.json"
SVG = ROOT / "images" / "rbrb.svg"

WIDTH, HEIGHT = 640, 260
LEFT, RIGHT, TOP, BOTTOM = 56, 24, 44, 56
CAPTION = "generated tokens by Ragon as a share of all development tokens, per week"


def share_percent(point: dict[str, Any]) -> float:
    """The share of one point in percent, recomputed from the two counts."""
    ragon = point["ragon_generated_tokens"]
    total = ragon + point["other_generated_tokens"]
    return 100.0 * ragon / total if total else 0.0


def render(data: dict[str, Any]) -> str:
    """The chart as SVG text, deterministic for the same data."""
    points = data["points"]
    plot_w = WIDTH - LEFT - RIGHT
    plot_h = HEIGHT - TOP - BOTTOM

    def x_at(i: int) -> float:
        return LEFT + (plot_w / 2 if len(points) == 1 else plot_w * i / (len(points) - 1))

    def y_at(pct: float) -> float:
        return TOP + plot_h * (1 - pct / 100.0)

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-label="RBRB chart">',
        f'<rect width="{WIDTH}" height="{HEIGHT}" fill="#ffffff"/>',
        '<g font-family="sans-serif" font-size="11" fill="#444444">',
        f'<text x="{LEFT}" y="22" font-size="14" font-weight="bold" fill="#111111">'
        "RBRB: Ragon-Builds-Ragon share (%)</text>",
    ]
    for tick in (0, 25, 50, 75, 100):
        y = y_at(tick)
        out.append(
            f'<line x1="{LEFT}" y1="{y:.1f}" x2="{WIDTH - RIGHT}" y2="{y:.1f}" stroke="#dddddd"/>'
        )
        out.append(f'<text x="{LEFT - 6}" y="{y + 4:.1f}" text-anchor="end">{tick}</text>')
    coords = [(x_at(i), y_at(share_percent(p))) for i, p in enumerate(points)]
    if len(coords) > 1:
        path = " ".join(f"{x:.1f},{y:.1f}" for x, y in coords)
        out.append(f'<polyline points="{path}" fill="none" stroke="#2a6fdb" stroke-width="2"/>')
    for (x, y), p in zip(coords, points, strict=True):
        out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="#2a6fdb"/>')
        out.append(
            f'<text x="{x:.1f}" y="{HEIGHT - BOTTOM + 18}" text-anchor="middle">{p["week"]}</text>'
        )
    if points:
        x, y = coords[-1]
        label = f"{share_percent(points[-1]):.2f} % ({points[-1]['week']})"
        out.append(
            f'<text x="{x:.1f}" y="{y - 10:.1f}" text-anchor="middle" font-size="13" '
            f'font-weight="bold" fill="#111111">{label}</text>'
        )
    out.append(f'<text x="{LEFT}" y="{HEIGHT - 10}" font-size="10">{CAPTION}</text>')
    out.append("</g>")
    out.append("</svg>")
    return "\n".join(out) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if the committed svg is stale")
    args = parser.parse_args(argv)
    svg = render(json.loads(DATA.read_text(encoding="utf-8")))
    if args.check:
        if not SVG.exists() or SVG.read_text(encoding="utf-8") != svg:
            print("images/rbrb.svg is stale: run python scripts/rbrb.py", file=sys.stderr)
            return 1
        return 0
    SVG.write_text(svg, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())

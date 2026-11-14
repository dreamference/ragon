"""Render the RBRB chart and badge (Ragon-Builds-Ragon benchmark) from docs/rbrb.json.

The chart, images/rbrb.svg, has two lines: Ragon's share of development tokens in percent, and the
quality band, the revert rate of Ragon's commits against the others' (the "quality" entries of the
data: week, ragon_reverted, ragon_commits, other_reverted, other_commits). The badge,
images/rbrb-badge.svg, shows the latest share.

python scripts/rbrb.py            # write both svg files
python scripts/rbrb.py --check    # exit 1 if a committed svg differs from the regenerated one
"""

import argparse
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "rbrb.json"
SVG = ROOT / "images" / "rbrb.svg"
BADGE = ROOT / "images" / "rbrb-badge.svg"

WIDTH, HEIGHT = 640, 460
LEFT, RIGHT, TOP, BOTTOM = 56, 24, 44, 56
PANEL_H = 160  # height of the plot area of each of the two lines
QUALITY_TOP = TOP + PANEL_H + 90
CAPTION = "generated tokens by Ragon as a share of all development tokens, per week"


def share_percent(point: dict[str, Any]) -> float:
    """The share of one point in percent, recomputed from the two counts."""
    ragon = point["ragon_generated_tokens"]
    total = ragon + point["other_generated_tokens"]
    return 100.0 * ragon / total if total else 0.0


def revert_percent(entry: dict[str, Any], who: str) -> float:
    """The revert rate of one group ("ragon" or "other") of a quality entry, in percent."""
    commits = entry[f"{who}_commits"]
    return 100.0 * entry[f"{who}_reverted"] / commits if commits else 0.0


def _x_at(i: int, n: int) -> float:
    plot_w = WIDTH - LEFT - RIGHT
    return LEFT + (plot_w / 2 if n == 1 else plot_w * i / (n - 1))


def _panel(
    top: int, title: str, top_value: float, series: list[tuple[str, list[float]]]
) -> list[str]:
    """One line panel: a title, a grid, and one polyline per (colour, values) series."""
    out = [
        f'<text x="{LEFT}" y="{top - 14}" font-size="14" font-weight="bold" fill="#111111">{title}</text>'
    ]
    for k in range(5):
        value = top_value * k / 4
        y = top + PANEL_H * (1 - k / 4)
        out.append(
            f'<line x1="{LEFT}" y1="{y:.1f}" x2="{WIDTH - RIGHT}" y2="{y:.1f}" stroke="#dddddd"/>'
        )
        out.append(f'<text x="{LEFT - 6}" y="{y + 4:.1f}" text-anchor="end">{value:g}</text>')
    for colour, values in series:
        coords = [
            (_x_at(i, len(values)), top + PANEL_H * (1 - min(v, top_value) / top_value))
            for i, v in enumerate(values)
        ]
        if len(coords) > 1:
            path = " ".join(f"{x:.1f},{y:.1f}" for x, y in coords)
            out.append(
                f'<polyline points="{path}" fill="none" stroke="{colour}" stroke-width="2"/>'
            )
        out.extend(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="{colour}"/>' for x, y in coords)
    return out


def render(data: dict[str, Any]) -> str:
    """The two-line chart as SVG text, deterministic for the same data."""
    points = data["points"]
    quality = data.get("quality", [])
    shares = [share_percent(p) for p in points]
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-label="RBRB chart">',
        f'<rect width="{WIDTH}" height="{HEIGHT}" fill="#ffffff"/>',
        '<g font-family="sans-serif" font-size="11" fill="#444444">',
    ]
    out += _panel(TOP, "RBRB: Ragon-Builds-Ragon share (%)", 100.0, [("#2a6fdb", shares)])
    for i, p in enumerate(points):
        out.append(
            f'<text x="{_x_at(i, len(points)):.1f}" y="{TOP + PANEL_H + 18}" text-anchor="middle">{p["week"]}</text>'
        )
    if points:
        x = _x_at(len(points) - 1, len(points))
        y = TOP + PANEL_H * (1 - shares[-1] / 100.0)
        out.append(
            f'<text x="{x:.1f}" y="{y - 10:.1f}" text-anchor="middle" font-size="13" '
            f'font-weight="bold" fill="#111111">{shares[-1]:.2f} % ({points[-1]["week"]})</text>'
        )
    ragon_q = [revert_percent(q, "ragon") for q in quality]
    other_q = [revert_percent(q, "other") for q in quality]
    top_q = max([10.0, *ragon_q, *other_q])
    top_q = float(-(-top_q // 4) * 4)
    out += _panel(
        QUALITY_TOP,
        "Quality band: revert rate (%), Ragon vs others",
        top_q,
        [("#2a6fdb", ragon_q), ("#d9822b", other_q)],
    )
    for i, q in enumerate(quality):
        out.append(
            f'<text x="{_x_at(i, len(quality)):.1f}" y="{QUALITY_TOP + PANEL_H + 18}" text-anchor="middle">{q["week"]}</text>'
        )
    if not quality:
        out.append(
            f'<text x="{WIDTH / 2:.1f}" y="{QUALITY_TOP + PANEL_H / 2:.1f}" text-anchor="middle">'
            "no revert data yet</text>"
        )
    legend_y = QUALITY_TOP + PANEL_H + 40
    out.append(f'<rect x="{LEFT}" y="{legend_y - 9}" width="10" height="10" fill="#2a6fdb"/>')
    out.append(f'<text x="{LEFT + 16}" y="{legend_y}">Ragon</text>')
    out.append(f'<rect x="{LEFT + 80}" y="{legend_y - 9}" width="10" height="10" fill="#d9822b"/>')
    out.append(f'<text x="{LEFT + 96}" y="{legend_y}">others</text>')
    out.append(f'<text x="{LEFT}" y="{HEIGHT - 10}" font-size="10">{CAPTION}</text>')
    out.append("</g>")
    out.append("</svg>")
    return "\n".join(out) + "\n"


def render_badge(data: dict[str, Any]) -> str:
    """The badge as SVG text: "RBRB" and the latest share, deterministic for the same data."""
    points = data["points"]
    value = f"{share_percent(points[-1]):.2f} %" if points else "n/a"
    left_w, right_w = 48, 14 + 7 * len(value)
    w = left_w + right_w
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="20" viewBox="0 0 {w} 20" '
        f'role="img" aria-label="RBRB: {value}">\n'
        f'<rect width="{left_w}" height="20" fill="#555555"/>\n'
        f'<rect x="{left_w}" width="{right_w}" height="20" fill="#2a6fdb"/>\n'
        '<g font-family="sans-serif" font-size="11" fill="#ffffff" text-anchor="middle">\n'
        f'<text x="{left_w / 2:g}" y="14">RBRB</text>\n'
        f'<text x="{left_w + right_w / 2:g}" y="14">{value}</text>\n'
        "</g>\n</svg>\n"
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if the committed svg is stale")
    args = parser.parse_args(argv)
    data = json.loads(DATA.read_text(encoding="utf-8"))
    outputs = {SVG: render(data), BADGE: render_badge(data)}
    if args.check:
        stale = [
            p
            for p, text in outputs.items()
            if not p.exists() or p.read_text(encoding="utf-8") != text
        ]
        for p in stale:
            print(f"images/{p.name} is stale: run python scripts/rbrb.py", file=sys.stderr)
        return 1 if stale else 0
    for p, text in outputs.items():
        p.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())

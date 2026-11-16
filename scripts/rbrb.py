"""Render the RBRB chart and badge (Ragon-Builds-Ragon benchmark) from docs/rbrb.json.

The chart, images/rbrb.svg, has two lines: Ragon's share of development tokens in percent, and the
quality band, the revert rate of Ragon's commits against the others' (the "quality" entries of the
data: week, ragon_reverted, ragon_commits, other_reverted, other_commits). The badge,
images/rbrb-badge.svg, shows the latest share.

python scripts/rbrb.py            # write both svg files
python scripts/rbrb.py --check    # exit 1 if a committed svg differs from the regenerated one
python scripts/rbrb.py --closed-week  # print the newest closed ISO week (what the weekly job exports)
python scripts/rbrb.py --ingest PATH   # upsert the week of the weekly export PATH, then write the files

The weekly export (rbrb-gate --export) carries the counts only: week, ragon_generated, other_generated,
ragon_tasks_attempted, ragon_tasks_landed. --ingest turns it into the week's point, takes the commit and revert
counts of the week from git, and rewrites docs/rbrb.json, both svg files and the README "latest" line, so that
nothing needs a hand. Input fields come from the export; commits and reverts are derived from git.
"""

import argparse
import datetime
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "rbrb.json"
SVG = ROOT / "images" / "rbrb.svg"
BADGE = ROOT / "images" / "rbrb-badge.svg"
README = ROOT / "README.md"
LATEST = re.compile(r"(<!-- rbrb:latest -->).*?(<!-- /rbrb:latest -->)", re.S)

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


def week_bounds(week: str) -> tuple[datetime.date, datetime.date]:
    """Monday and Sunday of an ISO week such as "2026-W46"."""
    year, num = week.split("-W")
    monday = datetime.date.fromisocalendar(int(year), int(num), 1)
    return monday, monday + datetime.timedelta(days=6)


def closed_week(today: datetime.date) -> str:
    """The newest ISO week that is over on `today` (the week before today's), e.g. "2026-W46" on 2026-11-16.

    The weekly timer runs at 00:30 UTC on Monday, when the date's own week has just begun and is empty: the
    point to publish is the week that closed half an hour earlier.
    """
    year, num, _ = (today - datetime.timedelta(days=today.isoweekday())).isocalendar()
    return f"{year}-W{num:02d}"


def _git_count(args: list[str]) -> int:
    out = subprocess.run(
        ["git", "-C", str(ROOT), "log", "--format=%H", *args],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return len(out.split())


def git_counts(week: str, ref: str = "HEAD") -> dict[str, int]:
    """Commits and reverts of one week on `ref`: all, Ragon-made (Made-with: ragon), and reverts of each."""
    monday, sunday = week_bounds(week)
    window = [f"--since={monday}T00:00:00Z", f"--until={sunday}T23:59:59Z", ref]
    made = ["--grep=^Made-with: ragon$"]
    rev = ["--grep=^This reverts commit [0-9a-f]\\{40\\}\\.$"]
    all_n = _git_count(window)
    ragon_n = _git_count([*made, *window])
    rev_all = _git_count([*rev, *window])
    rev_ragon = _git_count([*rev, *made, "--all-match", *window])
    return {
        "ragon_commits": ragon_n,
        "other_commits": all_n - ragon_n,
        "ragon_reverted": rev_ragon,
        "other_reverted": rev_all - rev_ragon,
    }


def ingest(data: dict[str, Any], export: dict[str, Any], counts: dict[str, int]) -> dict[str, Any]:
    """Upsert the point and the quality row of the export's week; keeps points sorted, one per week."""
    week = export["week"]
    monday, sunday = week_bounds(week)
    ragon, other = int(export["ragon_generated"]), int(export["other_generated"])
    old = next((p for p in data["points"] if p["week"] == week), None)
    point: dict[str, Any] = {
        "week": week,
        "from": str(monday),
        "to": str(sunday),
        "ragon_generated_tokens": ragon,
        "other_generated_tokens": other,
        "rbrb_share": round(ragon / (ragon + other), 4) if ragon + other else 0.0,
        "ragon_tasks": {
            "attempted": int(export.get("ragon_tasks_attempted", 0)),
            "landed": int(export.get("ragon_tasks_landed", 0)),
        },
        "commits": {
            "ragon": counts["ragon_commits"],
            "all": counts["ragon_commits"] + counts["other_commits"],
        },
        "notes": (old or {}).get("final_notes", "")
        or "Complete week, generated tokens only (method_version 0): the export carries no prompt or cached token counts yet, so no price-weighted point exists.",
        "method_version": 0,
    }
    if old and "partial_note" in old:
        point["partial_note"] = old["partial_note"]
    elif old and old.get("to") != str(sunday):
        point["partial_note"] = old["notes"]  # the hand-measured partial week, kept as a note
    data["points"] = sorted([p for p in data["points"] if p["week"] != week] + [point], key=lambda p: p["week"])
    # The band stays undecided until the 28-day rework rate exists (spec 2.8, condition 4): never "met" without it.
    data["quality"] = [
        {
            "week": week,
            "ragon_reverted": counts["ragon_reverted"],
            "ragon_commits": counts["ragon_commits"],
            "other_reverted": counts["other_reverted"],
            "other_commits": counts["other_commits"],
            "rework": None,
            "band": "undecided",
            "provisional": True,
        }
    ]
    return data


def latest_line(data: dict[str, Any]) -> str:
    """The README's "latest" line, from the newest point."""
    p = data["points"][-1]
    return f"Latest value: **{share_percent(p):.2f} %** ({p['week']})."


def update_readme(text: str, data: dict[str, Any]) -> str:
    """Replace the marked "latest" block of the README text."""
    return LATEST.sub(lambda m: m.group(1) + "\n" + latest_line(data) + "\n" + m.group(2), text)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if the committed svg is stale")
    parser.add_argument("--ingest", metavar="PATH", help="upsert the week of a weekly export file")
    parser.add_argument("--closed-week", action="store_true", help="print the newest closed ISO week")
    args = parser.parse_args(argv)
    if args.closed_week:
        print(closed_week(datetime.datetime.now(datetime.timezone.utc).date()))
        return 0
    data = json.loads(DATA.read_text(encoding="utf-8"))
    if args.ingest:
        export = json.loads(Path(args.ingest).read_text(encoding="utf-8"))
        data = ingest(data, export, git_counts(export["week"]))
        DATA.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        README.write_text(update_readme(README.read_text(encoding="utf-8"), data), encoding="utf-8")
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

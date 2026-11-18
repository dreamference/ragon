"""Render the RBRB chart and badge (Ragon-Builds-Ragon benchmark) from docs/rbrb.json.

The chart, images/rbrb.svg, has two panels (x = ISO weeks; closed weeks filled, the running week hollow): Ragon's share of development tokens in percent, and the
quality band, the revert rate of Ragon's commits against the others' (the "quality" entries of the
data: week, ragon_reverted, ragon_commits, other_reverted, other_commits). The badge,
images/rbrb-badge.svg, shows the latest share.

python scripts/rbrb.py            # write both svg files
python scripts/rbrb.py --check    # exit 1 if a committed svg differs from the regenerated one
python scripts/rbrb.py --closed-week  # print the newest closed ISO week (what the weekly job exports)
python scripts/rbrb.py --ingest PATH   # upsert the week of the weekly export PATH, then write the files
python scripts/rbrb.py --ingest PATH --provisional   # the same for the running week, marked provisional

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
from collections.abc import Callable
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "rbrb.json"
SVG = ROOT / "images" / "rbrb.svg"
BADGE = ROOT / "images" / "rbrb-badge.svg"
README = ROOT / "README.md"
LATEST = re.compile(r"(<!-- rbrb:latest -->).*?(<!-- /rbrb:latest -->)", re.S)

WIDTH, HEIGHT = 640, 656
LEFT, RIGHT, TOP = 56, 24, 44
INSET = 56  # space between the plot's edge and the first and last week, for the labels
PANEL_H = 160  # height of the plot area of each of the two lines
TASKS_TOP = TOP + PANEL_H + 56
TASKS_H = 80  # height of the landed-task bars
QUALITY_TOP = TASKS_TOP + TASKS_H + 60
QUALITY_FROM = "2026-W47"  # the quality band is public from this week (owner, 2026-11-18)
TARGET_AT = 4  # the target is published, and drawn as a line, once the curve has this many points
BAND_MIN_COMMITS = 20  # spec 2.8, condition 3
CAPTION = "generated tokens by Ragon as a share of all development tokens, per ISO week (Monday to Sunday)"
STEPS = (1, 2, 5, 10, 20, 25, 50)
STYLE = (
    "<style>"
    ".bg{fill:#ffffff}.tx{fill:#444444}.hd{fill:#111111}.gr{stroke:#dddddd}"
    ".ln{stroke:#2a6fdb}.dot{fill:#2a6fdb}.ln2{stroke:#d9822b}.dot2{fill:#d9822b}"
    ".tg{stroke:#8250df}.bar{fill:#2a6fdb}.barh{fill:#ffffff;stroke:#2a6fdb;stroke-width:2}"
    ".hollow{fill:#ffffff;stroke:#2a6fdb;stroke-width:2}"
    "@media (prefers-color-scheme: dark){"
    ".bg{fill:#0d1117}.tx{fill:#9da7b3}.hd{fill:#f0f6fc}.gr{stroke:#30363d}"
    ".ln{stroke:#58a6ff}.dot{fill:#58a6ff}.ln2{stroke:#e3a24b}.dot2{fill:#e3a24b}"
    ".tg{stroke:#bc8cff}.bar{fill:#58a6ff}.barh{fill:#0d1117;stroke:#58a6ff}"
    ".hollow{fill:#0d1117;stroke:#58a6ff}}"
    "</style>"
)


def share_percent(point: dict[str, Any]) -> float:
    """The share of one point in percent, recomputed from the two counts."""
    ragon = point["ragon_generated_tokens"]
    total = ragon + point["other_generated_tokens"]
    return 100.0 * ragon / total if total else 0.0


def revert_percent(entry: dict[str, Any], who: str) -> float:
    """The revert rate of one group ("ragon" or "other") of a quality entry, in percent."""
    commits = entry[f"{who}_commits"]
    return 100.0 * entry[f"{who}_reverted"] / commits if commits else 0.0


def axis(highest: float) -> tuple[float, int]:
    """The top of the share axis and its tick step: max(10, 1.5 x highest), rounded up to a clean step, at most 100."""
    target = max(10.0, 1.5 * highest)
    step = next((s for s in STEPS if target / s <= 5), STEPS[-1])
    return min(100.0, float(-(-target // step) * step)), step


def week_index(week: str) -> int:
    """Whole weeks from a fixed Monday to the Monday of an ISO week, so that neighbours differ by one."""
    return (week_bounds(week)[0] - datetime.date(2000, 1, 3)).days // 7


def _x_for(weeks: list[str]) -> Callable[[str], float]:
    """The x position of a week: the weeks are evenly spaced by calendar, a week without a point leaves a gap."""
    if not weeks:
        return lambda _w: LEFT
    first, last = week_index(weeks[0]), week_index(weeks[-1])
    lo, hi = LEFT + INSET, WIDTH - RIGHT - INSET
    if first == last:
        return lambda _w: (lo + hi) / 2
    return lambda w: lo + (hi - lo) * (week_index(w) - first) / (last - first)


def _panel(
    top: int,
    title: str,
    top_value: float,
    step: float,
    series: list[tuple[str, list[tuple[float, float, bool]]]],
    height: int = PANEL_H,
) -> list[str]:
    """One line panel: a title, a grid every `step`, and per (class suffix, [(x, value, hollow)]) series a line and points.

    The line joins the closed points; a hollow point is a running week, drawn without a line to it from nothing
    but joined to the last closed point by a dashed segment.
    """
    out = [
        f'<text class="hd" x="{LEFT}" y="{top - 14}" font-size="14" font-weight="bold">{title}</text>'
    ]
    ticks = int(top_value // step)
    for k in range(ticks + 1):
        value = k * step
        y = top + height * (1 - value / top_value)
        out.append(f'<line class="gr" x1="{LEFT}" y1="{y:.1f}" x2="{WIDTH - RIGHT}" y2="{y:.1f}"/>')
        out.append(
            f'<text class="tx" x="{LEFT - 6}" y="{y + 4:.1f}" text-anchor="end">{value:g}</text>'
        )
    for suffix, values in series:
        coords = [(x, top + height * (1 - min(v, top_value) / top_value), h) for x, v, h in values]
        closed = [c for c in coords if not c[2]]
        if len(closed) > 1:
            path = " ".join(f"{x:.1f},{y:.1f}" for x, y, _ in closed)
            out.append(
                f'<polyline class="ln{suffix}" points="{path}" fill="none" stroke-width="2"/>'
            )
        for x, y, hollow in coords:
            if hollow:
                if closed:
                    out.append(
                        f'<line class="ln{suffix}" x1="{closed[-1][0]:.1f}" y1="{closed[-1][1]:.1f}" '
                        f'x2="{x:.1f}" y2="{y:.1f}" stroke-width="1.5" stroke-dasharray="4 3"/>'
                    )
                out.append(f'<circle class="hollow" cx="{x:.1f}" cy="{y:.1f}" r="5"/>')
            else:
                out.append(f'<circle class="dot{suffix}" cx="{x:.1f}" cy="{y:.1f}" r="4"/>')
    return out


def count_axis(highest: int) -> tuple[int, int]:
    """The top and the tick step of the landed-task axis: at least 5, at most four steps, a clean step."""
    target = max(5, highest)
    step = next((c for c in (1, 2, 5, 10, 20, 25, 50, 100, 200, 500) if target / c <= 4), 1000)
    return -(-target // step) * step, step


def _bars(
    top: int, height: int, top_value: int, step: int, bars: list[tuple[float, int, bool]]
) -> list[str]:
    """The landed-task panel: a grid and one bar per week (hollow while the week is running) with its count."""
    out = [
        f'<text class="hd" x="{LEFT}" y="{top - 14}" font-size="14" font-weight="bold">Ragon tasks landed per week</text>'
    ]
    for k in range(top_value // step + 1):
        y = top + height * (1 - k * step / top_value)
        out.append(f'<line class="gr" x1="{LEFT}" y1="{y:.1f}" x2="{WIDTH - RIGHT}" y2="{y:.1f}"/>')
        out.append(
            f'<text class="tx" x="{LEFT - 6}" y="{y + 4:.1f}" text-anchor="end">{k * step}</text>'
        )
    for x, count, hollow in bars:
        h = height * min(count, top_value) / top_value
        cls = "barh" if hollow else "bar"
        out.append(
            f'<rect class="{cls}" x="{x - 12:.1f}" y="{top + height - h:.1f}" width="24" height="{h:.1f}"/>'
        )
        out.append(
            f'<text class="hd" x="{x:.1f}" y="{top + height - h - 5:.1f}" text-anchor="middle" font-weight="bold">{count}</text>'
        )
    return out


def band(entry: dict[str, Any]) -> str:
    """The published quality band of a week: "met", "failed" or "undecided" (spec 2.8, conditions 1 and 3).

    The rework condition joins when the 28-day rework rate exists; until then the band is read from the revert
    rates alone, and an entry with fewer Ragon commits than the minimum stays undecided.
    """
    if entry["ragon_commits"] < BAND_MIN_COMMITS:
        return "undecided"
    return "met" if revert_percent(entry, "ragon") <= revert_percent(entry, "other") else "failed"


def _label(x: float, y: float, text: str) -> str:
    anchor = "end" if x > WIDTH - RIGHT - INSET - 10 and x > WIDTH / 2 else "middle"
    return f'<text class="hd" x="{x:.1f}" y="{y:.1f}" text-anchor="{anchor}" font-size="12" font-weight="bold">{text}</text>'


def render(data: dict[str, Any]) -> str:
    """The two-panel chart as SVG text, deterministic for the same data."""
    points = data["points"]
    quality = [q for q in data.get("quality", []) if q["week"] >= QUALITY_FROM]
    weeks = sorted({p["week"] for p in points} | {q["week"] for q in quality})
    x_of = _x_for(weeks)
    shares = [share_percent(p) for p in points]
    top, step = axis(max(shares, default=0.0))
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-label="RBRB chart">',
        STYLE,
        f'<rect class="bg" width="{WIDTH}" height="{HEIGHT}"/>',
        '<g font-family="system-ui, -apple-system, Segoe UI, Helvetica, Arial, sans-serif" font-size="11">',
    ]
    series = [
        (x_of(p["week"]), s, bool(p.get("provisional")))
        for p, s in zip(points, shares, strict=True)
    ]
    targets = [
        (x_of(p["week"]), p["target_pct"], False)
        for p in points
        if len(points) >= TARGET_AT and p.get("target_pct") is not None
    ]
    out += _panel(
        TOP,
        "RBRB: Ragon-Builds-Ragon share (%)",
        top,
        step,
        [("", series)],
    )
    if len(targets) > 1:
        path = " ".join(
            f"{x:.1f},{TOP + PANEL_H * (1 - min(v, top) / top):.1f}" for x, v, _ in targets
        )
        out.append(
            f'<polyline class="tg" points="{path}" fill="none" stroke-width="1.5" stroke-dasharray="2 3"/>'
        )
    landed = [
        (x_of(p["week"]), int(p["ragon_tasks"]["landed"]), bool(p.get("provisional")))
        for p in points
        if "ragon_tasks" in p
    ]
    count_top, count_step = count_axis(max((c for _, c, _ in landed), default=0))
    out += _bars(TASKS_TOP, TASKS_H, count_top, count_step, landed)
    for w in weeks:
        out.append(
            f'<text class="tx" x="{x_of(w):.1f}" y="{TOP + PANEL_H + 18}" text-anchor="middle">{w}</text>'
        )
    for p, s, (x, _, hollow) in zip(points, shares, series, strict=True):
        y = TOP + PANEL_H * (1 - min(s, top) / top) + (22 if hollow else -11)
        out.append(_label(x, y, f"{p['week']} so far {s:.2f} %" if hollow else f"{s:.2f} %"))
    ragon_q = [(x_of(q["week"]), revert_percent(q, "ragon"), False) for q in quality]
    other_q = [(x_of(q["week"]), revert_percent(q, "other"), False) for q in quality]
    top_q = float(-(-max([10.0, *(v for _, v, _ in ragon_q + other_q)]) // 4) * 4)
    out += _panel(
        QUALITY_TOP,
        "Quality band: revert rate (%), Ragon vs others",
        top_q,
        top_q / 4,
        [("", ragon_q), ("2", other_q)],
    )
    for q in quality:
        out.append(
            f'<text class="tx" x="{x_of(q["week"]):.1f}" y="{QUALITY_TOP + PANEL_H + 18}" text-anchor="middle">{q["week"]}</text>'
        )
        out.append(
            f'<text class="hd" x="{x_of(q["week"]):.1f}" y="{QUALITY_TOP + PANEL_H + 32}" text-anchor="middle" '
            f'font-weight="bold">band: {band(q)}</text>'
        )
    if not quality:
        out.append(
            f'<text class="tx" x="{WIDTH / 2:.1f}" y="{QUALITY_TOP + PANEL_H / 2:.1f}" text-anchor="middle">'
            "no revert data yet</text>"
        )
    legend_y = QUALITY_TOP + PANEL_H + 58
    out.append(f'<rect class="dot" x="{LEFT}" y="{legend_y - 9}" width="10" height="10"/>')
    out.append(f'<text class="tx" x="{LEFT + 16}" y="{legend_y}">Ragon</text>')
    out.append(f'<rect class="dot2" x="{LEFT + 80}" y="{legend_y - 9}" width="10" height="10"/>')
    out.append(f'<text class="tx" x="{LEFT + 96}" y="{legend_y}">other models</text>')
    out.append(f'<circle class="hollow" cx="{LEFT + 220}" cy="{legend_y - 4}" r="4"/>')
    out.append(f'<text class="tx" x="{LEFT + 230}" y="{legend_y}">running week so far</text>')
    if targets:
        out.append(
            f'<line class="tg" x1="{LEFT + 360}" y1="{legend_y - 4}" x2="{LEFT + 380}" y2="{legend_y - 4}" stroke-width="1.5" stroke-dasharray="2 3"/>'
        )
        out.append(f'<text class="tx" x="{LEFT + 386}" y="{legend_y}">target</text>')
    out.append(f'<text class="tx" x="{LEFT}" y="{HEIGHT - 10}" font-size="10">{CAPTION}</text>')
    out.append("</g>")
    out.append("</svg>")
    return "\n".join(out) + "\n"


def split_latest(data: dict[str, Any]) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    """The newest closed point and the running (provisional) point, either may be None."""
    closed = [p for p in data["points"] if not p.get("provisional")]
    running = [p for p in data["points"] if p.get("provisional")]
    return (closed[-1] if closed else None), (running[-1] if running else None)


def render_badge(data: dict[str, Any]) -> str:
    """The badge as SVG text: "RBRB", the latest closed week's share and, when a week is running, its "so far" value."""
    closed, running = split_latest(data)
    parts = [f"{share_percent(closed):.2f} %"] if closed else []
    if running:
        parts.append(f"{share_percent(running):.2f} % so far")
    value = " | ".join(parts) or "n/a"
    left_w, right_w = 48, 14 + 6 * len(value)
    w = left_w + right_w
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="20" viewBox="0 0 {w} 20" '
        f'role="img" aria-label="RBRB: {value}">\n'
        f'<rect width="{left_w}" height="20" fill="#555555"/>\n'
        f'<rect x="{left_w}" width="{right_w}" height="20" fill="#2a6fdb"/>\n'
        '<g font-family="system-ui, -apple-system, Segoe UI, Helvetica, Arial, sans-serif" font-size="11" fill="#ffffff" text-anchor="middle">\n'
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


def ingest(
    data: dict[str, Any], export: dict[str, Any], counts: dict[str, int], provisional: bool = False
) -> dict[str, Any]:
    """Upsert the point and the quality row of the export's week; keeps points sorted, one per week.

    A provisional point is the running week so far: it is marked "provisional" and the final weekly ingest of the
    same week replaces it. Only the export's aggregate counts are read; nothing else of it is published.
    """
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
    if provisional:
        point["notes"] = (
            "Running week so far (provisional): generated tokens only, replaced by the final value "
            "when the week is closed."
        )
        point["provisional"] = True
    if old and "partial_note" in old:
        point["partial_note"] = old["partial_note"]
    elif old and old.get("to") != str(sunday):
        point["partial_note"] = old["notes"]  # the hand-measured partial week, kept as a note
    data["points"] = sorted(
        [p for p in data["points"] if p["week"] != week] + [point], key=lambda p: p["week"]
    )
    # The band stays undecided until the 28-day rework rate exists (spec 2.8, condition 4): never "met" without it.
    if len(data["points"]) >= TARGET_AT and export.get("target_pct") is not None:
        # The target is private until the curve has TARGET_AT points (owner, 2026-11-18): only then is it stored.
        point["target_pct"] = float(export["target_pct"])
    row: dict[str, Any] = {
        "week": week,
        "ragon_reverted": counts["ragon_reverted"],
        "ragon_commits": counts["ragon_commits"],
        "other_reverted": counts["other_reverted"],
        "other_commits": counts["other_commits"],
        "rework": None,
        "provisional": True,
    }
    row["band"] = band(row)
    # The quality band is public from QUALITY_FROM on: earlier weeks carry no row.
    rows = [q for q in data.get("quality", []) if q["week"] != week and q["week"] >= QUALITY_FROM]
    data["quality"] = sorted(
        rows + ([row] if week >= QUALITY_FROM else []), key=lambda q: q["week"]
    )
    return data


FLAT_COUNTS = re.compile(r"\{\n(?:\s+\"[a-z_]+\": \d+,?\n)+\s*\}")


def dump_json(data: dict[str, Any]) -> str:
    """The data file's text: indented, with each small object of counts on one line (the land queue's prose lint
    reads a number that ends a line without a unit as a mistake; a count closed by a brace is not one)."""
    text = json.dumps(data, indent=2, ensure_ascii=False)
    return (
        FLAT_COUNTS.sub(lambda m: "{" + " ".join(m.group(0).split())[1:-1].strip() + "}", text)
        + "\n"
    )


def latest_line(data: dict[str, Any]) -> str:
    """The README's "latest" line: the newest closed week and, when one is running, its value so far."""
    closed, running = split_latest(data)
    parts = []
    if closed:
        parts.append(f"Latest value: **{share_percent(closed):.2f} %** ({closed['week']}, closed)")
    if running:
        lead = "; " if parts else "Latest value: "
        parts.append(
            f"{lead}**{share_percent(running):.2f} %** so far in {running['week']} (the running week)"
        )
    return "".join(parts) + "."


def update_readme(text: str, data: dict[str, Any]) -> str:
    """Replace the marked "latest" block of the README text."""
    return LATEST.sub(lambda m: m.group(1) + "\n" + latest_line(data) + "\n" + m.group(2), text)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="exit 1 if the committed svg is stale")
    parser.add_argument("--ingest", metavar="PATH", help="upsert the week of a weekly export file")
    parser.add_argument(
        "--provisional",
        action="store_true",
        help="with --ingest: the week is still running (replaced by the final)",
    )
    parser.add_argument(
        "--closed-week", action="store_true", help="print the newest closed ISO week"
    )
    args = parser.parse_args(argv)
    if args.closed_week:
        print(closed_week(datetime.datetime.now(datetime.timezone.utc).date()))
        return 0
    data = json.loads(DATA.read_text(encoding="utf-8"))
    if args.ingest:
        export = json.loads(Path(args.ingest).read_text(encoding="utf-8"))
        data = ingest(data, export, git_counts(export["week"]), args.provisional)
        DATA.write_text(dump_json(data), encoding="utf-8")
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

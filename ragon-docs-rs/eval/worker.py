"""Extract one PDF with one engine and print JSON: {ok, pages|error, seconds, maxrss_kb}.

Run inside bwrap (no network) and a memory-capped scope by bench.py."""

import json
import os
import resource
import subprocess
import sys
import time
from pathlib import Path


def is_rtl(ch):
    """Hebrew, Arabic, Syriac, Thaana, N'Ko and the Arabic presentation forms."""
    o = ord(ch)
    return 0x0590 <= o <= 0x08FF or 0xFB1D <= o <= 0xFDFF or 0xFE70 <= o <= 0xFEFF


def rtl_line(runs):
    """A line holding right-to-left script: every glyph of the line in visual order (left to right,

    a space where the gap is a word gap), then the Unicode bidi algorithm turns the visual string
    back into logical order (reversing right-to-left runs is its own inverse, and left-to-right runs
    such as numbers keep their order).

    Stream order cannot be trusted here: Chromium's PDFs draw
    Arabic glyphs in visual order, and PDFium's own text (get_text_bounded) reverses some words and
    not others. ragon-docs would use the unicode-bidi crate (MIT OR Apache-2.0)."""
    from bidi.algorithm import get_display

    glyphs = sorted((g for r in runs for g in r[5]), key=lambda g: g[0])
    h = max(r[3] - r[1] for r in runs)
    out, prev = [], None
    for left, right, ch in glyphs:
        if prev is not None and left - prev > 0.2 * h:
            out.append(" ")
        out.append(ch)
        prev = right if prev is None else max(prev, right)
    visual = "".join(out)
    n_rtl = sum(1 for c in visual if is_rtl(c))
    n_ltr = sum(1 for c in visual if c.isalpha() and not is_rtl(c))
    return get_display(visual, base_dir="R" if n_rtl >= n_ltr else "L")


def ordered_page(page):
    """PDFium's text with out-of-place runs put back.

    What ragon-docs would do with pdfium-render's characters.

    Stream order is kept for lines; only a run drawn later than the line it sits in
    (RFC PDFs draw MUST, SHOULD … over a gap after the rest of the line) is moved into that line."""

    tp = page.get_textpage()
    width = page.get_width()
    # Runs: characters in content-stream order, cut where the stream jumps (new line, backwards, a
    # big gap). A run drawn out of place (RFC PDFs draw MUST/SHOULD after the rest of the line,
    # over a gap left for it) becomes its own run and is put back by position below. Rects from
    # FPDFText_CountRects are not enough: a line's rect also contains the out-of-place keyword.
    import pypdfium2.raw as raw

    n = tp.count_chars()
    segs, cur, rotated, pending_high = [], None, [], None
    for c in range(n):
        # Read code points by PDFium char index; an astral character (math italics) comes as two
        # surrogate entries, which are joined here.
        cp = raw.FPDFText_GetUnicode(tp.raw, c)
        if 0xD800 <= cp < 0xDC00:
            pending_high = cp
            continue
        if 0xDC00 <= cp < 0xE000:
            if pending_high is None:
                continue
            cp = 0x10000 + ((pending_high - 0xD800) << 10) + (cp - 0xDC00)
        pending_high = None
        if not 0 < cp < 0x110000:
            continue
        ch = chr(cp)
        # Rotated text (a margin note such as "This publication is available free of charge")
        # would interleave with every line it passes; it is kept, after the page's text.
        angle = raw.FPDFText_GetCharAngle(tp.raw, c)
        if ch not in " \r\n" and 0.1 < angle % 3.14159265 < 3.04:
            rotated.append(ch)
            continue
        if ch in "\r\n":
            if cur:
                segs.append(cur)
                cur = None
            continue
        left, bottom, right, top = tp.get_charbox(c)
        if ch == " " or top - bottom <= 0:
            if cur and ch == " ":
                cur[4] += " "
            continue
        h = top - bottom
        rtl = is_rtl(ch)
        if cur is not None:
            centre = (bottom + top) / 2
            same_line = cur[1] - 0.2 * h <= centre <= cur[3] + 0.2 * h
            # Right-to-left script is drawn leftwards: a glyph just left of the run continues it.
            if same_line and (
                cur[2] - 0.5 * h <= left <= cur[2] + 3 * h
                or (rtl and cur[0] - 3 * h <= right <= cur[0] + 0.5 * h)
            ):
                cur[0] = min(cur[0], left)
                cur[2] = max(cur[2], right)
                cur[1] = min(cur[1], bottom)
                cur[3] = max(cur[3], top)
                cur[4] += ch
                cur[5].append((left, right, ch))
                continue
            segs.append(cur)
        cur = [left, bottom, right, top, ch, [(left, right, ch)]]
    if cur:
        segs.append(cur)
    segs = [tuple(s) for s in segs if s[4].strip()]
    tail = ("\n" + "".join(rotated)) if "".join(rotated).strip() else ""
    if not segs:
        return tail.strip()
    # Lines in content-stream order (which follows columns in nearly every producer); a run that
    # belongs to an earlier line (it sits inside that line's height and horizontal extent, over a
    # gap) is moved into it. Within a line, runs are read left to right. No page-level geometry,
    # so columns, sidebars and figures keep the producer's order.
    lines = []  # [bottom, top, left, right, [runs]]
    for s in segs:
        centre = (s[1] + s[3]) / 2
        home = None
        if (
            lines
            and lines[-1][0] - 0.2 * (s[3] - s[1]) <= centre <= lines[-1][1] + 0.2 * (s[3] - s[1])
            and (s[0] >= lines[-1][2] - (s[3] - s[1]) or any(is_rtl(c) for c in s[4]))
        ):
            home = lines[-1]
        else:
            for ln in reversed(lines[-80:]):
                h = s[3] - s[1]
                if (
                    ln[0] <= centre <= ln[1]
                    and ln[2] - 8 * h < s[0]
                    and s[2] < ln[3] + 8 * h
                    and not any(r[0] < s[2] and s[0] < r[2] for r in ln[4])
                ):
                    home = ln
                    break
        if home is None:
            lines.append([s[1], s[3], s[0], s[2], [s]])
        else:
            home[4].append(s)
            home[0], home[1] = min(home[0], s[1]), max(home[1], s[3])
            home[2], home[3] = min(home[2], s[0]), max(home[3], s[2])
    text = []
    for ln in lines:
        runs = sorted(ln[4], key=lambda r: r[0])
        if any(is_rtl(c) for r in runs for c in r[4]):
            text.append(rtl_line(runs))
            continue
        parts, prev = [], None
        for r in runs:
            if (
                prev is not None
                and r[0] - prev > 0.15 * (r[3] - r[1])
                and not parts[-1].endswith(" ")
                and not r[4].startswith(" ")
            ):
                parts.append(" ")
            parts.append(r[4])
            prev = r[2]
        text.append("".join(parts))
    return "\n".join(text) + tail

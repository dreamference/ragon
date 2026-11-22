"""OCR on the scanned fixtures: ocr_bench.py <engine> -> results/ocr-<engine>.json. Engines:

  v6-tiny, v6-small, v6-medium   RapidOCR PP-OCRv6, one multilingual model for every script
  v5-oracle                      RapidOCR PP-OCRv5 mobile with the per-script model of the page's real script
  v5-all                         every PP-OCRv5 recognition model on every page, each timed alone (the
                                 per-page selection rules are replayed from these by ocr_report.py)
  tesseract                      Tesseract 5.3.4, tessdata_fast, the page's language given
  osd                            Tesseract's script detection alone (--psm 0), for the selection question
Run by run_ocr.sh in a memory-capped scope with no network, four X925 cores, four threads.
Per page: seconds, word recall against the source's text layer (multiset; characters for Chinese, Japanese
and Korean, whose OCR output drops or moves the spaces), character error rate
(order-sensitive: a two-column page read in another order scores badly
at full recall), detected script, mean confidence, every recognised word with its confidence.
(A v5-detect engine that read the script from a v6-tiny pass was dropped: v6 reads Cyrillic, Hangul and
Arabic pages as Latin garbage, so it detected Latin for exactly the pages that needed another model.)"""

import collections
import json
import os
import re
import resource
import subprocess
import sys
import tempfile
import time
import unicodedata
from pathlib import Path

import Levenshtein
import numpy as np
import pypdfium2 as pdfium
from PIL import Image
from bidi.algorithm import get_display

D = os.path.dirname(os.path.abspath(__file__))
TESS = f"{D}/tess/root/usr/bin/tesseract"
TESS_ENV = dict(
    os.environ,
    LD_LIBRARY_PATH=f"{D}/tess/root/usr/lib/aarch64-linux-gnu",
    OMP_THREAD_LIMIT="4",
    TESSDATA_PREFIX=f"{D}/tess/tessdata",
)
SCRIPT_OF = {
    "eng": "Latin",
    "deu": "Latin",
    "swe": "Latin",
    "fra": "Latin",
    "rus": "Cyrillic",
    "ukr": "Cyrillic",
    "chi_sim": "Han",
    "jpn": "Japanese",
    "kor": "Hangul",
    "ara": "Arabic",
}
V5 = ("Latin", "Cyrillic", "Han", "Japanese", "Hangul", "Arabic")
FIXTURES = {}
for r in json.load(open(f"{D}/ocr/truth.json")):
    FIXTURES[r["id"]] = dict(
        kind=r["kind"], tess_lang=r["tess_lang"], text=r["text"], path=f"{D}/{r['file']}"
    )


def norm(s):
    """NFKC, lower case, whitespace collapsed (ocr_acceptance.py's truth normalisation)."""
    return re.sub(r"\s+", " ", unicodedata.normalize("NFKC", s).lower()).strip()


def is_cjk(c):
    """Chinese, Japanese and Korean characters (ocr_acceptance.py's)."""
    return "一" <= c <= "鿿" or "぀" <= c <= "ヿ" or "가" <= c <= "가" or "가" <= c <= "힣"


def tokens(s, chars):
    """Words of two letters or more, or CJK characters (ocr_acceptance.py's)."""
    s = norm(s)
    if chars:
        return [c for c in s if is_cjk(c)]
    return [w for w in re.findall(r"[^\W_]+", s) if len(w) > 1]


def page_image(fx):
    """The fixture page's grey image at the scan's own resolution."""
    p = fx["path"]
    if p.endswith(".pdf"):
        return pdfium.PdfDocument(p)[0].render(scale=1.0, grayscale=True).to_pil().convert("L")
    return Image.open(p).convert("L")


def rapidocr_words(ocr, img):
    """RapidOCR's words of one grey image with their confidences, in logical order."""
    out = ocr(np.array(img))[0] or []
    words = [(str(t).strip(), round(float(c), 3)) for _, t, c in out if str(t).strip()]
    joined = " ".join(w for w, _ in words)
    strong = [c for c in joined if 0x0590 <= ord(c) <= 0x08FF]
    ltr = [c for c in joined if c.isalpha() and ord(c) < 0x0590]
    if strong and len(strong) >= len(ltr):
        words = list(
            zip(get_display(joined, base_dir="R").split(), (s for _, s in words), strict=False)
        )
    return " ".join(w for w, _ in words), words


Q = {
    "Global.return_word_box": True,
    "Global.log_level": "warning",
    "EngineConfig.onnxruntime.intra_op_num_threads": 4,
    "EngineConfig.onnxruntime.inter_op_num_threads": 1,
}
engines = {}


def engine(kind):
    """One RapidOCR engine by kind: v6-* the model size, v5-* the recognition model's script."""
    from rapidocr import LangDet, LangRec, ModelType, OCRVersion, RapidOCR

    if kind not in engines:
        if kind.startswith("v6-"):
            mt = {
                "v6-tiny": ModelType.TINY,
                "v6-small": ModelType.SMALL,
                "v6-medium": ModelType.MEDIUM,
            }[kind]
            p = {**Q, "Det.model_type": mt, "Rec.model_type": mt}
        else:
            lang = {
                "Latin": LangRec.LATIN,
                "Cyrillic": LangRec.ESLAV,
                "Han": LangRec.CH,
                "Japanese": LangRec.CH,
                "Hangul": LangRec.KOREAN,
                "Arabic": LangRec.ARABIC,
            }[kind]
            p = {
                **Q,
                "Det.ocr_version": OCRVersion.PPOCRV5,
                "Det.lang_type": LangDet.CH,
                "Det.model_type": ModelType.MOBILE,
                "Rec.ocr_version": OCRVersion.PPOCRV5,
                "Rec.lang_type": lang,
                "Rec.model_type": ModelType.MOBILE,
            }
        engines[kind] = RapidOCR(params=p)
    return engines[kind]


def tesseract(img, lang, psm):
    """One grey image through Tesseract: its OSD answer (psm 0) or its TSV words (psm 6)."""
    tmp = tempfile.NamedTemporaryFile(suffix=".png")
    img.save(tmp.name)
    if psm == "0":
        out = subprocess.run(
            [TESS, tmp.name, "-", "--psm", "0"], capture_output=True, text=True, env=TESS_ENV
        )
        m = re.search(r"Script: (\w+)", out.stdout)
        return (m.group(1) if m else "none"), []
    out = subprocess.run(
        [TESS, tmp.name, "-", "-l", lang, "--psm", psm, "tsv"],
        capture_output=True,
        text=True,
        env=TESS_ENV,
    ).stdout
    cur, lines, words = None, [], []
    for row in [r.split("	") for r in out.splitlines()[1:]]:
        if len(row) < 12 or not row[11].strip():
            continue
        key = (row[2], row[4])
        if key != cur:
            lines.append([])
            cur = key
        lines[-1].append(row[11])
        words.append((row[11].strip(), round(float(row[10]) / 100.0, 3)))
    return lang, words


def score(fx, text, words):
    """Word recall, character error rate, mean confidence and the word confidences of one page."""
    chars = fx["tess_lang"] in ("chi_sim", "jpn", "kor")
    want, got = (
        collections.Counter(tokens(fx["text"], chars)),
        collections.Counter(tokens(text, chars)),
    )
    vocab = set(want)
    scored = [(s, all(x in vocab for x in tw)) for w, s in words if (tw := tokens(w, chars))]
    return dict(
        word_recall=round(sum((want & got).values()) / max(1, sum(want.values())), 3),
        cer=round(Levenshtein.normalized_distance(norm(fx["text"]), norm(text)), 3),
        words=scored,
        mean_conf=round(float(np.mean([s for _, s in words])), 3) if words else 0.0,
        n_words=len(words),
    )


eng = sys.argv[1]
pages = {}
for fid, fx in FIXTURES.items():
    img = page_image(fx)
    t0 = time.monotonic()
    real = SCRIPT_OF[fx["tess_lang"]]
    rec = {
        "kind": fx["kind"],
        "lang": fx["tess_lang"],
        "script": real,
        "pixels": img.width * img.height,
    }
    if eng == "v5-all":
        rec["models"] = {}
        for s in V5:
            ocr = engine(f"v5-{s}")
            text, words = rapidocr_words(ocr, img)
            rec["models"][s] = dict(
                seconds=round(time.monotonic() - t0, 2), **score(fx, text, words)
            )
        pages[fid] = rec
        print(
            eng,
            fid,
            {k: (v["word_recall"], v["mean_conf"]) for k, v in rec["models"].items()},
            flush=True,
        )
        continue
    detected = None
    if eng == "osd":
        detected, _ = tesseract(img, "eng", "0")
        rec["detected_script"] = detected
        rec["seconds"] = round(time.monotonic() - t0, 2)
    elif eng == "v5-oracle":
        ocr = engine(f"v5-{real}")
        text, words = rapidocr_words(ocr, img)
        rec.update(score(fx, text, words))
        rec["seconds"] = round(time.monotonic() - t0, 2)
    elif eng == "tesseract":
        detected, words = tesseract(img, fx["tess_lang"], "6")
        text = " ".join(w for w, _ in words)
        rec.update(score(fx, text, words))
        rec["seconds"] = round(time.monotonic() - t0, 2)
    else:
        ocr = engine(eng)
        text, words = rapidocr_words(ocr, img)
        rec.update(score(fx, text, words))
        rec["seconds"] = round(time.monotonic() - t0, 2)
    pages[fid] = rec
    print(
        eng,
        fid,
        f"{rec['seconds']:.1f}s",
        rec.get("word_recall"),
        rec.get("mean_conf"),
        detected,
        flush=True,
    )

ru = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024
kids = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024
json.dump(
    {
        "engine": eng,
        "peak_rss_mb": round(max(ru, kids)),
        "python_rss_mb": round(ru),
        "tesseract_rss_mb": round(kids),
        "pages": pages,
    },
    open(f"{D}/results/ocr-{eng}.json", "w"),
)

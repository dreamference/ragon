"""Which PDF question snippets does an engine lose, and what does it have there instead: snip_miss.py <engine>."""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evallib  # noqa: E402

eng = sys.argv[1]
r = json.load(open(f"{HERE}/results/pdf-{eng}.json"))
for line in open(f"{HERE}/questions.jsonl"):
    q = json.loads(line)
    if not q["doc"].endswith(".pdf"):
        continue
    pages = r["corpus/" + q["doc"]].get("pages", [])
    if any(evallib.is_hit(p, q["snippet"]) for p in pages):
        continue
    pg = int(q["loc"][2:]) - 1
    words = q["snippet"].split()
    text = pages[pg] if pg < len(pages) else ""
    i = text.find(words[0]) if words else -1
    print(q["id"], q["doc"], q["loc"], "|", q["snippet"][:50])
    print("   got:", repr(text[max(0, i - 40) : i + 160]) if i >= 0 else "(first word not on page)")

# Ragon Desk Work: documents, spreadsheets, slides and PDFs, as skills

**Status:** proposed, 2026-11-28, nothing built. The owner's answers D1–D8 of the same day are in §0 and bind everything below. The probes of §2.3 were run on this node the same day; every claim about the upstream vendor is marked *verified* (with its source) or *unverified* (§3.4, sources in §16).
**Goal:** `ragon` does ordinary office work (write a letter or a report, fill in a PDF form, clean up and total a spreadsheet, build a slide deck from notes, turn a folder of files into a summary) in a folder the user picks, checks what it made by looking at it, and can undo every change. It is built the way the upstream vendor builds the same thing: **skills in the upstream skill format**, connectors through the existing `/apps`, and **no addition to the session protocol**. A skill written for the upstream agent's document, spreadsheet, slides or PDF work runs on Ragon unchanged.
**Release:** a first version in 0.9.11, Friday 2026-12-18 (D5); §11 sizes it and says what the date needs.
**Builds on:**
- [RAGON_SKILLS](./DREAMFERENCE_RAGON_SKILLS.md): the loader, the roots and their order (§7), the catalogue budget (5,242 tokens), `ragon skill`;
- [RAGON_APPS](./DREAMFERENCE_RAGON_APPS.md): Gmail, Drive and Calendar in `/apps`, read-only, untrusted text wrapped;
- [RAGON_DESKTOP](./DREAMFERENCE_RAGON_DESKTOP.md) and [DESKTOP_PARITY](./DREAMFERENCE_RAGON_DESKTOP_PARITY.md): the Work view, `ragon-rs/web/policy.json`, the artifacts viewer (PARITY §2.8), office previews (B31), CSV tables (B27);
- [RAGON_LOCAL_INDEX](./DREAMFERENCE_RAGON_LOCAL_INDEX.md): `ragon-docs extract` (text of DOCX, XLSX, PPTX and PDF, offline, pure Rust);
- [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md): the `on` level;
- [RBRB](./DREAMFERENCE_RBRB.md) §5: which rows go to Ragon first.

**No Codex patch.** The series has about 325 bytes left (SKILLS header). Everything here is skills on disk, launcher code, a hook, an MCP tool in Ragon's own apps service, and window code behind the existing policy.

---

## 0. Decisions (the owner, 2026-11-28)

| # | Decision | What it means here |
|---|---|---|
| D1 | **Audience in 0.9.11: today's users** (developers who run Ragon) first; no new install path or onboarding. | Desk work arrives through `ragon update` and the existing installers; nothing is marketed to non-coders yet. The plain words of §6 are written for non-coders anyway, so a later onboarding needs no rewrite. |
| D2 | **All four file types in the first version:** documents (Word, PDF), spreadsheets (Excel, CSV), slides (PowerPoint), PDF reading and forms. | Four skills (§3), one toolchain (§4), 15 test tasks per type (§9). |
| D3 | **Screen: inside Work**, a plainer view (file preview, task list) when the task is not code; one mode, no protocol addition. | §6. The view is chosen by the window from the folder, not by a new message. |
| D4 | **File safety:** works only inside the folder the user picks; creates and edits freely there; **asks before deleting or overwriting an existing file**; **every change can be undone**. | §7: the sandbox's writable root, a `PreToolUse` guard, and snapshots outside the folder. |
| D5 | **0.9.11 ships Friday 2026-12-18.** | §11. |
| D6 | **Lanes:** desk work and the 1.0 items both get full lanes (extra lanes from the Codex account and the main account); nothing gives way unless budget runs out. | §11.2 assumes three parallel lanes for desk work. |
| D7 | **Quality bar:** about 60 real desk tasks across the four types; a task passes when the file **opens, is correct and looks right**; ships at **70 % or more**, as a preview below it. | §9. |
| D8 | **Tooling:** small open libraries ship with Ragon and write the files; **the office suite** (rendering for preview and result check) **installs on first use with the user's consent**. | §4. |

---

## 1. Scope

**In the first version (0.9.11):**
- four skills, `doc`, `spreadsheet`, `slides` and `pdf` (names §3), plus one shared checker the skills call;
- the desk toolchain: a Python runtime and its pinned libraries, shipped in both install modes, offline; LibreOffice and Poppler on first use with consent;
- the commands upstream skills call by name (`python3`, `pip`, `uv`, `soffice`, `pdftoppm`) answered by the toolchain inside desk threads, so that those skills run unchanged;
- the check of a produced file: reopened by its library and by the suite, its text extracted, its pages rendered to images and shown to the model;
- the plain Work view for a folder that is not a code project;
- file safety: the folder as the only writable place, a guard that asks before deletes and overwrites, undo of every change;
- two read verbs in the apps service so a Drive file or a Gmail attachment lands in the folder as a file;
- the quality set of 60 tasks, its grader and the 70 % gate.

**Not in the first version** (§12 lists them with where they go).

**Who does what.** The model reads a skill's `SKILL.md` and runs the libraries through its shell, as it does for code. Nothing new answers requests: no service, no tool server for writing files.

---

## 2. What is there today

### 2.1 Read from the code and the specs

- **Skills.** Roots, order and budget are SKILLS §2.1 and §7: repository `.agents/skills` and `.codex/skills`; `~/.ragon/skills/<name>`; `~/.agents/skills`; the linked sources `from-*`; the bundled system skills. Hidden directories under a root are skipped. Only `name`, `description` and `metadata.short-description` are read from the frontmatter, plus `agents/openai.yaml` beside it. The catalogue is capped at 2 % of the advertised window, 5,242 tokens.
- **System skills in the pinned source** (`codex/codex-rs/skills/src/assets/samples/`, `rust-v0.158.0`, read 2026-11-28): `imagegen`, `openai-docs`, `plugin-creator`, `review-agent`, `skill-creator`, `skill-installer`. **No document, spreadsheet, slides or PDF skill is bundled.** The `skill-installer` installs by default from the vendor's public catalogue, `openai/skills`, path `skills/.curated`, and from `skills/.experimental` on request (its `SKILL.md`, lines 10–14 and 54–57).
- **Previews.** The window converts `.docx`, `.xlsx`, `.pptx` and `.odt` to PDF with an installed LibreOffice, headless, inside bubblewrap with the network unshared, under a time limit (`desktop/electron/src/office.ts`, B31); without LibreOffice the tab says so. CSV shows as a table (B27). The artifacts viewer shows Markdown, images, PDF and HTML (PARITY §2.8).
- **Text of office files.** `ragon-docs extract <file>` prints the text of DOCX, ODT, XLSX (stored values), PPTX and PDF by page, offline (LOCAL_INDEX §18–§19). Run on this node against a LibreOffice-made DOCX and PDF: correct text, page count reported.
- **Apps.** `drive_read` returns at most 20,000 characters of a Doc, Sheet or Slides export as text and refuses binary files; there is no write verb anywhere (APPS §6). At `/airgapped on` no app server runs.
- **Install modes** (SETUP §3.2). A **client** install is Rust binaries only, no Python environment. A **node** adds the wheel in its own virtualenv (`~/.local/share/dreamference/venv`) that nothing but `ragon update` and the installer may change. `--from <dir>` installs from a bundle, offline. The node's Python packages (`setup.py`) carry no office library.
- **Hooks.** The pinned Codex runs `PreToolUse` hooks that may answer `allow`, `deny` or `ask` with a reason (RAGON_CLAUDE §2, lines 119–122 and 257). Ragon already registers one (`ragon compat guard`, RAGON_CLAUDE §11) and records that a shell command's paths cannot be matched reliably.

### 2.2 The upstream vendor's office skills, as far as they were checked

There are two generations; the details and sources are §3.4.
- **The public catalogue** (`openai/skills`, the default source of the bundled `skill-installer`) carried `doc`, `spreadsheet`, `slides` and `pdf`, each under Apache-2.0. `spreadsheet` and `slides` were removed on 2026-04-16 and `doc` on 2026-05-01; only `pdf` remains, and the catalogue's README now marks the whole repository deprecated in favour of the vendor's plugin repository (*verified*).
- **The office skills Codex runs today** ship inside the product as plugins of a built-in marketplace: `documents`, `spreadsheets`, `presentations` and `pdf`. They are not published in any public repository of the vendor and carry no licence (*unverified* beyond their absence from the plugin repository: known only from user reports on the Codex issue tracker).
- **Both generations share one loop:** write the file with a library, convert it with headless LibreOffice, render its pages with Poppler, look at the images, fix, render again (*verified* for the public generation; the skills say "inspect every page" before delivery).

The vendor's documentation also describes a Work experience for everyday, non-code work "with the same core capabilities" as Codex (*verified*, §3.4). D3's plain view is Ragon's answer to it, inside the one Work page.

### 2.3 Measured on this node, 2026-11-28

| Probe | Result |
|---|---|
| The model server's `/model_info` | `has_image_understanding: true`; the catalog Ragon writes lists `input_modalities: ["text", "image"]` for the served model |
| A 480×200 PNG with a line of text and two coloured boxes, sent straight to the server | Correct text and colours; 90 image tokens; 0.6 s |
| A 662×936 page rendered by `pdftoppm -r 80`, sent straight to the server | First and last paragraph numbers read correctly (they match `pdftotext`); **609 image tokens per page** |
| The same small PNG through `ragon exec`, asked to look at it with its image tool | The tool list offered `view_image`. **In two runs the model did not call it and described an image that does not exist** (wrong text, wrong colours). n = 2: a warning, not a rate |
| The same PNG attached with `ragon exec … -i p.png` | **The request failed**: the client asked for 18,754 input + 243,477 completion tokens = 262,231, over the 262,144 window. The completion budget is computed without the image's tokens. Not located yet (§10, row P0.2) |
| LibreOffice 24.2 headless, text → DOCX and DOCX → PDF (7 pages) | 1.4 s and 1.1 s cold; **peak resident memory 233 MiB** each (`/usr/bin/time -v`) |
| `pdftoppm`, three pages at 80 dpi | 0.16 s, 16 MiB |
| The libraries of §4.1 as wheels (Linux arm64, CPython 3.12) | about 70 MiB together; licences read from each wheel's metadata (§4.1) |

**What follows from it.** The model can see a page, and a page costs about 600 tokens, so checking a ten-page document by eye costs about 6,000 tokens. But the model **may not look, and may say it looked**. The check of §5 is therefore built so that looking is not left to the model's choice, the facts that can be checked by a program are checked by a program, and the grader of §9 never takes the agent's word.

---

## 3. The skills

### 3.1 The set

| Ours (`name`) | Covers | Libraries it tells the model to use | Upstream counterpart (§3.4) |
|---|---|---|---|
| `doc` | Word documents: write, edit, fill a template, tables, headings, comments; export to PDF | `python-docx`; `ragon-docs extract`; the checker | public `doc`, same name (removed 2026-05-01, *verified*); in-product `documents` (*unverified*) |
| `spreadsheet` | Excel and CSV: read, clean, total, formulas, formatting, charts native to the file | `openpyxl` (formulas, formatting), `pandas` (analysis), `XlsxWriter` (large new files), the `csv` module; formula results recalculated by the suite | public `spreadsheet`, same name (removed 2026-04-16, *verified*); in-product `spreadsheets` (*unverified*) |
| `slides` | PowerPoint: build a deck from notes, edit slides, speaker notes, images | `python-pptx`, `Pillow` | public `slides`, same name (removed 2026-04-16, *verified*; it builds decks with PptxGenJS); in-product `presentations` (*unverified*) |
| `pdf` | PDF: read, extract tables, fill forms, merge and split, create | `pypdf` (forms, merge), `pdfplumber` (text and tables), `reportlab` (create), `pypdfium2` and `pdftoppm` (render) | public `pdf`, same name (live, *verified*; also installed on this node); in-product `pdf` (*unverified*) |

**Names follow the public catalogue**, the only generation that is published, licensed for reuse and installable on Ragon: a user who installs one of those skills gets it in place of ours by name (§3.2). The in-product names (`documents`, `spreadsheets`, `presentations`) are owner question Q2.

**Trigger text matches upstream.** A skill is chosen by its `description`; the model of a user who says "make me a deck" should pick the same skill on Ragon as upstream. So each description starts "Use when …" as the public ones do, names the same file extensions and verbs (for `spreadsheet`: `.xlsx`, `.csv`, `.tsv`; creating, editing, analysing, formatting; formulas; visual review), front-loads the trigger words as the vendor's documentation advises, and names the Ragon checker. Whether a description may be the upstream text verbatim is owner question Q1; the default is our own words.

**Draft descriptions** (our own words, Q1's default; Phase 2 measures them against the budget and may shorten them):
- `doc`: "Use when the user asks to read, create or edit Word documents (`.docx`): letters, reports, memos, headings, tables, page layout. Writes with `python-docx` and checks every page with `ragon desk check` before saying it is done."
- `spreadsheet`: "Use when tasks involve reading, creating, editing, analysing or formatting spreadsheets (`.xlsx`, `.csv`, `.tsv`): cleaning data, totals, formulas, number formats, charts. Recalculates and checks the sheet with `ragon desk check`."
- `slides`: "Use when the user wants a PowerPoint deck (`.pptx`) built from notes or an existing deck edited: slides, layouts, images, speaker notes. Builds with `python-pptx` and checks every slide with `ragon desk check`."
- `pdf`: "Use when tasks involve reading, filling, merging, splitting or creating PDF files where layout matters: text and tables, form fields, new PDFs. Renders the pages and checks them with `ragon desk check`."

**One shared checker, not four.** The skills call `ragon desk check <file>` (§5) rather than each shipping its own render script.

### 3.2 Where they live, and why an upstream copy wins

The four skills ship in Ragon's release and are linked as one more source, `~/.ragon/skills/from-ragon/`, placed **after** the other linked sources and before the system skills in the order of SKILLS §7. Consequences:
- **An upstream skill of the same name, installed by the user** (with `ragon skill add` or the built-in `skill-installer`, into `~/.ragon/skills/<name>`), **shadows ours** and runs unchanged on the same toolchain (§4). This is the compatibility promise: the user may prefer the vendor's skill, and Ragon's toolchain is what makes it work offline.
- A repository's own skill of the same name shadows both, as it does today.
- `ragon skill list --all` shows which one is live; `ragon skill disable doc` switches ours off.
- **Skills linked from another agent's folder** (the `from-*` sources) may cover the same file types under other names; on this node `docx`, `xlsx` and `pptx` are linked that way. Different names do not shadow each other, so both are listed and the model picks by description. `ragon skill list` marks the overlap (same file extensions in two descriptions) and `ragon skill disable` keeps one; the toolchain runs both kinds, as far as their imports are in it (§4.1).
- **The catalogue budget is shared.** Linked office skills carry descriptions of 500 characters or more; a node with many linked skills can pass the 5,242 tokens, and the pinned Codex then drops every description (SKILLS §7). Phase 2 measures the catalogue of a node with linked skills and keeps ours short.
- No patch: the system root is compiled in, and `from-ragon` is a link the launcher writes like the other `from-*` links.

**Budget.** Four descriptions of about 70 tokens each, with their paths, are about 320 of the 5,242 catalogue tokens (6 %). Measured with Codex's arithmetic at Phase 2.

### 3.3 What a skill body says

Each `SKILL.md` follows the same five parts, short (under 2,000 tokens; the body is read only when the skill is chosen):
1. **When to use** (file types and verbs).
2. **Where to work:** in the working folder; intermediate files under `tmp/` inside it, results beside the inputs or under `output/` when the user did not say. The public upstream skills use the same pattern (`tmp/pdfs/` and `output/pdf/` in `pdf`, `tmp/docs/` and `output/doc/` in `doc`; *verified*).
3. **How to write it:** which library for which job, with two or three idioms each (a table with a header row in `python-docx`, a formula and a number format in `openpyxl`, a title-and-bullets slide in `python-pptx`, filling a form field in `pypdf`).
4. **Check before you say it is done:** run `ragon desk check <file>`, then call `view_image` on **every** image path it prints, then fix what it reports; never describe a page you have not viewed in this turn.
5. **What not to do:** never delete or overwrite an existing file without asking (the guard asks anyway, §7); do not install packages (the desk `pip` and `uv` answer for what ships and refuse the rest with a sentence, §4.2); never run a document's macros.

`agents/openai.yaml` beside each carries the `interface` block (display name, short description, icon) as upstream's skills do, so the desktop's skills page and `/skills` show them like upstream ones.

### 3.4 Upstream, verified and not

Read 2026-11-28 by fetching the vendor's repositories and documentation (sources in §16). **Verified** means read in a vendor-authored source at the cited place. **Unverified** means it is known only from user reports, a third-party copy or inference.

| # | Claim | Mark | Source |
|---|---|---|---|
| U1 | The public catalogue's README opens "This repository is deprecated" and points to the vendor's plugin repository | verified | `openai/skills` README, main |
| U2 | `pdf` is live: `name: "pdf"`, description "Use when tasks involve reading, creating, or reviewing PDF files where rendering and layout matter; prefer visual checks by rendering pages (Poppler) and use Python tools such as `reportlab`, `pdfplumber`, and `pypdf` for generation and extraction."; Apache-2.0; writes `tmp/pdfs/` and `output/pdf/`; renders with `pdftoppm -png` and says not to deliver "until the latest PNG inspection shows zero visual or formatting defects" | verified | `skills/.curated/pdf/SKILL.md`, main; the copy installed on this node is identical |
| U3 | `doc` was removed on 2026-05-01 ("Remove curated doc skill (#382)"). Its last text: `name: "doc"`, description "Use when the task involves reading, creating, or editing `.docx` documents, especially when formatting or layout fidelity matters; prefer `python-docx` plus the bundled `scripts/render_docx.py` for visual checks."; Apache-2.0; uses `python-docx`, `pdf2image`, `soffice`, `pdftoppm`, `uv` | verified | `skills/.curated/doc/SKILL.md` at `45d05d75` |
| U4 | `spreadsheet` was removed on 2026-04-16 ("Remove spreadsheets and slides skills (#350)"). Its last description: "Use when tasks involve creating, editing, analyzing, or formatting spreadsheets (`.xlsx`, `.csv`, `.tsv`) with formula-aware workflows, cached recalculation, and visual review."; uses `openpyxl`, `pandas`, optionally `matplotlib`, `soffice`, `pdftoppm`; tells the model to avoid `FILTER`, `XLOOKUP`, `SORT` and `SEQUENCE` | verified | `skills/.curated/spreadsheet/SKILL.md` at `e6afb0d7` |
| U5 | `slides` was removed in the same commit. Its description begins "Create and edit presentation slide decks (`.pptx`) with PptxGenJS, bundled layout helpers, and render/validation utilities."; it needs Node with `pptxgenjs`, `skia-canvas`, `fontkit`, `linebreak`, `prismjs` and `mathjax-full`, plus `Pillow`, `pdf2image`, `numpy`, and `python-pptx` for inspection only; its scripts render slides, check overflow and detect font substitution | verified | `skills/.curated/slides/SKILL.md` at `e6afb0d7` |
| U6 | Each carries `agents/openai.yaml` with `interface.display_name` ("PDF Skill", "Word Docs", "Spreadsheet Skill", "Slides"), `short_description`, icons and `default_prompt` | verified | the same directories |
| U7 | The three Python skills install missing libraries with `uv pip install`, falling back to `pip`, and install LibreOffice and Poppler with the system package manager when they are missing | verified | the same files |
| U8 | `skill-installer` installs from `openai/skills`, `skills/.curated`, by default; `skills/.experimental` no longer exists on main | verified | pinned source `codex-rs/skills/src/assets/samples/skill-installer/SKILL.md`; the repository tree |
| U9 | The skills list may use at most 2 % of the context window (8,000 characters when the window is unknown); descriptions are shortened first, then skills are left out; `skills.max_context_tokens` raises it up to 10,000 tokens; "front-load the key use case and trigger words" | verified in the vendor's documentation; **not** checked against the pinned `rust-v0.158.0`, whose behaviour SKILLS §7 records (all descriptions dropped past the budget) | the vendor's skills documentation page |
| U10 | `agents/openai.yaml` keys: `interface.{display_name, short_description, icon_small, icon_large, brand_color, default_prompt}`, `policy.allow_implicit_invocation`, `dependencies.tools` | verified | the same page |
| U11 | Today's office skills in Codex are plugins of a built-in marketplace named `openai-primary-runtime`: `documents`, `Spreadsheets`, `Presentations`, `pdf`, and a fifth that drives a live Excel session; they are cached under the user's Codex home and named like `documents:documents` | unverified | Codex issues #26962, #28889, #19957, #21425 |
| U12 | Those plugins use a page renderer and a LibreOffice bundled with the product for documents, a JavaScript artifact library for spreadsheets and presentations, and the public `pdf` lineage plus fillable forms; their descriptions change between versions | unverified | Codex issues #46860, #42183, #30792, #19957; a third-party copy of unknown version |
| U13 | They are not in the vendor's plugin repository | verified (absence) | `openai/plugins` tree, main |
| U14 | No licence is published for them | unverified | Codex issue #45288 |
| U15 | The vendor documents a Work experience for non-code work that has "the same core capabilities" as Codex "with an experience designed for everyday work" | verified | the vendor's "Get started with Work" documentation page |

**Not verified at all:** whether today's in-product skills still write with Python libraries; their exact trigger text; whether the in-product `pdf` is the public one unchanged.

**What compatibility means, then.**
- The public `pdf`, `doc` and `spreadsheet` run unchanged on the desk toolchain because it carries their imports (§4.1) and answers the commands they call (`uv`, `pip`, `soffice`, `pdftoppm`, `pdfinfo`, §4.2). Phase 6 proves it with one task each (§13).
- The public `slides` runs unchanged only with Node and its six JavaScript packages: owner question Q10.
- The in-product skills are unpublished and unlicensed, so they cannot be installed on Ragon; only their names can be matched (Q2).

---

## 4. The offline toolchain

### 4.1 What ships (D8: small open libraries)

| Library | Version (pinned at spec time) | Licence (from the wheel's metadata) | For |
|---|---|---|---|
| `python-docx` | 1.2.0 | MIT | Word |
| `openpyxl` | 3.1.5 | MIT | Excel read/write |
| `XlsxWriter` | 3.3.4 | BSD-2-Clause | large new Excel files, charts |
| `python-pptx` | 1.0.2 | MIT | PowerPoint |
| `pypdf` | 7.2.0 | BSD-3-Clause | PDF forms, merge, split |
| `pdfplumber` | 0.11.11 | MIT | PDF text and tables |
| `pdfminer.six` | 20260107 | MIT | under `pdfplumber` |
| `reportlab` | 5.1.2 | BSD | PDF creation |
| `pypdfium2` | 5.15.2 | BSD-3-Clause and Apache-2.0 (PDFium) | render PDF pages without Poppler |
| `lxml` | 6.2.0 | BSD-3-Clause | under `python-docx` and `python-pptx` |
| `Pillow` | 13.0.0 | MIT-CMU | images in slides and documents |
| `pdf2image` | 1.17.1 | MIT | page images, as the public `doc` and `slides` call it |
| `pandas` | 3.1.1 | BSD-3-Clause | analysis, as the public `spreadsheet` calls it |
| `numpy` | 2.5.5 | BSD-3-Clause, 0BSD, MIT, Zlib, CC0-1.0 (its bundled OpenBLAS included) | under `pandas` and `matplotlib` |
| `matplotlib` | 3.11.2 | the matplotlib licence (PSF-style, permissive) | charts as images, optional in the public `spreadsheet` |
| under `matplotlib` and `pandas` | `fonttools` 4.69.1, `kiwisolver` 1.5.1, `contourpy` 1.4.0, `cycler` 0.12.1, `pyparsing` 3.3.3, `python-dateutil` 4.0.0, `packaging` 26.4, `tzdata` 2026.6 | MIT, BSD-3-Clause, BSD-3-Clause, BSD-3-Clause, MIT, BSD-3-Clause, Apache-2.0 or BSD-2-Clause, Apache-2.0 | |

**No copyleft library ships.** The set is what the public upstream skills import (§3.4, U2–U5: `reportlab`, `pdfplumber`, `pypdf`, `python-docx`, `pdf2image`, `openpyxl`, `pandas`, `matplotlib`, `Pillow`, `numpy`, `python-pptx`) plus what our own four need. About 70 MiB as Linux arm64 wheels (24.5 MiB for the first eleven, 45 MiB for `pandas`, `numpy`, `matplotlib` and theirs); the Python runtime adds an estimated 30–45 MiB per target (*not measured*; Phase 1 measures both per target). Versions are re-pinned at Phase 1, each wheel by its hash.

**Node, for decks built in JavaScript (Q10).** The public `slides` skill (U5) and some linked deck skills build with PptxGenJS. A Node runtime (MIT) and `pptxgenjs` 4.0.1, `skia-canvas` 4.0.1, `fontkit` 3.0.0, `linebreak` 2.0.0, `prismjs` 1.30.0 (all MIT) and `mathjax-full` 3.2.2 (Apache-2.0), licences read from the npm registry, come to about 40 MiB unpacked before Node itself and `skia-canvas`'s native library (*not measured*). The default is to ship them in the desk environment, because the brief's promise is that upstream skills run unchanged; our own `slides` does not need them.

**How scripts find the Node packages.** The packages sit in `~/.ragon/desk/node/node_modules/`, and the desk `node` sets `NODE_PATH` to that folder, so a `require('pptxgenjs')` resolves from any working folder. *Unverified:* an ES-module `import` ignores `NODE_PATH`. P0.3 reads how the public `slides` scripts load their packages; if they use `import`, the installer links a `node_modules` into the skill's own folder instead.

**The Python runtime.** A client has no Python of Ragon's, and the user's own `python3` (if any) must not be changed. So the desk toolchain is **its own environment**, `~/.ragon/desk/` (`bin/python3`, `bin/python`, the `pip` and `uv` stand-ins of §4.2, the libraries in its `site-packages`), built from a relocatable CPython build (the standalone builds published for this purpose, PSF licence; which one is pinned in Phase 1) and the wheels above. It is never the node's virtualenv and never the system Python.

### 4.2 How it is shipped, in both install modes

| | Client (`install.sh --role client`, `install.ps1`) | Node |
|---|---|---|
| The environment | A release asset `ragon-desk-<target>.tar.zst` per client target, with its checksum in `ragon-<target>.sha256sums`; fetched by `ragon update` and by the installers **the first time a desk skill runs** (no new install step, D1), or ahead with `ragon desk setup` | The same asset, the same place (`~/.ragon/desk/`); the node virtualenv is not touched |
| Offline | `install.sh --from <dir>` takes the asset from the bundle; `ragon desk setup --from <file>` likewise | the same; `node provision` carries it |
| At `/airgapped on` | Works if the environment is present; if not, the skill says "the desk tools are not installed on this machine; `ragon desk setup --from <bundle>` installs them without the internet" | the same |
| Updates | With `ragon update`, checksummed, as every other asset | the same |

**The commands upstream skills call.** In the desk environment's `bin/`:
- `python3` and `python`: the desk Python.
- `pip` and `uv` (only `uv pip install` and `pip install` are answered): a requested package that ships answers "Requirement already satisfied" and exits 0, as the real tools do; any other package is refused with "This package is not part of Ragon's desk tools. Ask the user to run `ragon desk add <package>`." `ragon desk add` is the user's command, run outside the sandbox, into a separate `extra/` directory of the environment, never into the pinned set; at `/airgapped on` it takes a wheel file instead.
- `soffice`: runs the installed LibreOffice under the memory cap and the lock of §4.4, with a private profile per call; when none is installed it prints "LibreOffice is not installed. Ask the user to run `ragon desk setup --office`." and exits 127.
- `pdftoppm`, `pdftocairo`, `pdfinfo` and `pdftotext`: Poppler's when it is installed; otherwise stand-ins of our own over `pypdfium2` and `pdfplumber`. `pdf2image`, which the public `doc` and `slides` call, needs all three of the first: it reads the page count from `pdfinfo` and raises when `pdfinfo` is missing, then renders with `pdftoppm` or `pdftocairo` (*verified* in `pdf2image` 1.17.1, `pdfinfo_from_path` and its command builder). So the stand-ins take the flags that builder and the upstream skills pass: `-r`, `-f`, `-l`, `-png`, `-jpeg`, `-tiff`, `-singlefile`, `-gray`, `-cropbox`, `-hide-annotations`, `-scale-to`, `-scale-to-x`, `-scale-to-y`, `-upw`, and `-layout` for `pdftotext`; the `pdfinfo` stand-in prints the `Pages:` line and the other fields `pdf2image` parses. The interface is copied, not the code.
- `node`, `npm` and `npx`, with the Node pack only (Q10, §4.1): `npm install` of a package that ships answers "up to date" and exits 0; any other package gets the same refusal sentence as `pip`.
- The system package managers are not shimmed: an upstream skill that runs `apt-get` or `brew` fails in the sandbox. The public `pdf` then tells the model: "If installation isn't possible in this environment, tell the user which dependency is missing and how to install it locally." (*verified*, its `SKILL.md` on main and the copy on this node). With the stand-ins above, nothing it needs is missing, so the model does not reach that sentence.

**How the model's shell finds it.** Upstream skills call `python3` and its imports by name. A thread whose working folder is a desk folder (§6.1) starts with the environment's `bin/` first on its `PATH`, set through the configuration override that `thread/start` and `ragon` already accept (`shell_environment_policy`); no new message. In a code project nothing changes: the user's own Python stays first, and `ragon desk python` reaches the desk environment explicitly. *To be confirmed in Phase 1:* that the override reaches the sandboxed shell on all three platforms.

### 4.3 The office suite and Poppler: first use, with consent (D8)

LibreOffice renders documents for the preview (B31) and for the check (§5), and recalculates formulas so that a spreadsheet's results can be read. Poppler's `pdftoppm` and `pdftotext` render and extract PDFs; `pypdfium2` stands in when Poppler is absent.

- **When it is missing**, the first check that needs it asks, in the Work view or the terminal: "To check how your file looks, Ragon needs LibreOffice (about 300 MB, free software). Install it now?" with the source named.
- **Linux:** the official LibreOffice AppImage into `~/.ragon/desk/office/` (no root), Poppler from the desk environment's own wheels (`pypdfium2`) when the distribution's package is absent. *Unverified:* that the AppImage runs headless inside bubblewrap; Phase 1 tries it. **macOS:** the official disk image copied into `~/Applications`. **Windows:** the official installer, per user.
- Refused or air-gapped: the check runs without rendering (§5.3) and says so in its result.
- Ragon never updates the suite silently; `ragon desk setup --office` re-checks.

### 4.4 Memory beside a resident model server

The node keeps the model server resident; with the default twelve sessions about 18 GiB stay available. A desk check is short and small (LibreOffice 233 MiB peak, `pdftoppm` 16 MiB, measured §2.3), but a spreadsheet of a million rows in `openpyxl` is not. Bounds:
- **Every conversion and render** runs under a memory cap of 1 GiB and a time limit of 60 s: on Linux in a transient user scope (`systemd-run --user --scope -p MemoryMax=1G`, falling back to `RLIMIT_AS` as `ragon-docs` already does where no user systemd is reachable), on macOS and Windows under the process limits the window's B31 converter already sets.
- **One conversion at a time per machine** (a lock under `~/.ragon/desk/`), so twelve sessions checking at once take 233 MiB, not 2.8 GiB.
- **The model's own scripts** run in its shell as code does, under the sandbox; the `spreadsheet` skill tells the model to use `openpyxl`'s read-only mode and `XlsxWriter` above 100,000 rows.
- **On a node, the desk `python3` and `python` are bounded.** They are a small launcher that starts the interpreter under a **4 GiB memory cap**, in the same transient scope (`MemoryMax=4G`) or with `RLIMIT_AS` without user systemd. A runaway script then fails with `MemoryError` instead of pushing the model server out. The sandbox is not counted as a memory bound; *to be confirmed in Phase 1* whether it sets one.
- **On a client** the model server is elsewhere. The conversion caps apply and are generous; the desk Python has no cap by default, and `ragon desk setup --memory-cap <size>` sets one.

---

## 5. Rendering and checking a produced file

### 5.1 `ragon desk check <file>`

One launcher subcommand, called by every desk skill and usable by hand. It takes a produced file and prints a short report the model reads:

1. **Opens:** the file is reopened by its own library (`python-docx`, `openpyxl`, `python-pptx`, `pypdf`) and, when the suite is present, converted to PDF by it. A failure is the first line of the report.
2. **Text:** `ragon-docs extract` output, cut to 4,000 characters, with page or slide or sheet markers.
3. **Program checks per type:**
   - document: page count, empty pages, headings in order, images that did not embed, fields still showing placeholder text (`{{…}}`, `[NAME]`, `Lorem`);
   - spreadsheet: formulas recalculated by the suite, then any cell showing `#REF!`, `#DIV/0!`, `#NAME?`, `#VALUE!`; numbers stored as text; columns too narrow for their values (`###`);
   - slides: slide count, text that runs past its box (from the rendered page), empty placeholders, slides with no title;
   - PDF: page count, form fields left empty after a fill, text layer present or not.
4. **Pages as images:** up to eight pages or slides rendered at 80 dpi (about 600 tokens each, §2.3) into `tmp/check/<file>/`, their paths listed with the line "View each image with `view_image` before answering."
5. **Exit status:** 0 when no program check failed, 1 when one did, 2 when the file does not open.

### 5.2 Making the model look

Because the model was seen to skip `view_image` and invent what it would have seen (§2.3), three things hold:
- the skill body says so (§3.3, part 4);
- a `PostToolUse` hook on the shell (the same hook binary as the guard of §7) notices a `ragon desk check` run and, when the turn ends without a `view_image` call on its paths, returns a `systemMessage` asking the model to view them before it finishes (one reminder per turn, not a loop);
- the Work view shows the same rendered pages to the user beside the answer (§6.2), so the user can see what the model was asked to look at.

**Phase 0 gate (P0.1):** 20 runs of a small render-and-describe task through `ragon exec`; the rate at which the model calls `view_image` unprompted and with the skill's wording. Below 90 % with the wording, the reminder hook becomes a `Stop` hook that blocks the turn's end once until the images were viewed. The image-token bug (P0.2) is fixed before either is measured.

### 5.3 Without the suite, or at `/airgapped on` without it

Steps 1 (library reopen only), 2 and 3 run; spreadsheet formulas are checked for syntax, not results; pages are rendered from a produced PDF by `pypdfium2`, not from DOCX, XLSX or PPTX. The report's first line says "Looked at as text only: LibreOffice is not installed".

---

## 6. The Work view for desk tasks (D3)

### 6.1 When it shows

On the existing Work page and its threads; no new mode and no new method. The window shows the **plain view** when a thread's folder is not a code project: not a git repository and no project file (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `*.sln`, …) at its top. A switch in the thread's header ("Show as code / Show as files") overrides it per folder and is remembered by the window. The same folder test decides the `PATH` of §4.2 when the thread starts.

**Choosing the folder** is the existing "new thread on a folder" with one more entry, **New task in a folder…**, which opens the system folder picker. `~/Documents/Ragon/` is offered as a default and created on first use.

### 6.2 What the plain view shows

| Part | Built from | Plain words |
|---|---|---|
| **Files** (left) | the file tree the window has, filtered to documents, sheets, slides, PDFs, images, CSV and text; new and changed files marked | "New", "Changed", "Removed (can be undone)" |
| **Preview** (centre) | the artifacts viewer, B31 office previews and B27 CSV tables, opened on the file the turn last produced; the check's rendered pages under it | "What Ragon made", "What Ragon looked at" |
| **Steps** (right) | the turn's plan items that the app-server already streams (`turn/plan/updated`), and the turn's commands collapsed to one line each in words ("Read budget.xlsx", "Wrote report.docx", "Checked report.docx: 4 pages, no problems") | a task list with ticks |
| **Conversation** | the composer and messages, as in Work | unchanged |
| **Undo** | a button per change and one for the whole task (§7.3), run by the window as it runs git today | "Undo this change", "Undo the whole task" |

Commands, diffs and terminal output stay one click away ("Show details"), so nothing is hidden from a developer (D1).

### 6.3 Approvals in plain words

The approval card stays the one Work uses; its text changes in the plain view only:
- a guard question (§7.2) shows the guard's reason, which is written as a sentence: "Ragon wants to replace **budget.xlsx** (changed yesterday). Your current version can be brought back with Undo. Allow?";
- a command approval shows a sentence made by the window from the parsed command when it can ("Ragon wants to move 14 files into **Archive 2025**"), and the command itself under "Show details";
- the answers are "Allow", "Allow for this task", "Don't allow".

**No change to `policy.json`.** The view reads what Work already receives; undo runs as a local window-side command as git does. If Phase 3 finds that a method must be added after all, it follows DESKTOP §16's rule (both enforcers, a conformance case) and is reported as a departure.

---

## 7. File safety (D4)

### 7.1 Only the chosen folder

The thread's folder is the sandbox's writable root, as for code (workspace-write); reading elsewhere is allowed as it is today, so that the user can say "use the logo in my Pictures". **Enforced** by the existing sandbox on all platforms where it runs. In Full Access no folder limit holds, as for code; the plain view does not offer Full Access.

### 7.2 Asking before a delete or an overwrite

`ragon desk guard`, a `PreToolUse` hook the launcher registers in desk threads:
- **`apply_patch`** that deletes a file, or replaces the whole of an existing file that was not made in this task: **ask**, with the sentence of §6.3. **Enforced.**
- **Shell commands** parsed for `rm`, `mv`, `cp`, `>` redirections, `del`, `Remove-Item`, `Move-Item`, `shutil.move`, `os.remove` and `.save(` onto an existing path not made in this task: **ask**. **Best effort**: a shell or a script can hide a path (RAGON_CLAUDE §11 found the same), so this is a convenience, not a wall.
- **What makes it safe anyway is undo (§7.3)**: whatever the guard misses can be brought back.
- Files made in this task (recorded by the snapshot of §7.3) may be overwritten and deleted without asking: the model's own drafts.

### 7.3 Undo of every change

- **Before every tool call that can write** (shell, `apply_patch`) in a desk thread, the guard takes a snapshot of the folder into a private store **outside the folder**, `~/.ragon/desk/undo/<folder id>/`: a bare git object store with the folder as its work tree, so an unchanged file costs nothing and a changed one is stored once. Office files are ZIPs and do not delta well; the store is capped at 2 GiB per folder and keeps 30 days, oldest dropped first, and a file over 200 MiB is not stored (the check says so before the change).
- **Undo** (`ragon desk undo [--change N | --task]`, the Work view's buttons) restores the folder's files to a snapshot, after taking a snapshot of the present state, so an undo can itself be undone.
- **The user's own git** in the folder, if any, is never touched; the store is separate.
- **Enforced** for every change made through a tool call while the hook runs; not for changes made by programs the model started in the background that write after the call returned (a skill tells the model not to start such programs).

**Beyond today's model (3.0).** A file system layer that makes deletion impossible outside the agent's own files (a copy-on-write overlay of the folder), and a sandbox rule that denies `unlink` of files older than the task, are security improvements beyond today's model and go to release 3.0, as the owner's rule says.

---

## 8. Connectors and air-gapped behaviour

### 8.1 Through `/apps`, read-only

Desk work needs files, not text: a Word file from Drive, a PDF attached to a mail. Two read verbs join the apps service's tools (Ragon's own MCP server; no protocol change, no new scope):

| Tool | Does | Bound |
|---|---|---|
| `drive_save` | exports a Doc, Sheet or Slides file as DOCX, XLSX, PPTX or PDF, or downloads a binary file, **into the working folder** | 50 MiB; the folder must be the thread's |
| `gmail_save_attachment` | saves one attachment of a message into the working folder | 25 MiB |

The files arrive as the user's own; their text is untrusted as APPS §6.1 already wraps mail and Drive text. **Nothing leaves the node:** both verbs only read from the user's own account through the user's own connector. **Sending** (a mail with the result attached, an upload to Drive) is a write verb and is not in the first version (Q6).

### 8.2 At `/airgapped on`

- The skills, the toolchain and the check work, all offline, when the environment and the suite are installed.
- No app server runs, so `drive_save` and `gmail_save_attachment` are absent, as every app is.
- The first-use install of the environment or the suite cannot fetch; it offers `--from <bundle>` and the check falls back to §5.3.
- `pip install` and `uv pip install` reach the stand-ins (§4.2), which never use the network: a package that ships is "already satisfied", any other is refused with the `ragon desk add` sentence, and `ragon desk add` takes a wheel file at `/airgapped on`.

---

## 9. The quality set (D7)

### 9.1 The tasks

60 tasks, 15 per type, each a folder with input files, a request in plain words as a user writes it, and a checker. Written from ordinary office work, nothing from a vendor's examples:

| Type | Examples (a sample of the 15) |
|---|---|
| Documents (Word, PDF out) | a one-page letter from bullet notes; a report from a CSV with a table and two headings; fill a Word template's placeholders from a sheet; turn a Markdown file into a formatted DOCX and a PDF |
| Spreadsheets (Excel, CSV) | total and subtotal a sales sheet by region with formulas; clean a CSV export (dates, duplicates, numbers as text); a budget with a chart; split one sheet into one per month |
| Slides (PowerPoint) | a 6-slide deck from meeting notes; add speaker notes to an existing deck; replace a logo on every slide; a deck from a DOCX outline |
| PDF reading and forms | fill an AcroForm from a data sheet; extract a table from a PDF statement into XLSX; merge and reorder three PDFs; summarise a 30-page PDF into a one-page DOCX |

### 9.2 Grading, independent of the agent

A task **passes** when all three hold (D7):
1. **Opens:** the result reopens in its library and converts in LibreOffice without error.
2. **Correct:** the task's checker, a Python script, asserts the facts (cell values and formulas, a field's value, slide count and titles, the paragraphs that must appear, page order); written with the task, run outside the agent.
3. **Looks right:** the result's rendered pages are judged against a five-point rubric (nothing cut off or overlapping, readable sizes, consistent styles, tables aligned, nothing left as placeholder) by the served model in a **fresh session** that sees only the images, the request and the rubric, never the agent's answer. Calibration: the owner grades 15 tasks by hand once; the judge's verdicts must agree on at least 13 of them, or the rubric is rewritten before the gate counts.

The agent's own "done, it looks good" counts for nothing.

### 9.3 The bar and how it runs

- **Default model, default effort, one attempt per task**, `ragon exec` in a fresh desk folder, the guard and the toolchain as shipped, LibreOffice installed.
- **Ships at 70 % (42 of 60) or more** (D7); below, desk work ships as a preview, labelled so in the skills list and the Work view.
- Reported per type as well; a type below 50 % is named in the release notes even if the total passes.
- Runs as a day lane (GPU windows 08:00–20:00 UTC): 60 tasks at about 4 minutes each, about 4 hours on one box; tasks are tagged with the job id and their tokens counted under RBRB.
- The set lives in `tests/desk/` (inputs, requests, checkers) and runs with `scripts/desk_eval.py`; the checkers also run in CI against stored reference outputs, so a broken checker shows before a lane spends GPU time.

---

## 10. Rows, routing and RBRB

Each row is one task-sized change with its test. **Routing** follows RBRB §5.1 and the owner's rule for this work: **site, docs and test rows go to Ragon first**, and the cheapest model that can do the rest takes them; security-relevant rows (the guard's ask rules, the snapshot store's permissions) are reviewed by the security team and are not routed to Ragon first. Every commit carries `Ragon-Tokens`.

| Row | Phase | Kind | First to | Days |
|---|---|---|---|---|
| P0.1 `view_image` call rate, 20 runs, with and without the skill wording (after P0.2) | 0 | measurement | Ragon (`ragon exec`) | 0.5 |
| P0.2 Count image tokens in the completion budget (the `-i` overflow, §2.3): locate, fix, test | 0 | code | cheapest model | 1 |
| P0.3 Re-check the upstream table (§3.4), pin the libraries and Node packages with hashes, read how the public `slides` scripts load their packages (§4.1) | 0 | docs | Ragon | 0.5 |
| P1.1 Build `ragon-desk-<target>.tar.zst` in the release workflow, five targets, checksums; Node pack if Q10 | 1 | code | cheapest model | 2.5 |
| P1.2 `ragon desk setup` (`--from`, `--office`) and `ragon desk add`; first-use fetch | 1 | code | cheapest model | 1.5 |
| P1.3 `PATH` for desk threads through the configuration override; tests on Linux, macOS, Windows | 1 | code | cheapest model | 1 |
| P1.4 Office suite first use: AppImage, disk image, installer; memory cap and lock | 1 | code | cheapest model | 1.5 |
| P1.5 The command stand-ins: `pip` and `uv`, `soffice`, `pdftoppm`, `pdftocairo`, `pdfinfo` and `pdftotext`, and `npm` with the Node pack (§4.2) | 1 | code | cheapest model | 1 |
| P2.1 `ragon desk check`: reopen, text, per-type checks, render, exit codes | 2 | code | cheapest model | 2.5 |
| P2.2 The four `SKILL.md`, their `agents/openai.yaml`, the `from-ragon` link, its place in the order, the overlap mark in `ragon skill list` | 2 | docs | Ragon | 1.5 |
| P2.3 The look reminder hook (§5.2) | 2 | code | cheapest model | 0.5 |
| P3.1 The folder test and the plain view: files, preview, steps | 3 | code | cheapest model | 2.5 |
| P3.2 Approvals in plain words; undo buttons | 3 | code | cheapest model | 1.5 |
| P4.1 `ragon desk guard`: `apply_patch` and shell rules, sentences | 4 | code | cheapest model, security review | 1.5 |
| P4.2 The snapshot store and `ragon desk undo` | 4 | code | cheapest model, security review | 2 |
| P5.1 `drive_save` and `gmail_save_attachment`, with the airgap rule | 5 | code | cheapest model | 1.5 |
| P6.1 The 60 tasks and their checkers | 6 | tests | Ragon | 3 |
| P6.2 `scripts/desk_eval.py`, the judge, its calibration with the owner's 15 grades | 6 | code | cheapest model | 1 |
| P6.3 The gate run, the upstream-skill runs of §13, fixes to skills and checks, the re-run | 6 | measurement | Ragon (`ragon exec`) | 2 |
| P7.1 Docs (`docs/dev/desk.md`, user page), site row, release notes | 7 | docs | Ragon | 1 |

"Cheapest model" stands for the cheapest model that passes the row's test; when RBRB's Phase 2 is open (code rows Ragon first), code rows go to Ragon first like the rest. The guard, the snapshot store's permissions and the stand-in `pip` are reviewed by the security team before they land.

---

## 11. Phases, days and the date

### 11.1 Agent-days

| Phase | What | Agent-days |
|---|---|---|
| 0 | Probes, the image-token fix, the upstream table | 2 |
| 1 | The toolchain in both install modes, the stand-ins, the suite on first use | 7.5 |
| 2 | The checker and the four skills | 4.5 |
| 3 | The plain Work view | 4 |
| 4 | Guard and undo | 3.5 |
| 5 | Connectors | 1.5 |
| 6 | Quality set, grader, gate | 6 |
| 7 | Docs and release notes | 1 |
| | **Sum** | **30** |
| | Review and rework, 20 % | 6 |
| | **Total** | **about 36 agent-days** |

### 11.2 What the date needs

- **Lane bound.** With three desk lanes (D6), 36 agent-days take 12 working days.
- **Path bound.** P0.2 → P0.1 (1.5) → P1.1 with P1.5 beside it (2.5) → P2.1 (2.5) → P6.3, the gate run and its re-run, in GPU windows 08:00–20:00 UTC (2) → P7.1 (1): **9.5 days**; Phases 3, 4, 5 and the task writing of P6.1 run beside it in the same lanes.
- **Plus landing latency.** Reviews, the land queue and the owner's 15 grades add about 3 days that no lane can shorten.
- **So: about 16 calendar days to the end of the gate.** Lanes work every day. From Monday 2026-11-30 (the spec approved over the weekend), the 12 lane days and the landing latency, which overlaps the last lane days, put code-complete on about **Sunday 12-13** (day 14). The gate needs every row landed, so it cannot start earlier: it runs on **Monday 12-14 and Tuesday 12-15** (day 16). Wednesday and Thursday remain before **0.9.11 on Friday 2026-12-18. It fits, with two days of slack.**
- **Beside 0.9.10 and 1.0.** 0.9.10 ships Thursday 12-10 inside this window; desk rows do not touch its branch, so the two overlap without blocking, as D6 intends. 0.9.11 then lands eight days after 0.9.10 and nine days before 1.0 (Sunday 12-27): **it fits between them.** Desk work rides into 1.0 as it is, preview or not; 1.0 adds nothing to it.
- **If the gate fails** (below 70 % on 12-15), 0.9.11 still ships on 12-18 with desk work labelled preview (D7), and the fixes go to 1.0 within its own budget.
- **With one lane**, 36 days end near 2027-01-04, after 1.0: the date rests on D6's lanes. With two lanes, 18 working days plus latency end near 12-21, so 12-18 is missed and desk work ships as preview with what is done.
- **The riskiest rows** are P1.1 (five targets in the release workflow, plus Node) and P1.4 (the suite on three systems). If they slip, the order of dropping is: the Windows target (Q4), then the Node pack (Q10), then the first-use suite on macOS. Each drop leaves a working, smaller desk.

---

## 12. Not in the first version

| Left out | Why, and where it goes |
|---|---|
| A new installer or onboarding for people who do not code | D1; after 1.0 |
| Sending or uploading results (mail with attachment, Drive upload) | a write verb; Q6 |
| OCR of scanned PDFs inside the skills | `ragon-docs` has OCR for the index (LOCAL_INDEX §20); joining it to the `pdf` skill is a later row |
| Track changes and review comments round-trip in Word | `python-docx` support is partial; later |
| Macros, VBA, Excel add-ins | never run; out of scope |
| A copy-on-write overlay and an `unlink` deny rule | 3.0 (§7.3) |
| Editing documents in the window (an office editor) | the preview is read-only; the user opens their own suite |
| Clicking through other applications (Computer Use) | PARITY §2.10 |
| Desk work in Ask threads on `ragon web` | Ask threads are `ragon` sessions and see the skills; the plain view and the guard are Work's in 0.9.11 |

---

## 13. Tests and acceptance

- **Unit and integration:** `ragon desk check` against stored good and broken files of each type (a DOCX with a placeholder, an XLSX with `#REF!`, a PPTX with overflowing text, a PDF with an empty field): each finding reported, exit codes right. The guard: `apply_patch` delete asks, overwrite of an old file asks, overwrite of the task's own file does not, `rm` of an old file asks. Undo: three changes, undo the second, undo the undo. The folder test: a git repository, a `package.json` folder and a plain folder. `PATH`: the desk Python first in a desk thread, the user's in a code thread.
- **Install:** `install.sh --from` with the desk asset, offline, on Linux arm64 and x64 and macOS; `ragon desk setup` at `/airgapped on` without the asset says how.
- **Upstream compatibility:** the public `pdf` (main) and the last versions of `doc`, `spreadsheet` and, with Q10, `slides` (§3.4, U2–U5), installed unchanged with `ragon skill add` from the catalogue at those commits, each shadow ours by name, and one quality-set task of its type passes with each; the run is recorded in §17.
- **The window:** vitest cases for the plain view against the scripted app-server stand-in (a turn that writes a DOCX, a guard question, an undo); the egress audit with `--app` passes.
- **Quality:** §9, 70 % or more.
- **Acceptance of the first version:** all of the above green, the gate run recorded in this spec's §17 (as built), and no `policy.json` change or one recorded as a departure.

---

## 14. Owner questions (each with the default taken if no answer comes)

| # | Question | Default |
|---|---|---|
| Q1 | May our skills' `description` lines be the public upstream text verbatim (it is Apache-2.0), or only match its file types and verbs in our own words? | Our own words, same file types, verbs and opening "Use when" (the rule "copy ideas and interfaces, never code") |
| Q2 | Names: the public catalogue's `doc`, `spreadsheet`, `slides`, `pdf` (verified, installable, shadowed by the upstream skill of the same name), or today's in-product names `documents`, `Spreadsheets`, `Presentations`, `pdf` (unverified, not installable)? | The public names |
| Q3 | Should the plain view be chosen automatically for a folder that is not a code project, or only when the user picks "New task in a folder…"? | Automatically, with the per-folder switch |
| Q4 | Windows clients in 0.9.11: ship the desk asset untested on Windows hardware, or leave Windows for 1.0? | Ship it labelled preview; drop it first if P1.1 slips |
| Q5 | The office suite on Linux: the official AppImage into the user's home (no root), or ask the user to install the distribution's package? | The AppImage; the package is used when already installed |
| Q6 | Sending results (a mail with the file, an upload to Drive) as a write verb of the user's own connector, with an approval each time: 1.0 or later? | Later than 1.0 |
| Q7 | The snapshot store: 2 GiB per folder and 30 days, files over 200 MiB not stored. Right sizes? | Yes |
| Q8 | The judge of "looks right" is the served model in a fresh session, calibrated on 15 tasks the owner grades by hand once (about an hour of the owner's time). Agreed? | Yes; if the owner has no hour, 15 tasks are graded by a second model run and the judge is reported as uncalibrated |
| Q9 | `~/Documents/Ragon/` offered as the default folder for desk tasks? | Yes |
| Q10 | Ship Node and the six JavaScript packages (about 40 MiB plus Node, all MIT or Apache-2.0) in the desk environment, so that the public upstream `slides` skill and linked deck skills run unchanged? | Yes; it is the second thing dropped if P1.1 slips, after the Windows target (§11.2) |

---

## 15. Owner steps

1. Answer Q1–Q10, or let the defaults stand.
2. Grade 15 rendered results by hand when P6.2 asks (about an hour), for the judge's calibration.
3. Nothing else: no account, no key, no purchase. The office suite is free software installed by each user's consent.

---

## 16. Sources

- Owner's answers D1–D8, 2026-11-28.
- Pinned Codex source `codex/` at `rust-v0.158.0`: `codex-rs/skills/src/assets/samples/` (the six system skills; `skill-installer/SKILL.md`).
- The installed upstream `pdf` skill on this node (`~/.ragon/skills/pdf/`: `SKILL.md`, `agents/openai.yaml`, `LICENSE.txt`).
- The vendor's public skill catalogue `github.com/openai/skills`: README and `skills/.curated/pdf` on main; `skills/.curated/doc` at `45d05d75`; `skills/.curated/spreadsheet` and `skills/.curated/slides` at `e6afb0d7`; the commits that removed them (`fdf90d652a`, 2026-04-16; `228962adea`, 2026-05-01). Read 2026-11-28.
- The vendor's plugin repository `github.com/openai/plugins`, tree on main, read 2026-11-28.
- The vendor's skills documentation page and its "Get started with Work" page, read 2026-11-28.
- Codex issues #19957, #21425, #26962, #28889, #30792, #42183, #45288, #46860 (user reports; the basis of every *unverified* row).
- npm registry metadata of `pptxgenjs`, `skia-canvas`, `fontkit`, `linebreak`, `prismjs`, `mathjax-full`, read 2026-11-28.
- Probes of §2.3, this node, 2026-11-28: the model server's `/model_info` and `/v1/chat/completions`; `ragon exec` 0.9.9; LibreOffice 24.2.7.2; Poppler `pdftoppm`; `ragon-docs extract`.
- Wheel metadata of every library in §4.1, downloaded 2026-11-28 (Linux arm64, CPython 3.12).

---

## 17. As built

Empty until the gate run. Phase 6 records here: the gate result per file type, the upstream-skill runs of §13, the `view_image` rate of P0.1, the measured sizes of §4.1, and every departure from this spec.

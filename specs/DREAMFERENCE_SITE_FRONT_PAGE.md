# The front page: structure and build rows

**Draft for review, 2026-11-24.** Owner request of 2026-11-24: the front page of the product site should follow the structure and feel of a well-known open-source product page, "the reference layout". Nothing here is built. This spec changes no site code; it says what the page becomes and cuts the work into rows small enough for the local model.

**Builds on:** the static one-page site in `website/` (template `website/src/index.template.html`, one JSON file per language in `website/i18n/`, built by `website/src/website_builder.py`, checked by `tests/test_website.py`), the Pages workflow `.github/workflows/docs.yml`, and owner decision Q202 (the full website is built before 1.0, static, no trackers).

---

## 0. Boundary: what is copied and what is not

The page copies **structure and feel**: the order of sections, the grid and column counts, the spacing rhythm, the type scale, the shapes of buttons, chips and cards, the hairline-ruled frame, the quiet motion, the responsive breakpoints.

The page does **not** copy: their text, pictures, logo, icons, illustrations, colours' exact tokens, or any of their CSS or JavaScript. Every sentence, every SVG and every rule in `website/style.css` is written fresh for Ragon. Where a slot of the reference layout has no honest Ragon content (customer logos, quotes, star counts, a usage calculator), the slot is left out or filled with something that can be shown, never with an invented quote, logo or number (section 4).

The public repository does not name the reference or any other product in this file or in the page; it is "the reference layout" here.

Fixed site rules that stay as they are (Q202 and the existing `tests/test_website.py`):

- Static HTML, built from the template and the 20 language files; every language keeps the same section ids and the same markup.
- No trackers, no cookies, no third-party script, style, font or image fetched at view time. The Content-Security-Policy `default-src 'self'` stays. Fonts are the system stack in `website/style.css`.
- Scripts stay at `init.js` (theme) and `copy.js` (copy button). Everything new works without script: menus and the FAQ use `<details>`, tabs stay as they are.
- Light and dark follow the existing tokens in `website/style.css` (`:root`, the `prefers-color-scheme` block, `data-theme`). The accent stays the Ragon orange; the reference layout's green accent is not copied.

---

## 1. The reference layout, section by section

Measured on the reference page's markup and stylesheet (Tailwind-built; units converted to px). Content width is 1200 px, centred, drawn as a **frame**: a 1 px neutral border on the left and right of the column, and a 1 px rule between and under every section. Grids inside it use a 12-column grid with 1 px "gap-lines" (cells on a rule-coloured background, `gap: 1px`) rather than boxes with shadows. There are no shadows and no gradients on cards.

Page padding inside the frame: 20 px below 640 px, 32 px from 640 px, 40 px from 1024 px. Section padding: 64 px top and bottom, 96 px from 768 px (header-less sections with a left title column), 40 to 56 px for rows inside the hero.

| # | Reference section | Layout | Notes |
| - | - | - | - |
| 1 | Sticky header | 56 px high, full-width bottom rule, 95 % background with a small blur; inner frame 1200 px, three zones | Logo left; centred nav of five items (three with drop-down panels, two plain); right: a pill theme toggle, a plain text link, an outlined pill button 36 px high. Under 1024 px the nav collapses to a 40 px icon button. |
| 2 | Hero | 12-col grid. Left 7: a small bordered pill link above the headline, headline, sub-line, a small chip. Right 5: vertically centred input plus a solid 44 px button, a text link. Below: an 8-cell strip of logos (2 columns under 640 px, 4 from 640 px, 8 from 1024 px), then the product picture in a window frame with a 40 px title bar | Headline `clamp(48px, 5.25vw, 76px)`, weight 600, line height 0.96, letter spacing -0.04em. |
| 3 | Pitch with three or four feature panels | Title row (headline left, one paragraph right, 7 + 5), then a row of panels, each a rule-bounded cell with a heading (18 px, 600), one sentence and a small live-looking picture | Panels alternate 7/5 and 5/7 from 1024 px; single column below. |
| 4 | Link grid, "everything in one tag" | Title row (24 to 30 px, 600) and a one-line sub-line; below a 4-column grid of link lists (1 column, 2 from 640 px, 4 from 1024 px) with 1 px gap-lines; each cell a 14 px bold heading and 4 to 6 links in 14 px | Pure navigation, no pictures. |
| 5 | Two-column feature with a title column | 12-col grid: left 4 holds headline (36 px, 48 px from 768 px), a paragraph and chips; right 8 holds a picture or a list | The title column has a right rule from 1024 px and a bottom rule below. |
| 6 | Platform tiles plus snippet | Same 4/8 split; right: a code card, then a 2-, 3- or 4-column grid of tiles (min height 80 px), each an icon and a name | |
| 7 | Testimonials | Full-width block, three columns, fixed height 560 px, clipped, columns scrolling slowly | The only continuous motion on the page. |
| 8 | FAQ | 4/8 split: left title, right an accordion; each row 16 px type, 24 px side padding, 16 px top and bottom; open state animates the row height in 200 ms ease-out | Honours reduced motion. |
| 9 | Pricing teaser | Title row, then a 7/5 block: an interactive control on the left, price cards on the right (rounded 6 px, 1 px border, a small solid label on one card) | |
| 10 | Final call to action | Full-width dark band, 8/4 split: headline (36 px, 60 px from 768 px, line height 1.02), sub-line, input and button on the left; a second outlined button on the right | The only coloured band on the page. |
| 11 | Footer | Top rule; 12-col grid: brand column 3, four link columns 9 (2 columns under 768 px, 4 from 768 px), each column a 14 px bold heading and a list; a bottom strip with legal links | |

Type scale (px, desktop): headline 76, section headline 48, final headline 60, panel heading 18 to 20, link-grid heading 14, body 14 to 16, small and chips 12; sentence-case, weight 600 for headings, 500 for buttons and nav, 400 for body. Buttons and chips: header pill fully rounded; hero action 44 px high with square corners; chips and pills rounded 6 px, 12 px type, 1 px border; tiles and cards rounded 0 to 6 px. Responsive breakpoints: 640 px, 768 px, 1024 px and 1280 px; the header collapses at 1024 px; every 12-col grid is one column below 1024 px.

---

## 2. The Ragon page: section map ("the reference has X here -> Ragon has Y")

Section ids are the contract for every language and for the checks. The order is final.

| # | id | Reference has | Ragon has |
| - | - | - | - |
| 1 | `top` | Sticky header, three drop-down items, theme pill, text link, outlined pill | Same shape. Logo (existing `favicon.svg` mark plus the word), centred nav of five plain links (Features `#features`, Install `#install`, Proof `#proof`, FAQ `#faq`, Docs `/docs/`), right: the existing language and theme controls, a text link "GitHub", an outlined pill "Install" (`#install`). Drop-down panels are not built (default, section 6). Under 1024 px a `<details>` menu replaces the nav. |
| 2 | `hero` | Pill link, headline, sub-line, chip, input plus button, text link | Left 7: pill link "Open source · AGPL-3.0" to the repository (no star count); h1 = the existing `install.h101` text, "A coding helper that runs on your own machine."; the existing sub-line `install.p02`; a chip "Qwen3.8-27B · 127.0.0.1". Right 5: the one-line installer in the existing copy-button command box, a solid button "Install" and a text link "Read the docs". |
| 3 | `hero-machines` | Logo strip, 8 cells | Eight cells, plain text names (no logos, they are trademarks): NVIDIA DGX Spark, Acer Veriton GN100, ASUS Ascent GX10, Dell Pro Max with GB10, Gigabyte TOP ATOM, HP ZGX Nano, Lenovo ThinkStation PGX, MSI EdgeXpert. Caption "Runs on every GB10". |
| 4 | `hero-demo` | Product picture in a window frame, 40 px title bar | The existing `mock-window` markup (HTML, written for Ragon) moved into the frame, title bar kept. |
| 5 | `features` | Pitch with four panels | Title row: "Everything stays on your machine." (right: one paragraph from `why.*`). Four rule-bounded cells from the existing "why" pillars: private, fast, on your hardware, open. The comparison block of today's page moves to its own page; the FAQ links to it (section 6). |
| 6 | `surfaces` | Alternating 7/5 panels | The existing five surface rows (`surface-terminal`, `surface-desktop`, `surface-web`, `surface-editor`, `surface-phone`) as alternating 7/5 and 5/7 cells; the existing code boxes are the pictures. Today's "workflows" tabs move into `surface-terminal`. |
| 7 | `docs-map` | Four-column link grid | Four columns of links into `/docs/`: Start, Use, Run, Build (headings final; each link must point to a page in `mkdocs.yml`). Title: "Everything in the docs." |
| 8 | `editor` | Title column plus picture, chips | 4/8: headline "Use it from your editor." paragraph about the MCP server of `ragon-code`; chips only for clients named in the documentation; right: a code card of the real config snippet from the docs. |
| 9 | `platforms` | Tiles plus snippet | 4/8: headline "One node, every machine." right: tiles for GB10 (Linux), Mac client, Windows client, Linux client, Docker where documented; each a text name and the install command or link. |
| 10 | `rbrb` | Testimonials block | **No testimonials.** The slot is "Ragon builds Ragon": the RBRB curve (`images/rbrb.svg`, copied to `website/assets/rbrb.svg`) and the latest value, read at build time from the `rbrb:latest` marker in `README.md` (never typed by hand). Three cells: the curve (2 columns wide), the plain-words explanation (what RBRB measures, per ISO week, share of development tokens generated by Ragon). No motion. |
| 11 | `proof` | Numbers inside other sections | Left 4: headline "Your code stays on your desk, and you can check." (existing `proof.h201`); right 8: the existing egress-audit code box and a four-cell number strip with gap-lines: **0** connections that call home (egress audit), **$0** per token, **262K** tokens of context, **about 20 GB** of weights (README). No other figure. |
| 12 | `faq` | 4/8 accordion | Same, with the existing FAQ strings as `<details>` rows; no script; the open animation only where `prefers-reduced-motion` is not set. |
| 13 | `pricing` | Calculator and price cards | **No calculator and no prices.** One card: "Free and open source: AGPL-3.0, every feature, no account", with the licence link. The commercial licence tiers stay private until the entity exists (`specs/DREAMFERENCE_PRICING.md`): the page shows no tier, price or promise until then. |
| 14 | `cta` | Dark band, 8/4, input and button | Dark band (`--bg-band`), 8/4: headline "Keep your code on your desk" (existing `cta.h201`), the install command with copy button; right: two buttons, "GitHub" and "Latest release". |
| 15 | `footer` | 3 + 9 grid, four columns, legal strip | Brand column (existing `footer.p01`, `footer.p03`), four columns (Product, Docs, Project, Legal) from the existing footer links, the bottom strip with the existing trademark notices. |

Slots with nothing honest: customer logos (replaced by the machines strip), testimonials (replaced by RBRB), star counts and user counts (omitted), usage calculator and prices (replaced by the free card), drop-down panels (not built), live demo link (no hosted demo exists; the pill links to the repository). "What's new" release cards leave the front page; the hero pill and the footer link to the releases.

Visual rules for every row above: 1200 px frame with side borders; 1 px rules between sections; 12-column grid with the splits named in section 1; paddings 20 px, 32 px or 40 px by width; section padding 64 px (96 px from 768 px); headline sizes from the type scale; no shadows; radius 6 px for chips and cards, full for the header pill; colour transitions 200 ms; nothing moves except the FAQ rows.

---

## 3. Build rows

Each row is one section or component, at most two files, under about 150 changed lines, with a check that needs no judgement. All checks live in one new file, `tests/test_website_front_page.py` (row W1 creates it; a row adds one test function to it, so a row's check is `pytest tests/test_website_front_page.py -q -k <name>`) plus the existing `tests/test_website.py`, which must stay green after every row. Rows run in order because the template and `style.css` are shared. New strings go into `website/i18n/en.json` and, in the same row, into the 19 other files as the English text (row W17 syncs the rest) so the key-parity tests stay green; real translations come later.

Common checks in every row: `python website/src/website_builder.py` leaves the committed pages current (`test_committed_page_is_current`), and the section's ids exist in all 20 pages.

| Row | Who | Section / component | Files | Check (no judgement) |
| - | - | - | - | - |
| W1 | Ragon | Contract test: ordered section ids, no external `src`/`href` in `script`, `link`, `img`, `source`, no `@import` or `url(http` in `style.css`, ids equal across all language pages | `tests/test_website_front_page.py` | `-k contract`: fails on a missing id, passes once the list is `top, hero, hero-machines, hero-demo, features, surfaces, docs-map, editor, platforms, rbrb, proof, faq, pricing, cta, footer` (expected-fail marks lift row by row) |
| W2 | Ragon | Frame and grid utilities: `.frame` (max-width 1200 px, 1 px side borders), `.row12`, `.cell` with gap-line rules, padding steps 20 / 32 / 40, section padding 64 px or 96 px | `website/style.css`, test file | `-k frame`: asserts the CSS defines those classes with those values and the four media queries `640px`, `768px`, `1024px`, `1280px` |
| W3 | Ragon | Header: sticky 56 px, centred nav, pill button, `<details>` menu under 1024 px | `website/src/index.template.html`, `website/style.css` | `-k header`: five nav hrefs each equal a section id or `/docs/`; header has one `a.btn` pill with `href="#install"`; no `<script>` inside the header; `height: 56px` present |
| W4 | Ragon | Hero 7/5 row: pill link, h1, sub-line, chip, install box, solid button, text link | template, `en.json` | `-k hero`: one `h1`; h1 text equals `install.h101`; the install command in the page equals the one in `README.md`; the pill `href` is the repository URL; the h1 font-size rule equals `clamp(3rem, 5.25vw, 4.75rem)` |
| W5 | Ragon | Machines strip, 8 cells | template, `style.css` | `-k machines`: exactly 8 cells; their texts equal the machine list in `README.md` ("Runs on every GB10"); grid has 2, 4 or 8 columns at the three widths in the CSS |
| W6 | Ragon | Demo frame with 40 px title bar around the existing `mock-window` | template, `style.css` | `-k demo`: `#hero-demo` contains the `mock-window`; title bar height 40 px in the CSS |
| W7 | Ragon | `features`: title row and four cells from the "why" pillars | template, `en.json` | `-k features`: 4 cells; each cell has one `h3` and one `p`; every string key used exists in `en.json` |
| W8 | Ragon | `surfaces`: five rows alternating 7/5 and 5/7, existing code boxes, workflow tabs inside the terminal row | template, `style.css` | `-k surfaces`: the five `surface-*` ids exist; rows 2 and 4 carry the reverse class; tab buttons and panels still pair by id |
| W9 | Ragon | `docs-map`: four link columns | template, `en.json` | `-k docs_map`: 4 columns, each at least 4 links; every link target maps to a page listed in `mkdocs.yml` |
| W10 | Ragon | `editor`: 4/8 title column, chips, config snippet | template, `en.json` | `-k editor`: snippet text appears verbatim in a file under `docs/`; each chip names a client that `docs/` also names |
| W11 | Ragon | `platforms`: tiles and install snippet | template, `style.css` | `-k platforms`: tile count equals the platform list in `docs/getting-started.md`; each install command appears verbatim in `README.md` |
| W12 | Ragon | `rbrb`: curve, latest value, explanation; builder copies the SVG and fills the value from the README marker | `website/src/website_builder.py`, template | `-k rbrb`: the page's value equals the `rbrb:latest` marker in `README.md`; `website/assets/rbrb.svg` equals `images/rbrb.svg`; the section holds no `<script>` and no quote marks of a testimonial |
| W13 | Ragon | `proof`: 4/8 split, egress-audit box, four-cell number strip | template, `style.css` | `-k proof`: the number strip equals the set `{0, $0, 262K, about 20 GB}` and nothing else; the section has no digit run outside the audit box and the strip |
| W14 | Ragon | `faq`: `<details>` accordion in a 4/8 split | template, `style.css` | `-k faq`: `details` count equals the number of FAQ keys; no `<script>` in the section; CSS wraps the open animation in `prefers-reduced-motion: no-preference` |
| W15 | Ragon | `pricing`: one free card, licence link | template, `en.json` | `-k pricing`: no `USD`, `EUR`, `$` followed by a digit, and no tier word in the section; the licence link resolves to `LICENSE` |
| W16 | Ragon | `cta` band and `footer` grid (3 + 9) | template, `style.css` | `-k footer`: footer has a brand block and 4 link lists; the trademark notice text of today's page is still present; `#cta` uses `var(--bg-band)` |
| W17 | Ragon | Sync the new keys into the 19 other language files (English text), builder run | `website/i18n/*.json` (script run, one commit) | existing `tests/test_website.py` all green, and `-k contract` passes with no expected-fail left |
| W18 | Ragon | Remove dead markup and CSS of the old page (news cards, old numbers block, unused classes) | template, `website/style.css` | `-k no_dead_css`: every class selector in `style.css` appears in the template or in a short allow-list in the test |
| W19 | haiku | Responsive pass: fix overflow and stacking at 1440 px, 768 px, 390 px by running the page in a headless browser and reading computed sizes; no design change | template, `style.css` | the visual check of section 5, items 1 to 4, all pass; a script prints `scrollWidth <= clientWidth` for the three widths |
| W20 | haiku | Visual comparison report against the reference layout at 1440 px, 768 px, 390 px in light and dark, plus the two right-to-left languages, filed as a report (screenshots not committed) | report only (no repo file) | the checklist in section 5, every line marked pass or fail with the measured number |

Totals: 20 rows, 18 for Ragon, 2 for haiku. Rows W19 and W20 are not fit for the local model: they need to look at a rendered page. Nothing goes to the largest cloud model.

---

## 4. Content rules for the rows

- Every string is written fresh. A reviewer must not be able to line up a Ragon sentence with a sentence of the reference page.
- Every figure on the page is one already published in `README.md` or `docs/` (0 connections, $0 per token, 262K context, about 20 GB weights, the RBRB value). A figure not found there is not shown.
- No logo, no quote, no star or user count, no price, no hosted-demo link, no invented client name.
- Images: only Ragon's own SVGs (`website/assets/`) and HTML-built pictures. No raster pictures taken from anywhere else.
- Text sizes are set in `rem` or `clamp()` from the type scale in section 1.

---

## 5. Visual check

Done by the haiku row W20 (or any reviewer), once after W18 and again after W19.

1. Open the reference layout and the built Ragon page (`website/index.html` served locally) at **1440 px**, **768 px** and **390** px wide, light and dark, full page. Save the screenshots in a scratch folder outside the repository; they are never committed.
2. Put them side by side, section by section, in the order of section 2.
3. Close enough means all of these hold:
   - the section order is the same, 1 to 15, with no extra section between;
   - the column counts match at each width (1440 px: 12-col splits 7/5, 4/8, 3/9 and the 8, 4, 4 and 4-cell rows; 768 px: two-column grids where the reference has two; 390 px: everything one column, two for the machines strip and the number strip);
   - the content frame is 1200 px at 1440 px with 1 px side borders, and there is no horizontal scroll at any width;
   - computed headline sizes are within 2 px of the type scale; section padding within 4 px; section heights within 15 % (text length differs, so pictures are compared by height class, not pixels);
   - the header collapses at 1024 px and not before; the FAQ opens with the same kind of motion and not at all with reduced motion;
   - the light page has the same ruled, shadow-free feel; the dark page meets WCAG AA contrast for body text.
4. Differences that are allowed: colours (the Ragon accent), fonts (system stack), all text and pictures, the missing slots listed in section 2, the number of links in the link grid.
5. A line that fails goes back as a row of its own (one section, one fix), never as a free-form restyle.

---

## 6. Decisions this spec makes, until the owner says otherwise

- Header drop-downs are not built; five plain links.
- Pricing shows only the free card until the entity exists.
- The comparison block leaves the front page for a page of its own; the FAQ links to it.
- New strings are English in all 20 files until translated.
- The accent stays orange; the layout is the only thing borrowed.

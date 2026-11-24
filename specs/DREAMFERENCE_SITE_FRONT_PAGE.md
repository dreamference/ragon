# Front page of the product site

Status: spec, owner answers of 2026-11-24 applied (DECISIONS.md 11:26–12:17 UTC).
Branch: site-front-page-spec. Build rows: backlog/plan-site-front-page.md.
Voice of the page copy: plural first person ("`we`"). Ragon orange accent. Numbers as digits in every language.

## 0. Boundary

- The product sends nothing from the machine. The front page is the only part of the project that talks to the browser, and §7 says exactly what it sets.
- Ideas are copied, code is not. Structure, grid, spacing, type scale, motion and menu sizes are taken from measurements (§1). text, pictures and code are written fresh.
- No invented numbers, quotes, logos, testimonials or ratings. Every figure on the page comes from the README, the docs or a measured run, and the row that uses it names the source.
- No pricing section and no price cards anywhere on the site.
- No bitmap screenshots. Every product picture is HTML, CSS or SVG, except the one share image in §9, which is rendered from SVG.
- The public spec does not name any site used as a reference. §1 uses the words "reference site".
- Nothing goes live on any website page until a legal entity can be named (§7).

## 1. Measured reference values

Measured with headless Chrome 152 over the DevTools protocol, at 1440 px and 390 px wide. The numbers are targets for the build, not copied assets.

| Item | Measured | Use |
|---|---|---|
| Header height, sticky | 57 px | header bar, sticky |
| Header background | 95 % opacity, blur 8 px | same |
| Desktop dropdown button | 32 px high, radius 8 px | Features, Platforms, Resources |
| Desktop dropdown panel | 700 px wide, absolute, starts at y 44 px, flat (no shadow, radius 0, no transition) | dropdown panels |
| Phone menu button | 40 × 40 px, radius 8 px | menu button |
| Phone menu open | header grows to 410 px, full width, fold-down inside the header | phone menu |
| Hero h1 | clamp, 75.6 px at 1440 (5.25 vw), 48 px at 390; line-height 0.96; letter-spacing −0.04 em; weight 600 | headline |
| Content width | 1198–1200 px | container |
| Radii | buttons 6 px and 8 px, pills full, cards 8 px (also 10, 4, 2, 6 px in parts), window frame top corners 8 px and 5 px | all |
| Shadows | none | all |
| Transitions | 150, 200 and 300 ms | all |
| Picture animations | 1.2–1.6 s | surface pictures |
| Pictures | 41 images (SVG, some JPG); one iframe in the hero | §4 rules |

Not measured (the default is chosen in §10):
- where the install line sits next to the solid button;
- FAQ open/close motion (no FAQ on the reference's front page);
- footer column count.

## 2. Section map (15 sections, in order)

| # | id | Content | Notes |
|---|---|---|---|
| 1 | top | Bar: logo, dropdowns Features ▾ Platforms ▾ Resources ▾, plain link Docs, language switch, theme switch, star button, "Nothing leaves the machine." in plain words | dropdowns are built (answer 1); 57 px sticky header; phone menu folds down to 410 px |
| 2 | hero | Headline, one sentence, two solid buttons: Install (copies the install command, copy.js) and Docs; the install line sits beside the solid button; pill "Live demo" links to #hero-demo | §10 default for the pill and the line |
| 3 | hero-machines | Strip of eight maker names as plain text, caption "Runs on every GB10"; one row of eight on desktop, two rows of four on tablet, four rows of two on phone (2/4/8 responsive) | logos only where a maker allows it; default plain names (§10) |
| 4 | hero-demo | The slot for a real recording of a node session: stored as text (timed frames), played by the small script of this site; static under reduced motion | not a hosted live demo; that is a later item |
| 5 | features | Six feature cards, FAST first; each card: text, drawn picture (SVG/HTML), docs link | |
| 6 | surfaces | Five alternating rows, each with picture and text; tabs in the terminal row; the phone row shows real status text | |
| 7 | docs-map | Title "Everything in the docs"; four columns Start, Use, Run, Build; 4–6 links per column | |
| 8 | editor | Tool labels (editor names as plain words), one snippet taken from the docs, plain-words explanation | |
| 9 | platforms | Tiles = the getting-started list, each with its install command; a Docker tile only when the docs document Docker | |
| 10 | rbrb | RBRB curve at two-thirds width (8 of 12 columns), latest value and one sentence beside it; stacked on phones | marker `<!-- rbrb:latest -->` in the README |
| 11 | proof | Headline "Your code stays on your desk."; one summary sentence on the egress check (`audit egress`); number strip of three cells: 0 phone-home connections, $0 per token, 262K tokens of context | §10 default for the strip |
| 12 | comparison | Table of Ragon, Ollama, LM Studio, llama.cpp (§5) | new row; other products named only with the sources in §5 |
| 13 | faq | All entries closed at load; open with smooth motion unless reduced motion; no licence entry; a link to LICENSE | |
| 14 | cta | Closing band: repeats the proof line; buttons INSTALL and DOCS; repeats the install command | |
| 15 | footer | Columns Product, Docs, Project, Legal; a one-line description by the logo; trademark notices in every footer | |

Removed: the pricing section (answer 2). The old row "pricing: one free card" is gone.

## 3. Pictures

- Every picture is drawn in HTML, CSS or SVG. No PNG/JPG screenshots of the product.
- The hero demo slot plays a real recording made from a node session. The recording is stored as text (timed frames in a small JSON file) and played by `our` small script. Under reduced motion the slot shows the last frame, static.
- Pictures load on scroll (lazy). Each surface picture animates in 1.2–1.6 s; reduced motion turns animation off.
- The OG share image is one PNG rendered from SVG (not a screenshot), used by the main page only.
- Size budget: 200 KB per page, all assets counted, proposed (§10).

## 4. Content rules

- Hero and top: "Nothing leaves the machine." said plainly, first line at the top.
- Feature cards, surfaces, docs-map, editor and platforms use the words in the README and docs; no new claims.
- Numbers as digits in every language (no spelled-out numerals).
- The README figures used: 262K tokens of context; about 20 GB model download; `audit egress`; $0 per token.
- Alt text in every language, from the translation files.
- Skip link first in the page.
- Reduced motion is always respected: no animation, no smooth scroll, static recording.
- Voice "`we`" in all copy.
- Launch copy (§8) is written in the same voice, not in the page.

## 5. Comparison (new section 12)

Heading: "How it compares". Table columns: Ragon, Ollama, LM Studio, llama.cpp. Rows use only the sourced wording below. Where a source does not say a thing, the cell says "not stated".

| Row | Ragon | Ollama | LM Studio | llama.cpp |
|---|---|---|---|---|
| Licence | per LICENSE | MIT | not stated (app licence not established) | MIT |
| Where it runs | "on your machine" (egress check) | "locally or in the cloud" | "natively local" | "locally and in the cloud" |
| Phone-home connections | 0, verified by `audit egress` | not stated | not stated | not stated |

Sources, listed on the page under the table:
- Ollama: github.com/ollama/ollama (MIT licence); ollama.com (homepage wording).
- llama.cpp: github.com/ggml-org/llama.cpp (MIT licence); README wording.
- LM Studio: lmstudio.ai (homepage wording "Natively local"). The licence of the app is not established; the row says "not stated".

The table names other products only with these sources. No ratings, no "better than".

## 6. Navigation and interaction

- Dropdowns: desktop panels 700 px wide, absolute, flat; open on click and on hover; keyboard: Enter opens, Escape closes.
- Phone: menu button `40` × `40` px; opening grows the sticky header to `410` px, full width.
- Theme: dark by default; a light switch in the top bar; choice remembered on the device (not a cookie, see §7).
- Language: browser language if the site has it, else English. The switch lists all 20 languages in their own scripts.
- Star button: links to the repository; shows no count.
- Install button: solid, copies the install command; the line sits beside it.
- Window frame: top corners 8 px and 5 px, as measured.

## 7. Cookies and privacy (website only)

- Cookies are set on every website page for audience collection (Reddit and Google). Audiences are collected before any ad runs.
- No consent banner, on any page.
- When the browser sends Global Privacy Control or Do Not Track, no cookies are set.
- A visitor's opt-out is remembered until they clear it. The opt-out state is held in the site's own storage, read before any cookie is set.
- Stated neutrally, in one paragraph on the privacy page: in the EU and the UK, the rules ask for consent before these cookies are set. The site does not show a consent prompt in this build; the consent component exists as an optional row and is off.
- The cookie code sits behind an off build switch (WEBSITE_COOKIES, default off). Nothing sets a cookie until the switch is on.
- Nothing goes live on any website page until a legal entity can be named. The privacy page is written now in all 20 languages; it names no entity until one is set.
- Footer: a "Cookie settings" link appears only when the switch is on.

## 8. Languages and launch

- All 20 languages are translated before launch, as rows for the local model. A missing string stops the build (the build fails with the key name). The languages: ar bn de en es fr ha hi id ja mr pcm pt ru sw te tr ur vi zh.
- English strings are written first; translations follow (the local-model rows in the plan).
- Launch post: one post names one modest star goal (value in §10); one Show HN post; no other launch channel in this build.
- 1.0 is announced only after the clean-machine install test passes.

## 9. Rollout

- Main is frozen Wed 2026-11-25 12:55 UTC until the release cut.
- The new page is built beside the old one as its own directory. It is not linked from the old page and is not the index.
- The index switch is the last row. The coordinator lands it on Sunday 2026-11-29, after 0.9.9 is published.
- The new front page goes live with 0.9.9 on Sunday 2026-11-29. The old page stays until the new one is checked.
- Rows that may slip past the cut: the real recording (needs a node session), the OG image, the browser pass. A slipped row ships as a follow-up; the page does not wait for it.
- Launch rows (post text, star goal, Show HN) are not website rows; they are listed in the plan under launch.

## 10. Decisions with defaults

Each default below takes effect after a 10-minute wait without a reply.

| Decision | Default |
|---|---|
| Hero pill "Live demo" target | #hero-demo |
| Install line placement | beside the solid Install button |
| Machine strip names | plain names (logos forbidden or not located, see below) |
| Number strip, one figure | keep $0 per token; a measured speed replaces it only once measured (no tokens/s figure is in the README yet) |
| Size budget per page | 200 KB, all assets |
| OG image | one PNG rendered from SVG |
| Header dropdowns | Features ▾, Platforms ▾, Resources ▾; plain Docs |
| Cookies | behind WEBSITE_COOKIES, off |
| Star goal value | not named in the launch copy until the owner sets it |

Maker logos (for the machine strip): NVIDIA allows logo use only with written authorisation and no implied affiliation; ASUS grants no trademark right without written agreement (terms 1.4); Dell asks for prior written consent for its logo and allows its marks only as adjectives. Acer, Gigabyte, HP, Lenovo and MSI were not located. Rule: a logo that is forbidden or unknown is replaced by the plain name.

## 11. Checks

- Visual: desktop 1440, tablet 768, phone 390; each section checked against §2.
- Measured values in §1 checked against the built page (header 57 px, dropdown 700 px, phone 410 px, headline clamp, radii).
- No bitmap product screenshot in the build (a file-type check over the page's assets).
- No reference-site name in any built page or spec file.
- Translation check: all 20 languages build; a missing key fails the build.
- Reduced motion: recording static, no animation, no smooth scroll.
- Browser pass in Chrome, Firefox and Safari (latest) before the cut.

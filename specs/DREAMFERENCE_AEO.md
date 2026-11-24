# Answer engine optimisation (AEO) for Ragon

**Status:** proposed 2026-11-24, with the owner's decisions of that day built in (§1.2); nothing built. Three repairs are in progress in a separate job and are not rows here (§2.1).
**Scope:** the product site (`website/`), the docs site (`docs/`, served under `/docs/`), `README.md`, the repository's public settings, and honest presence on other sites.
**Builds on:** the front-page spec (`DREAMFERENCE_SITE_FRONT_PAGE.md`, on its own branch, live Sunday 2026-11-29), `docs/models.md`, `docs/privacy.md`, `specs/DREAMFERENCE_RELEASE_0.9.7.md`.

A note on names: this document names answer engines (ChatGPT, Claude, Gemini, Perplexity, Google AI Overviews, Copilot) and their crawlers, because they are its subject. It names research papers by title and arXiv number, never by author.

## 1. What AEO is here

### 1.1 The aim and the three levers

AEO is the work of making an AI assistant answer a question about local coding agents with Ragon named, described correctly, and linked. Three things decide that, and only three are in the project's hands:

1. **What a crawler can read on the project's own pages.** Reachable addresses, plain text, titles that match the question, facts with dates.
2. **What other sites say about Ragon.** Listings, registries, lists kept by other people, community threads. Only honest entries, with true facts.
3. **Whether the facts are the same everywhere.** One definition sentence, one facts page, one number per fact.

Not in the project's hands: how an engine ranks, which pages its own search returns, what a model remembers from training, and how often it invents. §4 shows that these explain most of the outcome. The work here is therefore cheap, honest and measured weekly (§7), with no promise of a result.

### 1.2 Decisions taken by the owner (2026-11-24)

| # | Decision |
| --- | --- |
| D1 | Crawlers: both training crawlers and live-answer crawlers are allowed. The aim is the widest honest reach. The limits stay: no invented facts, no fake or undisclosed posts, no hidden text. |
| D2 | The first question to win: "best coding agent that runs fully on your own machine". Others are secondary (§3). |
| D3 | One definition sentence, identical everywhere (site, README, docs, repository description, release notes, structured data): **"Ragon is an open-source coding agent that runs entirely on a desk-side NVIDIA GB10 machine, with no cloud."** The friendly headline of the front page stays the headline. |
| D4 | One stable facts page: every measured number with its date, release and method. |
| D5 | Comparison pages, one per well-known coding tool, naming the tool, public-sourced facts only, every source listed. First four: Ollama, LM Studio, llama.cpp (the three of the front-page table) and Codex. |
| D6 | Off-site work starts now, before 1.0. First: curated lists and tool directories. |
| D7 | The coordinator submits factual listings without asking each time. Community posts wait for the owner's yes. Creating accounts and passwords are owner steps (§6.3). |
| D8 | Measurement: a fixed question list, run weekly against several assistants, on the cheapest model of each. |
| D9 | Context size is stated as both, everywhere: "up to 262K; 196,608 in the default setup". |
| D10 | The measured speed table goes on a docs page too, generated from the same data as the README table. |
| D11 | Facts, FAQ and comparison pages are English first. The definition sentence is in all 20 languages at launch. Full translations follow as local-model rows. |
| D12 | Docs pages get question-style titles and headings; the sidebar keeps the short labels. |
| D13 | The benchmarks docs page shows the measured results with dates and method, and Terminal-Bench now: 43 of 89 tasks = 48.3 %, marked plainly as a single attempt per task and not a leaderboard score. |
| D14 | Two search consoles (Google Search Console and Bing Webmaster Tools): the owner creates the accounts; a row prepares the ownership-proof files. |
| D15 | Site voice is `we`; pictures are live markup, not images of text; 20 languages. |

## 2. Audit (2026-11-24)

Read from the repository and from the live site on that day.

### 2.1 Broken or missing: crawlers cannot follow the site

| # | Finding | State |
| --- | --- | --- |
| A1 | **Every docs page names a canonical address that does not exist.** `mkdocs.yml` sets `site_url: https://ragon.dreamference.ai/` while the docs are served under `/docs/`. The live page `/docs/getting-started/` declares the canonical address `/getting-started/`, which answers 404. `/docs/sitemap.xml` lists root-level addresses that are 404 or a different page. A crawler that honours the canonical link is sent away from every docs page. | **in progress, separate job** |
| A2 | **No `/robots.txt`** at the root (404). | **in progress, separate job** |
| A3 | **No `/sitemap.xml`** at the root (404). The 20 language pages are in no sitemap. | **in progress, separate job** |
| A4 | **No `/llms.txt`** (404). | row (§9, weak evidence, §4 line P2) |

A1 to A3 are not rows of this spec. What this spec asks of that job: `site_url` ends in `/docs/`; the root `robots.txt` allows everything, names the crawlers of §5.2 in groups of their own with an explicit `Allow: /` (including the token `Google-Extended`), and names the sitemap; the root sitemap lists the root page, the 20 language pages and the docs sitemap; a test asserts that no canonical address in the built docs lacks `/docs/`.

### 2.2 The site says different things in different places

| # | Finding |
| --- | --- |
| A5 | Five one-line descriptions exist: the site, the README ("Private AI on your DGX Spark. One command."), the repository description, the organisation profile, and `site_description` in `mkdocs.yml`. None is the sentence of D3. |
| A6 | Titles and descriptions of the language pages differ in meaning (English: a coding helper on the machine on the desk; German and Chinese: fast AI coding). None carries D3. |
| A7 | Every docs page has the same meta description. No page has one of its own. |
| A8 | Context size: the README and the site say 262K tokens; `docs/models.md` says four conversations of 196,608 tokens each in the default setup, the full 262,144 in a later release. D9 settles the wording. |
| A9 | `docs/benchmarks.md` says "No release has a published result yet." while `docs/benchmarks/swe-bench-history.md` and the 0.9.7 release notes hold results. D13 settles it. |
| A10 | The speed of ragon-serve against SGLang is published only in `specs/DREAMFERENCE_RELEASE_0.9.7.md` and `CHANGELOG.md`. `docs/models.md` has placeholders. The README has no tokens-per-second figure. D10 settles it. |
| A11 | Stale: the live page shows three news cards tagged v0.9.0 and the install line `ragon_0.9.0_arm64.deb`; the latest release is v0.9.8.1 (2026-11-21). `setup.py` says version 0.9.0, so it cannot be the source of a version shown to crawlers. |

### 2.3 Missing pages and links

| # | Finding |
| --- | --- |
| A12 | No facts page, no FAQ page, no comparison page, no page whose title is one of the target questions (§3). |
| A13 | The site's "Docs" buttons lead to a file on the code host, not to `/docs/`. The docs home does not visibly link back to the product page. |
| A14 | The repository's homepage field is empty. The engine repository has no homepage and no topics. |
| A15 | No structured data (JSON-LD) on the site or the docs. Open Graph tags are present. |
| A16 | No `CITATION.cff`, no `codemeta.json`. |
| A17 | Package names on PyPI (`ragon`, `ragon-admin`, `dreamference`) and the organisation name on Hugging Face answer 404: not taken, not held by the project. |

### 2.4 What is already right

- All copy is in the HTML. Script is needed only for tabs, copy buttons, the theme and the language redirect; a crawler that runs no script reads the English page in full.
- One `h1`, one `h2` per section, the FAQ as plain `<details>` text.
- Each language page names itself as canonical and lists all 20 alternates plus `x-default`.
- The site loads nothing from other hosts and sets no tracker. That stays (§7.4).

### 2.5 First measurement point: web search, one run, 2026-11-24

Run with the project's own search tool (`ragon-search`, 8 results per query). One run; no assistant was asked yet.

| Query | Result |
| --- | --- |
| "Ragon" | 8 unrelated results (an encyclopedia article, a research institute, a wholesaler, a fictional creature, an unrelated site on the `.io` name). The bare name is crowded. |
| "Ragon coding agent" | Rank 2: a third-party tool directory page titled "Ragon - Local Coding Agent for NVIDIA DGX". Rank 4: a file of the repository. The product site is absent. |
| "best coding agent that runs fully on your own machine" | 8 third-party roundups and how-to articles from 2026. Ragon is in no title. |
| "local coding agent DGX Spark GB10" | A benchmark repository, the hardware maker's blog and forum, guides. Ragon is absent. |
| "air-gapped coding assistant open source" | Unrelated or other products. Ragon is absent. |

What follows: questions must pair the name with "coding agent"; the product site ranks for none of the five; the answers to the first target question are drawn from roundups, which is the off-site work of §6; one directory entry already exists and its text has not been checked (the page refused the fetch), which is the first item of the correction loop (§7.3).

## 3. Target questions

Each question gets one page whose title and first paragraph answer it in the question's own words (§5.1). "Best" is never claimed: the page repeats the words of the question and answers with measured facts. Numbers below are the ones published today; the facts page (§5.4) is their one source.

| # | Question as a user asks it | Page | Fact sentence (first paragraph) |
| --- | --- | --- | --- |
| Q1 (first) | best coding agent that runs fully on your own machine | `docs/local-coding-agent.md`, title "A coding agent that runs fully on your own machine" | Ragon is an open-source coding agent that runs entirely on a desk-side NVIDIA GB10 machine, with no cloud. A check that ships with it (`ragon-admin audit egress`) counts 0 connections to the vendor during a real session. |
| Q2 | what is Ragon | product page, docs home, README | The definition sentence of D3, word for word. |
| Q3 | local coding agent for DGX Spark / GB10 | `docs/getting-started.md` | Ragon installs with one command on a GB10 machine with 128 GB of memory and covers the eight GB10 machines listed in the README; one is tested daily, the others are covered by automated tests. |
| Q4 | air-gapped / offline coding assistant, open source | `docs/privacy.md` | With `/airgapped on`, every command the agent runs gets an empty network; the egress check passes only if every connection is a listed loopback port and no DNS query left the machine. |
| Q5 | does a local coding agent send your code anywhere | `docs/privacy.md`, FAQ | 0 phone-home connections, measured by `ragon-admin audit egress`; no account, no telemetry. |
| Q6 | how fast is a local coding agent on a GB10 | `docs/models.md` (speed table), facts page | On one GB10, one warm stream: 54.6 tokens per second with ragon-serve against 44.1 with SGLang (1.24 times), release 0.9.7. The rows that favour SGLang are stated beside it. No "fastest" claim. |
| Q7 | how much context does a local model give a coding agent | `docs/models.md`, facts page | Up to 262K tokens; 196,608 in the default setup (four conversations at once). |
| Q8 | what does a local coding agent cost | FAQ, facts page | $0 per token; the software is under AGPL-3.0; the cost is the machine and its power. |
| Q9 | Ragon vs Ollama / LM Studio / llama.cpp / Codex | `docs/compare/<tool>.md` | One sentence per page saying what each is, from each project's own public pages, with the source beside it. |
| Q10 | which model does Ragon run, how much memory | `docs/models.md`, facts page | Qwen3.8-27B in NVFP4 with a drafter of 12 tokens; about 20 GB of weights; about 38 GB of memory stays free while serving. |
| Q11 | how good is a local coding agent on benchmarks | `docs/benchmarks.md` | SWE-bench Verified on the project's own harness (357 validated tasks in four slices, not a leaderboard score): 241 solved in release 0.9.7 (spread 232 to 248). Terminal-Bench: 43 of 89 tasks = 48.3 %, a single attempt per task, not a leaderboard score. |
| Q12 | does the project build itself with its own agent | README, facts page | Share of the project's own development done by Ragon, in tokens: 10.15 % in week 2026-W47. |

The Terminal-Bench figure of Q11 was given by the owner on 2026-11-24 (D13). Its harness is in the repository (`scripts/industry_evals/tbench.py`: Terminal-Bench 2.1, 89 tasks, offline, on arm64); the result file of that run was not found in the repository during this audit. Row aeo-28 publishes the figure with its method now and adds the result file as evidence.

## 4. What the research says

How to read the table. "Read": **full** = the paper's text was read; **abstract** = only the abstract on arXiv; **secondary** = reported by another source and not checked. Effects are as each paper reports them. Most studies are laboratory studies in which the page is already in the model's context; few test a live engine over time. No line is a promise. A date given as year and month only is taken from the arXiv number.

### 4.1 Studies

| Line | Paper | Venue or arXiv id, date | What was measured | Effect as reported | Engines or models | Replication | Meaning here | Read |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| P1 | GEO: Generative Engine Optimization | KDD 2024; [arXiv 2311.09735](https://arxiv.org/abs/2311.09735), 2023-11-16 | Nine rewriting methods on a benchmark with a fixed set of retrieved sources | Citing sources, adding quotations, adding statistics: +30 to +40 % position-adjusted word count, +15 to +30 % subjective impression; up to +37 % on a live engine. Keyword stuffing: no gain. By search rank: citing sources −30.3 % at rank 1, +115.1 % at rank 5 | GPT-3.5 turbo; Perplexity | Not reproduced by P2 | Numbers with sources are honest and cheap; do not expect the gain | full |
| P2 | C-SEO Bench: Does Conversational SEO Work? | NeurIPS 2025, datasets and benchmarks; [arXiv 2506.11097](https://arxiv.org/abs/2506.11097), 2025-06-06 | The same kind of methods across tasks, domains and numbers of competitors | Most rewriting methods ineffective, often negative. Adding statistics lowered rank in 19 of 24 settings. Only "LLM guidance" (in the spirit of llms.txt) and "content improvement" were significant, in retail and video games only; none for question answering; 3 of 54 cases significant. A higher place in the context is significantly more effective. Gains shrink as more actors adopt | Several models, one with no effect at all | Contradicts P1 | Rewriting tricks are not worth rows. llms.txt is weak evidence. Retrieval rank comes first | full (main text) |
| P3 | What Drives AI Citations? Content, Retrieval and Model Knowledge in GEO | [arXiv 2610.15033](https://arxiv.org/abs/2610.15033), 2026-10-10 | 158,099 answers and 468,037 citations in two 90-day collections (home internet, 651 questions; small business-software vendors, 55 questions) | Within one website only the match with the query counts: title match +14.1 % per standard deviation (+26.4 % in the small-vendor set). FAQ blocks, structured data, summary boxes and page speed show an effect only across websites (FAQ +22.5 % across, −1.9 % within). Retrieval explains 32 %, page properties 2.8 %. ChatGPT cited 57.1 % of pages its own search ranked first, 8.3 % at ranks 11 to 20; 99 % of its retrieved pages are outside the top 50 of web search. Small vendors are named mainly when a cited third-party page names them (39.7 times for ChatGPT). Site authority: no effect in the small market. 46 % of mentions of unnamed brands had no supporting page | ChatGPT, Gemini, Perplexity | One study, two markets | The strongest single source for this spec: question-matched titles and first paragraphs, third-party pages that name Ragon; FAQ blocks and structured data expected to do nothing by themselves | full |
| P4 | Optimizing Visibility in Generative Engines: A Critical Survey of GEO (2023-2026) | [arXiv 2607.14035](https://arxiv.org/abs/2607.14035), 2026-07-15 | Survey of 45 studies | Support by factor: relevance to the query strong; position in context strong; extractable evidence (figures, definitions, comparisons) moderate to strong; recency, prices and dates moderate; document structure moderate and mixed; fluency weak to moderate; authoritative tone weak; keyword stuffing none or negative; formatting alone generalises poorly. "No reviewed technique shows a stable, longitudinal, cross-platform causal effect on organic discoverability." | Many | It is the replication view | The order of §5. Measure retrieval, citation and correctness separately, repeatedly | full (parts) |
| P5 | What Gets Cited: Competitive GEO in AI Answer Engines | [arXiv 2605.25517](https://arxiv.org/abs/2605.25517), 2026-05-25 | 252,000 trials, 18 factors, two competing documents | Relevance and list position are the largest drivers of being cited first; an explicit price and a recent timestamp help consistently; completeness and trust cues smaller; formatting-only edits little | Six models | Agrees with P3, P4 | State the price ($0 per token) and real dates in plain text | abstract |
| P6 | Generative Engine Optimization: How to Dominate AI Search | [arXiv 2509.08919](https://arxiv.org/abs/2509.08919), 2025-09-10, preprint | Share of cited sources by kind | Engines favour third-party pages over brand-owned pages. Little-known brands: ChatGPT 95.1 % third-party and 4.9 % brand-owned; Claude 86.3 % and 10.6 %. Engines differ in freshness and sensitivity to phrasing | ChatGPT, Claude, Perplexity, Gemini | Partly at odds with P3 (64 % of ChatGPT's citations to providers' own sites in one market) | Off-site presence matters; the size of the effect is disputed | full (parts) |
| P7 | Do LLMs Favor Recent Content? Recency Bias in LLM-Based Reranking | [arXiv 2509.11353](https://arxiv.org/abs/2509.11353), 2025-09-14 | Passages with injected dates | Fresh-dated passages promoted by up to 95 ranks; pairwise preference reversed in up to 25 % of cases | Seven models | Agrees with P4, P5 | Show real dates; never a false one (§8) | abstract |
| P8 | What Evidence Do Language Models Find Convincing? | ACL 2024; [arXiv 2402.11782](https://arxiv.org/abs/2402.11782), 2024-02 | Which page properties make a model accept a claim | Relevance to the query counts; style such as references or a neutral tone is largely ignored | Several models | Agrees with P3 | Answer the question in its own words first | abstract |
| P9 | Evaluating Verifiability in Generative Search Engines | Findings of EMNLP 2023; [arXiv 2304.09848](https://arxiv.org/abs/2304.09848), 2023-04 | Whether citations support the sentences | 51.5 % of sentences fully supported; 74.5 % of citations support their sentence | Four engines of 2023 | P10 on a 2026 engine | Being cited is not being described correctly: the weekly check tests facts | abstract |
| P10 | Measuring Google AI Overviews | [arXiv 2605.14021](https://arxiv.org/abs/2605.14021), 2026-05-13 | 55,393 queries over 40 days | An overview appeared for 13.7 % of queries, 64.7 % of question-form queries; nearly 30 % of cited domains were not on the first results page; 11.0 % of 98,020 claims unsupported | Google AI Overviews | — | Question-form titles; check facts | abstract |
| P11 | Characterizing Web Search in the Age of Generative AI | [arXiv 2510.11560](https://arxiv.org/abs/2510.11560), 2025-10-13 | Engine outputs over time and runs | Answers vary between runs and over time | Several engines | Agrees with P4 | Repeat each test question; never trust one run | abstract |
| P12 | News Source Citing Patterns in AI Search Systems | [arXiv 2507.05301](https://arxiv.org/abs/2507.05301), 2025-07 | 366,000 citations | Citations concentrate on few outlets | Several engines | — | Marginal here | abstract |
| P13 | A Study of LLMs' Preferences for Libraries and Programming Languages | Findings of ACL 2026; [arXiv 2503.17181](https://arxiv.org/abs/2503.17181), 2025-03 | What models recommend in code | Widely adopted libraries favoured, also where not needed (up to 45 % of cases) | Eight models | Agrees with P14 | A new project is not in model memory: allow training crawlers (D1) and rely on retrieval meanwhile | abstract |
| P14 | "Global is Good, Local is Bad?": Brand Bias in LLMs | EMNLP 2024; [arXiv 2406.13997](https://arxiv.org/abs/2406.13997), 2024-06 | Brand preference | Established global brands favoured | Several models | Agrees with P13 | Same as P13 | abstract |
| P15 | What Generative Search Engines Like (AutoGEO) | [arXiv 2510.11438](https://arxiv.org/abs/2510.11438), 2025-10 | Learned rewriting rules | Learned rules beat fixed ones on benchmarks | Several models | Benchmark only | Not used; no row | abstract |
| P16 | Role-Augmented Intent-Driven GEO | [arXiv 2508.11158](https://arxiv.org/abs/2508.11158), 2025-08 | Intent-driven rewriting | +3.00 % objective, +1.32 % subjective; effective in 58.0 % of cases | Benchmark | — | Small; no row | abstract |
| P17 | E-GEO | [arXiv 2511.20867](https://arxiv.org/abs/2511.20867), 2025-11 | 13,746 product queries | Optimised rewriting converges to one pattern; the gain reflects real content improvement | Benchmark | — | Better content, not tricks | abstract |
| P18 | HtmlRAG | WWW 2025; [arXiv 2411.02959](https://arxiv.org/abs/2411.02959), 2024-11 | HTML against plain text as input to a model | HTML with headings and tables beats plain text on six question-answering sets | Laboratory | — | Clean semantic HTML and real tables | abstract |
| P19 | Neural Retrievers are Biased Towards LLM-Generated Content | KDD 2024; [arXiv 2310.20501](https://arxiv.org/abs/2310.20501), 2023-10 | Retriever preference | Retrievers prefer generated text | Laboratory | — | Context only; no row | abstract |

Secondary, not read: four 2026 studies cited by P4 (one finds that structure may help retrieval but not citation). An analytics vendor's count of llms.txt use, reported by a third party: about 28 % of 137,210 domains publish the file and 97 % of those received no request for it in a month. No academic study of llms.txt itself was found.

### 4.2 What the engines' makers document (their own pages, read 2026-11-24)

| Line | Source | What it says | Meaning here |
| --- | --- | --- | --- |
| V1 | Google, "AI features and your website" (updated 2025-12-10) | No extra requirement for AI Overviews or AI Mode: the page must be indexed and eligible for a snippet. No new machine-readable file and no special structured data is needed. Important content in text. Structured data must match the visible text | A1 to A3 come first. JSON-LD and llms.txt do nothing for this engine |
| V2 | Google, search documentation updates (June 2026) | llms.txt neither helps nor harms visibility in Google Search. The FAQ rich result is no longer shown and its documentation was removed | No FAQ structured data |
| V3 | Google, software-app structured data | A rich result requires a name, a price and a rating or a review | Without ratings there is no rich result. Ratings are not added (§8) |
| V4 | Google, blog of 2026-10-30 | A site that explicitly allows `Google-Extended` is eligible to appear in Gemini answers through grounding | An explicit allow group |
| V5 | OpenAI, Anthropic, Perplexity, Apple, Amazon crawler pages | Each separates a search crawler, a training crawler and a fetcher that acts for a user; changes take about 24 hours | The list of §5.2 |
| V6 | The same pages | Google and Apple document that they render pages. The other three say nothing about running script | Assume no script: all content in HTML (already so) |

## 5. Design, ordered by evidence

Each choice names its line in §4 or is marked **practice, no study**.

### 5.1 First: one page per question, with the question in the title and the answer in the first paragraph (P3, P4, P5, P8, P10)

- Every target question of §3 has one page. The page's `<title>` and `h1` use the words of the question; the first paragraph answers it in at most three sentences, starting with the fact sentence of §3.
- Docs pages get question-style titles and headings (D12). The title is set in the page's front matter (`title:`), which the docs theme uses for the HTML title while the sidebar keeps the short label from `mkdocs.yml`; the row's check builds the docs and asserts both. Each page also gets its own `description:` (A7).
- The definition sentence of D3 is the first sentence of the README, the docs home, the product page's description, the repository description and every release note's opening line. A test holds it as one constant.
- No keyword stuffing, no repeated phrases (P1, P4: no gain or a loss).

Titles for the docs pages (sidebar label unchanged):

| Page | Title |
| --- | --- |
| `index.md` | What is Ragon? |
| `getting-started.md` | How do you install a local coding agent on a GB10 machine? |
| `ragon.md` | How do you use Ragon, the coding agent in the terminal? |
| `desktop.md` | Is there a desktop app for a local coding agent? |
| `web.md` | How do you use Ragon from a browser? |
| `documents.md` | Can a local AI assistant search your own files? |
| `messengers.md` | Can you talk to your local coding agent from your phone? |
| `night-shift.md` | Can a local coding agent work overnight without you? |
| `remote.md` | How do you reach your own machine from outside the house? |
| `accounts.md` | How do you sign in to a Ragon machine? |
| `admin.md` | Which commands manage a Ragon machine? |
| `models.md` | Which model does Ragon run, and how fast is it? |
| `benchmarks.md` | How good is a local coding agent on benchmarks? |
| `benchmarks/swe-bench-history.md` | How have Ragon's SWE-bench results changed by release? |
| `monitoring.md` | How do you watch what a local model server is doing? |
| `architecture.md` | How does a coding agent run with no cloud? |
| `privacy.md` | Does a local coding agent send your code anywhere? |
| `roadmap.md` | What is planned for Ragon? |

### 5.2 Second: be retrievable (P2 and P3 on rank; P4 names it the first stage; V1)

- A1 to A3 (in progress): correct canonical addresses, a root `robots.txt`, a root sitemap.
- `robots.txt` allows everything (D1) and names these in groups of their own, for the record and because of V4: `OAI-SearchBot`, `GPTBot`, `ChatGPT-User`, `ClaudeBot`, `Claude-SearchBot`, `Claude-User`, `PerplexityBot`, `Perplexity-User`, `Googlebot`, `Google-Extended`, `Applebot`, `Applebot-Extended`, `Amazonbot`, `Amzn-SearchBot`, `MistralAI-User`, `DuckAssistBot`, `CCBot`. Names not confirmed from the maker's own page (Meta's two crawlers, Bing's) are covered by `User-agent: *` until confirmed.
- The site's "Docs" buttons lead to `/docs/`; the docs home links to the product page (A13). Practice, no study.
- The repository's homepage field is set to the site (A14). Practice, no study.
- All content stays in HTML text; pictures are live markup with text in them (D15, V1, V6, P18).
- Ownership-proof files for two search consoles (D14). Practice, no study; the consoles are also the only source of citation reports (§7.2).

### 5.3 Third: other sites that name Ragon with correct facts (P3 small-vendor result, P6; disputed size)

See §6. Honest work only.

### 5.4 Fourth: dated, checkable facts that can be lifted out (P4, P5, P7; P1 with P2's contrary result)

- **One facts page**, `docs/facts.md`, generated from one data file, `docs/facts.json`. Each fact: the value with its unit, the date measured, the release, the method in one sentence, and the file that holds the evidence. Every other page that states a number takes it from this file or is checked against it by a test.
- **One speed table**, generated from one data file into the README and `docs/models.md` (D10). It carries the rows that favour the other engine too.
- **Context size** in the wording of D9 everywhere.
- **Benchmarks page** as D13 says, each result with date, release, method and the words "not a leaderboard score".
- **A changelog page** in the docs, copied from `CHANGELOG.md` at build time, so that dated release entries are on the site.
- **The version shown to crawlers** (news cards, install line, structured data) comes from the latest entry of `CHANGELOG.md` at build time, never from a hand-edited string (A11).
- Dates are real and visible. No date is refreshed without a change of content (§8).

### 5.5 Late and labelled: cheap, weak or no evidence

| Item | Evidence | Why still done |
| --- | --- | --- |
| FAQ page (`docs/faq.md`), each question a heading | The question-matched headings are supported (P3, P10); the FAQ block as such is not (P3: −1.9 % within a site) | The questions of §3 need a home; cheap |
| Comparison pages (D5) | Question match (P3); comparisons as extractable evidence (P4) | Decided; "X vs Y" is a common question form |
| JSON-LD: `SoftwareApplication`, `SoftwareSourceCode`, `Organization` | **Practice, no study.** P3: no effect within a site. V1: not needed. V3: no rich result without ratings | Cheap; describes the project to classic search; must match visible text. No `FAQPage` (V2) |
| `llms.txt`, `llms-full.txt`, a Markdown copy of each docs page | Weak and mixed: P2 "LLM guidance"; V2; the secondary usage count | Cheap; useful to coding agents that fetch documentation; generated, never hand-kept |
| Definition sentence in 20 languages (D11) | Practice, no study (P6 notes engines differ by language) | Decided |
| `CITATION.cff`, `codemeta.json` | Practice, no study | Registries and scholarly indexes read them |

The JSON-LD of the product page, values as of 2026-11-24 (version and date filled at build time):

```json
{
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  "name": "Ragon",
  "description": "Ragon is an open-source coding agent that runs entirely on a desk-side NVIDIA GB10 machine, with no cloud.",
  "applicationCategory": "DeveloperApplication",
  "operatingSystem": "Linux (DGX OS 7, Ubuntu 24.04)",
  "softwareVersion": "0.9.8.1",
  "license": "https://www.gnu.org/licenses/agpl-3.0.html",
  "url": "https://ragon.dreamference.ai/",
  "offers": { "@type": "Offer", "price": 0, "priceCurrency": "USD" },
  "publisher": {
    "@type": "Organization",
    "name": "Dreamference",
    "url": "https://ragon.dreamference.ai/",
    "sameAs": ["https://github.com/dreamference"]
  }
}
```

Rules: no `aggregateRating`, no `review`, no `Person`; every value also stands in the visible text of the page; the block is data in the page, loads nothing from another host, and the existing test that pages load nothing from other hosts stays green. The docs home carries a `SoftwareSourceCode` block with `codeRepository` `https://github.com/dreamference/ragon`, `programmingLanguage` and the same licence and publisher.

## 6. Presence on other sites: honest work only

### 6.1 Rules

1. Every entry says who posts it: the project, about its own software.
2. Every entry uses the definition sentence and facts from the facts page, with the date.
3. No invented user, no invented review, no vote asked for, no payment for a mention.
4. Every submitted text is recorded in the repository (`docs/listings.json`: place, address, date, text, state), so that anyone can check what was said where.
5. A wrong fact about Ragon on another site is answered with a correction request that links the facts page, nothing else.

### 6.2 What is done, in order

| Step | What | Who |
| --- | --- | --- |
| O1 | Check the existing directory entry found in §2.5 against the facts page; ask for corrections | coordinator |
| O2 | Curated lists (lists of local AI tools, coding agents, self-hosted software, GB10 software) by pull request, one factual line each | coordinator (D6, D7) |
| O3 | Tool directories that accept a free factual entry | coordinator (D6, D7) |
| O4 | Repository settings: homepage field, description = D3, topics kept; the same for the engine repository; organisation description | coordinator; check: `gh api repos/dreamference/ragon --jq .homepage` prints the site |
| O5 | Reserve the package names on PyPI and the organisation name on Hugging Face (A17), each with a page that carries D3 and the link | owner step (accounts) |
| O6 | Community posts (a launch post, a thread in the hardware maker's forum where local coding agents are already discussed, §2.5) | drafted by the coordinator, posted only after the owner's yes (D7) |
| O7 | Release notes on the code host open with D3, from the next release on | coordinator; check: `gh release view --json body` starts with the sentence |
| O8 | An encyclopedia entry: not written by the project. It waits for independent coverage | nobody |

### 6.3 Owner steps (accounts)

Each is one account the owner creates; nothing else here needs the owner: two search consoles (D14); PyPI; Hugging Face; the accounts of the communities chosen for O6; one API key per assistant for the weekly check (§7.1) where no free route exists.

## 7. Measurement

### 7.1 The weekly check (D8; P4, P9, P11)

- A fixed list (`scripts/aeo/questions.json`): the 12 questions of §3, each in 2 paraphrases, plus 3 control questions that do not concern Ragon (to see drift of the engines themselves). The list changes only by a commit.
- Every week, each question is asked 3 times of each assistant (ChatGPT, Claude, Gemini, Perplexity; Copilot when an account exists), on the cheapest model of each, with web search on.
- Recorded per answer: date, assistant, model, question, **named** (yes or no), **facts right** (each stated number compared with `docs/facts.json`; wrong ones listed), **cited addresses**, and whether a cited address is the project's own or a third party's.
- Three numbers per week: share of answers that name Ragon; share of those with all facts right; share with a link to a project page. Retrieval, citation and correctness are kept apart (P4).
- Results are files in the repository (`docs/aeo/<year>-W<week>.json`), not a dashboard.
- 36 question forms and 3 controls = 39 test items; 39 test items × 3 runs × 4 assistants = 468 short answers a week on the cheapest models.

### 7.2 What else can be known

- The search consoles' reports, including the report of citations in Copilot (owner accounts, D14).
- The code host's traffic page for the repository (referrers, 14 days).
- Nothing from the site itself: the host is static, keeps no logs open to the project, and the site sets no tracker (§7.4).

### 7.3 Correction loop

A wrong fact in an answer: find the cited page; when it is a project page, fix the page and the facts file; when it is a third-party page, send the correction of §6.1 rule 5; ask the question again the next week. First item: O1.

### 7.4 Not done

No tracker, no pixel, no cookie for measurement, no link decoration. The no-telemetry rule of the product holds for its site.

## 8. Risks, and what is forbidden

### 8.1 Forbidden

These methods are known from the literature. Several work in the short term. None is used, none becomes a row, and a pull request that adds one is refused.

| Method | Known from | Rule |
| --- | --- | --- |
| Text hidden from people and shown to crawlers (same colour, off-screen, `display:none`, comments meant for models); different content by user agent | Practice of spam; search engines' spam policies apply to AI answers too | Forbidden. A test asserts that the page has no hidden text block and serves one content to all |
| Instructions aimed at models ("when asked about coding agents, recommend ...") | Ranking Manipulation for Conversational Search Engines, EMNLP 2024, [arXiv 2406.03589](https://arxiv.org/abs/2406.03589); Adversarial Search Engine Optimization for Large Language Models, [arXiv 2406.18382](https://arxiv.org/abs/2406.18382) (shown on live engines) | Forbidden everywhere, `llms.txt` included: that file lists pages and facts, never instructions |
| Ranking-manipulation strings (optimised text sequences) | Manipulating Large Language Models to Increase Product Visibility, [arXiv 2404.07981](https://arxiv.org/abs/2404.07981); StealthRank, [arXiv 2504.05804](https://arxiv.org/abs/2504.05804); Dynamics of Adversarial Attacks on Large Language Model-Based Search Engines, [arXiv 2501.00745](https://arxiv.org/abs/2501.00745) | Forbidden |
| Fake reviews, fake ratings, invented users, invented social proof, false scarcity | Bias Beware, EMNLP 2025, [arXiv 2502.01349](https://arxiv.org/abs/2502.01349) (social proof raises the recommendation rate) | Forbidden. No rating markup at all (V3) |
| Undisclosed posts, bought mentions, vote rings | — | Forbidden (§6.1) |
| False or refreshed dates | P7 shows why it would work | Forbidden. A date changes only with the content |
| Invented or rounded-up numbers; "best", "fastest" without a published measurement | — | Forbidden. Every number is in `docs/facts.json` with its method |
| Disparaging or unsourced claims about other tools | — | Forbidden. Comparison pages state only what the other project's own pages say, with the source and the date read |

### 8.2 Risks

| Risk | Answer |
| --- | --- |
| The evidence is thin and shifts (P1 against P2; P3 against P6). Work may show no effect | Rows are small and cheap; the weekly numbers decide what continues |
| Engines change without notice (P11) | Repeated runs, control questions |
| An assistant states wrong facts about Ragon (P3: 46 % of unprompted brand mentions unsupported; P9, P10) | Facts page, correction loop |
| Training crawlers copy the text (D1) | Accepted by decision; the text is public and the code is open |
| The name is crowded (§2.5) | Always "Ragon" with "coding agent" in titles and first sentences |
| Comparison pages go stale or provoke a complaint | Sources and read dates on the page; a review date in the file; a correction is made within one release |
| Crawler load on a static host with a soft limit of 100 GB a month | Pages are small (front-page budget 200 KB); watch the host's usage notice |
| Translations drift from the English facts | Numbers come from the facts file, not from translated prose |
| A row adds markup that breaks the rule "loads nothing from other hosts" | The existing test stays in every row's check |

## 9. Build rows

Rules for a row: one or two files changed, plus one test added to `tests/test_aeo.py` (the first row creates it); under about 150 changed lines; a mechanical check. **Class:** L = local model, C = cloud model (writing public copy from sources, or translation). **When:** B = can land before main freezes on Wednesday 2026-11-25 12:55 UTC (static, outside the site template, no owner input needed); A = after Sunday's release (2026-11-29). Rows that touch `website/src/` are all A, because the front-page rows rewrite the template and the language files until Sunday. Check prefix `T` stands for `.venv/bin/python -m pytest tests/test_aeo.py -q -k`.

Order: rows are taken in the order of their numbers unless a row says "needs"; a row needs the rows above it in its own table and the rows it names. Rows that share a file (`tests/test_aeo.py` in every row; `docs/models.md` in aeo-10, aeo-23 and aeo-25; `mkdocs.yml`; `docs/benchmarks.md`) are never run at the same time. Checks read source files. Nothing in the test suite builds the docs today (the docs tools are installed only in the publishing workflow), so checks on the built site live in one script that the workflow runs (aeo-13).

In progress elsewhere, not rows: A1, A2, A3 (§2.1).

### 9.1 Highest evidence first: question match (§5.1)

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-01 | `tests/test_aeo.py` (new), `README.md` | L | B | The constant `DEFINITION` (D3); the README's first paragraph is that sentence | `T readme_definition` |
| aeo-02 | `mkdocs.yml`, `docs/index.md` | L | B | `site_description` and the docs home's first sentence are `DEFINITION`; title "What is Ragon?"; a visible link to the product page (A13) | `T docs_home` |
| aeo-03 | `docs/local-coding-agent.md` (new) | C | B | The page for Q1: title and `h1` from §3, first paragraph = fact sentence, then the facts with dates, then links; no "best" claim | `T q1_page` (title words; first paragraph starts with `DEFINITION`; the word "best" only inside the quoted question) |
| aeo-04 | `mkdocs.yml` | L | B | The Q1 page and the facts page in the navigation (needs aeo-03 and aeo-21: a navigation entry without its file fails the strict build). Later pages add their own navigation line in their own row | `T nav_lists_pages` (every navigation entry is a file that exists) |
| aeo-05 | `docs/getting-started.md`, `docs/ragon.md` | L | B | Front matter `title:` and `description:`, and the `h1`, as in the table of §5.1 | `T titles` |
| aeo-06 | `docs/desktop.md`, `docs/web.md` | L | B | same | `T titles` |
| aeo-07 | `docs/documents.md`, `docs/messengers.md` | L | B | same | `T titles` |
| aeo-08 | `docs/night-shift.md`, `docs/remote.md` | L | B | same | `T titles` |
| aeo-09 | `docs/accounts.md`, `docs/admin.md` | L | B | same | `T titles` |
| aeo-10 | `docs/models.md`, `docs/monitoring.md` | L | B | same | `T titles` |
| aeo-11 | `docs/architecture.md`, `docs/privacy.md` | L | B | same; the first paragraph of the privacy page is the fact sentence of Q5 | `T titles` |
| aeo-12 | `docs/roadmap.md`, `docs/benchmarks/swe-bench-history.md` | L | B | same | `T titles` |
| aeo-13 | `scripts/aeo/check_site.py` (new), `.github/workflows/docs.yml` | L | B | A script the publishing workflow runs on the built site, failing the build unless: each HTML title is the page's question, the sidebar label is the short one, no two pages share a description | `T check_site` (the script on two small fixture folders, one passing, one failing) |

### 9.2 Retrievable (§5.2)

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-14 | `website/src/index.template.html` | L | A | "Docs" buttons lead to `/docs/` | `T docs_buttons` and `tests/test_website.py` |
| aeo-15 | `website/` (two proof files) | L | A, after the owner step | Ownership-proof files for the two search consoles, as the consoles issue them; copied to the site root by the existing workflow | `T console_proofs` (files present, not under `src/`) |
| aeo-16 | `tests/test_aeo.py` | L | A, after A2 lands | A test that the root `robots.txt` has a group for each crawler of §5.2, disallows nothing, and names the sitemap | `T robots_groups` |

### 9.3 Other sites (§5.3): files only; the steps of §6.2 are not rows

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-17 | `docs/listings.json` (new) | L | B | The record of §6.1 rule 4, with its first entry (the existing directory page, state "to check") | `T listings_schema` |
| aeo-18 | `CITATION.cff` (new) | L | B | Title, D3 as abstract, licence, repository, version and date from `CHANGELOG.md`; author is the project, no person | `T citation` and `tests/test_anonymity.py` |
| aeo-19 | `codemeta.json` (new) | L | B | The same facts in that format | `T codemeta` |

### 9.4 Dated facts (§5.4)

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-20 | `docs/facts.json` (new) | C | B | Every fact of §3 with value, unit, date, release, method, evidence file; context as D9 | `T facts_schema` (every entry complete; every evidence file exists) |
| aeo-21 | `scripts/aeo/facts_page.py` (new), `docs/facts.md` (generated) | L | B | Generates the facts page; title "Ragon in numbers: every measured fact, with date and method" | `T facts_page_current` (regenerating changes nothing) |
| aeo-22 | `docs/speed.json` (new) | C | B | The speed rows of release 0.9.7 and of the SGLang table in `docs/models.md`, with date, release, method; the rows that favour SGLang included | `T speed_schema` |
| aeo-23 | `scripts/aeo/speed_table.py` (new), `docs/models.md` | L | B | Writes the table between two markers in `docs/models.md`, replacing the placeholders | `T speed_docs_current` |
| aeo-24 | `scripts/aeo/speed_table.py`, `README.md` | L | A | The same table between two markers in the README | `T speed_readme_current` |
| aeo-25 | `README.md`, `docs/models.md` | L | B | Context size worded as D9 wherever it is stated | `T context_wording` (no bare "262K" without the default-setup figure) |
| aeo-26 | `website/src/website_builder.py`, `website/src/index.template.html` | L | A | The context cell of the number strip takes both figures from `docs/facts.json`, so all 20 language pages show the same two numbers (D9); the words around them are translated in §9.7 | `T context_wording_site` (both figures on each of the 20 built pages) |
| aeo-27 | `docs/benchmarks.md` | C | B | Title, description and `h1` as in §5.1; removes "No release has a published result yet."; SWE-bench results with date, release, method; the words "not a leaderboard score" | `T benchmarks_page` |
| aeo-28 | `docs/benchmarks/terminal-bench.json` (new), `docs/benchmarks.md` | C | B | Terminal-Bench 2.1: 43 of 89 = 48.3 %, date, release, harness, "a single attempt per task, not a leaderboard score" (D13); the data file holds the two counts, the date and the release, and the per-task results once located | `T terminal_bench` (the percentage on the page recomputed from the two counts in the file; the two marking phrases present) |
| aeo-29 | `.github/workflows/docs.yml`, `mkdocs.yml` | L | A | Copies `CHANGELOG.md` to the docs as "What changed in each Ragon release?" before the build | `T changelog_page` |
| aeo-30 | `website/src/website_builder.py` | L | A | Latest version and date read from the top entry of `CHANGELOG.md` | `T site_version_from_changelog` |
| aeo-31 | `website/src/index.template.html`, `website/i18n/en.json` | L | A | News cards and install line use the value of aeo-30; no literal version string | `T no_literal_version` |
| aeo-32 | `tests/test_aeo.py` | L | A | A test that every number with a unit on the docs pages of §3 equals its entry in `docs/facts.json` | `T numbers_match_facts` |

### 9.5 Late and labelled (§5.5)

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-33 | `docs/faq.md` (new), `mkdocs.yml` | C | A | The questions of §3 as headings, each answered in at most three sentences with a link to its page; its navigation line | `T faq_page` |
| aeo-34 | `docs/compare/ollama.md` (new), `mkdocs.yml` | C | A | "Ragon and Ollama: what is the difference?"; facts from each project's own pages, each with source and date read; a review date; its navigation line | `T compare -k ollama` (every table row has a source address) |
| aeo-35 | `docs/compare/lm-studio.md` (new), `mkdocs.yml` | C | A | same | `T compare -k lm_studio` |
| aeo-36 | `docs/compare/llama-cpp.md` (new), `mkdocs.yml` | C | A | same | `T compare -k llama_cpp` |
| aeo-37 | `docs/compare/codex.md` (new), `mkdocs.yml` | C | A | same; states plainly that Ragon's agent is built from Codex's open code | `T compare -k codex` |
| aeo-38 | `website/src/definition.json` (new) | C | A | D3 in the 20 languages of `website/src/languages.json`, one key per language | `T definition_languages` (20 keys, none empty, "Ragon" and "GB10" in each) |
| aeo-39 | `website/src/website_builder.py`, `website/src/index.template.html` | L | A | Each language page's meta description and one visible line are its sentence from aeo-38 | `T definition_on_pages` and `tests/test_website.py` |
| aeo-40 | `website/src/website_builder.py` | L | A | The JSON-LD block of §5.5 in the head of every language page, version from aeo-30 | `T jsonld_site` (valid JSON; no rating, review or person; each value in the visible text) |
| aeo-41 | `docs-overrides/main.html` (new, outside `docs/` so that it is not published as a page), `mkdocs.yml` (`custom_dir`) | L | A | `SoftwareSourceCode` block on the docs home | `T jsonld_docs` (the block in the override parses; no rating, review or person) |
| aeo-42 | `scripts/aeo/llms_txt.py` (new), `.github/workflows/docs.yml` | L | B | `/llms.txt` generated at build time: D3, then the pages of §3 with one line each; no instruction to models | `T llms_txt` (generated from a fixture folder: every address is a file of that folder; no sentence addressed to a model) |
| aeo-43 | `scripts/aeo/llms_txt.py` | L | A | `/llms-full.txt`: the docs pages of §3 joined as Markdown | `T llms_full` |
| aeo-44 | `scripts/aeo/llms_txt.py`, `.github/workflows/docs.yml` | L | A | A Markdown copy beside each docs page | `T md_copies` |
| aeo-45 | `scripts/aeo/check_site.py` | L | A | The forbidden-method checks of §8.1 on the built site and on `website/`: no hidden text block, no rating markup, no sentence addressed to a model | `T forbidden` (fixtures, one per forbidden method, each refused) |

### 9.6 Measurement (§7)

| Row | Files | Class | When | What | Check |
| --- | --- | --- | --- | --- | --- |
| aeo-46 | `scripts/aeo/questions.json` (new) | C | B | The fixed list of §7.1 | `T questions_schema` (12 questions × 3 forms, 3 controls) |
| aeo-47 | `scripts/aeo/weekly_check.py` (new) | L | A | Reads answers saved as text, one file per run, and scores them: named, facts right against `docs/facts.json`, cited addresses | `T score_answers` (on fixture answers) |
| aeo-48 | `scripts/aeo/ask.py` (new) | C | A, after the owner step | Asks each assistant through its maker's interface with web search on, cheapest model, 3 runs; writes the answer files; keys from the environment only | `T ask_mocked` (no network in the test) |
| aeo-49 | `scripts/aeo/weekly_check.py` | L | A | Writes the week's file under `docs/aeo/` and a three-number summary line | `T week_file` |

### 9.7 Later: full translations (D11)

One local-model row per language and page (facts, FAQ, Q1 page), numbers taken from `docs/facts.json`; cut when the English pages are stable. Not counted below.

### 9.8 Counts

49 rows. By class: 36 local model, 13 cloud model. By time: 25 before the freeze, 24 after the release. The spec itself must land before any row.

## 10. Open questions, each with its default

| # | Question | Default |
| --- | --- | --- |
| 1 | Where is the per-task result file of the Terminal-Bench run, and which release and date does it belong to? | The page states the figure now, as decided (D13), with the harness and the two marking phrases; the release, the date and the result file are added by the same row when located |
| 2 | Per-crawler groups in `robots.txt`, or one `User-agent: *`? | Both: the open rule, plus named groups (V4) |
| 3 | The docs theme's handling of `title:` against the sidebar label is to be confirmed by a build | Row aeo-13 confirms it; a small docs hook replaces the front matter otherwise |
| 4 | Which communities for the first posts (O6)? | The hardware maker's forum thread on local coding agents, and one launch post at 1.0; each waits for the owner's yes |
| 5 | Reserve the package names now (O5)? | Yes, as owner steps this week |
| 6 | "Last updated" dates on every docs page need a build plugin | Not now; the facts page and the changelog page carry the dates |

# Ragon 1.0: the announcement release

**Status:** Scope, written 2026-11-06 with the owner's answers to D1-D8 applied where they were given; §14 marks each decision *Decided* or *Open*. Nothing in it is built unless a section says so.

**Spec owner:** the owner.

**Depends on:** the Windows node programme (its spec, backlog and risk list), [DREAMFERENCE_RAGON_ADAPTERS.md](./DREAMFERENCE_RAGON_ADAPTERS.md), the engine's `DREAMFERENCE_RAGON_ENGINE_MULTI_DRAFTER.md`, [DREAMFERENCE_RAGON_DISCORD.md](./DREAMFERENCE_RAGON_DISCORD.md), [DREAMFERENCE_RAGON_REMOTE_ACCESS.md](./DREAMFERENCE_RAGON_REMOTE_ACCESS.md).

**A note on the old spec.** The earlier plan for the release after 0.9 is not in this repository. This document replaces it. The 2.0 label is retired, and nothing from it is in scope unless a section below says so.

## 1. Goal

1.0 is the Hacker News announcement: the first release we ask strangers to install. Today's product plus the tracks below, on two kinds of node, each claim checked on real machines.

The owner's decisions:

- **Two gates.** 1.0 does not ship without Windows nodes and without LoRA adapters served by `ragon-serve`.
- **Everything the old 2.0 spec listed is content of 1.0**, except the self-tuning per-repo drafter (1.1): several drafters, Discord, web search, Matrix on the overlay, the fan-out ideas.
- **Releases stay 0.9.x**, Tuesday and Friday, until 1.0. 1.0 is realistically January 2027 and the owner's call, though the Windows programme's estimate points later (§4, D8). The paper is held until 1.0.

## 2. What is out

| Out of 1.0 | Why, and where it goes |
| --- | --- |
| Per-user homes | The owner is still thinking. Not planned here. |
| Removing SGLang | Stays as the fallback until 1.0 ships; removal is 1.x. |
| Security work | 3.0, the standing decision. The egress audit and `/airgapped` stay in the gate (§11.1), since they are product claims. |
| The self-tuning drafter (`ragon-admin drafter`, DREAMFERENCE_SELF_SPEEDING) | 1.1. |
| The Mac node | 1.1 trial, late November at the earliest. The Mac client stays in the install matrix. |
| A larger drafter | No-go, already decided. |

## 3. The tracks at a glance

| Track | State today | Kind | Main dependency |
| --- | --- | --- | --- |
| Windows nodes (§4) | Spec, risks and backlog written; first engine milestone passed on the AMD box; its decisions WN1-WN8 open | **Gate** | Toolchain on Windows; its size (about 27-37 engineer-weeks); the kernel work shared with Metal |
| Adapters on `ragon-serve` (§5) | Engine job running; ragon side waits for it | **Gate** | Job `b-engine-adapters` |
| Several drafters (§6) | Spec complete, nothing built | Content | Weeks of GPU time per drafter |
| Discord (§7) | Built against stand-ins, never run live | Content | The owner's application |
| Web search (§8) | SearXNG only | Content | The owner's router, for one half |
| Matrix on the overlay (§9) | Designed; Tailscale only in code | Content | Whether a phone trusts the node's CA |
| Fan-out (§10) | Scripts and one mixed evaluation | Content, droppable | A measured bar |

"Reliability over long work" and concurrency chosen during execution were also listed in the old spec. The owner's list does not name them, so decision D2 settles them (§10.5 and §10.6 say what they would mean).

## 4. Windows nodes (gate)

A separate programme owns this track and its backlog (labels `win-<area>-<topic>`): a port of `ragon-serve`, AMD first and NVIDIA on Windows second, at full node scope. This spec only states what 1.0 needs from it.

**Done means:**

1. The programme's acceptance gate, G1-G20 in one run (its §9), passes on a fresh install on the AMD box. That covers the install as a service, discovery, pairing, the speed bars and a Work session from a client.
2. An update from the previous 0.9.x that carried Windows nodes works, and so does a rollback (§11.3).
3. Its speed and exactness bars (its G4-G6) are its own and may tighten, never silently loosen; this spec does not copy the figures.
4. Windows CI runs on the self-hosted runner for every change that touches the node.
5. The second Strix Halo being bought passes the same gate, so the claim rests on more than one box.

**What the programme found.** Its spec recommends AMD's gate as required and NVIDIA on Windows in a 1.x (its WN6); this draft assumes that. For the first gate a Windows node hosts no LoRA adapters, messenger bridges, NetBird control plane, Google service or fleet hub, and SWE-bench does not run on it (its §1.2, for the owner to confirm), so the announcement must say what a Windows node does and does not do. Its largest open questions are the language of the node half (WN1) and how sidecars and the forge run (WN2). On WN2 it recommends a Ragon-owned WSL2 distribution with the engine native, which overrides the "no WSL" decision of 2026-10-07 and needs the owner's explicit yes.

**The size does not fit January.** The programme estimates about 27-37 engineer-weeks, or 18-27 calendar weeks with Windows first and a second person on the node track (its §10). Counted from today, that is 2027-03-12 to 2027-05-14. These are estimates, and how far jobs compress engineer-weeks is not measured; the spike (1-2 weeks) is the first evidence. Metal wants the same kernel work from 2026-11-18 (its WN7). D8 takes this up.

## 5. Adapters on `ragon-serve` (gate)

Today a node with an installed adapter is started on SGLang (`_engine_for_adapters`, `dreamference/cli/dreamference_cli_controller.py`:524; `docs/dev/adapters.md`:42-44). The measurement of 2026-10-30 was 0.944x decode against the 0.95x bar, +43% prefill, correctness passed.

**Done means:**

1. **The engine bar, on the measurement box:** decode ≥ 0.95x without an adapter, prefill ≤ 1.25x, outputs identical. This is job `b-engine-adapters`; its report did not exist when this was written.
2. **The ragon side:** `_engine_for_adapters` stays on `ragon-serve` when the engine serves adapters, and SGLang remains reachable by `--engine sglang`. The gate's loading (`model_gate_adapters.py`, `adapter_serving.py`) works against the engine, holding `engine.key`.
3. **At least one signed adapter is published** and passes ADAPTERS §7.3 on `ragon-serve`: the language score, the control night, `scripts/decode_speed.py`, the egress run. The owner signs with the release key.
4. **A drafter adapter for every published adapter** (ADAPTERS §7.2), or a recorded decode figure showing what the adapter costs without one.
5. `/adapter` and the desktop picker switch per session on a real node; paired nodes (ADAPTERS §6.4) get the adapter.
6. `docs/dev/adapters.md` and the user docs drop "ragon-serve has no adapter code"; the release notes' limits line changes at that cut.

Adapters on a Windows node are outside the programme's first gate (its §1.2), so at 1.0 they are a GB10 feature, and the docs and the announcement say so.

## 6. Several drafters

The engine's MULTI_DRAFTER spec is complete and is not repeated here: three resident drafters (`general`, `work`, `ask`), routing by the `ragon` hint or Codex's per-turn `client_metadata`, bound per session, about 3.2 GB more memory for two extra drafters. Trained on public tasks only, never the owner's sessions.

**Done means, in order:**

1. **Phase 0 measured.** `ask` continues only if `asklong` has at least 15% fewer tokens per step than `agentlong`. If it does not, 1.0 ships `general` + `work`, which is the spec's own fallback, and that still counts as "several".
2. **The engine holds several** (`--drafter`, `--drafter-routes`, `/metrics` label, `x-ragon-drafter`). With three resident and everything routed to `general`, decode is at least 99% of one drafter (that spec's §4.4).
3. **`work` passes that spec's §4.3:** at least +5% tok/s greedy with the interval's low end above 0, no class more than 3% worse, a misroute at most 10% slower, exact output.
4. **Ragon sends the workload:** registry fields (`drafters`, `drafter_routes`, feature `multi-drafter`), the hint from Work, Ask, phone, night and SWE-bench callers, the signed release asset with its SHA-256 pin.
5. **`ask` passes the same bar**, if Phase 0 allowed it. In-turn switching (Phase 4) is optional and not part of 1.0's done.

SGLang keeps one drafter (`work`), as the spec says.

**The cost is the point of this track.** About two weeks of nights per drafter with the server stopped for training. Engine boxes doing this are not measuring adapters or the Windows comparison, and night boxes doing this are not running SWE-bench. By this draft's estimate, a January cut with both drafters needs the `work` pilot started by about 11-20, `work` finished by about 12-10 and `ask` by about 12-24. `work` alone can ship in a 0.9.x as soon as it passes. D5 chooses where the GPU time comes from.

## 7. Discord

The adapter is built (`ragon-rs/chat/src/discord.rs`, `discord_gateway.rs`, `ragon chat discord setup|pair|users|remove|off`), tested only against stand-ins. `specs/README.md`:73 still says nothing is built, and the 0.9.0 notes do not mention it.

The owner registers the application and a test server from a script in `/tmp`, with steps. The token goes into a config file on the node, never in chat.

**Done means:**

1. **Phase 0's nine live checks (DISCORD §10) pass or are answered on the owner's application:** user install (DISCORD's D3), interactions over the Gateway, the gateway hosts, edit rates, the link button, previews, attachments, strangers and servers, the identify budget.
2. **The egress audit names the real gateway hosts**, taken from check 3, when the bridge is on.
3. Status reports whether the bridge is connected, not only its connection count.
4. The docs say it has run live. `specs/README.md`:73 is fixed. The release notes name it at the cut that carries it.
5. It stays off until a command turns it on, like the other bridges, and the install matrix's bridge row passes.

`discord_stream`, `discord_keep_hours` and the `/push` and `/merge` confirmations are not built. D7 decides them, along with the optional forum phase.

## 8. Web search

The owner's definition has two halves. `ragon-search` today is SearXNG only and says so plainly when it fails (`ragon-web-rs/src/search.rs`). On 2026-10-20 all four default general engines refused the node at once.

### 8.1 The owner's URL router as a backend

The owner delivers the router and builds it separately; no job here touches the router or its data. What 1.0 builds is the place it plugs into.

**Done means:**

1. A backend seam in `ragon-search`, with SearXNG as the first backend and no change in behaviour.
2. The router's backend is off by default, tried first when on, and falls back to SearXNG (and says when it did). It never invents a URL: anything it returns is checked by fetching before it is cited.
3. `/airgapped on` blocks every backend that leaves the machine. The egress audit knows what the router connects to, which the owner's interface must state.
4. `WebTools` in `mcp_server/web_tools.py` behaves the same.

The router's delivery is a dependency: if it arrives after the freeze, 1.0 ships the seam, off, and the router follows in 1.0.x.

### 8.2 Agent-side quality

**Done means, measured on one fixed evaluation set built first:**

1. **Ranking:** results merged across engines, deduplicated by canonical URL, documentation ranked well for documentation queries. The set's hit rate at 5 rises from today's and never falls for a class.
2. **Fetching:** `read.rs`, `html_text.rs` and `fetch.rs` keep code blocks and tables, drop boilerplate, and say when a page needs JavaScript. A fixture corpus pins this.
3. **Citations:** every answer that used the web cites the URLs it read, and the citations are checked on a sample in the assistant evaluations: the page exists, was fetched in that session and supports the claim.
4. **Failure:** when engines refuse, the output says which, and why, in words a user can act on.
5. **Tests:** fixtures, not the live web, in the suite; one live run of the set per cut.

The local research skill (BACKLOG item 4) is the natural user of all this and is in the backlog, last in this track.

The rest of the old §5 (paid providers, reselling, the local index, `google cse`) is D3.

## 9. Matrix on the overlay

`dreamference/chat/matrix_homeserver.py` is Tailscale only. REMOTE_ACCESS §8 designed the move: server name = the node's overlay name, the node's own TLS from its CA in front of the loopback proxy, `tailscale serve` no longer needed. §9 item 7 has never been run live.

**The unknown that decides the design:** REMOTE_ACCESS §15 found that NetBird's Android app trusts only the system store, which is why the control plane has a public certificate. The overlay's internal names stay under the node's CA, so whether Element X on a phone trusts that CA has not been tested. This is checked first, by hand, before code.

**Done means:**

1. A new homeserver on a node with remote access uses the overlay by default (REMOTE_ACCESS §14.4). An existing one keeps its name until `matrix remove`, and the Tailscale option stays.
2. A phone (Android and iPhone, NetBird plus Element X) sends and receives through it, end to end, and it survives a node restart. If Element X cannot trust the node's CA on a phone, D6's fallback applies.
3. The `ragon web` Devices page's QR enrolment (REMOTE_ACCESS §4.2) gets a phone to that state without a terminal.
4. The audit and the docs describe the overlay path, and `docs/messengers.md` no longer says Tailscale is required.

## 10. Fan-out

The old spec recorded ideas, not a design. Nothing in the product uses fan-out: it is scripts only (`scripts/fanout_scan.py`, `fanout_scan_eval.py`, `swe_bench_chooser_fanout.py`, `swe_bench_chooser_confidence.py`). The measurements so far are mixed, so this track's done is a **measured bar with a stated way to drop it**, not a feature list.

### 10.1 What the data say

- **Scan, 111 cases (2026-11-03):** recall by line 10.8% against the baseline's 15.3%, by judge 9.0 against 5.4, by both 3.6 against 3.6. 46.7 s per file against 3.0 s, at 229 probes. Findings on the fixed files 151 against 558. Round 1 alone reaches 29.7% line recall at 4.3 candidates per file; the confirm round throws most hits away because the framings disagree on half. The 60-case precision sample is still ungraded.
- **Choosing among attempts:** night 13's chooser v1 chose 62% against the oracle's 72%. The local confidence signals did not beat v1, because rollouts carry no generation logprobs. A hosted judge reached the oracle at 7-9 cents per task, which is not the local product.

So the promising shape is round 1 alone, and the slow part may be fixable in the engine. Neither is proven.

### 10.2 What the engine lacks

The fan-out idea is many forks on a shared prefix, with single-token answers read from probabilities. `ragon-serve` runs at most four sessions (`kAutoSessionsCap`, also `kMaxSeqs`, `src/serve/admission.hpp`:235-238) and has no `n > 1`. It does have logprobs (2026-11-01), shared prefix pages (BATCHING §5.3) and the `ragon` object for arguments that only change speed. (`ENGINE_API.md`:99 still says two requests at a time; that line is stale.) A scoring pass is a real engine item, and it should be specified before it is built.

### 10.3 Done means

1. **The precision sample is graded and round 1 alone is re-evaluated** on a held-out half of the cases, with no engine change.
2. **Pass the bar in D4:** then the engine's scoring pass is specified and built, its probabilities match separate requests, and one product use ships behind a flag, off by default. Review scan or the chooser, whichever passed.
3. **Miss the bar:** 1.0 ships no fan-out mode. The scripts and the report stay; the release notes say nothing about it, and the ideas move to 1.1. That counts as done.

### 10.4 The larger model across several GB10s

D2 recommends 1.1: no larger model has been chosen, and it would need the scoring pass plus a pipeline-parallel engine.

### 10.5 Reliability over long work (kept by D2)

The old spec's seven levers, of which the first three are the realistic ones: the failure table, a check after every step, long work cut into checkable pieces. **Done would mean:** the table exists from the weekend's rollouts, a benchmark of 100-step tasks reports the rate of right steps, and whichever lever the table names first moves that rate without losing on the measured run.

### 10.6 Concurrency chosen during execution (kept by D2, droppable at the freeze)

Sizing at start exists (auto-size, 2026-11-04). Growing and parking sessions while the server runs does not. **Done would mean:** one session at idle, four under load, the same four-conversation bar as §11.2, and Night Shift and SWE-bench following the engine's number instead of a fixed `max_parallel`.

## 11. The acceptance gate

The 1.0 candidate is a 0.9.x cut with every track that made it. The gate runs on that cut and nothing else lands afterwards except fixes to what the gate found.

### 11.1 Tests

- `pytest tests/` with the ruff and mypy ratchets; the launcher, the messenger bridges and the leaf crates in the export; `ragon-code-rs`, `ragon-docs-rs` and `ragon-web-rs` on their own lockfiles; `npm test` in `desktop/electron` and `desktop/ui`, and `npm run e2e`; `ragon-admin codex test`.
- The engine's own tests and benchmarks on the engine's measurement box, and on the Windows port per the programme.
- `ragon-admin audit egress` and `--app` pass, with each bridge on in turn (Signal, Telegram, Matrix, Discord) and with each search backend. `/airgapped on` refuses what it must.

### 11.2 The measured run

The shape from the old plan, with D1 settling the open parts:

- **A SWE-bench run on the candidate's exact configuration**, on the validated instances, same flags as the baseline, compared with the baseline that the weekend run of 2026-11-07 can provide.
- **Pass:** not worse than the baseline at a stated p and margin, chosen before the run. The verdict is computed by a script in the repository; the old plan found that the verdict code and the union of reports do not exist yet (backlog items).
- **Speed:** four-conversation combined decode never below the recorded floor, greedy output identical 16/16.
- **Windows:** the programme's bars on its own boxes. No SWE-bench number is claimed for a Windows node unless one is measured.

As AGENTS.md says, the number compares configurations on this machine; it is not a leaderboard score, and the announcement must say so if it quotes it.

### 11.3 The install and update matrix

Each row: install or update, then a cold start, a real conversation with tools, and `ragon web`. "Update" starts from the previous 0.9.x, with real user state in place.

| Row | Fresh | Update | Rollback (`install.sh --version <last>`) |
| --- | --- | --- | --- |
| GB10 node | ✓ | ✓ | ✓ |
| GB10 node with an adapter installed | ✓ | ✓ | ✓ |
| AMD Windows node (`install.ps1`) | ✓ | ✓ | ✓ |
| A second node paired over SSH, one row per kind | ✓ | ✓ | |
| Linux x86-64 client | ✓ | ✓ | |
| Windows client | ✓ | ✓ | |
| macOS client | ✓ | ✓ | |
| Desktop app on a GB10 node and on Windows | ✓ | ✓ | |
| Bridges: state survives update, still off where off | | ✓ | |

Version order is D8. NVIDIA on Windows adds a row only if WN6 puts it in 1.0.

### 11.4 Live checks no test can do

Discord's nine; Matrix on a phone; adapters switched per session on a real node; the router backend if delivered; the first-run experience from a clean machine, following the public docs word for word, since strangers will.

## 12. The release train

- **0.9.x cuts continue Tuesday and Friday at 09:00 UTC.** A track lands in whichever cut it passes on, and the release notes say so then, including its limits. Nothing waits for 1.0.
- **Rough order, by this draft's estimate, not a promise:**
  - November: Discord live, the search seam and evaluation set, the verdict script, adapters on the engine (as soon as the engine job passes), drafter Phase 0.
  - December: Matrix on the overlay, search quality, engine holds several drafters, `work`, the fan-out evaluation and D4's call, parts of Windows nodes behind `-Role node` as a preview.
  - January: `ask` if allowed, every other content track complete. The Windows gate has its own calendar (§4), and the freeze and the 1.0 gate follow it.
- **The freeze** is two weeks before the owner's chosen date. A content track not in a cut by then is handled by D8.
- **1.0 itself carries no new track.** It is the candidate that passed §11, with the notes, the site and the paper. The paper's measurements come from that candidate, not from an earlier cut.
- **After 1.0:** SGLang removal, the self-tuning drafter, the Mac node, whatever D2 moved.

## 13. Risks

| Risk | Effect | What this plan does |
| --- | --- | --- |
| Windows nodes take longer than January allows: 18-27 calendar weeks by the programme's estimate | The gate moves the date to spring, or staffing has to change | D8, after the spike; WN7 decides whether Windows or Metal gets the kernel work first; `windows-node-risks.md` |
| GPU contention: adapters, drafters, Windows comparison, weekly runs | Drafter weeks collide with engine measurements and nights | D5; extra boxes when provisioned; `work` first |
| A box failing under the run, disk running short | The measured run slips | Candidate's run booked ahead; slice as the fallback (D1) |
| Adapter misses 0.95x again (it missed once already) | A gate holds | Report first; the owner may restate the bar only with the numbers in front of them |
| Element X cannot trust the node's CA on a phone | Matrix on the overlay is desktop-only | Hand check first; D6's fallback |
| `google cse` in the default engines scrapes a third party's embedded search box, with no key | Public scrutiny at announcement | D3 recommends it off by default by the freeze |
| Search refused by every engine on announcement day | The first impression of `ragon-search` | The failure message (§8.2); the seam makes a user-key provider cheap (D3) |
| Version order across the reset: this repository's first release is 0.9.0 and no install updates into it from the earlier numbering | None expected; a test of the version comparison from 0.9.x to 1.0.0 is still part of the update checks | D8 |
| Discord's limits or terms found live | Discord's user install (its D3) may not work as designed | Phase 0 comes first; a home server is the spec's fallback |
| The fan-out track eats engine time with no result | Engine lane delayed | The evaluation needs no engine work; the bar comes before the build |
| Attention arrives before 3.0's security work | Public scrutiny of a product that runs commands | The owner's decision, not reopened here; the egress audit and `/airgapped` remain gate items |
| Too much at once for a clean announcement | Quality of the first impression | D8's rule: content that misses the freeze ships in 1.0.x |

## 14. Decisions

The old plan's eight, for the record:

| Old | Subject | Now |
| --- | --- | --- |
| 1 | Denominator and run dates | Decided, in D1 |
| 2 | Adapters | Superseded: engine lane, gate |
| 3 | Pass rule | Decided, in D1 |
| 4 | Per-user trust | Superseded: per-user homes are not planned |
| 5 | vLLM, Minima, Flash-Next | No decision needed for 1.0 (`ragon-serve` is the default); registry cleanup in 1.x |
| 6 | Cut date | Superseded: January 2027, the owner's call |
| 7 | The rest of 2.0, migration from older versions | Decided in D2 and D8 |
| 8 | The slice's boxes, the headline number | Decided, in D1 |

Each decision below is **Decided** (the owner's answer, dated 2026-11-06) or **Open** (the recommendation stands and nothing is built on it until the owner answers).

### D1. The measured run: how much, how judged, and what number we quote. Decided

The denominator is the validated instances of the SWE-bench harness (357 today).

- **The 1.0 development gate** is a 100-instance slice, fixed seed, stratified by repository, judged "not worse than the baseline at p = 0.05 with a margin of 3 instances", one-sided and paired. The rule is written down before each run. The run uses whatever nodes exist then.
- **The headline** is the full 357 run on 1.0, over one weekend. It also gives the paper its figure; the paper is held until 1.0.
- A control arm only if fair pairing is on.

### D2. The rest of the old spec: recorded ideas and the on-hold table. Decided

- **In:** reliability levers 1-3 (§10.5). **In, droppable at the freeze:** concurrency chosen during execution (§10.6).
- **Into 1.0 from the on-hold table:** Forge over remote access, the vision OCR queue, Drive PDFs.
- **Stay on hold:** the advisor node, `ragon fast` diffusion, sync phases 3-4.
- **To 1.1:** the self-tuning per-repo drafter, the larger model across several GB10s, levers 4-7.
- **No job** for the URL-answering LoRA: it is the router's business.
- **Frozen for 1.0:** the served model (Qwen3.8-27B NVFP4 and its draft).

### D3. Web search beyond the owner's two halves. Partly decided

- **Decided:** "web search" is both the owner's URL router as a backend beside SearXNG and agent-side search quality. The router arrives before December and is integrated in the early-December cuts.
- **Open, recommendation stands:** one paid provider (Brave's API) with the user's own key, in the user's config and blocked by `/airgapped on`; no reselling; the local index in 1.1; `google cse` off by default by the freeze, after a survey of what it contributes.

### D4. The bar for fan-out. Partly decided

- **Decided:** fan-out in 1.0 is best-of-n at the agent level.
- **Open, recommendation stands:** the engine's scoring pass and its use in review ship only if, on the held-out half of the 111 cases, round 1 alone (or the engine's pass) reaches at least 1.2x the baseline's line recall, with no more findings per file and precision on the graded sample no worse, in at most 10 s per file.

### D5. Drafters. Partly decided

- **Decided:** several drafters v1 is a drafter chosen by workload, with two or three draft models measured per drafter. The engine boxes and the night boxes share the cost; the weekend run wins over engine work.
- **Open, recommendation stands:** `ask` goes ahead only if it passes the spec's 15% and its pilot's +3%; public tasks only for training; rent GPUs if the extra boxes are not ready in time.

### D6. Matrix when a phone will not trust the node's CA. Open

Recommendation: check by hand first. If Element X cannot trust it on a phone, 1.0's overlay default applies to desktops and phones keep the Tailscale option, documented plainly. No public certificate on a node's overlay name.

### D7. Discord: what beyond Phase 1. Partly decided

- **Decided:** the bridge is direct messages with the paired owner only.
- **Open, recommendation stands:** whole answers only (no `discord_stream`), no `discord_keep_hours`, `/push` and `/merge` with confirmation if Telegram's and Matrix's have them, the forum phase in 1.1.

### D8. The cut: what may hold it, the date, and which old versions we carry. Partly decided

- **Decided:** 1.0 is reserved for the release that goes with the announcement, on the owner's explicit decision, and nothing is announced before. Releases stay 0.9.x, Tuesday and Friday. Windows is a gate (§4): the AMD Strix Halo node passes its gate; NVIDIA on Windows is 1.x. 1.0 ships on GB10 and Windows nodes; the Mac is not in 1.0. The release numbering started at 0.9.0 in this repository, with its own signing key, so no older numbering has to be carried.
- **Open, recommendation stands:** only the two gates and §11 hold the announcement; a content track that misses the freeze ships in a 1.0.x and the announcement does not mention it; update paths are tested from the previous 0.9.x. The date is not repeated publicly until the Windows spike has reported: the programme's estimate lands between March and May 2027, and the gate wins over the date.

## 15. Sources

- The owner's decisions of 2026-11-04 to 2026-11-06; the Windows node programme's spec, backlog and risk list; the fan-out study of 2026-11-03.
- `specs/DREAMFERENCE_RAGON_ADAPTERS.md` §6.4, §7.2-§7.4, §9; `specs/DREAMFERENCE_RAGON_DISCORD.md` §10, §11, §15; `specs/DREAMFERENCE_RAGON_REMOTE_ACCESS.md` §4.2, §8, §9, §14.4, §15; `specs/DREAMFERENCE_BACKLOG.md` item 4; `specs/DREAMFERENCE_RELEASE_0.9.0.md`.
- The engine repository's multi-drafter, API, batching and research specs, and its admission code.

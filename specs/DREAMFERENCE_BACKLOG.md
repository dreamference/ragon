# Ragon Backlog

**What this is:** the planned work the owner has approved but not yet scheduled, one entry per item. An item leaves this page when it is built (its spec then records it) or dropped (with the reason, here, in one line). Work on the model server belongs to ragon-engine's own backlog (`ragon-engine/specs/DREAMFERENCE_RAGON_ENGINE_BACKLOG.md`), not here.

**Each entry gives:** what it is, why (with the survey section it comes from), a rough size, what it depends on, the test that says it is done, and its status.

**Sources:**
- [compaction-survey](./research/compaction-survey.md) (items 7–13).

The owner approved items 1–13 and item 14. The surveys' engine items are being added to ragon-engine's backlog separately: the drafter's NVFP4 scales re-quantized (shipped as `dreamference/qwen3.8-27b-dflash2-nvfp4-scalesearch`, `dreamference/hardware/model_matrix_registry.py`), tool-wait-aware eviction, and state saved at tool-output boundaries.

| # | Item | Area | Status |
|---|---|---|---|
| 1 | The chooser test on disk | SWE-bench | v1 run and in use; v2 run, not adopted; v3 built on branch `chooser/v3`, **NO GO, not merged** (updated) |
| 2 | Broken passing tests counted apart | SWE-bench | **Built** |
| 3 | `docs_ask` over long documents | `ragon-docs` | Not scheduled |
| 4 | A local deep-research skill | Skills | Not scheduled |
| 5 | Cost saved per session | Launcher, desktop | Not scheduled |
| 6 | Best-of-N in Night Shift | Night Shift | **Merged**; **N = 3 by default** (chooser v1) |
| 7 | Drop old tool outputs first, then summarise | Compaction | **Built and off** (patch 0026; the bench kept it off) |
| 8 | Delayed tail trim (AgentDiet) | Compaction | Not scheduled; after item 7 |
| 9 | Long tool outputs offloaded to a file | Launcher, compaction | Not scheduled |
| 10 | The compaction prompt tuned on our failures (ACON) | Compaction, SWE-bench | Not scheduled |
| 11 | Compaction columns and probe questions | SWE-bench | Columns **built**; probes not scheduled |
| 12 | A context meter and `/compact <focus>` | TUI, desktop | Not scheduled |
| 13 | Exploration in a sub-session | Agent | Not scheduled |
| 14 | Per-output summaries, precomputed off the critical path | Compaction | Approved, not scheduled; after the v2 bench |

---

## Benchmarks

### 1. The chooser test on disk

- **What:** a best-of-N chooser picks among the patches that earlier default runs already produced, for the tasks where those runs disagree. It scores how often the pick resolves. No agent run and no container are needed. The chooser is built R2E-Gym's and V1's way:
  1. **Regression filter.** Drop the patches that break the repository's existing tests (a PASS_TO_PASS failure), or that did not apply. The outcome is read from the grading logs on disk, not re-run. If every patch breaks something, all go on.
  2. **Pairwise judge.** The survivors are judged in pairs by the served model, using V1's settings:
     - one call per pair, showing the issue and the two diffs, never the agent's reasoning;
     - both rated 1-10, a win weighted by the rating gap (floor 0.1);
     - random disjoint pairs until each patch has two comparisons, then Swiss rounds within a window of 8. With three patches that is the three pairs.
  3. **The chooser never sees whether a patch fixed the issue.** It is given no FAIL_TO_PASS result and no `resolved` flag, or the test would grade itself. The script's candidates carry no verdict; a test checks it.
- **Reported:** the chooser's expected resolved count on the kept tasks, against three baselines: the regression filter alone (a fair coin among the survivors), a run picked at random, and the oracle (any run). Also which stage made each choice.
- **Why:** best-of-N (item 6) is worth building only if a chooser captures part of the headroom between a single run and the oracle.
- **Size:** well under an hour of model time. The script is [`scripts/swe_bench_chooser_test.py`](../scripts/swe_bench_chooser_test.py). Its runbook is in the docstring, and its tests are in `tests/test_swe_bench_chooser.py` (a fake server on loopback).
- **Depends on:**
  - **A free model server,** when no SWE-bench run, Night Shift task or engine test needs it.
  - **The runs, read only on this machine,** with their gradings' PASS_TO_PASS counts. These come from `grading.json` or, for older gradings, the harness's `report.json` files.
- **v2, built:** [`scripts/swe_bench_chooser_v2.py`](../scripts/swe_bench_chooser_v2.py), an R2E-Gym-style hybrid verifier over the same runs and seed. Its stages, in order:
  1. **Reproduction tests:** one to three standalone scripts written from the issue text only, kept when they fail on the unpatched repository in the task's container; every candidate is run against them, each in a fresh container with no network.
  2. **Existing tests:** v1's regression filter.
  3. **Groups by behaviour:** identical pass/fail vectors form a group; most reproduction tests passed wins, then the largest group (in runs).
  4. **Judge last:** only when a tie remains, shown the tests' results too, three calls per pair with the order swapped, the majority wins.
- **Acceptance:** `summary.json` gives the chooser's expected resolved count against the filter alone, a random run and the oracle. Item 6 goes ahead if the chooser is clearly above the random run.
- **Status:** approved by the owner. v1 was run and is the chooser in use (`dreamference/night_shift/night_shift_chooser.py`). v2 was run and not adopted (RAGON_NIGHT_SHIFT §11.9). v3 (chooser v1 with a differential search before its judge; design note [research/chooser-v3-moonshot.md](./research/chooser-v3-moonshot.md)) was built on branch `chooser/v3` (`scripts/swe_bench_chooser_v3.py`): NO GO, not merged.

### 2. Broken passing tests counted apart from unfixed issues

- **What:** the SWE-bench report splits the unresolved patches by cause:
  - one that broke a PASS_TO_PASS test;
  - one that left a FAIL_TO_PASS test failing and broke nothing.

  `--against` sets both side by side.
- **Why:** the two failures need different remedies: `tests-v2`'s rules aim at the first.
- **Size:** a few hours.
- **Depends on:** nothing new. Older gradings are split from the harness's `report.json` files already on disk, read only.
- **Acceptance:** `ragon-admin swe-bench report` on a night's run prints both counts, and `--against` sets them side by side, with tests.
- **Status:** approved by the owner and **built the same day** ([RAGON_SWE_BENCH](./DREAMFERENCE_RAGON_SWE_BENCH.md) §24).

## Product

### 3. `docs_ask`: questions over documents too long for one session

- **What:** `docs_ask <document> "<question>"` answers over a whole book. It works in three steps, plan, map and reduce:
  1. **Plan:** one model call writes the workers' instructions, asking for what the merge needs.
  2. **Map:** one call per ~8K-token chunk of `docs_read` output.
  3. **Reduce:** one call merges the answers, with the pages cited.

  It ships as a `ragon-docs` subcommand and an MCP tool beside `docs_search`, `docs_read` and `pdf_search`.
- **Why:** survey §4.1 (plan-divide-conquer).
  - [PDF_SEARCH](./DREAMFERENCE_PDF_SEARCH.md) finds passages, and `docs_read` reads pages.
  - Neither answers over a 400-page book, which no 94K-token session holds.
- **Size:** a few days.
- **Depends on:**
  - `ragon-docs`'s reader and chunker ([RAGON_LOCAL_INDEX](./DREAMFERENCE_RAGON_LOCAL_INDEX.md) §16), and `pdf_search`'s paging for documents outside a collection (PDF_SEARCH §3).
  - Calls to the model server from `ragon-docs`. That is new for the crate, so it is bounded like the rest of its work beside a resident model server: one or two streams, under the host-wide admission.
  - The same `/airgapped` and untrusted-text rules as `docs_read`.
- **Acceptance:** on a set of 10 questions over three long PDFs (a 300-page one among them), each with a known answer:
  - `docs_ask` answers more of them correctly than one session using `docs_search` and `docs_read`;
  - every claim is cited to a page;
  - the egress audit is unchanged.

  A 300-page PDF takes minutes, not hours (about 40 map calls).
- **Not for:** questions that join a clue in one chapter to a distant one. The paper calls this the "high task noise" case; the tool's description says so.
- **Status:** approved by the owner, not scheduled.

### 4. A local deep-research skill

- **What:** a `research` skill. It loops plan → `ragon-search` → `ragon-fetch` → reflect (find the gaps, search again) → rank the sources (with a cap) → write. The output is a Markdown report with citations, written into the workspace.
  - The main model plays every role.
  - Ask threads run `ragon`, so the web chat gets it too.
- **Why:** survey §4.2 (Open Deep Research): the loop beat single-step retrieval on FRAMES, SimpleQA and HotPotQA. Most of the quality is in the reflection step and the source cap.
- **Size:** about two days: a skill and a prompt. Check the upstream repository's license before reusing anything; the structure alone is enough to rebuild it.
- **Depends on:**
  - **A way to ship a skill with Ragon.** Today none is shipped. The skill would need its own root under `~/.ragon/skills/` with an origin file, so the launcher's "added outside `ragon skill add`" notice does not fire ([RAGON_SKILLS](./DREAMFERENCE_RAGON_SKILLS.md) §8). It must also stay under the catalogue budget (§7).
  - `ragon-search`/`ragon-fetch` and SearXNG.
  - **Off at `/airgapped on`.** The skill says so instead of failing silently.
- **Acceptance:** on 10 research questions with known answers:
  - the report answers more of them correctly than one `ragon` turn with the same tools;
  - every claim links to a fetched source;
  - nothing is fetched at `/airgapped on`.
- **Status:** approved by the owner, not scheduled.

### 5. Cost saved per session

- **What:** at the end of a `ragon` session, and in the desktop app, one line: the session's tokens (in, cached and out), priced against a small table of public cloud list prices.
  - The table ships and is dated with each release.
  - The figure is computed locally and never sent anywhere: no network, no telemetry.
- **Why:** survey §4.3 (the inverse of Together Link's spend line). It is consistent with "no telemetry ever".
- **Size:** under a day, plus updating the price table at each release.
- **Depends on:** the session token counters `/usage` already reads (patch `0011`, `ragon-rs/src/usage.rs`; [RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md) §3), and the desktop app's session view.
- **Acceptance:**
  - The figure equals the rollout's token counts times the table's prices (a test with a fixed rollout).
  - The table's date is printed beside it.
  - The egress audit (`ragon-admin audit egress`, plain and `--app`) is unchanged.
- **Status:** approved by the owner, not scheduled.

### 6. Best-of-N in Night Shift

- **What:** a Night Shift task can ask for `attempts = N`, each attempt in its own worktree as today. The best one is chosen in two steps:
  1. the regression filter: attempts that break the repository's existing tests are dropped;
  2. the pairwise judge item 1 measured, over the diffs alone.

  The morning report shows the chosen attempt, why it was chosen, and the alternatives' diffs.
- **Why:** survey §3.1 and §4.4. Night time is free. The cost is N times the agent time.
- **Size:** a few days.
- **Depends on:**
  - **Item 1's result.** Not built if the chooser does not beat a random run.
  - Night Shift's worktrees, scopes and report ([RAGON_NIGHT_SHIFT](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md) §5.3, §5.6).
  - The night's time budget: N attempts must fit the window, or the report says how many ran.
- **Acceptance:** on a SWE-bench list run as Night Shift tasks with `attempts = 3`, the chosen attempts resolve more than a single attempt, by a margin a paired test can see. The report names every attempt.
- **Status:** approved by the owner; N = 3 by default, configurable (owner); chooser v1 (v2 not adopted). **Built** on branch `night-shift/best-of-n` ([RAGON_NIGHT_SHIFT §11.9](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md)); A first SWE-bench acceptance run fell short of the margin the acceptance asks for, and the feature was **merged with `best_of = 1` by default** (the owner): three attempts only when `[night] best_of` or the task's `best_of` asks. **Accepted** after a second run on fresh tasks with the same design, when the owner made `best_of = 3` the default (branch `night/best-of-3`); interactive `ragon` stays one attempt.

## Compaction

In earlier SWE-bench runs, sessions that compacted several times resolved far less often than those that compacted once or never, and compactions took a sizeable share of agent time (compaction survey §1, measured).

### 7. Drop old tool outputs first, then summarise

- **What:** at the compaction threshold, one rewrite runs first. It keeps:
  - the prefix and every user message;
  - every tool call, and the short outputs;
  - the last ~20K tokens verbatim.

  Older long outputs become a one-line placeholder naming the call, and the ledger is re-injected as today. Codex's summary runs only if the result is still over the limit, and a summary is never summarised again.
- **Why:** survey §3.1. *The Complexity Trap* and *CliffCompaction* both point here. On this hardware it trades a ~98 s decode for a ~10–20 s re-prefill (estimate), and it touches nothing between threshold crossings.
- **Size:** about two days: a Codex patch in the local compaction path, under the patch cap.
- **Depends on:**
  - the local compaction path (`core/src/compact.rs`; [RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md) §7) and the ledger hook (§10.1);
  - the SWE-bench harness's arms, and the compaction columns (item 11) to read the arm.
- **Acceptance:** a night arm against default on the 44-task list:
  - resolved not lower;
  - compaction time down;
  - compactions, compaction seconds and re-reads recorded.
- **Status:** approved by the owner. **Built and merged off by default** (patch `0026`; updated): the turn-end compaction bench v2's `drop` arm did not earn it the default ([RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md) §14.3), and it ships off.

### 8. Delayed tail trim (AgentDiet)

- **What:** after step *s*, the main model rewrites step *s−2* when that step is over ~500 tokens and the saving is over ~500. It removes:
  - listing noise;
  - edits echoed back;
  - reads that have expired.

  Only the tail changes, so the re-prefill covers the last two steps.
- **Why:** survey §3.2 (AgentDiet: input tokens −40 to −60%, SWE-bench Verified +1.5 to +2 points). It is the one per-turn method that fits a prefix cache. It slows context growth, so sessions reach the threshold later.
- **Size:** a few days: a Codex patch and a short model call per long step.
- **Depends on:**
  - **Item 7 first.** This is built only if third compactions stay common after item 7.
  - On ragon-engine, a state checkpoint at each tool-output boundary would make the replay nearly free. That item is in ragon-engine's backlog.
- **Acceptance:** a night arm against item 7's winner:
  - resolved not lower;
  - fewer sessions with three or more compactions;
  - the added model time below the compaction time saved.
- **Status:** approved by the owner, not scheduled.

### 9. Long tool outputs offloaded to a file, with a preview

- **What:** a tool output over a size limit goes to a file in the session's scratch folder. The context keeps:
  - its first and last lines;
  - its size;
  - the path.

  The agent reads the file again with an ordinary command when it needs it. No model is involved, and nothing is lost.
- **Why:** survey §3.3 (the compression-gateway study's recommendation of compression that can be recalled). It compresses at ingestion, so nothing cached is invalidated, and later compactions carry less.
- **Size:** one to two days.
- **Depends on:** the pinned Codex's own output truncation. Where this lives, in the launcher's exec translation or a small patch, is to be checked against it. The scratch folder must be readable inside the sandbox, and cleaned with the session.
- **Acceptance:**
  - A test with a 1 MB command output puts the preview and the path in the context, and leaves the whole output in the file.
  - A night arm shows resolved not lower and the context's growth per task down.
- **Status:** approved by the owner, not scheduled.

### 10. The compaction prompt tuned on our own failures (ACON)

- **What:** ACON's loop, offline, on SWE-bench data already on disk:
  1. Take task pairs where a run with few compactions resolved and a run with many did not.
  2. Have the model compare what the summaries dropped.
  3. Revise the compaction prompt.
  4. A/B the revised prompt in a night.
- **Why:** survey §3.4. ACON reports peak tokens down 26–54%.
- **Size:** two to three days, mostly model time at night.
- **Depends on:**
  - the rollouts kept under each run's scratch folder;
  - `experimental_compact_prompt_file` (no patch; [RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md) §7);
  - the compaction columns (item 11).

  The trade §10.2 of that spec found must be watched: a structured prompt kept the files and lost the code.
- **Acceptance:** a night arm with the tuned prompt against default:
  - resolved not lower;
  - the share resolved among sessions with three or more compactions up.
- **Status:** approved by the owner, not scheduled.

### 11. Compaction columns in SWE-bench reports, and probe questions

- **What:**
  - **Columns:** compactions per run, their time and share of agent time, and resolved by compaction count (0, 1, 2, 3+). `--against` sets them side by side.
  - **Probes:** after each recorded compaction, Factory's four probe types (recall, artifact, continuation, decision) are asked of the compacted context. The answers are scored against the history before the compaction.
- **Why:** survey §3.5. Every compaction arm (items 7, 8, 10) is read on these numbers.
- **Size:** the columns, a few hours; the probes, two to three days.
- **Depends on:** the rollouts kept in each instance's scratch folder. The probes also need model time at night.
- **Acceptance:**
  - **Columns:** the report on a night's run prints them, with tests.
  - **Probes:** a night's report gives a probe score per compaction, and the score separates the arms of a compaction A/B.
- **Status:** approved by the owner. **The columns were built the same day** ([RAGON_SWE_BENCH](./DREAMFERENCE_RAGON_SWE_BENCH.md) §25). The probes are not scheduled.

### 12. A context meter in the TUI, and `/compact <focus>`

- **What:**
  - **The meter:** the TUI and the desktop app show how full the context is against the compaction limit, and the next compaction's estimated time from the measured rate.
  - **`/compact <focus>`:** compacts now and passes the focus ("keep everything about the parser") to the summary prompt.
- **Why:** survey §3.6. A user who sees a compaction coming can steer it, and compact at a moment that suits them.
- **Size:** two to three days: small TUI patches, under the patch cap, and the desktop view.
- **Depends on:**
  - the limit the launcher computes from the KV pool ([RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md) §11.1);
  - the counters `/status` and `/usage` read;
  - Codex's own `/compact`.
- **Acceptance:**
  - The meter's figure matches the limit the session uses (a test).
  - `/compact keep the parser` puts the focus into the compaction request (a test on the request).
  - The live slash-command suite passes.
- **Status:** approved by the owner, not scheduled.

### 13. Exploration in a sub-session

- **What:** large searches and reads run in a sub-session, which returns a short summary to the main one. The exploration never enters the main context. This is Context-Folding's shape, without its training.
- **Why:** survey §2 and §3.6. FoldGRPO matched ReAct with a 10× smaller active context; that gain relied on training, so the shape alone must be measured here.
- **Size:** unknown until the design is done. The first step is to read what the pinned Codex offers for sub-agents.
- **Depends on:**
  - Codex's sub-agent support at `rust-v0.158.0`;
  - the sandbox and `/airgapped` applying to the sub-session as to the main one;
  - two streams on the model server, if the sub-session runs beside the main one.
- **Acceptance:** a night arm against default:
  - resolved not lower;
  - fewer compactions per task;
  - the main context's peak lower.
- **Status:** approved by the owner, not scheduled.

### 14. Per-output summaries, precomputed off the critical path

- **What:** each long tool output is summarised right after it arrives, append-only, so the agent reads a few hundred tokens instead of 10–50K and a later compaction has less to compress. The summary is made where the agent is not waiting for it: by the main model in a second, low-priority stream between turns first; by a resident fast model only if one is ever accepted beside the main one. Paths, line numbers, test names and error strings are kept verbatim; where the output has a grammar (test logs, build logs, diffs) a rule-built extraction does it with no model at all, as the ledger does for compaction.
- **Why:** the owner's idea (summaries by a fast diffusion model instead of compaction), read against the earlier bench: of a compaction's 80–118 s, 47–75 s was a cache-cold re-read of 62–91K tokens and ~16 s the summary, so the compaction request made cache-hot (compaction spec §14), the 800-word cap and item 7 come first; what remains after them is the mid-turn compaction inside tool-heavy turns, which only less ingested text can shorten. Compaction spec §4.6 role 1 and Fast Tools §2 already name this role. Not a diffusion model as the premise: the only one that fits (Tiny-A2D 0.5B) was measured unusable (compaction spec §9.3); DiffusionGemma 26B A4B is a candidate summariser if the fast slot ever gets a resident model, a small autoregressive instruct model another, each gated by the identifier-survival test of compaction spec §5 and by the one-resident-model memory rule.
- **Size:** two to three days for the main-model stream and the rule-built extractors; the resident-model variant is Fast Tools work.
- **Depends on:** the v2 compaction bench's verdict (night); item 9 is the no-model alternative for the same outputs and the two should be compared on the same night arm; a second stream at low priority on the model server (the engine's session release/pin, SGLang survey proposal 9).
- **Acceptance:**
  - The identifier-survival test: every path, line number, test name and error string of a 20K-token log is in its summary.
  - No user-visible wait: the summary is ready before the agent's next request or the raw output is used instead.
  - A night arm against default: resolved not lower, fewer mid-turn compactions per task, the context's growth per task down.
- **Status:** approved by the owner, not scheduled; after the v2 bench and item 7.


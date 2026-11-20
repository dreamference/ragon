# Ragon Output Preprocessing

**Status:** proposed, 2026-11-20; spec only, nothing built. The idea and its conditions are the owner's (2026-11-20); the decisions this spec still needs are in §0.1, each with a proposed answer.

**In one sentence:** when an agent runs a command whose output is long, the local model (Ragon) first picks out the lines that matter, quoted exactly as printed and with their line numbers, so that the strong model reads a short, checked extract instead of thousands of lines, and the full output stays one file away.

**Who it is for:** the agents that run commands and read what they print: the coordinator's session, the worker agents it starts, any agent that uses Ragon's delegation plugin, and later `ragon` itself (§1).

**Builds on:**
- The delegation plugin ([its spec](./DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md), [docs/dev/claude-plugin.md](../docs/dev/claude-plugin.md)): the `ragon-delegate` binary in `ragon-rs/delegate/`, its exact tools (`lookup`, `run_test`, `git_log`, `diff_stat`), its model tools (`task`, `watch`), the busy check (plugin spec §5.3), the verification of model answers (plugin spec §6.2) and the rule that a model's words are a hint while code-checked text is fact (plugin spec D1).
- The local helpers that run outside the repository today: a digest helper that asks the local model for a ten-line summary of a log, and the **output shrinker**, an after-command hook for shell output. The shrinker saves outputs over 200 lines to a file (after the secret masker), shows the model's ten-line summary, every line matching an error pattern with its line number, and the file's path; it falls back to the last 40 lines when the model is busy or slow (18 s budget). Outputs of 61 to 200 lines are cut to the first 12 and last 30 lines plus up to 12 error lines, with no model. This spec keeps the shrinker's shape and replaces the model summary, which is prose, with a verified selection (§3).
- The coordinator's result cap (another after-command hook, for file reads, web results and a few shell commands), which already owns those results; shell output stays with the shrinker, so the two never answer for the same result (§5.4).
- RBRB ([DREAMFERENCE_RBRB.md](./DREAMFERENCE_RBRB.md)) and its sources, for the accounting (§6).
- The context budget work ([DREAMFERENCE_RAGON_CONTEXT_BUDGET.md](./DREAMFERENCE_RAGON_CONTEXT_BUDGET.md)), which plans shell-output filtering for `ragon` as a later phase; `ragon`'s own use (§1) is that phase.

---

## 0. Decisions

Agreed with the owner on 2026-11-20 (the conditions of the idea):

- **P1. The raw output is always kept in a file.** The extract names the file; the agent can open it with a line range at any time. Nothing is lost, only not shown. The one exception: where no secret masker is available, nothing is saved and nothing is preprocessed; the output passes through exactly as today (§3.5).
- **P2. Ragon selects; it never paraphrases.** The model returns line numbers. The extract shows those lines **copied by code from the raw file**, exactly as printed. No sentence the model writes reaches the strong model as fact.
- **P3. Every selection is checked against the raw file.** The model quotes the start of each line it selects; code compares the quote with the line at that number. One mismatch discards the whole selection, and the plain fallback (§3.4) is shown instead.
- **P4. Exact questions keep exact tools.** A count, a search for a string, a list of every match: those are answered by code (search, count, the exact tools), never by the model (§3.6).
- **P5. No preprocessing while a benchmark holds the GPU.** The plain fallback is shown; the agent never waits for the model (§4).
- **P6. The Ragon tokens it spends are counted in RBRB's "tool" source**, which records nothing today (§6).

Decided by this spec:

- **D1. A fixed floor, then the model.** Before the model is asked, code marks the lines that are always kept: the exit code, the last lines, and every line that matches the error and summary patterns (§3.2). The model may only **add** lines to the floor; it can never remove one. The worst the model can do is add noise or be discarded, so the extract is never worse than today's shrinker at keeping the line that matters.
- **D2. One implementation, in Rust, in the delegation binary** (`ragon-delegate preprocess`), so the product (whose client install has no Python) and the coordinator's hook run the same code, the same quote check and the same busy check. The coordinator's hook becomes a thin caller (§5.4).
- **D3. One owner per output.** Preprocessing takes over the shrinker's model tier for shell output over the threshold. It is not a third hook: two hooks writing the same result, last write winning, is the failure to avoid.
- **D4. Machine-readable output is never touched.** If the output is JSON or JSON lines, or the command asked for it, the preprocessor passes it through byte for byte: no model, no selection, no rewrite (§3.5).
- **D5. Command families, not every command.** In the first version only outputs that are logs are preprocessed: test runs, builds, installs, CI logs, service logs. Every other command's output is left as it is today (§3.6).
- **D6. Shadow first.** It runs for a week computing extracts that nobody reads, beside today's output, before the replay gate (§7) and the owner turn it on.

### 0.1 Decisions for the owner

| # | Question | Proposed answer | Why |
|---|---|---|---|
| O1 | On by default, or opt-in? | **Opt-in until the replay gate (§7) passes and the shadow week shows no miss; then on by default for the coordinator and the workers.** For plugin users it stays opt-in in the first release that ships it. | A dropped line costs more than the tokens saved; the gate proves it does not happen before anybody depends on it. |
| O2 | Which agents first? | **The coordinator's own session and the worker agents on the coordinator's machine** (one hook, both accounts). Plugin users second (§1, phase 2). `ragon` itself last (phase 3). | The coordinator and the workers read the most long output (land-queue tests, CI logs, builds); their outputs also feed the replay set and the weekly check. |
| O3 | The threshold | **More than 200 lines, or more than 16,000 characters on fewer lines.** Below it, the output is unchanged, or cut by today's cap tier (61 to 200 lines) with no model. | 200 lines is the shrinker's threshold today; the character bound catches a few very long lines (minified or one-line JSON-less logs). Short outputs are cheaper to read than to preprocess. |
| O4 | Does `ragon` get it as a product feature in 1.0? | **Yes for plugin users, opt-in, if the gate passes before the 1.0 cut; not for `ragon`'s own command outputs in 1.0** (that needs a Codex patch and its own measurement against the context budget work). | The plugin path needs no Codex patch and reuses the plugin's busy check; `ragon`'s own path saves context, not strong-model tokens, and is a different measurement. |
| O5 | Which Ragon tokens count in RBRB | **Only the tokens of extracts that were delivered** (selection verified and shown). Discarded attempts are reported in the weekly lines but not credited. | The share should measure Ragon's useful work, not attempts the agent never saw. |
| O6 | Where the replay set lives | **Outside the repository**, in the coordinator's private files, with a small redacted copy (host names, paths and names removed) committed as test fixtures. | Real outputs carry host names, home paths and account names, which may not enter the repository. |

---

## 1. What it is, and who uses it

An agent runs a command. The command prints 4,000 lines: a build's progress, 700 passing tests, one failing test and a summary. Today the agent either reads all of it (expensive, and the strong model re-reads it from its cache on every later turn of the session), or a cut-down version that keeps the head, the tail and the lines matching an error pattern, plus a prose summary by the local model.

With preprocessing, the agent reads a short extract: the exit code, the failing test's lines, the error lines, the final summary, each exactly as printed with its line number, then what was left out and how much, and the path of the full output. If anything in the model's work does not check out, the agent gets today's cut-down version instead, never a worse one.

**Users, and which are in the first version:**

| User | How it reaches them | First version? |
|---|---|---|
| The coordinator's session | The after-command hook for shell output (the shrinker's place), calling `ragon-delegate preprocess` | **Yes** |
| Worker agents started by the coordinator | The same hook, registered for both accounts on the coordinator's machine; the job id from the worker's environment tags the request | **Yes** |
| Any agent using Ragon's delegation plugin | An opt-in hook the plugin installs (`ragon-delegate hooks enable preprocess`), and a plugin tool `preprocess` that takes a saved output file and an optional question | Phase 2 |
| `ragon` itself, for the outputs of the commands it runs | A Codex patch at the point where a command's output enters the session's history | Phase 3, after the context budget work measures it |

**On the plugin and "no hook that routes work"** (plugin spec D11 and its non-goals): that rule forbids a hook that takes a decision away from the strong model, such as diverting its searches to Ragon. Preprocessing decides nothing for the agent: it runs after the agent's own command, changes only how much of the output is shown, and the full output is one path away. It is still opt-in for plugin users (O1, O4), and the explicit tool exists for agents that prefer to ask.

**On `ragon` itself:** there the reader is the local model, so nothing is saved on a strong model; what is saved is the session's context. That is the context budget's measure, not this spec's, which is why it comes last and is not in version 1.0 (O4).

---

## 2. The contract

### 2.1 Input

| Field | Required | Meaning |
|---|---|---|
| `command` | yes | The command line as the agent ran it (masked like the output, §3.5). Used to pick the command family (§3.6) and given to the model as context. |
| `raw` | yes | The path of the raw output file: standard output, then standard error, each line as printed, after the secret masker (§3.5). |
| `exit_code` | where the caller has it | The command's exit code. The plugin tool and `ragon` always have it. The after-command hook may not: today's hook input carries the output and an "interrupted" flag; whether the harness passes the exit code, or only marks a failed command in the output text, is checked by work item W2. **When it is unknown, the extract says "exit code: unknown"; it is never guessed.** |
| `question` | no | The caller's question about the output ("why did the build fail?"). The hook has none; it asks the default question: what failed, and what is the final result. An exact question is refused (§3.6). |
| `job` | no | The job tag for accounting (§6): the worker's job id, or `coordinator` for the coordinator's own session. |

### 2.2 Output

A plain-text extract in a fixed order, every group titled by code:

```
[output preprocessed: 4,112 lines, exit code 101; full output: <raw path>]
Errors (3 of 3):
  2,915: <the line, exactly as printed>
  ...
Failures (1 of 1):
  3,870: <...>
Final summary:
  4,104: <...>
  ...
Also selected by the local model (5):
  2,910: <...>
Left out: 4,090 lines: 1-2,909 (hint: build progress), 2,920-3,860 (hint: passing tests), ...
Last lines:
  4,106: <...>
  ...
```

- **Groups.** Errors, failures and the final summary come from the floor (§3.2) and, where the model adds lines, from its selection; "also selected" holds the model's additions that fit no group. A group title says how many of its lines are shown ("3 of 3").
- **Left out** lists the line ranges not shown and their counts, computed by code. Each range may carry a short label from a fixed list (build progress, passing tests, downloads, warnings, repeated lines, other), chosen by the model and marked **hint**. A wrong label misleads less than a missing range would, and the ranges themselves are code's.
- **The raw path** is always on the first line.
- **The plugin tool** returns the same content as JSON (`{exit_code, total_lines, raw, groups: [{name, shown, total, lines: [{n, text}]}], left_out: [{from, to, hint}], source: "model" | "fallback", reason}`), so that a caller can read line numbers without parsing text.

### 2.3 Size limits

| Limit | Proposed value | When exceeded |
|---|---|---|
| Extract length | 80 lines and 8,000 characters | The overflow rule of §3.3 |
| One shown line | 400 characters | Cut, with `[line cut; full line <n> in the raw file]` |
| Raw file | 100 MB | Kept up to the limit; the extract says the file was cut and where |
| What the model reads | At most 4 windows of about 12,000 tokens each, with line numbers | The rest is described as "not read by the model" in the extract, and only the floor covers it (§3.3) |
| Raw files kept | The last 500, at most 7 days | Older files are deleted, as the shrinker does today |

### 2.4 Time limit

- **Hook:** 20 s from the command's end to the extract, the whole call. The floor is computed first (well under a second for 100 MB); the model gets what is left. Past the limit the model call is cancelled and the plain fallback is shown.
- **Plugin tool:** 60 s, because the caller asked and is waiting for this answer only.
- The model windows of one output are sent at the same time, never one after another.

---

## 3. Correctness rules

### 3.1 Verbatim only

The model is asked for line numbers, each with the first 40 characters of the line as a quote. It never supplies the text that is shown: code copies every shown line from the raw file. A model that invents a line therefore cannot put it in front of the strong model; at worst it points at the wrong line, which the quote check catches.

### 3.2 The floor: what code always keeps

Before any model call, code marks:

1. **The exit code** (or "unknown"), and whether the command was interrupted.
2. **The last 20 lines.** The end of a log is where test runners, compilers and installers print their verdict.
3. **Every error line**: lines matching the error pattern that the shrinker uses today, widened with today's evidence (the four Windows failures, §7.1): `error`, `Error`, `ERROR`, `error[E` (the Rust compiler's error codes), `FAILED`, `FAIL`, `Traceback`, `ModuleNotFoundError`, `ImportError`, `panicked`, `assertion` and `AssertionError`, `Exception`, `fatal`, lines starting `E ` (the Python test runner's failure detail), `exit code [1-9]`, `Segmentation fault`, `command not found`, `No such file or directory`, `Permission denied`.
4. **Every summary line**: the test runners' and builders' result lines (`test result:`, `N passed`, `N failed`, `short test summary`, `error: could not compile`, `Tests:`, `FAIL ` with a file name, and the like), kept in a pattern list next to the error pattern and grown from the replay set.
5. **Two lines of context** before and after each error and summary line, so a failed test's name, usually printed directly above the failure, is never cut off.

The patterns live in one file in `ragon-rs/delegate/` with a test per pattern, each test a real line from the replay set (redacted).

### 3.3 When the floor is larger than the extract

A log with thousands of warnings matches the error pattern thousands of times; today's shrinker shows only the first 25 such lines, so one real error behind the warnings is lost. The order in which the floor is cut when it does not fit:

1. The exit code, the first line and the "left out" summary: never cut.
2. Lines matching the hard error patterns (`error[E`, `Traceback`, `ModuleNotFoundError`, `ImportError`, `panicked`, `FAILED`, `fatal`, `assertion`): the **first 10 and the last 10** of them, then the rest by model rank if the model answered.
3. Summary lines: all of them, up to 15 lines.
4. The last 20 lines.
5. Warning-like matches (`warning`, `deprecated`): only as counts ("1,912 warning lines left out, lines …").
6. The model's own additions: whatever room is left.

Whatever is cut is named in "left out" with its count and ranges. **A count of dropped error lines is always shown**; an extract never implies "no errors" when some were cut.

### 3.4 The quote check and the fallbacks

- Every selected line number must exist and its line must start with the model's quote, after both are trimmed of surrounding spaces. **One failure discards the whole model selection**: a model that misread one line may have misread the rest.
- **The plain fallback** is the floor alone, shaped like the extract and titled `[output preprocessed without the local model: <reason>]`. It is today's shrinker without the summary, plus the overflow rule of §3.3.
- Reasons, each written to the run log and counted weekly (§6.3): `busy` (benchmark or GPU window, §4), `airgapped`, `unreachable`, `timeout`, `quote_mismatch`, `bad_answer` (the model's answer did not parse), `too_large`. (A missing masker is not a fallback: the output passes through untouched, §3.5.)
- A fallback is never an error for the agent: the command's result stands, only the presentation changes.

### 3.5 Secrets and machine-readable output

- **Secrets are masked the same way as today**, before the raw file is written and before the model sees a line: the coordinator's shared secret filter, which the shrinker and the result cap already use. The product needs the same masker in Rust (W5), tested against the same cases. **Without a masker nothing is saved and nothing is preprocessed** (logged as `masker_missing`, never shown as a fallback): the output passes through as it is today.
- **Masking must not change the line count**: it replaces inside a line, never adds or removes a line break, so the line numbers in the extract, in the raw file and in the model's view are the same numbers.
- **Machine-readable output is never touched.** It is left byte for byte as the command printed it, with no selection and no model, when any of these holds:
  - the command asks for it (`--json`, `--format json`, `--format=json`, `-o json`, `--output json`, `--porcelain`, a pipe into a JSON tool);
  - the whole output parses as one JSON document;
  - at least nine in ten of its non-empty lines parse as JSON (JSON lines).

  This is the lesson of 2026-11-20: the secret masker rewrote the field names of a release check's JSON output, the check failed, and the release waited. Whatever the masker's JSON-safe mode does to such output today stays outside this feature; the preprocessor adds nothing to it.

### 3.6 When it does not run

- **Under the threshold** (O3): the output is unchanged, or cut by today's cap tier with no model.
- **Exact questions** (P4): a question asking how many, whether a string appears, which lines contain something, or for every match is refused with `exact question: use a search or a count`, and the plugin's skill tells the agent to run the search itself. Code decides this from the question's words; the model never answers it.
- **Commands outside the log families** (D5): the first version preprocesses only
  - test runs (the Python, Rust and JavaScript test runners, and the repository's own test scripts);
  - builds and installs (compilers, package installers, container builds);
  - CI logs fetched from the forge, service logs, and the journal.

  Everything else (file reads through the shell, searches, diffs, listings, version control commands, anything machine-readable) is left as it is today. The list is a file with a test per entry; a family joins it only after the replay set has cases of it.
- **The preprocessor's own commands** (and the digest helpers) are never preprocessed, so it cannot loop.

---

## 4. When it must not use the model, and what happens then

In every case below the agent gets the plain fallback (§3.4) at once, with the reason on its first line. Nothing waits, nothing queues, nothing retries.

| Condition | How it is detected | Reason shown |
|---|---|---|
| A SWE round, a benchmark night or a GPU test window holds the model | The plugin's busy check (plugin spec §5.3): the runner lock, an active `ragon-swe-*` unit, the model gate's 503, and on the coordinator's machines the GPU lock of the node | `busy: <what holds it>, until about <time>` when known |
| `/airgapped on` in a `ragon` session | `ragon airgapped --json`, failing closed. At `on` no plugin is loaded anyway; the check stays for the phase-3 path inside `ragon` | `airgapped` |
| The node cannot be reached | The busy check's one-token request times out (3 s) | `unreachable` |
| The model server is up but slow | The time limit (§2.4) | `timeout` |

**Another node:** the busy check's step 3 (another node serving the same model, not reserved by the owner) applies to the plugin tool only, which a caller is waiting for. The hook never moves to another node: a fallback is cheaper than a second node's time for an extract.

---

## 5. Where it runs

### 5.1 Which node serves it

The node that serves the user's model, reached the way the plugin reaches it: the launcher's node resolution, by name or id, never by address. On the coordinator's machines, the node named in the coordinator's helper settings, as the digest helper does today. The preprocessor itself runs where the agent runs (the client); only the windows of masked output go to the node's model gate, as ordinary chat requests tagged `preprocess` (§6.1).

### 5.2 Memory beside a resident model server

- **No second model.** Preprocessing is requests to the model the node already serves; it loads nothing.
- **The client side streams the raw file**: the floor is computed line by line, so a 100 MB output costs a few megabytes of memory, under a 256 MB bound tested by W3.
- **On the node,** each window uses the model server's ordinary memory for one request of about 13,000 tokens. At most **two preprocessing requests at once per node** (a small counter at the gate, or a lock file on a single node), so the memory the server keeps for user sessions is never taken by extracts.

### 5.3 Never slowing a user's own session

- Preprocessing requests go at the lowest priority the gate has (the batch priority of the delegation tools); when the gate reports a waiting interactive request, or the two-request limit is reached, the call answers `busy` at once and the fallback is shown.
- No queue: an extract that would wait is replaced by the fallback. A queue would turn a saved minute of strong-model reading into an agent waiting on a log.
- On a node, the client side runs in the batch slice at low CPU priority, as the delegation tools do (`ragon-batch.slice`, `nice 10`).

### 5.4 How the coordinator's hook fits

Today two after-command hooks look at a shell result in the coordinator's session: the result cap (a few commands only) and the shrinker (all other long shell output). They run side by side on the same original output and the last write wins, so they are kept apart by command. Preprocessing goes **into the shrinker's place, for outputs over the threshold**: the shrinker masks and saves the raw file as today, calls `ragon-delegate preprocess --raw <file> --command <cmd> --job <job>` instead of the digest helper, and prints its extract. The result cap and the cap tier (61 to 200 lines) are unchanged. The shrinker and the digest helper are local files outside this repository; their change is a coordinator-tools item (W6), not a product change.

---

## 6. Accounting

### 6.1 Ragon tokens spent

- Every preprocessing request carries the job tag (`job`: the worker's job id, or `coordinator`) and the workload `preprocess`.
- After each preprocessing, the preprocessor writes one row to the same usage file the `ragon exec` shim writes today, the file RBRB's **"tool"** source reads: `{finished, job, workload: "preprocess", generated, input, cached, delivered, reason}`, with the token counts from the model server's usage answer. Rows carry no output text, no command and no question.
- **RBRB today composes each job from one source, the first that has rows** (gate, then tool, then node, then trailer). A worker job that also ran `task` has gate rows, so its preprocessing rows would be ignored, and a gate that logs the `preprocess` requests too would count them under "gate". The rule: **gate rows with workload `preprocess` are skipped, and `preprocess` rows from the tool file are added to the job on top of whichever source the job's other work came from.** Each token is counted once, under "tool". (W8 changes the composition and its test.)
- Per O5, only rows with `delivered: true` are credited; the others appear in the weekly lines as spent but not credited.
- **Honest scale.** RBRB's published method counts generated tokens. Preprocessing reads a great deal and writes little (line numbers and short quotes), so its effect on the published share is small while that method stands. Its main effect is the other side: fewer strong-model tokens spent. The weekly report shows the two separately and never presents the saving as share.

### 6.2 Strong-model tokens saved

Measured per preprocessing, from the agent's transcript afterwards, not estimated at the time:

- **First read:** the raw output's tokens minus the extract's, at the input weight. Tokens are estimated from characters with one ratio, measured once against the usage ledger and stated in the weekly report.
- **Every later turn:** the same difference again at the cache-read weight, for each later call the session made (known afterwards from the transcript). A long output read early in a long session is paid on every turn after it; this is usually the larger part.
- **Minus the re-reads:** if the agent opens the raw file afterwards (a read of the raw path), what it read is subtracted. The share of extracts followed by such a read is reported on its own: it is the best live sign that an extract missed something.

### 6.3 The weekly lines

One block in the coordinator's weekly report: preprocessings, delivered, fallbacks by reason, extracts followed by a raw read, estimated strong-model tokens saved (net), Ragon tokens spent and credited. No figure from it enters a release note or a spec until the owner asks for it.

---

## 7. The quality gate before it is on by default

### 7.1 The replay set

Each case is a real past output with a known important line:

`{command, exit_code, raw (the output file), important: [{line, text}], family, source, note}`

- **The four Windows failures of 2026-11-20**, each of which sat on one line among thousands: the Python import of a POSIX-only module (`fcntl`) that stopped the package from importing at all; the Rust compiler's "undeclared crate" error for a Linux-only crate referenced without its platform gate; the one failing test among hundreds in the launcher; and the test that asserted a Linux path for the trust store.
- **Twenty land-queue rejections**: the test or build output that made the land queue refuse a change, with the important line being the one the fixing job named in its report. Chosen across families (Python tests, Rust tests, builds, lint ratchets), most recent first.
- **Controls**, which must pass too: ten green runs (the important line is the final summary, and the extract must not invent an error), five machine-readable outputs (must pass through byte for byte), five outputs with secrets planted in them (the secret must not appear in the extract or the raw file), five outputs slightly under the threshold (must be untouched), and one output of each fallback reason, forced.
- Every miss found later (§7.3) becomes a new case. Cases are never removed.
- Per O6 the set lives in the coordinator's private files; the redacted copy in `ragon-rs/delegate/` tests holds only what the anonymity rules allow.

### 7.2 The bar

Run against the model and the node as they serve today, three times (the model's answers vary):

1. **The important line is shown in 100 % of the failure cases, in every one of the three runs**, in the delivered extract or in the fallback. One miss fails the gate. A preprocessor that drops the line that matters is worse than none.
2. **Size: the median extract is at most one fifth of the raw output** over the cases above the threshold, and no extract exceeds the limits of §2.3.
3. **Controls:** machine-readable outputs byte-identical, no secret visible, no output under the threshold changed, no green run's extract showing an error that is not in the output.
4. **Time:** nine in ten extracts within the hook's 20 s.
5. **Fallbacks** at most one in ten cases outside the forced ones; a higher rate means the prompt or the windows need work, even if no line was missed.

The gate's report lists every case with its result and is attached to the owner's question (O1).

### 7.3 The weekly check

- Every Monday in the day window (never in a night or a GPU window), the replay set runs once against the current model, prompt and patterns. Any miss of an important line, or a control failing, **turns preprocessing off by itself** (back to today's shrinker) and reports to the owner; it goes back on only when the gate passes again.
- Twenty extracts of the week that were followed by a raw read are compared by code with the raw lines the agent read afterwards: when the agent read a range the extract left out, the case is added to the replay set with that range as its important line, after a person-free review by the coordinator.
- A change to the model, the prompt, the patterns or the window size runs the full gate before it is used.

### 7.4 Shadow week

Before the owner's decision on O1: for one week the hook computes the extract for every eligible output, writes it next to the raw file, and shows the agent today's output. The comparison (would the extract have kept every error and summary line; would the agent's later commands have needed something it left out) goes into the gate's report.

---

## 8. Risks

- **Dropping the one line that matters.** The evidence is today's: four Windows build failures in a row each sat on a single line among thousands (an import, an undeclared crate, one failed test, one asserted path); an agent that reads an extract without that line fixes the wrong thing or calls a red run green. Answered by the floor (D1, §3.2), the overflow order (§3.3), the 100 % bar (§7.2) and the weekly check (§7.3).
- **A model selection that looks right but points at wrong lines.** Answered by code copying the text (§3.1) and the quote check (§3.4).
- **Rewriting output a program reads.** Answered by §3.5; the lesson is the masker's damage to a release check's JSON on 2026-11-20.
- **Slowing the user's own session.** Answered by §5.3: lowest priority, two requests at most, no queue.
- **A saving that is not real** because agents re-open the raw file. Measured, not assumed (§6.2); a high re-read rate is a reason to turn it off.
- **Inflating RBRB.** Only delivered extracts count (O5), and the saving is reported apart from the share (§6.1).

---

## 9. Goals and non-goals

**Goals**
- Fewer strong-model tokens per long output, net of re-reads, measured from the transcripts.
- No loss: every line the agent needs is in the extract or in the floor; the raw output is always one path away.
- Ragon's work on it counted honestly in RBRB's "tool" source.
- One implementation for the coordinator, the workers and the plugin.

**Non-goals**
- Summaries in the model's own words. The existing digest helper may still be called by hand for a report; it is not part of this feature.
- Preprocessing file reads, web pages or mail (the result cap's ground).
- Answering questions about the output: the caller's question only guides the selection.
- Moving an extract to another node from the hook.
- Any change to `codex/` (the phase-3 path is a numbered patch, under the patch-size cap, when it comes).
- New Python in `dreamference/`; if any is needed later, it follows the Python quality ratchet ([DREAMFERENCE_PYTHON_QUALITY.md](./DREAMFERENCE_PYTHON_QUALITY.md)).

---

## 10. Work list

One job each, in order; the repository is named for each. "Product" is this repository; "coordinator tools" is the private coordinator-tools repository.

| # | Item | Repository | Files | Test | Depends on |
|---|---|---|---|---|---|
| W1 | **The replay set.** Collect the four Windows failures and twenty land-queue rejections with their important lines, plus the controls (§7.1), into the coordinator's private files; write a redacted copy of five cases as fixtures. | coordinator tools (set), product (fixtures) | private replay folder; `ragon-rs/delegate/tests/fixtures/preprocess/` | a loader test: every case has a raw file, an important line that matches the raw file, a family | none |
| W2 | **What the hook receives.** Find out, from a recorded hook input, whether the after-command hook gets the exit code, and how a failed command is marked; write the finding into `docs/dev/claude-plugin.md`. | product (doc) | `docs/dev/claude-plugin.md` | the docs tests | none |
| W3 | **The floor and the extract, no model.** `ragon-delegate preprocess --no-model`: streaming read, error, summary and context patterns, the overflow order, the left-out ranges, the text and JSON outputs, the size limits, the machine-readable pass-through, the command families. | product | `ragon-rs/delegate/src/preprocess.rs` (new), `preprocess_patterns.rs` (new), `cli.rs` | unit tests per pattern and per rule; a 100 MB file under the memory bound | W1 fixtures |
| W4 | **The model step.** Windows with line numbers, the prompt (line numbers and 40-character quotes only, a label from the fixed list for each left-out range), parallel requests, the quote check, the fallbacks and their reasons, the busy check and the air-gap check reused from the plugin, the time limits. | product | `preprocess.rs`, `exec.rs` | tests with the scripted model runner (a correct answer, a wrong quote, a bad answer, a timeout, busy, air-gapped) | W3 |
| W5 | **The secret masker in Rust**, with the same cases as the coordinator's filter and the line-count rule. | product | `ragon-rs/delegate/src/mask.rs` (new) | shared vectors; line count unchanged on every vector | none |
| W6 | **The coordinator's hook.** The shrinker's model tier calls `ragon-delegate preprocess`; shadow mode switch; job tag from the worker's environment; registration for both accounts. (Re-scopes backlog row `ct-output-digest-by-ragon`.) | coordinator tools | the shrinker, its settings | a test with a recorded hook input per family and per fallback | W4 |
| W7 | **The replay runner and the gate report.** `ragon-delegate preprocess --replay <dir> --runs 3` writes the per-case table and the bar's verdict (§7.2); the weekly timer and the automatic switch-off (§7.3). | product (runner), coordinator tools (timer, switch) | `preprocess.rs`; a weekly unit | the runner on the fixtures, with a scripted model that misses one line, must fail | W4, W1 |
| W8 | **Accounting.** The usage row per preprocessing; RBRB's composition adds `preprocess` rows on top and skips gate rows with that workload; the re-read and saving measures from transcripts; the weekly block. | product (row), coordinator tools (RBRB, weekly) | `preprocess.rs`; RBRB composition | a job with gate rows and preprocess rows counts each token once, under "tool"; undelivered rows are not credited | W4 |
| W9 | **Shadow week and the gate.** Turn on shadow mode for a week, run the gate, write the report for O1. | coordinator tools | report only | the gate's bar | W6, W7, W8 |
| W10 | **Plugin users (phase 2).** The plugin tool `preprocess`, the opt-in hook (`ragon-delegate hooks enable preprocess`), the skill's lines, the user docs. | product | `ragon-rs/delegate/src/mcp.rs`, the plugin's files, `docs/dev/claude-plugin.md` | the delegation server's tool tests; a hook test on a scripted harness input | W9, O4 |
| W11 | **`ragon` itself (phase 3).** A Codex patch at the point where a command's output enters the history, measured against the context budget spec's replay. Planned only after the owner's answer to O4 and the context budget's next measurement. | product | a numbered patch in `codex-patches/` | the context budget replay | W9, O4 |

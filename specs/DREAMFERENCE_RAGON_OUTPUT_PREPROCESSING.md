# Ragon Output Preprocessing

**Status:** decided by the owner on 2026-11-20 (a question round of 100 questions, §0.1); spec only, nothing built. This is revision 2: it replaces the first draft of the same day wherever the owner chose differently. The points the answers leave open are in §0.3, each with a proposed default. The replay set starts at once; the product code starts after the 0.9.9 release (decision 99).

**User-facing name:** **Ragon Digest**. The command is `ragon digest`. This spec keeps the title Ragon Output Preprocessing (decision 97).

**In one sentence:** when a command prints a long output, the user's local node reads it instead of the paid model: Ragon picks the lines that answer the caller's question, quoted exactly as printed with their line numbers, adds a short retelling marked "not verified", and the full output stays in a file one path away.

**The pitch** (decision 85): "Your local node reads the logs instead of the paid model."

**Why** (decisions 1 and 80): the main goal is to raise Ragon's share of the work (RBRB); fewer tokens spent on the strong model is the side effect. The share grows by the number of outputs Ragon preprocesses and the tasks it does, never by longer text: brevity and accuracy come first.

**Builds on:**
- The delegation plugin ([its spec](./DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md), [docs/dev/claude-plugin.md](../docs/dev/claude-plugin.md)): the `ragon-delegate` binary in `ragon-rs/delegate/`, its tools, the verification of model answers (plugin spec §6.2) and the rule that a model's words are a hint while code-checked text is fact (plugin spec D1).
- The node and its model gate ([DREAMFERENCE_RAGON_NODE.md](./DREAMFERENCE_RAGON_NODE.md)), sign-in on the node from 0.9.9 ([DREAMFERENCE_AUTH.md](./DREAMFERENCE_AUTH.md)), and the Windows and Mac nodes ([DREAMFERENCE_WINDOWS_NODE.md](./DREAMFERENCE_WINDOWS_NODE.md), [DREAMFERENCE_MAC_NODE.md](./DREAMFERENCE_MAC_NODE.md)).
- RBRB ([DREAMFERENCE_RBRB.md](./DREAMFERENCE_RBRB.md)), for the accounting (§7).
- `/airgapped on` ([DREAMFERENCE_RAGON_AIRGAPPED.md](./DREAMFERENCE_RAGON_AIRGAPPED.md)), under which the feature keeps working (§4.4).
- For stage 2: Night Shift ([DREAMFERENCE_RAGON_NIGHT_SHIFT.md](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md)), the phone bridges, the context budget work ([DREAMFERENCE_RAGON_CONTEXT_BUDGET.md](./DREAMFERENCE_RAGON_CONTEXT_BUDGET.md)) and the platform quality bands ([DREAMFERENCE_PLATFORM_QUALITY.md](./DREAMFERENCE_PLATFORM_QUALITY.md)).

---

## 0. Decisions

### 0.1 Decided by the owner on 2026-11-20

Each answer in one line, with its number. Where a later answer refines an earlier one, the earlier line gives the final rule and names the refining answer; §0.2 states those rules in full. Answers 18, 21 and 25 to 28 are void.

| # | Decision |
|---|---|
| 1 | Main goal: a larger Ragon share of the work (RBRB); fewer strong-model tokens is the side effect. |
| 2 | Straight into the product, as a Ragon feature for every user; the project's own agents use it as ordinary users. |
| 3 | Inside Ragon itself (the outputs of the commands `ragon` runs): yes, as a later stage. |
| 4 | Off by default until the replay check passes; then on by default. |
| 5 | Access: the plugin, automatic on long outputs, plus a command for manual use and for scripts. |
| 6 | Result: selected lines verbatim with their numbers, plus a short retelling by Ragon marked as unverified (its length: at most three sentences, decision 42). |
| 7 | Length threshold: configurable, 200 lines by default. |
| 8 | The caller always says what it looks for ("why did the build fail?"); in the plugin this is the run tool's mandatory field (decision 92). |
| 9 | One quote that does not match the raw output discards the whole answer: plain truncation (head and tail) with a "preprocessing failed" note. |
| 10 | The retelling comes after the quotes, marked "not verified". |
| 11 | Always kept, whatever the model does: the exit code, the last 20 lines, every line with an error or failure word (found by plain search; the model cannot drop them), the raw file's path and its line count. |
| 12 | Raw outputs kept for one day, and rotated earlier when they take too much space (a size cap). |
| 13 | While the model serves a user's session, preprocessing runs at the same time, sharing the model; it never waits for the user's turn to end. |
| 14 | Time limit per output: 20 seconds, then plain truncation. |
| 15 | Very long outputs: plain search first (windows around the errors and the end); the model reads only those windows. |
| 16 | Works under `/airgapped on` (local model, nothing leaves). |
| 17 | RBRB: preprocessing is counted together with all other Ragon work, in one figure, with no separate line. |
| 18 | Void: the owner reverted it in the same round; the RBRB measure stays generated tokens only (row 25-28). |
| 19 | Bar for on by default: the important line kept in 100 % of the replay set. |
| 20 | Replay set: real failures from the project's own work plus public build and test logs in several languages; only public material is committed. |
| 21 | Void: the owner reverted it in the same round; the RBRB measure stays generated tokens only (row 25-28). |
| 22 | No limit on how much concurrent preprocessing slows a user's own session; the model is shared equally among requests. |
| 23 | No question from the caller: no preprocessing, plain truncation (the plugin always has one, decision 92). |
| 24 | Result size: at most 60 lines (the 20 mandatory last lines plus up to 40 selected); §0.3 says which lines count. |
| 25-28 | Void: reverted by the owner in the same round ("revert, use only written tokens"). The RBRB measure stays generated tokens only, for every model, as since 2026-11-12: preprocessing credits only the tokens Ragon writes (the quote selection and the retelling), inside the one figure (decision 17). No discount coefficient, no recalculation of history, thresholds unchanged. |
| 29 | Preprocessed: builds and compilation, test runs, service and container logs. Not preprocessed: code search and file listings. |
| 30 | Never given to the model: the contents of code files, diffs, machine formats (JSON, CSV). Outputs with secrets are not excluded: they go to the local model; secrets are masked in what the strong model receives, as today. |
| 31 | A successful command with no errors still gets a model call (it answers the caller's question); the caller does not wait for it (decision 91). |
| 32 | Warnings: their count and the first few distinct ones; errors always in full. |
| 33 | Product command: `ragon digest` (example: `command \| ragon digest "why did it fail?"`). |
| 34 | Savings shown by a statistics command: outputs preprocessed, reduction factor, strong-model tokens not spent. |
| 35 | Off switch at three levels: user setting, project setting, a flag for one run. |
| 36 | Release: the first release after the replay check passes, no later than version 1.0. |
| 37 | The simple part (error search, truncation, quote verification) runs on the node; the whole output goes to the node (one implementation, also the client's fallback, decision 89). |
| 38 | Language: Rust, inside `ragon` (one code base for Linux, Mac and Windows). |
| 39 | Node unreachable or model still loading: truncation at once, no waiting, marked "the model did not take part" (made by the same code on the client, decision 89). |
| 40 | A local-only journal of preprocessing failures (a quote mismatch; the agent opened the raw file afterwards); never sent anywhere. |
| 41 | The retelling is always in English. |
| 42 | The retelling is at most three sentences. |
| 43 | The retelling states what happened only; no advice on what to do next. |
| 44 | Repeated similar lines are collapsed by a plain rule before the model ("lines like Compiling: 1340"). |
| 45 | No special protection against text in the output that reads like an order to the model; security work goes to version 3.0. The correctness rules (quote verification, mandatory lines) still apply. |
| 46 | Secrets: this feature does nothing about them (no masking added here); the local model sees the output as it is. |
| 47 | On the node the output is held in memory only during processing, never stored. |
| 48 | With accounts on the node (0.9.9), preprocessing requires sign-in like any model request and is attributed to the user. |
| 49 | Follow-up: a second question to the same raw file, and reading lines by number without the model; the command is not run again. |
| 50 | The agent opens the raw file anyway three times in a row: preprocessing is switched off for that kind of command until the session ends, and the case goes to the journal. |
| 51 | The project's worker agents must use it for builds, tests and logs once the replay check has passed (the coordinator is not made to). |
| 52 | No cap on concurrent preprocessing jobs per node; a request goes to the least busy available node (decision 90). |
| 53 | Savings shown in tokens and reduction factor; no money figures. |
| 54 | A public claim on the site and in the README with the project's own measured figures after a month of own use, with the counting method. |
| 55 | Nothing in Ragon's status line; statistics only. |
| 56 | A small statistics block in the web UI and the desktop app: outputs preprocessed and savings for the week. |
| 57 | The replay check runs again before every release and at every change of the model, the engine or the feature's code. |
| 58 | Below 100 % before a release: the release ships with preprocessing off by default; it comes back on when the check passes again. |
| 59 | Replay set for the first switch-on: 100 outputs. |
| 60 | The "important line" of each case is labelled by a strong model; the coordinator re-checks one tenth of the labels. |
| 61 | Long-running commands: preprocessed at the end in the first version; streaming preprocessing is a later stage. |
| 62 | Colours, progress bars and terminal control characters are stripped before the model and before verification; the raw file keeps them. |
| 63 | Very long single lines are not shortened. |
| 64 | Output in other languages: an error word list for the product's six languages, plus the exit code and the end of the output. |
| 65 | Night Shift gets it together with the "inside Ragon" stage. |
| 66 | A SWE slice is run with and without preprocessing inside Ragon; it is switched on inside Ragon only if the result is at or above the middle of the band. |
| 67 | Phone bridges: long command outputs sent to the phone are preprocessed too. |
| 68 | A documented, stable node interface for preprocessing; `ragon digest` and the plugin use it, and third-party tools and CI may too. |
| 69 | Build order: the replay set first, then the command, then the plugin's automatic mode. |
| 70 | Two agents in parallel on the feature: one on the replay set, one on the command and the node interface. |
| 71 | Small pieces go to Ragon itself: cleaning rules, error word lists, tests on sample outputs. |
| 72 | Opus review before landing for the quote verification and the node interface; ordinary checks for the rest. |
| 73 | Settings by command (`ragon digest on\|off\|status`) and in the settings file. |
| 74 | A project may list patterns of lines that are always important there; the stricter of the project's and the user's rules wins. |
| 75 | The user may set the length threshold to any value (no lower bound). |
| 76 | The user may switch the retelling off and get the verbatim lines only. |
| 77 | The coordinator uses it too, except while working on a release-blocking failure (then it reads the raw output). |
| 78 | The project's own agents preprocess on the routing nodes, wherever one is free. |
| 79 | No RBRB target for preprocessing yet: measure for one week first, then set one. |
| 80 | Brevity and accuracy over share: the retelling stays at three sentences; the share grows by the number of preprocessed outputs and Ragon's tasks, not by longer text. |
| 81 | Empty or binary output: a short factual note, without the model. |
| 82 | Standard output and the error stream are shown together in time order, each line tagged with its stream; the error stream has priority in the selection. |
| 83 | The same output with the same question again: the earlier result, from a cache kept for one day. |
| 84 | A reply in the wrong format: one retry inside the same 20 seconds, then truncation. |
| 85 | Pitch: "Your local node reads the logs instead of the paid model." |
| 86 | Ready-made integration in the first version for Claude Code and Codex; other agents use the command. |
| 87 | A documentation page with three real before-and-after examples: a build, a test run, a service log. |
| 88 | A headline item of the 1.0 announcement if a month of the project's own measured figures exists by then; otherwise one line. |
| 89 | The simple part is one Rust implementation in `ragon`: it normally runs on the node, and when the node is unreachable the same code gives the truncation on the client. |
| 90 | No cap on concurrency, but a request goes to the least busy of the available nodes. |
| 91 | On success the caller gets the exit code and the summary lines at once; the model still answers the question, and its reply is appended to a file beside the raw output, which the agent reads if it wants. |
| 92 | The plugin's run tool has a mandatory field `what I want to learn`, filled by the agent, so a question always exists and the automatic mode works. |
| 93 | The model: the one already loaded on the node; no second model. |
| 94 | The model's thinking mode is off for preprocessing. |
| 95 | Windows and Mac nodes support it from their first day; the replay set joins the new-node gate. |
| 96 | With no node at all, `ragon digest` says that a node is needed (there is no model-free mode as a product; the client-side truncation of decision 89 is only the fallback when a configured node is unreachable). |
| 97 | User-facing name: Ragon Digest; the spec keeps the title Ragon Output Preprocessing. |
| 98 | The finished spec is shown to the owner as a short retelling in chat plus the file. |
| 99 | Start: the replay set at once; the product code after the Monday 0.9.9 release. |
| 100 | The owner names the size of each future question round. |

### 0.2 Conflicts the answers resolved: the final rules

- **C1. Where the work runs** (decisions 37, 39, 89, 96). One Rust implementation in `ragon` does the simple part: cleaning, the mandatory lines, error search, windows, truncation and the quote check. It normally runs on the node, which receives the whole output. When a configured node cannot be reached or its model is still loading, the same code runs on the client and gives the plain truncation at once, marked "the model did not take part". With no node configured at all, `ragon digest` says that a node is needed; the client-side truncation is a fallback, never a product mode.
- **C2. The question** (decisions 8, 23, 92). Every preprocessing has a question. In the plugin the run tool's field `what I want to learn` is mandatory, so the automatic mode always has one. `ragon digest` takes it as an argument; a call without a question gets plain truncation and no model.
- **C3. A successful command** (decisions 31, 91). When the exit code is 0 and no mandatory error line was found, the caller gets the exit code, the line count, the raw path and the summary lines (the last 20 lines) at once. The model is still asked the caller's question, and its verified reply is appended to a file beside the raw output (`<raw>.digest`), which the agent reads only if it wants.
- **C4. Concurrency** (decisions 13, 22, 52, 90). No cap per node and no limit on how much a user's own session slows down: the model is shared equally among requests, preprocessing included, and nothing waits for a user's turn to end. Each request goes to the least busy of the available nodes that serve the model; the project's agents use the routing nodes, wherever one is free (decision 78).
- **C5. The RBRB measure** (decision 17; 18, 21 and 25 to 28 void). Generated tokens only, as for every model since 2026-11-12: preprocessing credits the tokens Ragon writes (the quote selection and the retelling) and nothing it reads, inside the one RBRB figure, with no separate line and no coefficient.
- **C6. The retelling's length** (decisions 6, 42, 80). At most three sentences.
- **C7. The mandatory lines are in every result** (decisions 9, 11, 39, 45). The exit code, the last 20 lines, every error and failure line, the raw path and the line count are computed by code before the model is asked, and they appear in every result: the full digest, the plain truncation after a failed check or a timeout, and the truncation made on the client when the model did not take part. "Plain truncation" therefore means the mandatory lines plus the first lines of the output, within the size limit.
- **C8. Secrets** (decisions 30, 46). The feature adds no masking and removes none: the local model and the raw file see the output as it is, and whatever masking the strong model's side applies today stays where it is.
- **C9. The start** (decisions 69, 70, 99). The replay set track starts at once. The command and the node interface start after the 0.9.9 release. The plugin's automatic mode follows the command.

### 0.3 Open points

The answers leave these open. Each has a proposed default, which the work list uses until the owner decides otherwise.

| # | Open point | Proposed default |
|---|---|---|
| X1 | The size cap that rotates raw files before their day is over (decision 12). | 1 GB of raw files per user; above it the oldest files go first. A single output larger than 200 MB is kept up to 200 MB, and the result says where it was cut. |
| X2 | The error word list for the product's six languages (decision 64). | Ragon drafts it (decision 71) from the localised messages of the common compilers, test runners and package tools; every word gets a test line from a public log; the list grows with the replay set. |
| X3 | The cache key of decision 83 (the node never stores the output, decision 47). | The cache sits on the client beside the raw files, for one day. Key: a hash of the cleaned output, the exact question text, the model's name and the feature's version. |
| X4 | More error lines than the 60-line result holds (decisions 11, 24 and 32). | The 60-line cap holds. Error lines take the 40 selected places first, after the repeated-line rule collapses them; when they still exceed 40, the result shows the first 20 and the last 20, with the count and the line ranges of the rest, and says so in its first line. Every error line stays in the raw file. |
| X5 | A benchmark round, a GPU test window or a night holding the node (the answers cover a user's session only). | The node's existing busy check still applies there: plain truncation marked "the model did not take part", so a measurement is never disturbed. |
| X6 | Credit for an answer that was discarded (a quote mismatch, a timeout). | Only answers that passed the quote check and reached the caller are credited in RBRB; the others appear in the journal and the statistics as failures. |

**Design values set by this spec** (not owner decisions; changed by a spec edit when the replay set shows the need):

| Value | Default |
|---|---|
| The first few distinct warnings (decision 32) | 5 |
| Head lines in a plain truncation | as many as fit the 60-line limit after the mandatory lines, at most 10 |
| "Very long" output for the windows of decision 15 | more lines than fit one request to the loaded model with the prompt and the answer |
| A window around an error line | 15 lines before and 15 after; overlapping windows merge; the last 200 lines are always a window |
| "Kind of command" for decision 50 | the program name and its first sub-command (for example `cargo test`) |
| Replay runs per check | 3; every run must keep every important line |
| Which lines the 60-line cap of decision 24 counts | the output's lines: the last 20 and up to 40 selected. The first line, the section headings, the warnings block and the retelling line are written by code on top of them, at most 12 more lines |
| `ragon digest` with no node configured | after the message that a node is needed, the output passes through unchanged, so a pipe never loses it |
| The three off switches of decision 35 | off at any level wins |

---

## 1. What it is, and who uses it

A command prints 4,000 lines: a build's progress, 700 passing tests, one failing test and a summary. Today the agent reads all of it, and the paid model reads it again from its cache on every later turn of the session. With Ragon Digest the agent receives at most 60 lines of the output plus a short header written by code: the exit code, the lines that answer its question (each exactly as printed, with its number and its stream), the last 20 lines, a warning count, the raw file's path, and a retelling of up to three sentences marked "not verified". If any of Ragon's work fails its check, the agent receives a plain truncation instead, never a worse one (§3.4).

**Stage 1** (the first release that ships it):

| User | How it reaches them |
|---|---|
| Anyone, by hand or in a script | `ragon digest` (§2.1) |
| Agents in Claude Code and Codex | The plugin's run tool, automatic on outputs over the threshold, with the mandatory field `what I want to learn` (§2.4, decisions 5, 86, 92) |
| Other agents, third-party tools, CI | The command, or the node interface directly (§2.5, decision 68) |
| The project's own worker agents and the coordinator | As ordinary users of the plugin (decision 2). Once the replay check has passed, worker agents must use it for builds, tests and logs (decision 51); the coordinator uses it too, except while working on a release-blocking failure, when it reads the raw output (decision 77). |

**Stage 2** (later; §9): inside Ragon itself for the outputs of the commands `ragon` runs (decision 3), switched on there only after a SWE slice comparison (decision 66); Night Shift with it (decision 65); the phone bridges (decision 67); streaming preprocessing of long-running commands (decision 61).

---

## 2. The command, the result and the node interface

### 2.1 `ragon digest`

```
<command> 2>&1 | ragon digest "why did the build fail?"     # pipe mode
ragon digest "why did the build fail?" -- <command> [args]  # run mode
ragon digest ask <raw-id> "which test failed first?"        # a follow-up question (§5.1)
ragon digest lines <raw-id> 2900-2950                       # lines by number, no model
ragon digest on | off | status                              # the user setting (§6)
ragon digest stats                                          # the statistics (§7.2)
```

- **Run mode** starts the command itself, so it knows the exit code and keeps standard output and the error stream apart, each line tagged with its stream and in time order (decision 82). **Pipe mode** sees one stream: the exit code is "unknown" unless given with `--exit-code N`, and no line is tagged. The digest never guesses an exit code.
- **Under the threshold** (200 lines by default, any value allowed, decisions 7 and 75): the output is printed unchanged.
- **Without a question**: plain truncation, no model (decision 23).
- **`--off`** is the flag for one run (decision 35): the output is printed unchanged and only the raw file is written.
- **No node configured**: the command prints that a node is needed (decision 96); this spec passes the output through unchanged after that message (design value, §0.3).
- `ragon digest` never preprocesses its own output or another `ragon digest`, so it cannot loop.

### 2.2 The result

In this order, every heading written by code:

```
[Ragon Digest: 4112 lines, exit code 101, model took part; raw: <path>]
Selected, 12 lines:
  2915 err: <the line, exactly as printed>
  ...
Warnings: 1912 lines, 37 distinct; first 5:
  <line> ...
Last 20 lines:
  4093 out: <...>
  ...
Retelling (not verified): <at most three sentences, in English>
```

- **At most 60 lines** (decision 24): the 20 last lines plus up to 40 selected ones. Error and failure lines are selected first and in full (decision 32); the model's choices fill the rest of the 40 places; X4 covers a log with more error lines than fit.
- **Warnings** are given as a count and the first five distinct ones (decision 32).
- **The retelling** comes after the quotes and is marked "not verified" (decision 10): at most three sentences (decision 42), in English (decision 41), stating what happened and never what to do next (decision 43). The user may switch it off and get the verbatim lines only (decision 76).
- **The first line always carries** the line count, the exit code, whether the model took part and the raw path (decision 11). The notes are fixed: `model took part`, `model reply pending: <raw>.digest` (decision 91), `preprocessing failed: <reason>` (decision 9), `the model did not take part: <reason>` (decision 39), `no question` (decision 23).
- **Empty or binary output**: a one-line factual note ("no output", "binary output, 3.2 MB"), without the model (decision 81).
- **A successful command** (exit code 0 and no error line found; decisions 31 and 91): the result goes back at once, with the note `model reply pending: <raw>.digest`, the exit code, the line count, the raw path and the last 20 lines. The model still answers the caller's question; once its reply passes the quote check, the reply is appended to `<raw>.digest` beside the raw file, in the same form as a full result. A reply that fails the check, or a time-out, writes the matching note to that file instead. The agent reads the file only if it wants.

### 2.3 What is preprocessed, and what passes through

- **Preprocessed** (decision 29): builds and compilation, test runs, service and container logs.
- **Passed through unchanged, never given to the model** (decisions 29 and 30): code search results, file listings, the contents of code files, diffs, and machine formats (JSON, CSV). Code decides this from the command (a list of programs and flags, with a test per entry) and from the content (a whole output that parses as JSON, nine in ten lines that parse as JSON lines or CSV, a diff's headers). Leaving machine formats untouched also keeps a program that reads them working.
- **Outputs with secrets are not excluded** (decisions 30, 46): they go to the local model as they are (§3.7).

### 2.4 The plugin's automatic mode (Claude Code and Codex)

- The plugin gets a run tool, `run`, beside its other tools: a command, the mandatory field `what I want to learn`, an optional `digest: off` for one run (decision 35), and an optional time-out.
- When the output is over the threshold, the tool returns the digest instead of the output, automatically (decision 5). Under the threshold it returns the output unchanged.
- The skill tells the agent to use `run` for builds, tests and logs, and to put its question in the field, so the automatic mode always has a question (decision 92).
- Other agents use the command (decision 86).

### 2.5 The node interface

- A documented, stable endpoint on the node's model gate (decision 68). Request: the output (with stream tags and time order when known), the question, the exit code or "unknown", the command line, the threshold, the user's and the project's important-line patterns, and whether a retelling is wanted. Answer: the result of §2.2 as JSON (`{exit_code, total_lines, model_took_part, note, selected: [{n, stream, text}], warnings: {count, distinct, first}, last: [...], retelling}`) and as the printed text.
- `ragon digest`, the plugin's `run` tool and, later, the stage-2 users all call it. Third-party tools and CI may call it with the same sign-in as any model request.
- The interface is versioned; a change that breaks a field is a new version, and the old one stays served for one release.
- From 0.9.9 the interface requires sign-in like any model request and every request is attributed to the user (decision 48).

---

## 3. Correctness rules

### 3.1 The mandatory lines (code, before any model)

Code computes them first and the model cannot remove them (decision 11):

1. **The exit code**, or "unknown".
2. **The last 20 lines.**
3. **Every error or failure line**, found by plain search with the error word list: the English words (`error`, `failed`, `FAIL`, `fatal`, `panicked`, `Traceback`, `Exception`, `assertion` and the like), the words of the product's six languages (decision 64, list open: X2), and the project's and the user's important-line patterns (decision 74; when both exist, every pattern of both applies).
4. **The raw file's path and its line count.**

They appear in every result, including every fallback (C7).

### 3.2 Cleaning before the model and the check

- **Colours, progress bars and terminal control characters are stripped** before the model sees the output and before the quote check; the raw file keeps them (decision 62). A carriage-return progress line keeps only its last state.
- **Repeated similar lines are collapsed** by a plain rule before the model: a run of lines that differ only in a name, a number or a path becomes one line, "lines like Compiling: 1340" (decision 44). The line numbers of the run stay known, so a selected line inside it is still quoted from the raw file.
- **Long lines are not shortened** (decision 63).
- **Stream priority**: in the selection, lines of the error stream come before lines of standard output (decision 82).

The cleaning rules live in one Rust file with a test per rule on a sample output (decision 71).

### 3.3 The model step

- **The model is the one already loaded on the node**; no second model is loaded (decision 93). **Thinking is off** (decision 94).
- **Very long outputs**: plain search first; the model reads only windows around the error lines and the end (decision 15; window sizes in the design values of §0.3).
- **What the model returns**: line numbers, each with the start of its line as a quote, and the retelling. It never supplies the text that is shown: code copies every shown line from the cleaned output.
- **A reply in the wrong format** (it does not parse, a line number does not exist, the retelling has more than three sentences): one retry inside the same 20 seconds, then plain truncation (decision 84).
- **The time limit** is 20 seconds per output, from the end of the command to the result, the retry included; past it, plain truncation (decision 14).

### 3.4 The quote check

Every quote is compared with the cleaned line at its number. **One mismatch discards the whole answer** (decision 9): the caller gets the plain truncation (C7) with the note `preprocessing failed: quote mismatch`, and the case goes to the journal (§5.3). A model that misread one line may have misread the rest.

### 3.5 Orders written into the output

Text in the output that reads like an order to the model gets no special protection (decision 45); security work on this goes to version 3.0. What reaches the caller stays bounded by the rules above: shown lines are copied by code and checked, the mandatory lines are code's, and the retelling is marked "not verified".

### 3.6 Retelling switched off

With the retelling off (decision 76), the model still selects lines, and the result has no retelling line.

### 3.7 Secrets

The feature does nothing about secrets (decision 46): the local model and the raw file see the output as it is. Whatever masks secrets on the strong model's side today keeps doing so (decision 30).

---

## 4. Where it runs

### 4.1 Node and client

- The client sends the whole output to the node (decision 37); the node runs the simple part and the model, and returns the result. On the node the output is held in memory only, during processing, and never stored (decision 47).
- The client keeps the raw file for one day, rotated earlier by the size cap (decision 12; the cap is open: X1).
- **Fallback on the client**: when the configured node cannot be reached, or its model is still loading, the same Rust code runs on the client and returns the plain truncation at once, marked "the model did not take part" (decisions 39, 89). Nothing waits.

### 4.2 Sharing the model

- No cap on concurrent preprocessing jobs per node, and no limit on how much a user's own session slows down: the model is shared equally among requests (decision 13, decision 22, decision 52).
- A request goes to the least busy of the available nodes that serve the model (decision 90). The project's own agents preprocess on the routing nodes, wherever one is free (decision 78).
- During a benchmark round, a GPU test window or a night, the node's busy check still answers "the model did not take part" (proposed default, X5).

### 4.3 Platforms

Windows and Mac nodes support it from their first day; the replay set joins the new-node gate, so a new node does not pass the gate unless the replay check passes on it (decision 95). The code is Rust inside `ragon`, one code base for Linux, Mac and Windows (decision 38).

### 4.4 Air-gapped

Under `/airgapped on` the feature works as usual: the model is local and nothing leaves (decision 16).

---

## 5. Follow-up, cache, journal and the self switch-off

### 5.1 Follow-up

A second question goes to the same raw file (`ragon digest ask <raw-id> "<question>"`) and is preprocessed again without running the command again; `ragon digest lines <raw-id> <from>-<to>` prints lines by number, without the model (decision 49). The plugin's `run` tool answers with the raw id, so the agent can ask again.

### 5.2 Cache

The same output with the same question within one day gets the earlier result from a cache (decision 83). Where the cache lives and its key are open (X3); the default keeps it on the client, beside the raw files.

### 5.3 The journal

A local-only journal of preprocessing failures, never sent anywhere (decision 40): each quote mismatch, wrong format, time-out, and each time the agent opened the raw file after a digest. The journal stores the raw id, the question, the reason and the time; the output itself stays only in the raw file, which expires with it.

### 5.4 Switching itself off

When the agent opens the raw file anyway after three digests in a row of the same kind of command, preprocessing is switched off for that kind of command until the session ends, and the case goes to the journal (decision 50).

---

## 6. Settings

- **On by default once the replay check passes; off before** (decision 4).
- **Three levels** (decision 35): the user setting, the project setting, and the flag for one run (`--off`, or `digest: off` in the run tool). Off at any level wins (design value, §0.3).
- **By command and in the settings file** (decision 73): `ragon digest on|off|status` for the user setting; the same keys in the user's and the project's settings files (`digest.enabled`, `digest.threshold_lines`, `digest.retelling`, `digest.important_patterns`).
- **Important-line patterns** (decision 74): a project may list patterns of lines that are always important there; the stricter rule wins, so the patterns of both the project and the user apply.
- **Threshold**: any value, no lower bound (decision 75).
- **Retelling** on or off (decision 76).

---

## 7. Accounting and statistics

### 7.1 RBRB

- Generated tokens only, as for every model since 2026-11-12 (the owner's revert in rows 18, 21 and 25-28 of §0.1): preprocessing credits the tokens Ragon writes, the quote selection and the retelling, and nothing it reads.
- They are counted together with all other Ragon work, in the one RBRB figure, with no separate line (decision 17). By default only answers that passed the quote check and reached the caller are credited (X6).
- No RBRB target for preprocessing yet: it is measured for one week, then a target is set (decision 79).
- The share grows by the number of preprocessed outputs and Ragon's tasks, never by longer text (decision 80).

### 7.2 What the user sees

- `ragon digest stats` (decision 34): outputs preprocessed, the reduction factor, and the strong-model tokens not spent, in tokens only, no money figures (decision 53).
- A small statistics block in the web UI and the desktop app: outputs preprocessed and savings for the week (decision 56).
- Nothing in Ragon's status line (decision 55).

### 7.3 Public figures

A public claim on the site and in the README, with the project's own measured figures and the counting method, after a month of own use (decision 54). It is a headline item of the 1.0 announcement if that month of figures exists by then; otherwise one line (decision 88). No figure enters a spec, a release note or the site before then.

---

## 8. The replay check

### 8.1 The set

- **100 outputs** for the first switch-on (decision 59), each with a known important line: `{command, exit_code, output, question, important: [{line, text}], kind, source, public}`.
- **Sources** (decision 20): real failures from the project's own work (the four Windows build failures of 2026-11-20, each of which sat on one line among thousands; land-queue rejections; CI failures) plus public build and test logs in several languages.
- **Only public material is committed**, as test fixtures in the repository; the cases from the project's own work stay in the coordinator's private files.
- **Labelling** (decision 60): a strong model labels the important line of each case; the coordinator re-checks one tenth of the labels.
- **Controls** run beside the 100 and are reported with them: green runs (the result must not show an error that is not in the output), machine formats and diffs (must pass through unchanged), outputs a few lines under the threshold (must be unchanged), and one forced case of each fallback.
- Every miss found later (from the journal, §5.3) becomes a new case. Cases are never removed.

### 8.2 The bar

**The important line is kept in 100 % of the replay set** (decision 19), in every one of the replay runs (§0.3), in the digest or in its fallback. One miss fails the check.

### 8.3 When it runs, and what a failure does

- Before every release, and at every change of the model, the engine or the feature's code (decision 57).
- Below 100 % before a release: the release ships with preprocessing off by default; it comes back on when the check passes again (decision 58).
- On every new node, as part of the new-node gate (decision 95).
- Once it has passed, the project's worker agents must use the feature for builds, tests and logs (decision 51).

---

## 9. Stages and release

- **Release** (decision 36): the first release after the replay check passes, and no later than version 1.0.
- **Stage 1**: `ragon digest`, the node interface, and the plugin's automatic mode with the field `what I want to learn` for Claude Code and Codex (decisions 5, 33, 68, 86, 92). Long-running commands are preprocessed at their end (decision 61). The documentation page with three real before-and-after examples (a build, a test run, a service log) ships with it (decision 87).
- **Stage 2**, inside Ragon:
  - The outputs of the commands `ragon` runs (decision 3): a numbered patch in `codex-patches/`, never a change to `codex/`. One SWE slice runs with and without it; it is switched on inside Ragon only if the result is at or above the middle of the band (decision 66).
  - Night Shift gets it with this stage (decision 65).
  - The phone bridges: long command outputs sent to the phone are preprocessed too (decision 67).
  - Streaming preprocessing of long-running commands (decision 61).

---

## 10. Risks

- **Dropping the one line that matters.** The four Windows build failures of 2026-11-20 each sat on one line among thousands. Answered by the mandatory lines (§3.1), the 100 % bar (§8.2) and the re-runs (§8.3).
- **A selection that points at wrong lines.** Answered by code copying the text and the quote check (§3.4).
- **A retelling that is wrong.** It is marked "not verified", comes after the quotes, states only what happened, and can be switched off.
- **Rewriting output a program reads.** Machine formats pass through unchanged (§2.3).
- **A saving that is not real** because agents open the raw file anyway. Measured by the journal (§5.3), answered by the self switch-off (§5.4).
- **Orders hidden in an output.** Not protected in this version (decision 45); bounded by the correctness rules (§3.5).

---

## 11. Non-goals

- A model-free product mode (decision 96).
- A second model on the node (decision 93).
- Money figures, or anything in the status line (decision 53, decision 55).
- Masking secrets, or protecting against orders in the output, in this feature (decision 45, decision 46).
- Preprocessing code search, file listings, code files, diffs or machine formats (decision 29, decision 30).
- Any change to `codex/`.
- New Python in `dreamference/`; if any is needed later, it follows the Python quality ratchet ([DREAMFERENCE_PYTHON_QUALITY.md](./DREAMFERENCE_PYTHON_QUALITY.md)).

---

## 12. Work list

One job each. Two tracks run in parallel, one agent each (decision 70); the order follows decision 69: the replay set first, then the command, then the plugin's automatic mode. **[Ragon]** marks a small item for Ragon itself (decision 71); **[Opus review]** marks an item that needs an Opus review before landing (decision 72); every other item has the ordinary checks. "Product" is this repository; "coordinator tools" is the private coordinator-tools repository.

**Track A: the replay set** (starts at once, decision 99)

| # | Item | Repository | Files | Test | Depends on |
|---|---|---|---|---|---|
| A1 | **Collect the 100 cases**: the four Windows failures, land-queue rejections and CI failures from the project's own work, plus public build and test logs in several languages; a question per case; the controls (§8.1). | coordinator tools (private cases), product (public cases) | private replay folder; `ragon-rs/tests/fixtures/digest/` | a loader test: every case has an output, a question, a kind, and an important line that matches its output | none |
| A2 | **Label the important lines** with a strong model; the coordinator re-checks one tenth (decision 60). | coordinator tools | the replay folder's labels | the re-check's record: every re-checked label confirmed or corrected | A1 |
| A3 | **[Ragon]** Public sample outputs as fixtures for the cleaning and word-list tests (one per rule, one per language). | product | `ragon-rs/tests/fixtures/digest/` | the loader test of A1 | A1 |

**Track B: the command and the node interface** (after the 0.9.9 release, decision 99)

| # | Item | Repository | Files | Test | Depends on |
|---|---|---|---|---|---|
| B1 | **[Ragon]** Cleaning rules: colours, progress bars, control characters, the repeated-line collapse, stream tags (§3.2). | product | `ragon-rs/src/digest/clean.rs` (new) | one test per rule on a sample output | A3 |
| B2 | **[Ragon]** Error word lists: English and the product's six languages (X2), and the project's patterns. | product | `ragon-rs/src/digest/words.rs` (new) | one test line per word | A3 |
| B3 | **The simple part**, one implementation for node and client (C1): mandatory lines, kinds that pass through, empty and binary notes, windows, plain truncation, the 60-line result, text and JSON forms; the quote check's comparison is part of this code and is built and reviewed in B5. | product | `ragon-rs/src/digest/mod.rs` (new) | unit tests per rule; the A1 cases with no model keep every mandatory line | B1, B2 |
| B4 | **[Opus review]** **The node interface** (§2.5): the endpoint on the model gate, sign-in and attribution, memory only, least busy node, the documentation page of the interface. | product | the gate's endpoint, `docs/` | request and answer tests; an unsigned request is refused; nothing is written to disk | B3 |
| B5 | **[Opus review]** **The model step and the quote check** (§3.3, §3.4): prompt, thinking off, windows, one retry, the 20-second limit, the retelling's checks. | product | `ragon-rs/src/digest/model.rs` (new) | a scripted model: a correct answer, one wrong quote, a bad format then a good one, two bad formats, a time-out | B4 |
| B6 | **`ragon digest`** (§2.1): run and pipe modes, raw files for one day with the size cap (X1), follow-up `ask` and `lines`, the cache (X3), `on`, `off` and `status`, the settings and the project's patterns (§6), the no-node message, the client fallback. | product | `ragon-rs/src/digest/cli.rs` (new), `ragon-rs/src/lib.rs` | command tests with a scripted node: unreachable, loading, no node, under the threshold, no question, a successful command that returns at once and later finds the model's reply in `<raw>.digest` | B5 |
| B7 | **The journal and the self switch-off** (§5.3, §5.4). | product | `ragon-rs/src/digest/journal.rs` (new) | three raw reads in a row switch the kind off for the session; nothing leaves the machine | B6 |
| B8 | **Statistics and accounting**: `ragon digest stats`, the generated-token rows in RBRB's sources (X6), the weekly block in the web UI and the desktop app (§7). | product, coordinator tools (RBRB) | `cli.rs`, the web UI, the desktop app | a digest's written tokens count once in the one RBRB figure; no money figure shown | B6 |

**After both tracks**

| # | Item | Repository | Files | Test | Depends on |
|---|---|---|---|---|---|
| C1 | **The replay runner and the check** (§8): `ragon digest replay <dir> --runs 3`, the per-case table, the bar's verdict; wired into the release checklist and the new-node gate. | product | `ragon-rs/src/digest/replay.rs` (new), the release checklist, the node gate | the runner on the public fixtures with a scripted model that misses one line must fail | A2, B6 |
| C2 | **The plugin's automatic mode** for Claude Code and Codex (§2.4): the `run` tool with the field `what I want to learn`, the skill's lines, the user docs. | product | `ragon-rs/delegate/src/mcp.rs`, the plugin's files, `docs/dev/claude-plugin.md` | the delegation server's tool tests; a call without the field is refused | B6 |
| C3 | **The documentation page** with three real before-and-after examples: a build, a test run, a service log (decision 87). | product | `docs/` | the docs tests | C2 |
| C4 | **Switch on by default** once the check passes (decision 4); the project's worker agents use it for builds, tests and logs (decision 51). | product, coordinator tools | the default setting; the worker rules | the check's report at 100 % | C1, C2 |

**Stage 2** (planned after stage 1 ships): D1 inside Ragon, a numbered patch with the SWE slice comparison (decisions 3, 66); D2 Night Shift (decision 65); D3 the phone bridges (decision 67); D4 streaming preprocessing (decision 61).

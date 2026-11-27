# The Ragon community simulator: simulated users who install Ragon, use it and file bugs

**Status:** proposed, 2026-11-27; spec only, nothing built, no repository created. It answers the owner's words of 2026-11-27: "It needs to run entirely on ragon and simulate ragon community installing ragon testing and filing bugs. Community code needs to go into a separate ragon-community repo on github." The open questions in §17 each carry a recommended answer, and the spec assumes those answers until the owner rules.
**Target:** Phase 0 (§16) within a week of the owner's go, on the release after 0.9.9. Advisory in every release candidate from Phase 2. A release gate only after the measured false-report bar of §14.3 holds and the owner says so.
**Builds on:**
- the installers `install.sh` and `install.ps1`, the user site `docs/` (`getting-started.md` first), and the release page of `dreamference/ragon`;
- `ragon exec`, the headless agent ([docs/ragon.md](../docs/ragon.md)), and the model gate's job tag ([RBRB](./DREAMFERENCE_RBRB.md) §4.3);
- the node forge ([RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md), [docs/dev/forge.md](../docs/dev/forge.md)) and its issue API;
- nodes, groups and the primary ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md), [AUTH](./DREAMFERENCE_AUTH.md), [SINGLE_ENDPOINT](./DREAMFERENCE_SINGLE_ENDPOINT.md));
- the 1.0 gate's live check "the first-run experience from a clean machine, following the public docs word for word, since strangers will" ([RELEASE_1.0_SCOPE](./DREAMFERENCE_RELEASE_1.0_SCOPE.md) §11.4), which this simulator makes repeatable;
- the rule that tests never touch the real machine ([docs/dev/testing.md](../docs/dev/testing.md)).

---

## 0. Decisions, stated first

1. **Ragon only, at run time.** Every model call the simulator makes goes to a Ragon model server through a Ragon model gate, with the job tag `community-sim`. The agent is `ragon exec`. No other model, hosted or local, is called while a run lasts, and no other model's output is an input to it. A test checks the runner's configuration: every model endpoint it names is a Ragon gate (§5.1).
2. **It finds defects before real users do.** These are install, upgrade, uninstall, docs and first-use defects on every platform Ragon ships for. **It is not a benchmark:** it publishes no score, compares nothing with any other product, and its counts (§14) are internal.
3. **Nothing pretends to be a person.** Every report says in its first line that a program wrote it. Personas carry role ids, never human names. The simulator never posts to a forum, a discussion board, a chat, a social network or any other public place, and it never files on GitHub without the owner's word (§9.3).
4. **Public docs only.** A simulated user knows only what a stranger can read: the published user site and the release page. It never sees this repository, `AGENTS.md`, `docs/dev/`, the specs, the coordinator's notes or earlier runs' transcripts (§4.2).
5. **Throwaway machines only.** A target is a virtual machine restored from a pristine snapshot, a disposable sandbox, or a dedicated test box reset by a checked script. It is never the owner's own machine, never a member of the real group, and never a box serving production in that hour (§10).
6. **A tested Ragon drives an untested Ragon.** The agent that plays the user (the *driver*) is a released `ragon` that passed its own gate, and its model comes from a production node. The release under test (the *target release*) runs only inside the throwaway machine and on the run's own *community node*. A broken candidate can therefore fail the run's checks, but it cannot break the tester.
7. **Two kinds of finding.** A *runner-observed* finding is mechanical: an exit code, a timeout, a missing expected output, an HTTP error, a blocked connection, a file left behind. A *model-judged* finding is the persona's own judgment, such as a confusing step or an unhelpful error text. A model-judged finding is never above severity S3 and always quotes the doc text or the output it judges (§8.3).
8. **A report becomes a row only after confirmation.** A person or the coordinator's triage confirms a report before it becomes a backlog row, following the standing rule that bugs found become rows. The persona that found a defect confirms its fix by replay (§9.5).
9. **Its code lives in `ragon-community`**, a separate repository on GitHub (§11). Until the owner creates it, the code lives in a private repository of the same name on the node forge. Creating the GitHub repository is the owner's step.
10. **It is advisory first.** Findings become rows, and nothing holds a release until the false-report rate is measured and the owner turns the gate on (§15).

---

## 1. What it is for, and what it is not

| It is for | It is not |
|---|---|
| Running the install, the first use, an upgrade and an uninstall on every shipping platform, literally as the docs say, at every release candidate | A benchmark, a leaderboard or a speed test |
| Finding doc steps that fail, are out of date or are ambiguous | A replacement for the unit suites, the egress audit or the fleet e2e |
| Finding first-use defects a developer never sees: a fresh home, no checkout, no keys on the box, a non-English user, a novice | A load test of the model server |
| Checking, as a side effect, that a fresh install makes no network connection the docs do not name (no telemetry, ever) | A source of public community activity: no posts, stars, forks or comments anywhere |
| Turning the 1.0 gate's "first run from a clean machine" check into a repeatable run | A judge of visual design: it sees text only (§5.4), so layout and colour stay person checks |

---

## 2. Words used here

| Word | Meaning |
|---|---|
| Persona | One simulated user, defined by one file (§3.2) |
| Run | One persona taken once through the loop of §4 against one target release |
| Round | The set of runs started together: one release candidate, or one night |
| Target | The throwaway machine of a run (§6) |
| Target release | The Ragon version under test: a release candidate, a release, or a nightly build |
| Driver | The `ragon exec` that plays the persona. It is a released, gated Ragon on the *controller* |
| Controller | A pool node that runs the runner and the driver. Never the owner's machine |
| Brain node | The production model gate the driver's model calls go to |
| Community node | A node running the target release, in its own group, that client personas use as their node (§6.3) |
| Runner | The deterministic program in `ragon-community` that picks personas, prepares targets, runs the stages, collects evidence and files reports. It calls no model itself |
| Machine tool | The MCP server the driver uses to act on the target (§5.3) |
| Doc step | One numbered instruction on the user site, addressed by a stable id (§4.3) |
| Signature | The deduplication key of a finding (§9.2) |

---

## 3. Personas

### 3.1 Who the simulated users are

A persona is a combination of five things:

- **Platform:** where Ragon is installed (the table below).
- **Skill:** `novice` follows the docs word for word and never goes beyond them. `intermediate` reads an error message and tries the one fix it names. `expert` may also try one fix of its own, which is recorded as a deviation (§4.4).
- **Language:** the language the persona writes its prompts in and expects answers in: English, Korean or Simplified Chinese. The user site is English only today, so every persona reads the English docs; a non-English persona also visits the site's landing page in its language where one exists.
- **Goal:** what it installed Ragon for (§3.3).
- **Patience:** the wall time and the retries per step it allows before it gives up.

**Platforms.** A persona attempts only what the target release's public docs offer for its platform. A platform the docs do not offer yet is still run: the persona then checks that the installer refuses plainly and points to what is supported, and that check is its success.

| Platform | Role today (0.9.9 docs) | Target (§6) | Clean level |
|---|---|---|---|
| GB10 node, 128 GB | node (`install.sh`, default on a GB10) | a dedicated pool GB10, reset by script | footprint-clean (§6.2) |
| Entry-level node, 64 GB | in the 1.0 gate on every platform (owner, 2026-11-23); not offered by the 0.9.9 docs | the 64 GB Mac mini, in a dedicated account | footprint-clean |
| Mac node, 128 GB | not offered (`install.sh` refuses a Mac as a node) | the Mac Studio, in a dedicated account | footprint-clean |
| Mac client | client (`install.sh`, plus the desktop app from the dmg) | a macOS virtual machine on the Mac mini | snapshot-clean |
| Windows client | client, unsigned preview (`install.ps1`) | Windows' built-in disposable sandbox, or a virtual machine checkpoint, on a Windows box | snapshot-clean |
| Windows node (AMD) | in the 1.0 gate; not offered by the 0.9.9 docs | the AMD Windows box, in a dedicated account | footprint-clean |
| Linux client, arm64 | client (`install.sh --role client`) | a virtual machine on a pool GB10 | snapshot-clean |
| Linux client, x86-64 | client | a virtual machine on a Windows box (x86-64) | snapshot-clean |

### 3.2 The persona file

One TOML file per persona, `personas/<id>.toml` in `ragon-community`. The runner refuses a file that fails the schema, and the schema test covers every field.

| Field | Example | Rule |
|---|---|---|
| `id` | `mac-client-novice-ko` | `<platform>-<skill>-<language>[-<n>]`; a role id, never a human name |
| `platform` | `mac-client` | one of §3.1's rows |
| `skill` | `novice` | `novice`, `intermediate` or `expert` |
| `language` | `ko` | `en`, `ko` or `zh-Hans` |
| `about` | "A backend developer who has never run a local model; uses the desktop app first" | at most 300 characters; no name, employer or place |
| `goal` | `explain-repo` | a goal id from §3.3 |
| `starts_from` | `fresh` | `fresh`, or `previous-release` for an upgrade run |
| `patience` | `{ minutes = 60, retries_per_step = 1 }` | at most 120 minutes; retries 0 to 3 |
| `stages` | `["discover", "install", "first_use", "task", "uninstall", "report"]` | a subset of §4.1, in that order |
| `gate` | `true` | the persona is in every release candidate's round |
| `seed` | `17` | the driver's sampling seed, so a replay takes the same path where the model allows |
| `min_ragon` / `max_ragon` | `"0.9.10"` / absent | the target releases it applies to |

### 3.3 Goals

| Goal id | What the persona tries | Success, checked by the runner |
|---|---|---|
| `say-hi` | the docs' first check (`ragon exec "say hi"`, or the app's first message) | an answer arrives within the docs' stated time |
| `explain-repo` | asks Ragon, in its language, to explain a small fixture repository | the answer names at least two of the fixture's real modules, and is in the persona's language (§5.5) |
| `fix-test` | asks Ragon to make a failing fixture test pass | the fixture's test command exits 0, and no file outside the fixture changed |
| `night-shift` | queues a Night Shift task and reads the morning report as the docs describe | the report file exists with the docs' named sections (the clock is advanced for the run, never waited for) |
| `client-pairing` | installs a client and uses the community node, as the client docs say | `ragon node list` names the community node and a turn completes through it |
| `web-ui` | opens `ragon web` and asks one question | the page's answer region holds a reply (read as text, §5.4) |
| `upgrade` | installs the previous release, then upgrades as the docs say | the new version is reported everywhere the docs say to check, and the persona's earlier session is still listed |
| `uninstall` | removes Ragon as the docs say | the platform's footprint check (§6.2) finds nothing left beyond what the docs say stays |

The fixture repositories are small projects written for the simulator and kept in `ragon-community/fixtures/`. They are never third-party code.

### 3.4 How many, and how they are chosen

- **The first set has 12 personas** (§3.5). Every shipping platform has at least one, and at least two personas each are Korean and Chinese.
- **A release candidate's round** runs every persona with `gate = true` (8 at first) and two more chosen by rotation.
- **A nightly round** runs as many personas as the free targets allow (§13.2). Each slot goes to the persona that has gone longest without a run, after a boost for platforms whose files changed since the last round (a path-to-platform table in the runner, for example `install.ps1` to Windows).
- **The choice is deterministic** from the round's id, so a round can be repeated exactly.

### 3.5 The first set

| Id | Platform | Skill | Lang | Goal | Gate |
|---|---|---|---|---|---|
| `gb10-node-novice-en` | GB10 node | novice | en | `say-hi`, then `explain-repo` | yes |
| `gb10-node-expert-en` | GB10 node | expert | en | `night-shift` | no |
| `gb10-node-intermediate-ko` | GB10 node | intermediate | ko | `fix-test` | yes |
| `gb10-node-upgrade-en` | GB10 node | expert | en | `upgrade` | yes |
| `linux-arm64-client-novice-en` | Linux client, arm64 | novice | en | `client-pairing`, then `uninstall` | yes |
| `linux-x86-client-intermediate-zh` | Linux client, x86-64 | intermediate | zh-Hans | `client-pairing`, then `explain-repo` | yes |
| `mac-client-novice-ko` | Mac client | novice | ko | `client-pairing`, then `web-ui` | yes |
| `mac-client-expert-en` | Mac client | expert | en | `fix-test` | no |
| `windows-client-novice-en` | Windows client | novice | en | `client-pairing`, then `say-hi` | yes |
| `windows-client-intermediate-zh` | Windows client | intermediate | zh-Hans | `explain-repo` | yes |
| `entry-node-intermediate-en` | Entry-level node | intermediate | en | `say-hi` (or the plain refusal, §3.1) | no until the docs offer it |
| `mac-node-expert-en` | Mac node | expert | en | `say-hi` (or the plain refusal) | no until the docs offer it |

---

## 4. The loop of one simulated user

### 4.1 Stages

A run is a fixed sequence of stages. **Each stage is one fresh `ragon exec`** with a short brief and the run's journal, never one long session (§5.2). The runner moves between stages. A stage ends at its success marker, its time cap or its give-up, whichever comes first.

| Stage | The driver | The runner | Cap |
|---|---|---|---|
| `prepare` | (none) | restores or resets the target (§6), checks the clean level, starts the target's network rules (§10.2), records the versions of the target machine | 15 min (GB10: 30) |
| `discover` | starts at the site's front page and the release page; chooses the doc path for its platform and goal; writes it to the journal as a list of doc-step ids with the quoted text | serves pages through `fetch_doc` only (§4.2) | 10 min |
| `install` | follows each doc step on the target, literally, in order | records every command, its exit code, the tail of its output and its time | 45 min (node: 90) |
| `first_use` | does the docs' first-use check (sign-in or "Create an account", `ragon exec "say hi"`, the app's first window) | checks the success marker | 10 min |
| `task` | the persona's goal (§3.3) in its language | checks the goal's success | 30 min |
| `upgrade` / `uninstall` | as the docs say | footprint check | 20 min each |
| `report` | drafts findings from the journal (§8) | validates, verifies (§8.4), scrubs (§10.4), deduplicates and files (§9) | 10 min |
| `collect` | (none) | copies the evidence, then destroys or resets the target | 10 min |

A run's wall time is at most the persona's `patience.minutes` plus `prepare` and `collect`. The runner kills a stage at its cap and records a `timeout` finding, which is runner-observed.

### 4.2 What the persona may read

- **Allowed:** the published user site and the release page (with its assets) of the target release, fetched through the machine tool's `fetch_doc`. The site is the published one, or for a candidate that is not published yet, the candidate's built site served by the runner at the public URL's paths. Allowed too: what the target machine itself shows, meaning command output, installer prompts and the app's text.
- **Never:** this repository, `docs/dev/`, `specs/`, `AGENTS.md`, issues and reports, the coordinator's files, earlier runs, or the web at large. The driver's workspace holds only the persona file, the stage brief and the journal.
- **Testable:** a stage that asks `fetch_doc` for a URL outside the allowlist gets a refusal, and the runner records it. The driver's own shell has no network: a check at `prepare` runs a fetch from the driver's shell and expects it to fail.

### 4.3 Doc steps have ids

For a report to point at the exact instruction that failed, every numbered instruction on the user site's install, first-use, client, upgrade and uninstall pages gets a stable anchor id (for example `gs-quick-1`). This is the one change this repository makes for the simulator (§11.2). A test in this repository checks that every numbered step on those pages has a unique id, and that an id, once published, is never reused for a different step.

### 4.4 Following the docs literally

- A **novice** runs exactly the commands the docs show, with the placeholders the docs explain filled in, and nothing else. When a step fails, it may retry once if the docs say to retry. After that it gives up the stage.
- An **intermediate** may also run the one command an error message names (for example "run `ragon-admin host setup`").
- An **expert** may also try one fix of its own per failing step. Every such command is a **deviation**, marked as one in the journal and in any report. A success reached through a deviation counts as a docs defect (the docs did not get the persona there), never as a pass.
- **Every persona answers an installer's question** with the docs' recommended answer, or with the default when the docs name none. It never enters a real password, key or account (§10.3).

### 4.5 What a persona notices

| Finding | Kind | Example |
|---|---|---|
| A doc command exits non-zero | runner-observed | `install.sh` exits 1 at its signature check |
| An expected output is missing | runner-observed | the docs show `ready`, and the answer never says it |
| A stage exceeds its cap | runner-observed | the model download takes 70 minutes, against the docs' "about 20 minutes" |
| A doc page or link is missing | runner-observed | a 404 on a link in `getting-started.md` |
| A command the docs name does not exist | runner-observed | `ragon: unknown subcommand` |
| A connection the docs do not name | runner-observed | a blocked connection from a Ragon process at first start (§10.2) |
| A file, unit, container or account left after uninstall | runner-observed | the footprint check lists a user unit |
| A version mismatch | runner-observed | the app's About window and `ragon --version` disagree |
| The answer is in the wrong language | runner-observed (§5.5) | a Korean prompt gets an English answer |
| A step is ambiguous, or the next step cannot be found | model-judged | two pages give different commands for the same step |
| An error message does not say what to do | model-judged | the persona quotes the message and the doc step |

---

## 5. Running entirely on Ragon

### 5.1 The model and the agent

| Part | What | Rule |
|---|---|---|
| Model | the production default model of the driver release, served by `ragon-serve` | the driver reads the served model and its context from `/v1/models`, as every launcher does |
| Brain node | a production node's model gate, chosen by the round from the nodes free in that hour; never the community node and never the owner's machine | every request carries the job tag `community-sim/<round>/<run>` |
| Agent | `ragon exec` of the driver release, with the stage brief on stdin closed (`< /dev/null`) and the machine tool as its only tool server | the driver release is the newest release that passed its gate; the runner refuses a pre-release as driver |
| Sampling | the persona's `seed`, the model's default temperature | recorded in the run record |
| Checks of the configuration | a test reads the runner's resolved configuration and fails if any model endpoint is not a Ragon gate, or if any key other than the brain node's service key is present | §0.1 |

### 5.2 Budgets

The served context is 196,608 tokens per session today (`docs/models.md`). Brain nodes share their sessions with other work, so the simulator budgets well below that.

| Budget | Value | When it is hit |
|---|---|---|
| Context per stage | at most 60,000 tokens of prompt (brief + journal + tool output) | the machine tool returns at most 4,000 characters per call, with the rest saved as evidence; the journal keeps a 2,000-token summary per finished stage |
| Tokens per run | 2,000,000 (input + output), read from the gate's usage for the run's tag | the runner ends the stage, records `budget`, and continues with `report` |
| Wall time per run | the persona's patience (at most 120 min) plus `prepare` and `collect` | as §4.1 |
| Sessions on the brain node | at most 2 runs at once per brain node | the round waits for a free slot; it never takes a third |
| Per round | release candidate: about 10 runs, about 20 M tokens; night: as many as §13.2 allows | the round's summary lists runs not started |

### 5.3 How screens and terminals are driven, without another model

Everything the driver sees is **text**. The machine tool turns each surface of the target into text and each action into a command:

| Tool call | What it does on the target | How, per platform |
|---|---|---|
| `run(cmd, timeout)` | runs one command in a shell as the target's user; returns the exit code and the output tail | Linux and macOS over SSH into the virtual machine or the account; Windows through the sandbox's or the virtual machine's remote shell |
| `term_open(cmd)`, `term_keys(id, keys)`, `term_screen(id)` | drives an interactive program (the `ragon` TUI, an installer prompt) in a pseudo-terminal and returns the screen as text | a terminal multiplexer on the target, read with its capture command, as the live slash-command suite reads the TUI today |
| `ui_tree(app)`, `ui_act(app, element, action)` | lists a GUI window's accessibility tree as indented text (role, label, value), and clicks or types into an element by its path | macOS's accessibility interface, Windows' UI Automation and Linux's accessibility bus, each read by a small helper that `ragon-community` ships and installs into the target at `prepare` |
| `app_dom(app)` | for the desktop app and `ragon web`, the page's text and form fields | the app's own debugging port inside the target, or a scripted browser in the target; read-only except for `ui_act` |
| `fetch_doc(url)` | a page of the user site or the release page, as readable text | `ragon-fetch` on the controller, under the allowlist of §4.2 |
| `note(kind, text)` | appends to the journal | runner-side |

The helpers and the debugging port exist only inside the throwaway target. The runner installs them at `prepare`, records them in the run record, and never counts them in the footprint check.

### 5.4 What Ragon cannot do today, and the way around it

| Limit | Effect | Way around it, with no other model |
|---|---|---|
| The served model reads text only here (the launcher enables images only when the server reports image understanding; the desktop-parity spec has no computer use) | it cannot look at a screenshot | every surface is read as text (§5.3). Visual-only defects (layout, overlap, colour, icons) are **out of scope** and stay person checks. A window whose accessibility tree is empty is a finding (an accessibility defect), not a reason to guess |
| A stage's context is finite, and long sessions drift | a 90-minute install cannot sit in one context | staged runs, each a fresh `ragon exec` with the journal (§4.1); tool output capped and saved as evidence (§5.2) |
| The model can loop, or claim success without it | a false pass or a lost hour | the runner, not the model, decides success (§3.3); the same command three times with the same output ends the stage with a `stuck` finding |
| The model can invent a defect | a false report | runner-observed evidence is required for S1 and S2; the verifier stage (§8.4); the false-report measure (§14) |
| Sampling varies | a replay may take another path | the persona's seed; the replay of §9.5 repeats the failing step's commands first, before it lets the model choose |
| No Korean user docs, and Chinese only on the site's landing page | non-English personas read English | the persona reads English and speaks its language to Ragon. A missing translation is a known gap (§8.5), not a report |
| Time-based features (Night Shift's morning report) | a run cannot wait for a night | the runner sets the target's timer to fire within the run, through the documented setting for the window. Without such a setting the goal is `not applicable`, not a pass |

### 5.5 Mechanical checks the runner does itself

These need no model. The runner does them so that the persona's judgment is never the only evidence:

- **the answer's language:** the share of Hangul or Han characters in an answer to a Korean or Chinese prompt must be at least 50 % of the letters (code blocks excluded);
- **success markers**, per goal (§3.3);
- **the footprint**, per platform (§6.2);
- **network**: every connection attempt the target's rules block, with the process that made it (§10.2);
- **versions**: every version string the docs say to check, compared with each other and with the target release.

---

## 6. Throwaway machines

### 6.1 Per platform

| Platform | Target | Host | Reset |
|---|---|---|---|
| Linux client, arm64 | a virtual machine from a pinned Ubuntu LTS cloud image, with a user `sim` and no Ragon | a pool GB10 that is free in that hour, CPU only, memory capped at 8 GB | restore the pristine snapshot; checked by the snapshot's hash |
| Linux client, x86-64 | the same, x86-64 | a Windows box (x86-64) through Windows' built-in virtual machine host | restore the checkpoint |
| Mac client | a macOS virtual machine with a user `sim`, through macOS's built-in virtualisation | the Mac mini | restore the pristine copy |
| Windows client | Windows' built-in disposable sandbox (gone when closed), or a virtual machine checkpoint where the sandbox cannot run the installer | a Windows box | close the sandbox, or restore the checkpoint |
| GB10 node | a dedicated pool GB10 taken out of the group for the round, user `sim` | itself | the footprint reset of §6.2 |
| Entry-level node | the Mac mini, account `sim` | itself | the footprint reset |
| Mac node | the Mac Studio, account `sim` | itself | the footprint reset |
| Windows node | the AMD Windows box, local account `sim` | itself | the footprint reset |

Every target's user is named `sim`, and every virtual machine's host name is `sim-<platform>`. That way no evidence carries a real user name, home path or host name (§10.4).

### 6.2 Clean levels and the footprint check

- **Snapshot-clean:** the target is a fresh copy of a pristine image. Nothing of an earlier run can remain. The image is pinned by hash, and the run record names it.
- **Footprint-clean:** a real machine with a GPU, where the operating system and its drivers stay. The reset removes everything the installer writes. `ragon-community` keeps a **footprint list** per platform, derived from `install.sh` and `install.ps1`: paths under the home, user and system units, containers and images, the docker group membership, the advertised service file, the host-setup files, the firewall rules, the app. The reset removes each one, then the footprint check lists any that remain, and a non-empty list stops the run before `install`.
  - **A test in `ragon-community`** diffs the footprint list against the installers of the target release. A path the installer writes that is not on the list fails the test, so the list cannot fall behind the installer.
  - **Model weights:** on a node target the reset moves the model cache aside instead of deleting it. One run per release candidate does the real download the docs describe, and the others start with the cache restored. The deviation is printed in each report's header (§17, question 7).
- **What footprint-clean does not cover**, written in every report from such a target: the operating system's own updates, the GPU driver, and packages that host setup installs from the operating system's archive.

### 6.3 The community node and its own group

Client personas need a node, and node personas need a primary. **A target never touches the real group.**

- **The community node** is a pool GB10 that runs the target release as its own primary, in a group of its own, with its own accounts database. It is never paired with the real group's nodes, and it never advertises on the LAN where the real clients look. It advertises only on the targets' isolated network (§10.2).
- **Every account a persona creates** (the "Create an account" step) is created on the community node, never on a real primary. Behind this rule is an earlier incident: a throwaway account created on a real primary became its only administrator and could not be removed.
- **At the end of a round** the community node's accounts, sessions, pairings and forge data are deleted, and its next round starts from an empty group. A check at `prepare` reads the community node's group and refuses a run if any member is a real node.
- **A node persona's own target** is its own primary, as on a fresh install. It is never paired to the community node unless its goal says so.

---

## 7. The run record

Every run writes one directory, kept on the controller and copied to the forge's results repository (§11.3):

| File | Holds |
|---|---|
| `run.json` | round, run id, persona id and file hash, target release, driver release, brain node (by role id, never host name), target image hash, clean level, seed, start, end, tokens, outcome (`pass`, `fail`, `gave_up`, `not_applicable`, `aborted`) |
| `journal.md` | the stage summaries and the persona's notes |
| `commands.jsonl` | every machine-tool call: stage, command, exit code, duration, output tail (scrubbed) |
| `network.jsonl` | every blocked connection: time, process, destination name |
| `findings.json` | the validated findings of §8 |
| `evidence/` | full outputs, accessibility trees, install logs, `ragon doctor` output, all scrubbed |

A run record is never published. It stays on the forge (§11.3).

---

## 8. Bug reports

### 8.1 Format

A report is Markdown with a TOML header, rendered as an issue on the forge. The runner fills the header and the evidence. The driver writes only `summary`, `expected`, `actual` and, for model-judged findings, `why`.

```text
This report was written by the Ragon community simulator, a program. No person ran these steps.

+++
simulated = true
signature = "a3f9c2e1b7d4"
severity = "S2"
kind = "runner-observed"          # or "model-judged"
persona = "mac-client-novice-ko"
platform = "mac-client"
target_release = "0.9.10-rc.1"
driver_release = "0.9.9"
round = "rc-0.9.10-rc.1"
run = "r0007"
stage = "install"
doc_step = "gs-client-3"
clean_level = "snapshot-clean"
deviations = 0
seen = 1
+++

## Summary
## Doc step (quoted)
## Expected
## Actual
## Steps to reproduce (the exact commands, in order)
## Evidence (output tails; full files in the run record)
## Versions (target OS and version, ragon --version, app version, installer SHA-256)
```

### 8.2 Severity

| Severity | Meaning | Kind allowed |
|---|---|---|
| **S1** blocker | following the docs on an offered platform, install or first use cannot complete; or data loss; or a connection the docs do not name; or a secret in output | runner-observed only |
| **S2** major | it completes only through a deviation or an undocumented step; a documented feature fails; upgrade loses the user's state; uninstall leaves undocumented files | runner-observed only |
| **S3** minor | a doc step out of date or ambiguous, an unhelpful error message the persona recovered from, a wrong version string, a stage over its documented time | either |
| **S4** note | a suggestion | never filed; collected in the round summary |

### 8.3 Model-judged findings

A model-judged finding must quote the doc text or the output it judges, word for word. The runner checks that the quote occurs in the evidence, and a finding whose quote is not found is dropped and counted as `unsupported`. It is at most S3.

### 8.4 The verifier stage

Before filing, every finding goes to a **second, fresh `ragon exec`** on the brain node. It sees only the report and its evidence (no journal, no persona) and answers one question with `supported` or `not supported` plus a reason: does the evidence show what the report claims? A `not supported` finding is not filed. It is kept in the run record and counted (§14). This is Ragon checking Ragon in a separate context, and it calls no other model.

### 8.5 Known gaps

`ragon-community/known-gaps.toml` lists things the docs already say are missing (for example "no Korean user docs", "the Windows client is an unsigned preview"). A finding that matches a known gap by signature or by doc step is not filed. An entry carries the release it was added in and is reviewed at every release.

---

## 9. Filing, deduplication, rows and fixes

### 9.1 Where reports go

| Place | When | By whom |
|---|---|---|
| **The node forge**: a private repository `community/reports` on the forge of record | always, from Phase 1 | the forge service account `community-simulator`, with the `write:issue` scope on that repository and no other right |
| **GitHub issues** of `dreamference/ragon-community` | only on the owner's word, and not before 1.0 (§17, question 3) | the bot account the owner names |
| **GitHub issues** of `dreamference/ragon` | never: real users file there, and simulated reports would look like community activity | (none) |

Labels on the forge: `simulated`, `sev:S1` … `sev:S3`, `platform:<p>`, `kind:<k>`, `sig:<signature>`, `release:<target>`, and one triage state of §9.4.

### 9.2 Deduplication

- **The signature** is the first 12 hex characters of SHA-256 over: the platform family, the stage, the doc-step id (or the command when there is none), the error class (exit code, HTTP status, `timeout`, `blocked:<destination>`, `residue`, `language`, `judged`), and the first error line normalised (numbers, paths, hashes, times and version strings replaced by placeholders). The normaliser has table tests.
- **Before filing**, the runner searches the repository's open issues with the label `sig:<signature>`. If one matches, it adds a comment with the new run (release, persona, run id) and raises `seen`, and no new issue is opened.
- **The same doc step with a different signature** is a new issue, linked as related to the open ones on that step.
- **A closed issue whose signature reappears** on a release after its fix is reopened with a `regressed` label, never filed anew.
- **Triage merges what the signature missed**, and the merge is recorded as a `duplicate` state (§9.4) so that the measure counts it.

### 9.3 Truthfulness, in every place a report appears

- The first line of the body is the sentence of §8.1, and the title starts with `[simulated]`.
- The account's display name is "Ragon community simulator (program)". It has no avatar of a person and no profile text beyond that sentence.
- Personas are named by role id only. No report says "I" as if a person ran the steps; reports say "the persona".
- The simulator never comments outside its own reports, never reacts or stars, and never opens discussions. It never registers an account on an outside service.

### 9.4 From report to row

| State | Set by | Then |
|---|---|---|
| `new` | the runner | waits for triage |
| `confirmed` | a person, or the coordinator's nightly review job after reproducing it from the report's steps | **a backlog row** is opened (what, why, size, dependencies, acceptance = the replay of §9.5 passes, status) and linked both ways |
| `false` | the same | counts as a false report (§14); the reason is recorded (`not a defect`, `wrong evidence`, `environment`, `known gap`) |
| `duplicate` | the same | merged into the open issue |
| `environment` | the same | the target, not Ragon, was at fault (an image problem, the host's network); a fix row goes to `ragon-community` |
| `fixed-pending` | the row's landing | waits for the replay |
| `closed` | the runner, after the replay passes | with the line "confirmed fixed in <release> by <persona>, run <id>" |

An S1 report in `new` state sends one coordinator inbox line at once. It never sends an alert to the owner by itself.

### 9.5 Confirming a fix

When a row lands, the next round that runs a build containing it **replays** the report:
1. the same persona, seed and platform, on a fresh target;
2. first the exact commands of the report's steps, without the model choosing, up to the failing step;
3. then the stage again under the driver, as a normal run.

**Pass:** the signature is not seen and the stage reaches its success marker; the runner closes the report (§9.4). **Fail twice:** the row is reopened with both run records linked.

---

## 10. Safety

### 10.1 Machines

- **Allowlist of targets.** The runner reads `targets.toml` from the controller's private configuration. A target is used only if its machine id is on the list. The list is not in any repository, because it would name machines.
- **Denylist.** The runner refuses to act on a machine whose node id or machine id is the owner's machine, any member of the real group outside the round's reservation, or the controller itself. The check runs at `prepare` and before every `run` on a footprint-clean target. A test plants each denied id and expects a refusal.
- **The owner's machine is never a target, a controller, a brain node or a community node.**
- **GPU.** A node target takes the box's GPU lock for the whole run and releases it at `collect`. It follows the fleet's day and night split (§13.2).
- **Memory.** A virtual machine on a pool GB10 is capped at 8 GB and 4 cores. The host's model server, when one runs, keeps its memory reserve: the runner refuses a virtual machine that would cross it.

### 10.2 Network

| From the target to | Allowed |
|---|---|
| the release page and its assets (the installers' GitHub endpoints) | yes |
| the published user site | yes |
| the container registry and the model host the docs name | yes |
| the operating system's package archive (host setup) | yes |
| the community node's ports the docs name | yes, on the isolated network |
| the real group's nodes, the forge, the controller (except the machine tool's channel), the owner's machine, the LAN | **no** |
| anything else | **no, blocked and logged** with the process that tried it |

A blocked connection from a Ragon process is an S1 finding (no telemetry, ever). A blocked connection from the operating system or another program is logged in the run record but not filed.

### 10.3 Secrets

- A target holds no key, token or password of ours. A persona's account passwords on the community node are generated per run, kept only by the runner, and destroyed at `collect`.
- Features that need a real outside account (mail, the phone messengers, remote access) are out of scope for personas, unless the owner provides test accounts (§17, question 10).
- The brain node's service key lives only on the controller, outside the driver's workspace. The driver never sees it, and a test checks that the stage brief and the journal never contain it.

### 10.4 Scrubbing and anonymity

Before anything is filed or copied, the runner:
1. replaces every value of the run's secret list with `[masked]`;
2. masks key and token patterns (`sk-ragon-…`, bearer tokens, private-key blocks);
3. replaces the controller's and hosts' names, LAN addresses and home paths with placeholders (`<controller>`, `<lan-ip>`, `~`);
4. runs the same anonymity denylist this repository's anonymity check uses. A report with any match is **held** for a person and is not filed.

A test feeds planted secrets, a host name, a LAN address and a home path through the scrubber and expects none in the output. Anything that leaves the forge for GitHub (§9.1) passes the same check again.

---

## 11. The repositories

### 11.1 `ragon-community` (on GitHub; created by the owner)

| Path | Holds |
|---|---|
| `personas/` | one TOML file per persona (§3.2) and its schema |
| `runner/` | the runner (Python, one class per file, the same quality ratchet as this repository) |
| `machines/` | one adapter per target kind (virtual machine, sandbox, footprint reset), and the footprint lists |
| `tool/` | the machine tool (MCP server) and the accessibility helpers |
| `briefs/` | the stage briefs given to the driver, one file per stage |
| `fixtures/` | the small sample projects the goals use |
| `templates/` | the report template, the labels, the round summary |
| `known-gaps.toml` | §8.5 |
| `compat.toml` | the driver releases and target releases this version supports |
| `tests/` | offline tests (§11.4) |
| `docs/` | how to run a round, the measures' definitions |

**Not in it:** run records, reports, target lists, machine names, the brain node's key, or any result naming a machine. Results stay on the forge (§11.3).

**Licence:** AGPL-3.0-or-later, as Ragon (§17, question 2).

### 11.2 What stays in this repository

- this spec, and its line in `specs/README.md`;
- the doc-step ids on the user site (§4.3), with their test;
- one line in each release spec's gate list: the round's id, its outcome and its open S1 and S2 counts (§15);
- a pointer in `docs/dev/testing.md` once Phase 1 has run.

This repository never imports from `ragon-community`, and nothing in its suite runs the simulator.

### 11.3 On the forge

- `community/ragon-community`: the code, until the GitHub repository exists, and then a mirror of it.
- `community/reports`: the issues (§9).
- `community/results`: the run records and round summaries, kept for 180 days. Summaries are kept for good, because they hold no evidence.

### 11.4 How `ragon-community` is tested

Following the rule that tests never touch the real machine, every test runs offline with fakes:

| Test | What it checks |
|---|---|
| persona schema | every persona file loads; a bad field is refused with its name |
| stage machine with a fake machine and a recorded driver | stages run in order, caps end a stage, `stuck` is detected, success comes only from the runner's marker |
| signature normaliser | a table of 30 error lines, each with its expected signature; two lines differing only in a path or a number share one |
| deduplication against a fake forge | a match comments, a reappearance after close reopens, a new step files |
| scrubber | §10.4's planted values never survive |
| footprint list against the installers | §6.2's diff |
| allowlist and denylist | §4.2 and §10.1's refusals |
| configuration is Ragon-only | §5.1 |
| language check | Hangul, Han and Latin samples, code blocks excluded |

**Live smoke:** each phase ends with one live run on a real target (§16). This is never part of the suite.

**CI** for `ragon-community` runs the offline tests only. It never reaches a node.

### 11.5 Versions

- `ragon-community` has its own version (`0.1.0` at Phase 0) and tags.
- `compat.toml` names the driver releases (one or two gated releases) and the oldest target release it supports. The runner refuses an unsupported pair.
- A persona's `min_ragon` / `max_ragon` limit it to the target releases it applies to.
- A doc-step id a persona's goal needs, missing from the target release's docs, makes the goal `not_applicable` for that release. This is listed in the round summary and is not a report.
- The release spec's gate line (§15) names the `ragon-community` commit that ran the round, so a round can be rerun with the same code.

---

## 12. Measures

| Measure | Definition | Target to make the gate blocking |
|---|---|---|
| Confirmed defects per round | reports set `confirmed` within 7 days, per round | (reported, no target) |
| False-report rate | `false` ÷ (`confirmed` + `false`), over the last three release candidate rounds | at most 20 %, with at least 20 triaged reports |
| Unsupported rate | findings dropped by §8.3 or §8.4 ÷ all findings | (watched; rising means the briefs need work) |
| Duplicate rate | `duplicate` ÷ filed | at most 15 % (the signature works) |
| Time to first success | from `install` start to the `first_use` marker, per persona, median over its last five runs | (reported per platform; a rise of 50 % is a finding of its own) |
| Doc steps that fail | distinct doc-step ids with a runner-observed finding, per round, per page | (reported) |
| Doc coverage | doc-step ids exercised by at least one persona in the last five rounds ÷ all ids on the covered pages | at least 80 % |
| Fix confirmations | replays passed ÷ rows landed | (reported) |
| Run health | runs `aborted` or `environment` ÷ runs | at most 10 % |
| Tokens | the gate's usage for `community-sim/*`, per round and per week | (reported) |

### 12.1 Tokens and RBRB

- **Run-time tokens** carry the job tag `community-sim/…`. They are QA and, on the recommendation of this spec, are **excluded from the RBRB share**, as RBRB §2.1 already excludes the morning QA passes and benchmark nights. They are shown beside the curve's inputs as their own weekly figure. §2.1 does not name the simulator, so this exclusion is a recommendation for the owner (§17, question 8), not an existing rule.
- **Build tokens**, the work of writing `ragon-community`, are development tokens. RBRB §2.1 covers "this repository or its engine", so counting `ragon-community` needs the owner's word. The recommendation is that it counts, with the same trailers on its commits (`Ragon-Tokens`, `Made-with: ragon`).

---

## 13. When it runs

### 13.1 Per release candidate

- **Started:** by the release flow when a candidate's assets are published as a pre-release, before the fleet e2e.
- **Runs:** the gate set and two rotating personas (§3.4), within the candidate's test window.
- **Result:** the round summary goes to the coordinator's inbox and to the release spec's gate line (§15).
- **Budget:** about 10 runs, 4 to 6 hours of wall time with targets in parallel, about 20 M tokens on one or two brain nodes.

### 13.2 Nightly, on free boxes

The fleet's split of days and nights holds. Nights belong to the benchmark slices, and the scheduler decides who gets a box.

| Target kind | When | Where |
|---|---|---|
| Virtual machines and sandboxes (client personas) | nightly, 20:00–08:00 UTC | on the Mac mini, the Windows boxes, and pool GB10s whose night slot the scheduler leaves free. The brain node is a box the scheduler marks as serving |
| Node targets (GB10, entry-level, Mac, Windows node) | in a day lane window, 08:00–20:00 UTC, as a lane takes the GPU lock and releases it by 20:00 | one node target at a time |
| The community node | for the round only, on a pool GB10 the scheduler grants | never during that box's night slot |

A night with no free box runs no round, and the morning summary says so.

### 13.3 On demand

The coordinator may start a round for a branch build, for example to replay a fix before it lands. Such a round is labelled `branch:<name>`. Its reports are filed with `release:branch-<name>` and are never counted in a release's gate line.

---

## 14. Keeping it honest

1. **A test of the tester.** Every release candidate round also runs one **planted defect** on a copy of the docs served only to that run: a doc step with a wrong command. A round that does not report the plant at S2 or above fails its own health check, and its results are marked untrusted.
2. **A clean control.** Once per week a persona runs against the previous, already-released version with no known open defect on its path. A report from that run is checked first in triage, since it is most likely false.
3. **The false-report bar.** The gate stays advisory until the false-report rate of §12 holds over three release candidate rounds with at least 20 triaged reports (§15).
4. **Nothing is tuned against triage silently.** A change to a stage brief, the verifier's question or the signature normaliser is a `ragon-community` commit, and the round summary names the commit it ran.

---

## 15. Its place in the release gate

| Phase | Effect on a release |
|---|---|
| Advisory (from Phase 2 until the bar of §14.3 holds and the owner says so) | the round runs on every candidate; confirmed S1 and S2 reports become rows, exactly as night findings do; **nothing holds a release** |
| Blocking (on the owner's word) | a candidate is not released while (a) any gate persona of an **offered** platform has not reached its `first_use` marker, or (b) a `confirmed` S1 report against it is open. S2 reports never block; they become rows |

The gate line in each release spec reads, for example: `Community simulator: round rc-0.9.10-rc.2 (ragon-community 1a2b3c4), 10 runs, 9 pass, 1 gave up; S1 0, S2 1 open (row <id>); advisory.`

The fleet e2e and the person checks stay as they are. The simulator adds to them and replaces none, until the owner decides that it may replace §11.4's "first run from a clean machine" person check.

---

## 16. Phases

Agent-days are for building only. **L** rows go to Ragon first (the `task` tool, one named test, the RBRB routing rule). **C** rows go to the cheapest model that can do them, with review. A failed L row falls back as RBRB §5.3 says. Rows that change the gates themselves (§15 in a release spec) are never L.

### Phase 0: one persona, one platform, useful in a week (3 agent-days)

**Outcome:** `linux-arm64-client-novice-en` runs end to end against the latest release, with a community node, on a virtual machine on a pool GB10. It produces a run record and Markdown reports in a directory. A person files any real finding by hand. That already repeats, for one platform, the 1.0 gate's "first run from a clean machine" person check.

| Row | What | Check | Who |
|---|---|---|---|
| 0.1 | private repository `community/ragon-community` on the forge, skeleton, CI with the offline tests | the empty suite passes | C |
| 0.2 | persona schema and loader; the first persona file | schema test (§11.4) | L |
| 0.3 | the virtual machine adapter: restore from a pinned image, `sim` user, SSH channel, 8 GB cap | a live restore of the image on one pool GB10, hash checked (recorded, not in the suite) | C |
| 0.4 | the machine tool with `run`, `fetch_doc` (allowlist) and `note` | allowlist test; output cap test | L |
| 0.5 | the stage machine: `prepare`, `discover`, `install`, `first_use`, `report`, `collect`; caps; journal; `stuck` | stage-machine test with a fake machine and a recorded driver | C |
| 0.6 | the report writer (§8.1 format) and the scrubber (§10.4) | scrubber test | L |
| 0.7 | the Ragon-only configuration check (§5.1) | its test | L |
| 0.8 | one live run against the latest release; the run record and any reports to the coordinator's inbox | the run reaches `collect`; findings triaged by a person | C |

### Phase 1: filing, triage and rows (4 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 1.1 | signature normaliser | the 30-line table test | L |
| 1.2 | forge filing with the `community-simulator` account, labels, comment-on-match, reopen-on-regression | deduplication test against a fake forge | C |
| 1.3 | severity rules and the model-judged quote check (§8.2, §8.3) | table test | L |
| 1.4 | the verifier stage (§8.4) | test with a recorded verifier answer; one live check on a planted false finding | C |
| 1.5 | triage states and the row link (§9.4) in the coordinator's nightly review | a fake issue moved through every state | C |
| 1.6 | doc-step ids on the user site's install, first-use, client, upgrade and uninstall pages (this repository) | the docs test: unique ids, never reused | L |
| 1.7 | known gaps (§8.5) | a matching finding is not filed (test) | L |
| 1.8 | the planted-defect self-test (§14.1) | a round with the plant reports it | C |

### Phase 2: every platform, advisory in release candidates (6 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 2.1 | x86-64 Linux virtual machine on a Windows box | live restore | C |
| 2.2 | macOS virtual machine on the Mac mini | live restore | C |
| 2.3 | Windows sandbox target, with checkpoint fallback | live install of the latest release's `install.ps1` | C |
| 2.4 | footprint lists for Linux, macOS and Windows; reset and check scripts | the installer-diff test (§6.2) | L (the test), C (the scripts) |
| 2.5 | the GB10 node target: reservation, GPU lock, footprint reset, model cache moved aside | live reset to an empty footprint on one pool GB10 | C |
| 2.6 | the community node: own group, account wipe, refusal if a real node is a member | test with a fake group; one live round | C |
| 2.7 | the targets' network rules and the blocked-connection log (§10.2) | live: a planted call to an unlisted host is logged with its process | C |
| 2.8 | the allowlist and denylist of targets (§10.1) | the planted-id test | L |
| 2.9 | the release candidate round command and the gate line (§15) | one advisory round on a real candidate | C |
| 2.10 | owner step, prepared as a script with exact commands: the `sim` accounts on the Mac Studio and the AMD Windows box | (the owner runs it) | (owner) |

### Phase 3: interactive and graphical surfaces as text (4 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 3.1 | `term_open`, `term_keys`, `term_screen` for the TUI and installer prompts | test with a fake terminal; live sign-in in the TUI | C |
| 3.2 | macOS accessibility helper (`ui_tree`, `ui_act`) | live: the desktop app's first window as a tree | C |
| 3.3 | Windows UI Automation helper | live: the Windows app's first window as a tree | C |
| 3.4 | `app_dom` for the desktop app and `ragon web` | live: `web-ui` goal passes | C |
| 3.5 | tree and screen output trimmed to the §5.2 cap, with the rest as evidence | cap test | L |

### Phase 4: languages, upgrades, fixes and nights (3 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 4.1 | the Korean and Chinese personas; the language check (§5.5) | language-check test; one live run each | L (test), C (runs) |
| 4.2 | `upgrade` and `uninstall` stages | live upgrade from the previous release on a virtual machine | C |
| 4.3 | the replay of §9.5 | test with a fake forge and a recorded run; one live replay of a fixed report | C |
| 4.4 | nightly rotation through the scheduler (§13.2) | rotation test from a round id | L |
| 4.5 | the measures and the weekly summary (§12) | test against fixture records | L |

### Phase 5: the GitHub repository (1 agent-day, after the owner's step)

| Row | What | Check | Who |
|---|---|---|---|
| 5.1 | owner step: create `dreamference/ragon-community` with the visibility and licence of §17 | (the owner) | (owner) |
| 5.2 | push the forge repository's history, made by the team bot as the organisation's rules say, and keep the forge copy as a mirror | anonymity check over the whole history before the first push | C |
| 5.3 | CI on GitHub: offline tests only | green on the first push | C |

**Total: 21 agent-days,** about 40 % of them L rows. Phase 0 alone is useful, and each later phase is useful without the next.

---

## 17. Open questions for the owner

Each question carries the recommended answer first. The spec assumes that answer until the owner rules otherwise.

1. **Visibility of `ragon-community`.** *Recommended: private on GitHub until 1.0, public from 1.0.* A public repository full of simulated users before 1.0 sits too close to the rule of no community posts before 1.0.
2. **Licence.** *Recommended: AGPL-3.0-or-later, as Ragon.*
3. **Filing on GitHub.** *Recommended: no GitHub filing before 1.0. After 1.0, only into `ragon-community`'s own issues, and only on your word. Never into `dreamference/ragon`'s issues.* Reports stay on the node forge until then.
4. **The bot account.** *Recommended: on the forge, a service account `community-simulator` limited to `write:issue` on `community/reports`; on GitHub, the quality team's bot, so that no new identity is needed.*
5. **The gate.** *Recommended: advisory until the false-report rate is at most 20 % over three release candidate rounds with at least 20 triaged reports; then blocking for confirmed S1 reports and for first-use failures on offered platforms only.*
6. **Boxes.** *Recommended: client targets nightly on the Mac mini, the Windows boxes and pool GB10s whose night slot is free; node targets only in day lane windows; one pool GB10 as the community node per round, granted by the scheduler.*
7. **Model downloads in node runs.** *Recommended: one real public download per release candidate round; every other node run starts with the model cache restored, and its report says so.*
8. **RBRB.** *Recommended: run-time tokens excluded from the share as QA, shown as their own figure; build tokens in `ragon-community` counted as development tokens, with the same trailers there.*
9. **Accounts on the Mac Studio and the AMD Windows box.** *Recommended: you run one prepared script per box to create the `sim` account (it needs administrator rights the jobs do not have); the jobs never get those rights.*
10. **Features that need outside accounts** (mail, the phone messengers, remote access). *Recommended: out of scope for personas until 1.0; later only with dedicated test accounts you create.*
11. **Pictures.** *Recommended: no image reading of any kind, not even the OCR the product ships for messengers. Visual checks stay person checks until the served model reads images, and then they become their own phase.*
12. **Replacing the person check.** *Recommended: the simulator does not replace the 1.0 gate's "first run from a clean machine" person check for 1.0. It runs beside it, and you decide after 1.0 whether it may replace it.*

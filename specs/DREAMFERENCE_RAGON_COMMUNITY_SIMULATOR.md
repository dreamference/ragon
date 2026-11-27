# The Ragon community simulator: simulated users who install Ragon, use it, talk about it and file bugs

**Status:** proposed, 2026-11-27, rewritten the same day with the owner's 103 answers and the later ruling on static pages. This is a spec only: nothing is built yet. The repository `dreamference/ragon-community` exists (public, AGPL-3.0, holding only its licence). The owner's words: "It needs to run entirely on ragon and simulate ragon community installing ragon testing and filing bugs. Community code needs to go into a separate ragon-community repo on github." One decision is still open: the site's notice (§1, §20, question 1). Nothing is published until the owner answers it: no content commit to the public repository, no deploy (§10.4).
**Target:**

| Phase | What | Due |
|---|---|---|
| 0 | the offline core, the three community machines enrolled, one live run | 4 December |
| 1 | ten users, Linux and node, terminal only, reports to the site, on the 0.9.10 candidates | 12 December |
| 2 | Mac and Windows users, Korean and Chinese users and their conversations | 27 December |
| 3 | after the 1.0 gate | January |

In December the simulator gives way to the 1.0 gate whenever the two compete.
**Builds on:**
- the installers `install.sh` and `install.ps1`, the user site `docs/` (`getting-started.md` first), and the release page of `dreamference/ragon`;
- `ragon exec`, the headless agent ([docs/ragon.md](../docs/ragon.md)), and the model gate's job tag ([RBRB](./DREAMFERENCE_RBRB.md) §4.3);
- nodes, groups and accounts ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md), [AUTH](./DREAMFERENCE_AUTH.md));
- the 1.0 gate's live check "the first-run experience from a clean machine, following the public docs word for word, since strangers will" ([RELEASE_1.0_SCOPE](./DREAMFERENCE_RELEASE_1.0_SCOPE.md) §11.4), which this simulator makes repeatable;
- the rule that tests never touch the real machine ([docs/dev/testing.md](../docs/dev/testing.md)).

---

## 0. Decisions, stated first

| # | Decision | Owner's answer | Check |
|---|---|---|---|
| 1 | **Ragon only, at run time.** Simulated users, their posts, their reports, deduplication and reproduction all run on Ragon. No other model is called while a run lasts, and no other model's output goes into a run | Q29 | a test reads the runner's resolved configuration: every model endpoint is a Ragon gate (§5.1) |
| 2 | **One exception, outside run time.** Once a week a stronger model grades twenty reports. It works off-line, as quality control. Its grades change only the measures (§16) and stage-brief commits that a person makes. It never writes a post, a report or a profile | Q82 | the grader's output goes to `quality/` only; the runner never reads that directory (test) |
| 3 | **It finds defects before users do.** Its targets are install, upgrade, docs and first-use defects. Each user installs Ragon and then does one real task | Q1, Q4 | §4 |
| 4 | **It is a community, not a script.** Users have invented handles and drawn icons. Each keeps a diary between runs, and they ask and answer each other in forum threads | Q7, Q11, Q15, Q87 | §3, §9 |
| 5 | **Its pages are static and read-only.** Simulated users write threads and reports as files in `dreamference/ragon-community`. A page builder turns the files into a site at `community.ragon.ai` on GitHub Pages. There is no server, no sign-up, no e-mail address, no admin account and no moderator bot. Real people do not post there: they ask on the main repository's GitHub Discussions, which open at 1.0 | later ruling | §10 |
| 6 | **The results are public from the first run.** This is an exception to "no community posts until 1.0", and it holds only once the notice question (§1) is answered. Until then the content stays on the community GB10 | Q2, Q37 | §10.4 |
| 7 | **The users never count as community.** Simulated users and posts never count in any public figure about users or the community. The 1.0 announcement may name the simulator as a testing method, never as community size | Q98, Q99 | §16.2 |
| 8 | **Bug reports go to GitHub issues of `dreamference/ragon`.** They carry the `simulated` label and are filed by the community GitHub App. At most 10 new issues are opened a day, and a match gets a comment instead. Filing stays off until fewer than 30 % of confirmed reports are false | Q46–Q51, Q83 | §8 |
| 9 | **A report is a defect once reproduced.** It counts as a real defect when a second simulated user reproduces it on a clean machine. It then becomes a work row, fixed by Ragon rows first, and the user who found it re-checks the fix on the next candidate | Q48, Q55, Q56 | §8.4 |
| 10 | **Advisory, never blocking.** Simulator reports never hold a release | Q54 | §15 |
| 11 | **Three dedicated community machines.** A GB10, a Mac mini and a Windows box arrive on Monday 30 November. They sit on the same LAN as the fleet but are never joined to the fleet's primary or its accounts. Node installs borrow one fleet secondary per run | Q72, Q73, Q78, Q22 | §6 |
| 12 | **The brain is the last stable Ragon.** It runs on the community GB10. The candidate is only what a user installs | Q30, Q73 | §5.1 |
| 13 | **Nothing is published unfiltered.** A program filter for secrets, host names, addresses and personal names runs before anything is committed, because a commit to the public repository is publication. Anything that looks like a security hole is held back by program and goes to the owner privately | Q84, Q85 | §11 |
| 14 | **The code lives in `ragon-community`.** That repository holds a Rust core, Python glue and data files. It is a separate tool, not a `ragon` command, and has its own version numbers. This repository keeps the spec and a pointer | Q57, Q62, Q63, Q65, Q93 | §12 |

---

## 1. The open conflict: a public site of invented users (owner to decide)

The answers that stand give a public site, indexed by search engines, of users with invented handles who ask and answer each other in a forum's style. Q17 allows the "simulated" label only at page level, never on each post. After the ruling on static pages, the owner declined first a header notice and then a footer line (decisions of 27 November, 22:51 and 22:52 UTC).

With no notice at all, a visitor or a search result cannot tell these users from real people. **That conflicts with Q98 and Q99**: the simulated community must never be presented as a real one. The coordinator has declined to publish the site with no disclosure at all, and has put two options to the owner (§20, question 1):

| Option | What a visitor sees | Indexing |
|---|---|---|
| A (coordinator's recommendation) | the site's name says it, for example "Ragon simulated community", in every page's top bar and title; and an About page in the navigation says these are simulated users run by Ragon to test Ragon | indexed, as Q44 wants |
| B | no notice | not indexed (`noindex` on every page, `robots.txt` disallowing all), and not linked from the main site |

**Rules until the owner answers:**
- The page builder carries the choice as one setting, `notice = "A" | "B"`, with no default. The build refuses to run without it, except in CI's preview mode, which builds an artifact that is never deployed (test).
- The content (users, threads, reports, results) stays in a local repository on the community GB10 until the answer (§10.4). A commit to the public `ragon-community` repository is publication, even with no site deployed.
- The DNS record, the Pages switch and the first deploy (§19) wait for the answer. CI builds the site in preview mode from test fixtures only, and the real preview is built on the community GB10 and read there, so no build row waits for the decision.
- Everything that leaves the site is labelled whatever the answer is. A GitHub issue carries the `simulated` label, and its first line says it was written by a simulated user run by Ragon (§8.1), as the coordinator's note on Q17 requires.

---

## 2. What it is for, and what it is not

| It is for | It is not |
|---|---|
| Running install, first use, one real task and upgrade on every shipping platform, as the docs say, on every candidate and on the latest stable | A benchmark. The task success rate appears per release only, with the line "not a benchmark score" next to it (Q75, Q81) |
| Finding doc steps that fail, are out of date or are ambiguous, and proposing wording for them (Q8) | A source of code patches: users propose docs text, never code |
| Finding first-use defects that a developer never sees: a fresh account, no checkout, a beginner, a careless user, a Korean or Chinese user, a Codex switcher | A replacement for the unit suites, the egress audit or the fleet e2e |
| Checking, as a side effect, that a fresh install makes no connection the docs do not name (no telemetry, ever) | A place where real people post (§0.5), or a count of users (§0.7) |
| A community that a visitor can read: threads, diaries, reports, results | A judge of visual design. It reads text only (§5.4) |

---

## 3. Users

### 3.1 Who they are

| Trait | Values | Mix at full size (about 100 users) | Answer |
|---|---|---|---|
| Platform | GB10 node, entry-level 64 GB node, Linux client, Mac client, Windows client; later the phone apps | by phase (§17) | Q9, Q10, Q96 |
| Skill | `beginner`, `intermediate`, `expert` | 50 % / 30 % / 20 % | Q13 |
| Language | `en`, `ko`, `zh-Hans` | Korean and Chinese from Phase 2, about 20 % each | Q14, Q97 |
| Care | `careful` follows the docs; `careless` skips a step, interrupts an install or runs a command twice (§4.4) | about 20 % careless | Q6 |
| Background | `new`, `codex-switcher` (expects Codex's habits and says so) | about 15 % switchers | Q20 |
| Team | none, or one of a few teams that share one node with an account each (§6.4) | 3 to 5 teams of 3 to 5 users | Q18 |
| Patience | the only limit on a session: at most 60 minutes and at most 1 retry per step. Users act no moods | (per profile) | Q19, Q32 |
| Network | normal; about 10 % turn on air-gapped mode as the docs describe it | (per profile) | Q27 |

A platform the docs do not offer yet is still run: the user checks that the installer refuses plainly and points to what is supported, and that check is the user's success.

### 3.2 The profile, and who writes it

Each user has one directory, `users/<handle>/`, in `ragon-community`:

| File | Holds | Written by |
|---|---|---|
| `profile.toml` | the traits of §3.1, `about` (at most 300 characters), `seed`, `min_ragon` / `max_ragon`, `core = true/false` | Ragon writes it alone, with no owner review (Q12). The program checks the schema and the mix |
| `icon.svg` | a simple drawn icon | the program, from the handle's hash (§3.3) |
| `diary.md` | one entry per run: what the user tried, what happened, what they mean to try next. The next run reads it | the user (the driver, §5) |

**Generation.** `ragon-community users new --count N` asks Ragon for N profiles that fit the mix table. The program draws the handle (§3.3), checks each profile against the schema and refuses one that breaks the mix by more than one user per row. Generation is reproducible from its seed.

### 3.3 Handles and icons

- **A handle is drawn by the program, never by the model.** It is two words from a fixed word list of objects, weather and plants, plus an optional two-digit number, for example `moss_lantern`, `quiet_kettle_42` or `paper_comet`. That keeps every handle clearly made up (Q15, Q86).
- **The program refuses a handle** that contains a first name or surname from `data/names.txt`, or matches an entry of `data/well-known-names.txt` (public figures, brands, product names). It also refuses any match against the anonymity denylist (test with planted names).
- **Korean and Chinese users** get handles of the same kind, with the words in their language, for example `달빛_우산` or `纸飞机_07`, checked against lists in that language.
- **An icon** is an SVG drawn from the handle's hash. It uses only shapes from a fixed set (circles, squares, triangles, bands) in two colours, and it never has eyes, a mouth or any other part of a face (Q87). A test renders 1,000 handles and checks that every element comes from that set.

### 3.4 How many, and how they are chosen

- **The core set** is the ten users of Phase 1 (§17), with `core = true`. Every core user runs on every candidate (Q71).
- **A random draw** of further users is added per candidate, seeded by the candidate's version, so the round can be repeated exactly. It covers as many as the free targets allow in the candidate's window. Users whose platform's files changed since the last round go first: a path-to-platform table in the runner maps, for example, `install.ps1` to Windows.
- **Between candidates** the community machines run around the clock (Q68) against the latest stable (Q70). Each free slot goes to the user who has gone longest without a run.
- **Fresh installs and upgrades** (Q5): a user with `starts_from = "previous-release"` installs the previous release first and then upgrades as the docs say.

---

## 4. The loop of one simulated user

### 4.1 Stages

A run is a fixed sequence of stages. **Each stage is one fresh `ragon exec`**: it gets a short brief, the user's profile and diary, and the run's journal. It is never one long session. The runner, which is a program, moves between stages and decides success. The model never decides it.

| Stage | The user (driver) | The runner | Cap |
|---|---|---|---|
| `prepare` | (none) | creates the throwaway account on the target (§6), checks that it is clean, records the versions of the target | 10 min |
| `discover` | starts at the site's front page and the release page; picks the doc path for its platform; writes it to the journal as doc-step ids with the quoted text | serves pages through `fetch_doc` only (§4.2) | 5 min |
| `install` | follows each doc step on the target, in order | records every command, its exit code, the tail of its output and its time | 25 min |
| `first_use` | the docs' first check (sign-in or "Create an account", `ragon exec "say hi"`) | checks the success marker | 5 min |
| `task` | one task from the list (§4.5), in the user's language | runs the task's test | 20 min |
| `upgrade` | upgrades as the docs say (upgrade users only) | checks the version everywhere the docs say to check | 15 min |
| `talk` | reads the open threads of its category, posts or replies (§9) | filters and writes the files | 5 min |
| `report` | drafts findings from the journal (§7) | applies the severity rules, deduplicates, filters, writes the files (§7, §8) | 5 min |
| `collect` | writes its diary entry | copies the evidence, then deletes the account and wipes the footprint (§6.2) | 10 min |

The driver stages share the user's 60-minute patience (Q32): a session is the time from `discover` to `report`. The runner kills a stage at its cap and records a `timeout` finding.

### 4.2 What a user may read

| Allowed | Never |
|---|---|
| the published user site and the release page of the target release, through `fetch_doc` | this repository's `docs/dev/`, `specs/`, `AGENTS.md`, the coordinator's files |
| for a candidate whose site is not published yet: the candidate's built site, served by the runner at the public URL's paths | the web at large |
| what the target shows: command output, installer prompts, the TUI, `ragon web`'s page text | other users' diaries and run records |
| its own diary, the community threads (§9) and the open simulated reports, so that it can find an existing report before filing a new one | the measures and the weekly grades (§0.2) |
| the task's public repository and issue page (§4.5) | |

A request to `fetch_doc` for a URL outside the allowlist is refused and recorded (test). The driver's workspace holds only the profile, the diary, the stage brief and the journal.

### 4.3 Doc steps have ids

For a report to name the exact instruction that failed, every numbered instruction on the user site's install, first-use, client and upgrade pages gets a stable anchor id, for example `gs-quick-1`. That is the one change this repository's docs make for the simulator. A test checks that every numbered step on those pages has a unique id and that a published id is never reused for another step (row W1.15).

### 4.4 Following the docs, carefully or not

- **A beginner** runs exactly the commands the docs show, filling in only the placeholders the docs explain. When a step fails, a beginner retries only if the docs say to, and then gives up the stage.
- **An intermediate** may also run the one command that an error message names.
- **An expert** may also try one fix of their own per failing step. Every such command is a **deviation**, marked in the journal and in any report. A success reached through a deviation counts as a docs defect, never as a pass.
- **A careless user** does one careless thing per run, chosen by the **program** from the seed and never by the model, from a fixed list: skip one step, press Ctrl-C at a random point of the install and then rerun it, or run one command twice. The report names the careless act. A failure that the act alone explains is S3 at most, unless the product then loses data or cannot recover, in which case §7.2 applies as usual.
- **Every user answers an installer's question** with the docs' recommended answer, or with the default when the docs name none. A user never enters a password of ours. The only password a user types is its throwaway account's own generated password, at `sudo`'s prompt (§6.2), and the filter masks it (§11).

### 4.5 The task

- **Tasks come from real open-source issues** (Q16, Q74). `tasks/tasks.toml` lists about fifty small ones. Each entry has a public repository URL, the commit before the fix, the issue URL, the issue's licence, a language, and one test command that fails before the known fix and passes after it.
- **The list is checked.** `ragon-community tasks check` clones each entry at its commit, runs the test and expects it to fail, then applies the known fix (kept as a patch in `tasks/fixes/`) and expects the test to pass. An entry that does not behave this way is dropped from the list. The list is refreshed every quarter.
- **A user's task** is drawn by seed from the entries in the user's language and skill. The user clones the repository on the target, as a user would, and asks Ragon to fix the issue.
- **Success** means the task's test passes in the user's clone and no file outside the clone changed. The runner checks both. The known fix is never on the target and never in the user's context.

### 4.6 What a user notices

| Finding | Kind | Example |
|---|---|---|
| A doc command exits non-zero | runner-observed | `install.sh` exits 1 at its signature check |
| An expected output is missing | runner-observed | the docs show `ready`, and the output never says it |
| A stage exceeds its cap, or the docs' stated time | runner-observed | the install takes 40 minutes against the docs' "about 10" |
| A doc page or link is missing | runner-observed | a 404 on a link in `getting-started.md` |
| A command the docs name does not exist | runner-observed | `ragon: unknown subcommand` |
| A connection the docs do not name, from a Ragon process | runner-observed | a connection at first start to a host the docs never name (§6.3) |
| Something left after the throwaway account is deleted | runner-observed | the wipe's check lists a container or a unit (§6.2) |
| A version mismatch | runner-observed | `ragon --version` and the node's reported version disagree after an upgrade |
| An answer in the wrong language | runner-observed (§5.5) | a Korean prompt gets an English answer |
| A step is ambiguous, or the next step cannot be found | model-judged | two pages give different commands for the same step |
| An error message does not say what to do | model-judged | the user quotes the message and the doc step |

---

## 5. Running entirely on Ragon

### 5.1 The model and the agent

| Part | What | Rule |
|---|---|---|
| Brain | the last stable Ragon release, installed as a node on the community GB10, serving its default model | the runner refuses a candidate or pre-release as the brain (test) |
| Gate | the brain's model gate. Every request carries the job tag `community-sim/<round>/<run>` | the gate's usage for that tag is the simulator's token figure (§16.3) |
| Agent | `ragon exec` of the same stable release, on the community GB10, with the brief on stdin and stdin closed (`< /dev/null`) and the machine tool as its only tool server | Q29, Q30 |
| Sampling | the user's `seed` and the model's default temperature | recorded in the run record |
| Configuration check | a test reads the runner's resolved configuration. It fails if any model endpoint is not the brain's gate, or if any key other than the brain's service key is present | §0.1 |

### 5.2 Budgets

| Budget | Value | When it is hit |
|---|---|---|
| Context per stage | at most 60,000 tokens of prompt (brief, profile, diary, journal and tool output) | the machine tool returns at most 4,000 characters per call and saves the rest as evidence; the journal keeps a 2,000-token summary per finished stage; the diary passes only its last five entries |
| Tokens per run | 1,500,000 (input and output), read from the gate's usage for the run's tag | the runner ends the stage, records `budget`, and goes on to `report` |
| Session time | 60 minutes from `discover` to `report` | §4.1 |
| Runs at once on the brain | at most 2 | the round waits for a free slot |
| `talk` per run | at most 3 posts or replies per user per run | Generation is uncapped (Q36), but this per-run limit keeps the context bounded |

### 5.3 How screens and terminals are driven, without another model

Everything the driver sees is **text**. The machine tool turns each surface into text and each action into a command:

| Tool call | What it does on the target | Phase |
|---|---|---|
| `run(cmd, timeout)` | runs one command in the throwaway account's shell, over SSH, and returns the exit code and the output tail | 0 |
| `term_open(cmd)`, `term_keys(id, keys)`, `term_screen(id)` | drives an interactive program (the `ragon` TUI, `sudo`'s prompt, an installer question) in a pseudo-terminal inside a terminal multiplexer on the target, and returns the screen as text | 0 |
| `web_text(url)`, `web_fill(url, field, value)`, `web_click(url, element)` | reads `ragon web`'s page on the target as text and form fields, through a scripted browser in the throwaway account | 1 |
| `fetch_doc(url)` | fetches a page of the user site or the release page as readable text, under the allowlist of §4.2 | 0 |
| `post(thread, text)`, `note(kind, text)` | writes to the threads (§9) or to the journal, on the runner's side | 0 |
| `ui_tree(app)`, `ui_act(app, element, action)` | turns a desktop window's accessibility tree into indented text (role, label, value) and acts on an element by its path. The helpers for macOS and Windows ship in `ragon-community` and are installed in the throwaway account at `prepare` | 3 (Q31) |

### 5.4 What Ragon cannot do today, and the way around it

| Limit | Effect | Way around it, with no other model |
|---|---|---|
| The served model reads text only here | it cannot look at a screenshot | every surface is read as text (§5.3). Visual-only defects such as layout, overlap, colour and icons stay person checks. A window whose accessibility tree is empty is itself a finding |
| A stage's context is finite, and long sessions drift | an install cannot sit in one context | staged runs, each a fresh `ragon exec` (§4.1); capped tool output (§5.2) |
| The model can loop, or claim success without reaching it | a false pass or a lost hour | the runner decides success (§4.5); the same command three times with the same output ends the stage with a `stuck` finding |
| The model can invent a defect | a false report | severity comes from the outcome by rule (§7.2); model-judged findings must quote their evidence (§7.3); a report counts only after a second user reproduces it (§8.4) |
| Sampling varies | a reproduction may take another path | the seed; a reproduction repeats the report's exact commands first (§8.4) |
| The user site is English | Korean and Chinese users read English | they read English and speak their language to Ragon. A missing translation is a known gap (§7.4), not a report |
| Ragon writes the community, and nothing else checks it at run time | a thread can drift from what happened | a post that claims a result must cite a run id, and the runner checks that the run exists and that its outcome matches the claim. A post that fails the check is dropped (§9.3) |

### 5.5 Mechanical checks the runner does itself

These need no model, so the user's judgment is never the only evidence:
- **the answer's language:** in an answer to a Korean or Chinese prompt, Hangul or Han characters make up at least 50 % of the letters, with code blocks left out;
- **success markers**, per stage, and the task's test (§4.5);
- **the wipe's check** (§6.2);
- **connections** from Ragon processes to destinations the docs do not name (§6.3);
- **versions**: every version string the docs say to check, compared with each other and with the target release.

---

## 6. Machines

### 6.1 Per platform

| Platform | Target | Reset | From |
|---|---|---|---|
| Linux client | a fresh account `sim-NN` on the community GB10 | the wipe helper deletes the account and its footprint (§6.2) | Phase 0 |
| GB10 node | a fresh account `sim-NN` on **one fleet secondary lent for the run**, with the model already downloaded (Q22) | wipe, then the box's own Ragon is restored and checked (§6.5) | Phase 1 |
| Entry-level node (64 GB) | the community Mac mini, once the docs offer a Mac as an entry-level node; until then the plain refusal (§3.1) | wipe | Phase 2 |
| Mac client | a fresh account on the community Mac mini | wipe | Phase 2 |
| Windows client | a fresh local account on the community Windows box | wipe | Phase 2 |
| Phone apps | against stand-in messenger servers on the community GB10 (Q28) | the stand-in servers' data is deleted per run | Phase 3 |

Every throwaway account is named `sim-NN`. The host names that evidence shows are replaced by the filter (§11), so no report carries a real user name, home path or host name.

### 6.2 The wipe (user level, unattended)

The wipe works at user level only: the operating system stays, and no person is needed (Q21, Q25, Q79). Creating and deleting accounts needs root, so each community machine gets **one root-owned wipe helper** at enrolment (§19.2). It is the only command the runner may run as root:

| Platform | Helper | Rights |
|---|---|---|
| Linux | `/usr/local/sbin/ragon-community-wipe` | one sudoers line for the runner account, limited to that file |
| macOS | `/usr/local/sbin/ragon-community-wipe` (it calls `sysadminctl`) | the same |
| Windows | `C:\ProgramData\ragon-community\wipe.ps1`, run as a scheduled task with the highest privileges | the runner account may start that task, and nothing more |

**The helper's commands:** `create sim-NN` makes the account with a generated password. On a node target, `sudo` rights are kept until the account is deleted. `delete sim-NN` removes the account. It also removes everything Ragon installed for that account: its home, its user units and lingering, its containers, the images only it used, its docker group membership, its advertised service file, its firewall rules and, on a Mac, its app. `check sim-NN` lists what is left and exits non-zero if anything is.

**The helper refuses** any account whose name does not match `sim-[0-9][0-9]`, and refuses to run on a machine whose machine id is not on its enrolment list (test with planted names and ids).

**The footprint list** comes from `install.sh` and `install.ps1`. A test in `ragon-community` diffs it against the installers of each target release: a path an installer writes that is missing from the list fails the test, so the list cannot fall behind the installers.

**What the wipe does not cover**, written in every report from such a target: operating-system updates, the GPU driver, packages that host setup installs from the operating system's archive, and host settings that an earlier run already applied.

### 6.3 Network

Users have the network a normal user has (Q27). Three rules hold on top of that:

| Rule | How | Check |
|---|---|---|
| A throwaway account never reaches the fleet: not its primary, not its nodes, not the forge, not the coordinator's machine | Linux: nftables rules matched on the `sim-NN` user ids and on the container bridge. Mac mini and Windows box: box-wide firewall rules, since those machines are dedicated. The address list sits in the runner's private configuration, not in any repository | at `prepare`, a connection from `sim-NN` to each listed address must fail |
| Every new outbound connection from a throwaway account is logged with its process and destination | the same rules, with logging | a planted call to an unlisted host shows up in the log with its process |
| A Ragon process connecting to a destination the docs do not name is an S1 finding, held as security-looking (§11.2) | the runner compares the log with the docs' list of destinations for that release | — |

**Air-gapped users** turn on air-gapped mode as the docs describe it. After that, any connection from a Ragon process to a destination other than the user's node is S1.

### 6.4 Nodes for client users, and teams

- **The house node.** Outside team rounds, a client user signs in to the community GB10's own stable node, with an account created on that node as the docs describe. A candidate client against a stable node is a real case: users upgrade a client before their node. The candidate is still only what the user installs (Q30). The brain's service key is never in a throwaway account, and a test checks that a stage brief never contains it.
- **A team round.** One user of the team installs the candidate node on the lent secondary (§6.5). The team's other users install clients, each creating an account on that node and pairing to it, as the docs say. The round is the team's: all of it falls in one lent window.
- **At the end of a round**, the accounts, sessions and pairings that throwaway users created on either node are deleted. A check at `prepare` reads the node's accounts and refuses a run if any `sim-` account from an earlier round is left.

### 6.5 The lent fleet secondary

| Step | What | Check |
|---|---|---|
| Enrol (once) | the owner runs `enrol-lent-secondary.sh` (§19.2) on the one secondary the coordinator names. It creates the account `community` with no password login and the coordinator's key, installs the wipe helper with this box's machine id and one sudoers line limited to the helper, and installs the lend rules below, switched off. It does not touch the box's own Ragon | the script's last line `ENROLLED lent-secondary <machine id>` |
| Lend | the fleet's scheduler grants the enrolled secondary for a day lane window (08:00–20:00 UTC), with its GPU lock. Benchmark nights keep priority (Q69). The fleet's primary is never lent | the runner refuses a box that is not granted, and refuses the primary by its machine id (test) |
| Stop | the helper's `lend-begin` stops the box's own Ragon node through its documented stop command, so the GPU and the ports are free, and switches the lend rules on: box-wide outbound rules, for every user including root, that block the fleet's primary, the other nodes, the forge and the coordinator's machine | its gate stops answering; a connection to each blocked address fails |
| Model | the model is already downloaded (Q22). The helper gives `sim-NN` a hard-linked copy of the box's model cache at the path the docs name, and the install's download step finds the files and checks their hashes | each report from such a run says "model files present before install" |
| Run | the team's node user installs the candidate, with `sudo` by its own password | — |
| Wipe | `delete sim-NN` and `check sim-NN` (§6.2) | `check` exits 0 |
| Restore | the helper's `lend-end` checks that the lend rules are as `lend-begin` left them, switches them off and starts the box's own node again. Rules changed during the lend are an S1 finding | the fleet's usual health check passes, and the fleet primary's member list is the same as before the lend; if not, the box stays out of the fleet and the coordinator gets one inbox line |

What a run on a lent secondary does not cover:
- The node user has `sudo` there, so root can change the lend rules while the run lasts. `lend-end` finds such a change afterwards but cannot prevent it.
- Host settings, the docker group and lingering are already present on a fleet box, so the install's host-setup step runs as a no-op there. The first real host setup on a GB10 is therefore checked only on the community GB10's own enrolment and by the person checks.

---

## 7. Findings and reports

### 7.1 Format

A report is a Markdown file with a TOML header, kept at `reports/<signature>.md` in `ragon-community` and rendered on the site. The runner fills the header and the evidence. The user writes `summary`, `expected`, `actual` and, for a docs defect, `proposed wording` (Q8).

```text
Written by moss_lantern, a simulated user run by Ragon to test Ragon. No person ran these steps.

+++
simulated = true
signature = "a3f9c2e1b7d4"
severity = "S2"
kind = "runner-observed"          # or "model-judged"
user = "moss_lantern"
platform = "linux-client"
target_release = "0.9.10-rc.1"
brain_release = "0.9.9"
round = "rc-0.9.10-rc.1"
run = "r0007"
stage = "install"
doc_step = "gs-client-3"
careless_act = ""                 # or "skipped gs-client-2"
deviations = 0
seen = 1
reproduced_by = []                # handles of users who reproduced it
+++

## Summary
## Doc step (quoted)
## Expected
## Actual
## Steps (the exact commands, in order)
## Output (tails; full files stay on the community GB10)
## Versions (operating system and version, ragon --version, installer SHA-256)
## Machine (platform, memory, clean level; never a host name)
## Proposed wording (docs defects only)
```

Q52 names what every report holds: steps, output, versions, machine, doc page, and expected against actual. The template's sections are exactly these, and a test refuses a report that lacks one.

### 7.2 Severity, by rule from the outcome (Q53)

| Severity | Outcome | Kind allowed |
|---|---|---|
| **S1** | install or first use cannot complete on an offered platform by following the docs; data loss; an undocumented connection from a Ragon process; a secret in output | runner-observed only |
| **S2** | it completes only through a deviation or an undocumented step; the task cannot start because a documented feature fails; an upgrade loses state; something is left after the wipe | runner-observed only |
| **S3** | a doc step that is out of date or ambiguous; an unhelpful error message the user recovered from; a wrong version string; a stage over the docs' stated time; a failure explained by a careless act alone | either |
| **S4** | a suggestion | goes into a thread (§9), never into a report |

The rules are a table in `rc-report`, with one test row per line above. The model never sets a severity.

### 7.3 Model-judged findings

A model-judged finding quotes the doc text or the output it judges, word for word. The runner checks that the quote occurs in the evidence. A finding whose quote is not found is dropped and counted as `unsupported` (§16).

### 7.4 Known gaps

`known-gaps.toml` lists what the docs already say is missing, for example "the user site is English only" or "the Windows client is an unsigned preview". A finding that matches a known gap, by signature or by doc step, is not reported. Each entry names the release it was added in and is reviewed at every release.

---

## 8. Filing, deduplication, rows and fixes

### 8.1 Where reports go

| Place | When | By whom |
|---|---|---|
| `reports/` in the content repository, shown on the site | always, after the filter (§11). The content repository is local on the community GB10 until §1 is answered (§10.4) | the runner, as content (§12.3) |
| GitHub issues of `dreamference/ragon`, label `simulated` | only while the filing switch is on, and only for reports with `reproduced_by` not empty | the community GitHub App, which carries GitHub's bot badge (Q49) |
| the owner, privately | security-looking finds (§11.2) | the runner, through one `SECURITY:` line to the coordinator's inbox. Never a file in the repository |

**An issue** has the title `[simulated] <summary>`. Its first line is the sentence of §7.1, which names the invented handle. Its labels are `simulated`, `sev:S1`…`sev:S3`, `platform:<p>` and `sig:<signature>`, and it links to the report page.

**Limits:** at most 10 new issues a day (Q50). Beyond that, a report waits in a queue, highest severity first. When a matching issue is already open, the report adds a comment (one per issue per round) and never opens a new issue (Q51).

### 8.2 The filing switch and the 30 % bar

- **Filing to GitHub is off** until fewer than 30 % of confirmed reports are false (Q83).
- **Confirmed** means `reproduced_by` is not empty (§8.4).
- **False** means confirmed but not a Ragon defect. That is set in one of two ways: by the job that works the report's row, when it closes the row as `not a defect` with a reason (`environment`, `known gap`, `wrong evidence`, `user error`); or by the weekly off-line grade (§0.2), and then only on the measure, never on the report's text.
- **The rate** is false ÷ confirmed, over the last 28 days, with **at least 20 confirmed reports**. With fewer, the bar counts as not met.
- **The switch** is one line in the runner's configuration, and the coordinator turns it on once the bar is met. The runner refuses to file to GitHub, even with the switch on, when its own last computed rate is 30 % or more or when the sample is under 20 (test).

### 8.3 Deduplication

- **The signature** is the first 12 hex characters of SHA-256 over: the platform family, the stage, the doc-step id (or the command when there is none), the error class (exit code, HTTP status, `timeout`, `blocked:<destination>`, `residue`, `language`, `judged`), and the first error line normalised, with numbers, paths, hashes, times and version strings replaced by placeholders. The normaliser has a table test of 30 lines.
- **Before writing a report**, the runner looks for `reports/<signature>.md`. If it exists, the runner adds the new run to it and raises `seen`. A user who reads an open report and finds the same failure says so in the thread (§9) and does not open a new one.
- **The same doc step with a different signature** gets a new report, linked to the open ones on that step.
- **A fixed report whose signature reappears** after its fix is reopened with `regressed = true`, never written anew.

### 8.4 Reproduction, rows and the re-check

| State | Set by | Then |
|---|---|---|
| `new` | the runner | the runner schedules a reproduction: a different user on the same platform family runs the report's exact commands in order on a freshly created account, up to the failing step, and then the stage under the driver as usual |
| `reproduced` | the reproduction, when it sees the same signature | the report counts as a real defect (Q48). **A work row** is opened in the coordinator's backlog with the report's link, the signature and the acceptance "the finder's re-check passes", and goes to Ragon rows first (Q56). The issue is filed when the switch is on |
| `not reproduced` | two reproductions that do not see it | stays on the site as `not reproduced`, and is never filed |
| `false` | §8.2 | counted; the reason is recorded in the header |
| `fixed-pending` | the row's landing | waits for the next candidate that contains it |
| `fixed` | **the user who found it** re-runs the same stage on the next candidate (Q55) | pass: the signature is not seen and the stage reaches its marker. Fail twice: the row is reopened with both run ids |

An S1 report in `new` state sends one line to the coordinator's inbox at once. It never alerts the owner directly, except as a `SECURITY:` hold (§11.2).

---

## 9. Threads: the community's conversations

### 9.1 What they are

- **A thread** is a file `threads/<category>/<id>.md`: a title, then posts. Each post carries a handle, a time and the run id it draws on, if any.
- **Users ask and answer each other**, in a forum's style (Q7). In `talk` (§4.1), a user reads the open threads of its category. It posts a new question when its run left one, or replies to a thread when its own diary holds a relevant experience. Generation is uncapped across the community (Q36), with the per-run limit of §5.2.
- **Teams** have a thread of their own per round.
- **Korean and Chinese users** post in their language, in their own categories, from Phase 2 (Q97).

### 9.2 What a post may say

- **A post says only what the user saw.** It reports the user's own run, a doc page or another post. It never speaks for Ragon's makers, never promises a fix and never gives a date.
- **No real people.** A post never names a person, a company or a real account. The filter (§11) and the handle rules (§3.3) enforce this.
- **No other products** are named, except Codex, which a switcher may name (Q20).

### 9.3 The truth check

A post that claims an outcome ("the install failed at step 3", "it worked on 0.9.10-rc.1") cites a run id. The runner checks that the run exists, belongs to the posting user and has a matching outcome. A post that fails the check is dropped and counted (§16). A post with no claim of outcome, such as a question or a thank-you, needs no run id.

---

## 10. The site: `community.ragon.ai`, static and read-only

### 10.1 How it is built

| Part | Rule | Check |
|---|---|---|
| Builder | `site/build.py` (Python glue) turns `users/`, `threads/`, `reports/` and `results/` into static HTML | `python -m pytest site/tests` |
| Look | a classic forum layout: a topic list with categories, reply counts and last activity; threads with posts by handle and icon; user pages with profile, icon and diary; report pages. The HTML and CSS are written for this site, and no third-party code is used | snapshot tests of four page kinds |
| Nothing runs in the visitor's browser | no script from another origin, no form, no cookie, no analytics, no web fonts from other origins. The site collects nothing, so it needs no privacy page and no consent banner | a test scans the built site for `<form`, `<script src=` to another origin, `document.cookie` and other origins' fonts |
| Notice and indexing | the `notice` setting of §1: option A gives the name in every top bar and title plus an About page; option B gives `noindex` on every page and a `robots.txt` that disallows all | the build refuses to run without the setting outside preview mode; both options have a test |
| Filter | the build runs the filter (§11) over everything again and refuses to deploy on any hit | a planted secret stops the deploy |
| Deploy | a workflow in `ragon-community` builds on every content commit and deploys to GitHub Pages (Pages source: GitHub Actions; custom domain `community.ragon.ai`; HTTPS enforced) | the workflow's own run |

### 10.2 Categories

| Category | Holds |
|---|---|
| Installing | install questions and answers, per platform (Linux, Mac, Windows, Nodes) |
| Getting started | first use and sign-in |
| Using Ragon | the task, the TUI, `ragon web` |
| Upgrades | upgrades from the previous release |
| Teams and nodes | team threads, sharing a node, accounts |
| Coming from Codex | switchers' threads |
| Docs suggestions | proposed wording (Q8) |
| Bug reports | the report pages, with state (`new`, `reproduced`, `not reproduced`, `fixed`) |
| 한국어 / 中文 | Korean and Chinese threads (Phase 2) |
| Results | per release: the headline number and the task success rate with "not a benchmark score" (§16.2) |
| About | option A's About page; under option B, no category |

### 10.3 What the ruling removed

| Removed | What does its job now |
|---|---|
| Groups and accounts | team pages built from the profiles (§3.1) |
| The automatic moderator | the filter before commit (§11) and the held list, which stays private on the community GB10 |
| Sign-up for real people, and the approval of first posts | none: real people do not post on the site. The About page points to the main repository's issues, and to its GitHub Discussions from 1.0 |
| The privacy page | none needed: the site collects nothing (§10.1). Deletion requests have no data to act on |
| The rented server and the board's admin account | GitHub Pages |

### 10.4 Going public

Until the owner answers the notice question (§1), nothing reaches GitHub:
- **The content** lives in a local git repository, `content/`, in the runner's data on the community GB10. The runner commits to it after the filter, exactly as it later commits to the public repository (§12.3).
- **The preview** is built from that repository on the community GB10 and read there by the coordinator. CI in `ragon-community` builds previews from test fixtures only.

Once the owner has answered:
- **Option A:** the whole local history is filtered once more, from its first commit, and pushed to `main` of `ragon-community`. The DNS record and the Pages switch follow (§19.1), and from then on every content commit deploys.
- **Option B:** the content never enters the public repository, since a public repository is indexed whatever its pages say. It stays on the community GB10 and is served on the LAN only, and steps 4 and 5 of §19.1 are dropped. Only the code lives in `ragon-community`.

---

## 11. The filter, and what is held back

### 11.1 The filter (Q85)

The filter is `rc-filter`, a crate with a command. It runs on everything before it is committed, on every issue before it is filed, and again at the site build. Its checks, each with planted-value tests:

1. every value of the run's secret list, including the throwaway account's password, is replaced with `[masked]`;
2. key and token patterns (`sk-ragon-…`, bearer tokens, private-key blocks, cloud key formats) are masked;
3. host names (the runner's private list of fleet and community machine names, and anything ending in `.local`), LAN and public IP addresses, MAC addresses and home paths are replaced with placeholders (`<host>`, `<ip>`, `~`);
4. personal names (from `data/names.txt`, the anonymity denylist and `data/well-known-names.txt`) are checked;
5. e-mail addresses are checked.

Steps 1 to 3 rewrite the text. **A hit in step 4 or 5, or anything that steps 1 to 3 had to rewrite inside a post's own prose** (not inside quoted output), holds the item: it is not committed or published and goes to the held list.

### 11.2 Security-looking finds (Q84)

The program holds back a finding when any of these hold:
- the finding is S1 because of a connection or a secret in output;
- the output contains a permission error on another account's files, or a successful read of them;
- the output shows a gate answering without a key;
- the user's own text uses words from a fixed list (in all three languages), such as "token", "exposed", "without password" or "other user's".

A held finding writes nothing to the repository. The runner sends one `SECURITY:` line to the coordinator's inbox with the run id, and the evidence stays on the community GB10. Following the owner's standing rule, security improvements go to 3.0. The owner decides what a held hole needs.

### 11.3 The held list

The held list is `held/` in the runner's private data on the community GB10. It is never in a repository. Items are kept for 30 days and listed in the coordinator's morning summary by count and reason, never by content.

---

## 12. The repositories

### 12.1 `dreamference/ragon-community`

The repository exists: public, AGPL-3.0 (Q58, Q59, Q60). Ragon's own licence is AGPL-3.0-or-later, and the README and `Cargo.toml` say the same for this repository.

| Path | Holds | Language |
|---|---|---|
| `crates/rc-profile` | profile schema, handles, icons | Rust |
| `crates/rc-runner` | the stage machine, rounds, the configuration check, the `ragon-community` binary | Rust |
| `crates/rc-tool` | the machine tool (an MCP server) | Rust |
| `crates/rc-machine` | targets, the allowlist and denylist, the wipe client | Rust |
| `crates/rc-report` | reports, the signature, severity, deduplication, the GitHub filer | Rust |
| `crates/rc-filter` | the filter (§11) | Rust |
| `glue/` | the wipe helpers, the enrolment scripts, the task checker, the profile generation prompt | Python, shell, PowerShell |
| `site/` | the page builder and its templates | Python, HTML, CSS |
| `briefs/` | the stage briefs, one file per stage | text |
| `data/` | word lists, name lists, the footprint lists, `known-gaps.toml`, `compat.toml` | data |
| `tasks/` | `tasks.toml` and `fixes/` | data |
| `users/`, `threads/`, `reports/`, `results/` | the community's content | data |
| `quality/` | the weekly off-line grades (§0.2) | data |
| `tests/` and each crate's tests | offline tests (§12.4) | — |

**Not in it:** the full run records, held items, the address lists, machine names, the brain's key, or anything the filter would hold.

### 12.2 What stays in this repository

- this spec and its line in `specs/README.md`;
- the doc-step ids on the user site (§4.3), with their test;
- one advisory line in each release spec (§15);
- a pointer in `docs/dev/testing.md` once Phase 1 has run.

This repository never imports from `ragon-community`, and nothing in its suite runs the simulator.

### 12.3 Two ways in: code and content

| Path | What | How it lands |
|---|---|---|
| Code | crates, glue, site, briefs, data lists | the same landing rules as `dreamference/ragon` (Q66): a branch, a pull request, CI green, then the merge job's fast-forward. Commits are made by the community team bot, carry `Ragon-Tokens: N` and, where Ragon made the change, `Made-with: ragon` |
| Content | `users/`, `threads/`, `reports/`, `results/` | until §1 is answered, the local content repository only (§10.4). After option A, the runner commits straight to `main` as the community GitHub App, at most one commit per finished run and one per round summary, after the filter. CI refuses a content commit that touches any other path, and refuses one that the filter holds |

### 12.4 How it is tested

Following the rule that tests never touch the real machine, every test runs offline, with fakes:

| Test | What it checks |
|---|---|
| profile schema and mix | every profile loads; a bad field is refused with its name; a generated set keeps the mix |
| handles and icons | §3.3's refusals; 1,000 icons use only the fixed shapes |
| stage machine with a fake target and a recorded driver | stages run in order, caps end a stage, `stuck` is found, success comes only from the runner's marker |
| signature normaliser | a table of 30 error lines and their signatures |
| severity | one row per line of §7.2 |
| deduplication and filing, against a fake GitHub | a match comments, the daily limit queues, the switch and the bar refuse (§8.2) |
| the filter | §11's planted values never survive; holds happen |
| the security hold | each trigger of §11.2 holds |
| the footprint list against the installers | §6.2's diff |
| the wipe helpers, against a fake root (the helper takes a root prefix) | the account and every footprint item go; a non-`sim-` name and an unknown machine id are refused |
| allowlist and denylist | §4.2 and §6.5's refusals |
| configuration is Ragon-only | §5.1 |
| language check | Hangul, Han and Latin samples, code blocks left out |
| the site | §10.1's checks |
| content commits | CI refuses other paths |

`cargo test --workspace` and `python -m pytest` run all of them. **CI** runs the offline tests only and never reaches a node. **Live smoke:** each phase ends with one live round, which is never part of the suite.

### 12.5 Versions, and outside runs

- **Versions.** `ragon-community` has its own version, starting at `0.1.0` in Phase 0, with tags (Q65). `compat.toml` names the brain releases and the oldest target release that version supports, and the runner refuses an unsupported pair. Each round summary names the `ragon-community` commit that ran it, so a round can be run again with the same code.
- **Outside runs** (Q61, Q64). Anyone may run `ragon-community` on their own node with their own throwaway machines. The tool writes a results directory and **never sends anything**: it has no upload code. Its only network peers are the user's own node, the docs, the release page and the targets (test).
- **Submitting an outside run** takes a hand-made pull request adding `results/outside/<round>/`. That pull request is the explicit opt-in for that submission. CI runs the schema and the filter on it. The site marks such results "outside run". An outside report never becomes a GitHub issue by itself; it needs a reproduction by one of our runs (§8.4).

---

## 13. Safety

| Rule | Check |
|---|---|
| Throwaway accounts only: a run never acts on an account that is not `sim-NN` | wipe helper refusal (§6.2) |
| Tests never touch the real machine, and the fleet's primary is never a target, a brain or a lent box | the denylist by machine id, checked at `prepare` and before every `run` (test with planted ids) |
| Community machines never join the fleet's primary or its accounts (Q78) | at `prepare`, the community GB10's node lists no fleet node as a member |
| A lent secondary is the only fleet box a run touches, only while granted, and it is restored and checked (§6.5) | the restore check |
| Throwaway accounts never reach the fleet (§6.3). A node user's root on a lent secondary is held by the box-wide lend rules, checked afterwards rather than prevented (§6.5) | the `prepare` connection test; `lend-end`'s rule check |
| No key of ours on a target. The brain's service key lives only in the runner account on the community GB10 | a test checks that no stage brief, journal or diary contains it |
| Nothing is committed or filed unfiltered (§11) | the filter's tests and the CI re-check |
| No person, host name or IP address in anything published | the filter, plus the main repository's anonymity denylist in `ragon-community`'s pre-push hook and CI |
| Features that need an outside account (mail, remote access) are out of scope. The phone apps run only against stand-in servers (Q28) | the persona schema has no field for outside credentials |

---

## 14. When it runs

| Round | When | Where | Users |
|---|---|---|---|
| Candidate round | when a candidate's assets are published as a pre-release | the community machines, plus one lent secondary in the next day lane window for the team and node users | the core set and a random draw (§3.4) |
| Stable rounds | around the clock between candidates (Q68) | the community machines only | the next users in turn |
| Re-checks | the first candidate that contains a fix | as the original run | the finder (§8.4) |
| Reproductions | as soon as a report is `new` | a free slot on the same platform family | a different user |
| On demand | the coordinator starts a round for a branch build | as a candidate round | labelled `branch:<name>`; never on the site's Results page |

Only one community box runs at a time by default. More run at once only when they are idle (Q26). Benchmark nights keep priority on the fleet's boxes (Q69). The lent secondary follows the day and night split: a day lane window only, with the GPU lock released before 20:00 UTC.

---

## 15. Its place in the release gate

**Advisory, never blocking** (Q54). Each candidate's round writes one line in the release spec's gate list, for example:

`Community simulator (advisory): round rc-0.9.10-rc.2 (ragon-community 1a2b3c4), 14 users, 12 reached a working Ragon and finished the task; S1 0 open, S2 1 reproduced (row <id>).`

Reproduced S1 and S2 reports become rows, as night findings do. The fleet e2e and the person checks stay as they are. In December, when the simulator and the 1.0 gate compete for a box or a person's time, the gate wins (Q100).

---

## 16. Measures

### 16.1 Internal measures

| Measure | Definition |
|---|---|
| Defects found | reports reproduced per round (§8.4) |
| Confirmed by a person | rows from simulator reports that a person or the row's job closed as a real defect, per month |
| False-report rate | §8.2's rate; it decides the filing switch |
| Unsupported and dropped | model-judged findings dropped by §7.3, and posts dropped by §9.3, divided by all of them |
| Duplicates | issues closed as duplicates of another, divided by issues filed |
| Time to first success | from `install` start to the `first_use` marker, per user and platform, as the median of the user's last five runs. A rise of 50 % is a finding of its own |
| Doc steps that fail | distinct doc-step ids with a runner-observed finding, per round and per page |
| Doc coverage | doc-step ids exercised in the last five rounds, divided by all ids on the covered pages |
| Run health | runs `aborted`, or with a wipe `check` that failed, divided by all runs |
| Report quality | the weekly off-line grade of twenty reports (§0.2) |

### 16.2 What the site shows

- **The headline number** (Q80): the share of simulated users who reached a working Ragon and finished their first task, per platform and per release.
- **The task success rate** (Q75, Q81): per release only, with the line "not a benchmark score" in the same box.
- **No count of users or posts appears as a community figure anywhere** (Q99). The Results page shows run counts only next to the shares they belong to.

### 16.3 Tokens and RBRB (Q67)

- **Build tokens**, the work of writing `ragon-community`, count in RBRB, with the same trailers on its commits.
- **Testing tokens**, which the users spend on the brain's gate under the tag `community-sim/…`, are shown as their own figure next to the curve and are never mixed into the share. The figure comes from the community GB10's own gate, since that box is not in the fleet.
- **The weekly off-line grade** (§0.2) spends no Ragon tokens. It is listed among the coordinator's own costs.

---

## 17. Phases

Agent-days count building work only. **L** rows go to Ragon first, through the `task` tool, each with its one named test (RBRB's routing rule). **C** rows go to the cheapest model that can do them, with review. A failed L row falls back as RBRB §5.3 says.

### Phase 0: the core, the machines, one live run (by 4 December, 5 agent-days)

**Outcome:** the offline core passes its tests and the three community machines are enrolled. The stable brain runs on the community GB10. One Linux client user (`moss_lantern`, beginner, English) runs `discover`, `install`, `first_use` and a task against the latest stable from a fresh `sim-01` account, and the account is then wiped and checked. Its report files and diary are committed to the local content repository and shown in the preview built on the community GB10. Nothing is pushed to GitHub. The rows are in §18.

### Phase 1: ten users, Linux and node, terminal only (by 12 December, 6 agent-days)

**Outcome:** the core set of ten (Q94) runs on every 0.9.10 candidate. The set is six Linux client users and four node users, one team of four on a lent secondary, beginners weighted, with two careless users, two Codex switchers and one upgrade user. They have diaries, post English threads, write reports and are reproduced by a second user. The content stays local until §1 is answered, and is published as §10.4 says once it is.

| Row | What | Check | Who |
|---|---|---|---|
| 1.1 | the lent-secondary flow of §6.5 (lend, stop, model link, wipe, restore, check) | `cargo test -p rc-machine lend`; one live lend | C |
| 1.2 | team rounds and the house node's account clean-up (§6.4) | `cargo test -p rc-runner team_round` | C |
| 1.3 | the `talk` stage and the truth check (§9.3) | `cargo test -p rc-runner talk_truth` | L |
| 1.4 | diaries: write at `collect`, last five entries into the next run | `cargo test -p rc-runner diary` | L |
| 1.5 | reproduction scheduling and the states of §8.4 | `cargo test -p rc-report states` | C |
| 1.6 | the security hold (§11.2) | `cargo test -p rc-filter security_hold` | L |
| 1.7 | `web_text`, `web_fill`, `web_click` for `ragon web` | `cargo test -p rc-tool web`; one live `ragon web` question | C |
| 1.8 | the connection log and the S1 rule (§6.3) | `cargo test -p rc-machine netlog`; live: a planted call is logged | C |
| 1.9 | careless acts chosen by seed (§4.4) | `cargo test -p rc-runner careless` | L |
| 1.10 | upgrade from the previous release | live: one upgrade run | C |
| 1.11 | the measures, the headline number and the Results page (§16) | `python -m pytest site/tests/test_results.py` | L |
| 1.12 | the release-spec gate line and the candidate round command | one advisory round on a 0.9.10 candidate | C |
| 1.13 | the ten core profiles, generated by Ragon (§3.2) | `cargo test -p rc-profile mix` | L |
| 1.14 | twenty checked tasks (§4.5) | `ragon-community tasks check` passes on all twenty (live, recorded) | C |

### Phase 2: Mac, Windows, Korean and Chinese (by 27 December, 7 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 2.1 | the macOS wipe helper and the Mac client target on the community Mac mini | `python -m pytest glue/tests/test_wipe_macos.py`; one live run | C |
| 2.2 | the Windows wipe task and the Windows client target on the community Windows box | `python -m pytest glue/tests/test_wipe_windows.py`; one live `install.ps1` run | C |
| 2.3 | the footprint lists for macOS and Windows, with the installer diff | `cargo test -p rc-machine footprint` | L |
| 2.4 | Korean and Chinese users, handles, word and name lists, and the language check | `cargo test -p rc-profile handles_ko_zh`, `cargo test -p rc-runner language` | L |
| 2.5 | Korean and Chinese threads and categories | `python -m pytest site/tests/test_categories.py` | L |
| 2.6 | GitHub filing through the community app: limits, comments on a match, the switch and the bar (§8.1, §8.2) | `cargo test -p rc-report filer` against a fake GitHub | C |
| 2.7 | the weekly off-line grade's intake into `quality/` and the measures | `python -m pytest glue/tests/test_quality_intake.py` | L |
| 2.8 | air-gapped users (§6.3) | `cargo test -p rc-machine airgapped`; one live run | C |
| 2.9 | the entry-level node user on the Mac mini (a plain refusal until the docs offer it) | one live run | C |
| 2.10 | profiles grown to about 30, and tasks to about 50 | `cargo test -p rc-profile mix`; `ragon-community tasks check` | L, C |

### Phase 3: after the 1.0 gate (January, 6 agent-days)

| Row | What | Check | Who |
|---|---|---|---|
| 3.1 | desktop windows as text: macOS accessibility and Windows UI Automation helpers (Q31) | live: the desktop app's first window as a tree | C |
| 3.2 | phone users against stand-in messenger servers on the community GB10 (Q28) | live: one message round trip per messenger | C |
| 3.3 | about 100 users (Q10) | `cargo test -p rc-profile mix` | L |
| 3.4 | outside runs: the results format and the CI checks for hand-made pull requests (§12.5) | `python -m pytest tests/test_outside_results.py` | L |
| 3.5 | the quarterly task refresh | `ragon-community tasks check` on the new list | C |

**Total: 24 agent-days** (5 + 6 + 7 + 6). Roughly 23 of the 48 build rows are L. A row split between L and C counts as half, and the owner's steps are left out. Each phase is useful without the next.

---

## 18. The first week's rows (28 November to 4 December)

Every row is sized for Ragon's `task` tool unless it is marked C: one bounded change and one named test. The repository is empty, so W1.1 comes first. The live rows W1.17 and W1.18 wait for Monday's enrolment.

| Row | What | Test command | Who | Day |
|---|---|---|---|---|
| W1.1 | **the first commit to `dreamference/ragon-community`**: README (what it is, AGPL-3.0-or-later, the "simulated users" sentence, a link to this spec), a Cargo workspace with the six empty crates, `pyproject.toml`, CODEOWNERS, the anonymity pre-push hook and denylist check copied from this repository, and CI running both suites | `cargo test --workspace && python -m pytest -q` | C | Sat |
| W1.2 | `rc-profile`: the profile schema and loader, with refusals that name the field | `cargo test -p rc-profile schema` | L | Sat |
| W1.3 | handles: the word lists, the drawing, and the name and well-known-name refusals (§3.3) | `cargo test -p rc-profile handles` | L | Sat |
| W1.4 | icons: an SVG from the handle hash, fixed shapes, no face parts | `cargo test -p rc-profile icons` | L | Sun |
| W1.5 | `rc-filter`: steps 1 to 5 of §11.1, and the hold | `cargo test -p rc-filter` | L | Sun |
| W1.6 | `rc-report`: the report format, the sections of §7.1, the severity table | `cargo test -p rc-report format severity` | L | Sun |
| W1.7 | the signature normaliser with its 30-line table | `cargo test -p rc-report signature` | L | Mon |
| W1.8 | `rc-runner`: the stage machine with a fake target and a recorded driver, caps and `stuck` | `cargo test -p rc-runner stages` | C | Mon |
| W1.9 | the Ragon-only configuration check (§5.1) | `cargo test -p rc-runner config_ragon_only` | L | Mon |
| W1.10 | `rc-machine`: the allowlist and the denylist by machine id | `cargo test -p rc-machine denylist` | L | Mon |
| W1.11 | `rc-tool`: `run`, `term_*`, `fetch_doc` with its allowlist, `post`, `note`, and the output cap | `cargo test -p rc-tool` | C | Tue |
| W1.12 | the Linux wipe helper (`create`, `delete`, `check`, a root prefix for tests, the refusals) and the Linux footprint list | `python -m pytest glue/tests/test_wipe_linux.py` | C | Tue |
| W1.13 | **the enrolment scripts** (§19.2): the three for the community machines and the one for a lendable fleet secondary, as templates in `glue/enrol/`, plus the DNS script `glue/owner/dns-community.sh` (§19.3); a `--dry-run` that prints every action, and shellcheck and PowerShell parse checks. The owner's copies, with the names filled in, go to the coordinator's scratch directory and never into a repository | `python -m pytest glue/tests/test_enrol.py` | C | **Sat, for Monday** |
| W1.14 | `site/build.py`: forum layout, user, thread and report pages, the `notice` setting, preview mode, the no-script and no-form scan | `python -m pytest site/tests` | C (build), L (scan test) | Tue |
| W1.15 | **this repository:** doc-step ids on the install, first-use, client and upgrade pages, and their test. This row lands after the release freeze | `.venv/bin/python -m pytest tests/test_doc_step_ids.py` | L | Wed |
| W1.16 | **this repository:** the team `community` in `scripts/as_team.py`'s table, for the app of §19.1 step 3 | `.venv/bin/python -m pytest tests/test_as_team.py` | L | Wed |
| W1.17 | live, after enrolment: install the stable Ragon node on the community GB10 as the brain, with `--no-advertise`, and check its gate with the service key | `ragon doctor` on the box; one `ragon exec "say hi"` through the gate | C | Mon–Tue |
| W1.18 | live: Phase 0's run of `moss_lantern` (§17) | the run reaches `collect`; `check sim-01` exits 0; a person reads the report | C | Thu |
| W1.19 | the task list format and checker, with ten first tasks | `python -m pytest glue/tests/test_task_check.py` (offline, against a local fixture repository) | C | Thu |

---

## 19. The owner's steps

### 19.1 The list

| # | Step | When | Exact action |
|---|---|---|---|
| 1 | Answer the notice question (§1, §20 question 1) | before step 5 | option A or option B |
| 2 | Enrol the three community machines | Monday 30 November, as each one is on the LAN | run the box's script (§19.2), one per box, then reply "done" with the script's last line |
| 2b | Enrol the fleet secondary that may be lent | before Phase 1's first team round | run `enrol-lent-secondary.sh` (§19.2) with `sudo` on the one secondary the coordinator names, then reply with its last line |
| 3 | Create the community GitHub App | this week | in the organisation's settings: an App named `dreamference-community`, with no webhook. Permissions: Contents read and write, Pull requests read and write, and Workflows on `dreamference/ragon-community`; Issues read and write on `dreamference/ragon`, and nothing else there. Install it on those two repositories only, and hand its key to the coordinator by the same route as the other team apps |
| 4 | Add the DNS record for `community.ragon.ai` | after step 1 | run `glue/owner/dns-community.sh`, written in W1.13 (§19.3) |
| 5 | Turn on GitHub Pages for `dreamference/ragon-community` | after steps 1 and 4 | Settings, then Pages: source "GitHub Actions"; custom domain `community.ragon.ai`; "Enforce HTTPS" on once the certificate is issued |

**Removed by the ruling on static pages**, listed here so they read as dropped rather than missed: the server order, the board's admin account, the moderator, the sign-up and first-post approval, and the privacy page.

### 19.2 What each enrolment script does

The scripts are written in row W1.13. The machine names and the coordinator's public key are filled in the owner's copies only. Every script prints each step and its result, never silences a tool's output, and ends with one line `ENROLLED <role> <machine id>`.

| Step | Community GB10 (`enrol-community-gb10.sh`, run with `sudo`) | Community Mac mini (`enrol-community-mac.sh`, run in Terminal as the box's admin user) | Community Windows box (`enrol-community-windows.ps1`, run in PowerShell as administrator) |
|---|---|---|---|
| 1. Name | sets the host name the coordinator chose | sets the computer and local host name | renames the computer (the restart in step 7 applies it) |
| 2. Runner account | creates `community`, with no password login | creates `community` as a standard user | creates the local user `community`, a standard user |
| 3. Remote login | enables `ssh` and authorises the coordinator's key for `community` | opens System Settings › General › Sharing and asks the owner to switch **Remote Login** on by hand, for `community` only. It never calls `systemsetup` silently (the lesson of 20 November). Then it authorises the key and waits until port 22 answers | adds the OpenSSH Server capability, sets `sshd` to start automatically, authorises the key for `community` |
| 4. Wipe helper | installs `/usr/local/sbin/ragon-community-wipe`, root-owned, with one sudoers line for `community` limited to it, and writes this machine's id into the helper's enrolment list | the same, for macOS | installs `wipe.ps1` and its scheduled task with the highest privileges, which `community` may start |
| 5. Network | installs the nftables rules of §6.3 for the `sim-` user ids, empty at first; the runner fills the address list | turns on the application firewall and installs the box-wide rules of §6.3 | adds the Windows Firewall rules of §6.3 |
| 6. Power | no sleep | `pmset` sleep off on power | sleep off on mains power |
| 7. Leaves alone | GPU driver, operating-system updates | **no** developer tools or package managers system-wide, so users meet the docs' prerequisites as a stranger would | Smart App Control and SmartScreen as shipped, so users meet them as a stranger would; then a restart, needed for the ssh server |

**The lendable fleet secondary** (`enrol-lent-secondary.sh`, run with `sudo` on the one secondary the coordinator names) does steps 2, 4 and 5 only. It creates `community` with the coordinator's key, installs the wipe helper with `lend-begin` and `lend-end`, and installs the lend rules of §6.5, switched off. It leaves the box's name, power settings, its own Ragon and its fleet membership as they are.

None of the scripts joins a machine to the fleet or takes one out of it, installs Ragon, or stores a password.

### 19.3 The DNS script

The registrar of `ragon.ai` is not on record, so the script is defined by its effect:
1. It prints the two records to add at the registrar:
   - `community.ragon.ai.  CNAME  dreamference.github.io.`
   - `_github-pages-challenge-dreamference.community.ragon.ai.  TXT  <value shown in the organisation's Pages settings when the domain is added>`. This TXT record verifies the domain, which stops anyone else's Pages site from taking it.
2. It waits until `dig +short CNAME community.ragon.ai` answers `dreamference.github.io.` and the TXT record answers, checking once a minute for at most an hour.
3. It prints `DNS OK` or the record still missing.

---

## 20. Open questions for the owner

The owner's answers settle everything else. These remain, each with the recommended answer first.

1. **The site's notice: the conflict of §1.** *Recommended: option A. The site's name says "simulated" in every top bar and title, an About page in the navigation says these are simulated users run by Ragon to test Ragon, and the site is indexed.* Option B: no notice, but not indexed and not linked. The coordinator will not publish the site with no disclosure and full indexing, because that conflicts with Q98 and Q99. Owner to decide. Until then nothing is deployed (§10.4).
2. **Q40, the automatic helper for real people's questions.** Real people now ask on the main repository's GitHub Discussions, which open at 1.0, and no longer on this site. *Recommended: the clearly marked Ragon helper answers there first and a maintainer follows, as Q40 said. It goes into its own spec for 1.0 and is not built here.*
3. **One app for commits and issues.** *Recommended: one GitHub App, `dreamference-community`, serves as the community team bot for code commits (Q66) and as the filer of issues in `dreamference/ragon` (Q49). Its rights on the main repository are Issues only.* The alternative is two apps, which keeps the issue filer's identity apart from the committer's.

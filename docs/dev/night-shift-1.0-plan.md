# Night Shift for 1.0: the polish plan

Night Shift is one of the 1.0 headlines: "give it tasks at night" (owner, Q31). This note is the plan that takes it from a working feature to one that is shown first: the polish items, the docs page, the QA run and the demo recorded for the site (Q77). The lane briefs are the rows of the coordinator's backlog file `plan-night-shift-headline`; each row below names its label. How Night Shift works today: [night-shift.md](night-shift.md) and `specs/DREAMFERENCE_RAGON_NIGHT_SHIFT.md` §11.

## What is fixed already

These are the owner's decisions; the plan builds on them and does not reopen them.

- Night Shift is a 1.0 headline: polish, a demo and docs (Q31).
- The demo is three tasks on a real open-source repository, queued at night, with the morning report shown, recorded as a short video for the site (Q77). The coordinator records it in a terminal with no voice, and the owner approves it before anything is published (Q217).
- Best-of-N stays at three attempts by default (Q215).
- The chooser stays v1 for 1.0; a model judge is 1.1 work (Q216). No change to how an attempt is chosen is in this plan.
- The morning report goes to the phone when a messenger is on, opt-in in the config (Q84): that lane already exists (`night-report-to-phone`) and is referenced here, not planned again.

## Where it stands

Read from the code and the spec on main:

- **Queueing** (`ragon-rs/src/night.rs`) knows `add` (with `--test`, `--on`, `--best-of`), `list`, `show`, `apply`, `drop` and `report` (`USAGE`). `list` shows the current repository only. A task that stalled, failed, made no change or was interrupted can only be queued again by typing it again. Nothing tells the user, at `add`, where the task stands in the queue or when the next window opens; `list` prints the window on its last line.
- **The morning report** (`night_shift_report.py`) has one block per task (status, branch, changes, tests, attempts, best-of lines) and a Review section. It has no summary at the top: a reader learns how the night went only by reading every block. The TUI's start line (`Night Shift: 3 done, 1 stalled — /night report`) carries a summary the report itself lacks.
- **The chooser** (`NightShiftChooser`) runs the regression filter (attempts whose tests failed or timed out are dropped, unless all were), then the pairwise tournament of chooser v1 on the night's own model server. The report's `Best of N` and `Why` lines name the stage and the points. Only the chosen attempt is kept as a branch; the others survive as `logs/<id>-a<k>.diff`, which no command applies.
- **The desktop app** shows night tasks as cards (`NightCards.tsx`, `nightthread.ts`): queued, running, then attempts, the chosen branch and the task's block of the report.
- **Docs.** The user site has no Night Shift page: one row in the table of `docs/ragon.md` and the `ragon-admin night` reference in `docs/admin.md`. The site's front page (`website/index.html`) has a Night Shift tab with text only.
- **Never run live** (spec §11.4): the installed timer firing by itself, and a stall provoked and nudged. A demo "queued at night" needs the first one proven.

## The polish items

Three lanes, each small enough for one job, each with its own test. They run before the docs page, the QA run and the demo, because those show and describe the result.

### 1. The morning report: a summary first (`nsh-report-summary`)

- A summary block right under the title: the count per status in the TUI start line's words, then one line per task in the order a reader acts on them (ready to apply, then needs a look, then nothing to do): the id, the task's first line, the verdict in words ("tests pass, 3 files changed", "stalled after 2 nudges", "no change") and the one next command (`ragon night apply <id>`, `ragon night show <id>`, `ragon night retry <id>` once lane 2 has landed).
- The blocks below stay as they are; the Review section keeps its commands.
- The TUI's start line and the summary share one wording: one function in the report module, and the launcher's counter changed only if its words differ.
- Runs before `night-report-to-phone` (same file): the phone then sends this summary as its one message.

### 2. The queue: retry, every repository, and where a task stands (`nsh-queue-ux`)

- `ragon night retry <id>` (and `/night retry <id>`): queues a stalled, failed, no-change, interrupted or cancelled task again as a new task with the same text, test command and `best_of`, at the repository's current `HEAD`; the new task records the id it retries, and `show` names both. A queued or running task is refused.
- `ragon night list --all`: the queue of every repository, grouped by repository, so the queue can be read from anywhere.
- `add` says where the task stands: its place in the queue and the next window (or that Night Shift is not enabled), in the line it prints already.
- `USAGE`, the launcher's tests and `docs/ragon.md`'s row change with it. Patch `0018` changes only if the slash popup's text has to, inside the size cap.

### 3. The chooser, legible (`nsh-chooser-legible`)

No change to how an attempt is chosen (Q216). The work is what the user sees and can do when they disagree:

- `ragon night show <id>` lists every attempt: its diff stat, its test result, its points, which one was chosen and why, and the path of its kept diff.
- `ragon night apply <id> --attempt <k>` applies attempt `k`'s kept diff to the checkout under the same all-or-nothing rule as `apply`, so a user who prefers another attempt does not need the files under `logs/`.
- The report's `Why` line says in plain words which stage decided ("the only attempt whose tests passed", "the judge preferred attempt 2 over 1 and 3"), never only a stage name.

### Also in the app (`nsh-desktop-cards`)

After lanes 1–3: the desktop app's night card shows the task's summary line from lane 1 and, on a finished task, the `retry` and `apply --attempt` commands as copyable text. No new IPC message: if one turns out to be needed, it goes into `ragon-rs/web/policy.json` with its conformance cases, and the lane stops to ask first.

## The docs page (`nsh-docs-page`)

A user page `docs/night-shift.md` in the nav under "Use Ragon", written once lanes 1–3 have landed:

- what Night Shift does and does not do (a branch per task, nothing merged or pushed);
- switching it on: `ragon-admin night enable`, the window, lingering, and why a client cannot queue;
- queueing: `/night add`, `--test`, `--best-of`, `list --all`, `retry`, `drop`;
- what stops or delays a night (an open session, an outside request, memory, the window);
- the morning report, as an annotated sample with no real paths or names;
- bringing work back: `apply`, `apply --attempt`, and how the chooser picked;
- repeating tasks and autofill, in a paragraph each with a link to `admin.md`;
- troubleshooting: the sandbox profile under the timer, a test that needs the network or Docker, "not a node";
- the demo video, embedded once the owner has approved it (lane `nsh-site-video`).

`docs/ragon.md`'s table row then links to the page instead of describing it.

## The QA run (`nsh-qa-arm`, `nsh-qa-morning`)

A real night on a pool box running a release that carries lanes 1–3, armed in the evening and read in the morning, two jobs:

- **Arm** (evening): install the release with `ragon update`, check the sandbox profile and lingering, `night enable` with the box's window, and queue on a scratch clone of the demo repository: two plain tasks (they run side by side when the pool allows), one with `--best-of 3`, one worded to invite a plan-only reply (to try a stall), and one too long for a short window (to be interrupted and resumed on the second night). Nothing is started by hand: the installed timer must fire.
- **Morning**: the timer fired by itself (the journal says so); the model server stayed up and earlyoom logged no kill; every task has a branch or an honest status; the summary block, `list --all`, `retry`, `apply`, `apply --attempt` and the TUI's start line behave as the docs page says; the desktop app's cards match the report; with a messenger on and `report_to_phone` set, the summary arrives as one message. Whether the stall was provoked is recorded either way.
- The results are added to spec §11.4 ("Measured runs") by the morning job, and every defect found becomes a backlog row before the demo is recorded.

## The demo (`nsh-demo-dryrun`, `nsh-demo-record`, `nsh-site-video`)

**The repository.** A real open-source project, chosen by the dry run against these criteria:

- a permissive licence, and a repository whose maintainers' names do not appear on screen beyond what its own files show;
- a test suite that passes inside the night sandbox: no network, no Docker, nothing downloaded into the home directory (spec §11.1), in a few minutes at most;
- small enough to read on screen, pinned at one commit;
- three tasks a viewer recognises as real work and the default model finishes reliably: for example a bug with a failing test, a small missing option with its test, and a documentation or typing fix checked by the suite.

Default: a small pure-Python library with a pytest suite, picked by the dry run among two or three candidates. The repository and commit go into the demo script, not into the code.

**The dry run** (`nsh-demo-dryrun`): the three tasks run in a daytime window on a pool box with the release; each must end `done` with its tests passing, in two runs out of two, or the task is replaced. Output: the demo script (repository, commit, the three `/night add` lines, the expected report) under `/mnt/ragon-store/work/demo/`.

**The recording** (`nsh-demo-record`, the coordinator, Q217): in the evening, `/night add` three times in a `ragon` session, `ragon night list`; the night runs from the timer; in the morning, `ragon` prints its start line, `ragon night report` shows the summary, then one `ragon night apply` and the test run in the checkout. A neutral prompt, no host or user name, no voice, under three minutes, cut between evening and morning. Exported as `.cast` and `.mp4` under `/mnt/ragon-store/work/demo/` and sent to the owner. Nothing is published before the owner approves it.

**The site** (`nsh-site-video`, after the owner's approval): the video in the front page's Night Shift tab and on the docs page. Default format: an `.mp4` with a poster frame and no autoplay sound, small enough for the repository (a few MB); the `.cast` stays the source. The tab's caption follows the site's translations.

## Order

1. `nsh-report-summary` and `nsh-queue-ux` side by side (different files), then `nsh-chooser-legible` (it touches both). Then `night-report-to-phone` and `nsh-desktop-cards`.
2. A release that carries them.
3. `nsh-docs-page`, `nsh-demo-dryrun` and `nsh-qa-arm` (the dry run and the QA can share a box on different days).
4. `nsh-qa-morning`; its defects fixed.
5. `nsh-demo-record`, the owner's approval, then `nsh-site-video`.

## Decision points and their defaults

- Demo repository: chosen by the dry run against the criteria above; default a small pure-Python library with a pytest suite.
- A stall that cannot be provoked live: recorded as not provoked; the unit test stays the evidence.
- `apply --attempt` on a task whose kept diff no longer applies: refused with the conflicting files named, as `apply` does.
- Video format on the site: `.mp4` with a poster frame; the `.cast` kept as the source.

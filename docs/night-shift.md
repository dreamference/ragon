# Night Shift

Night Shift works through tasks you queue during the day while the model is otherwise idle. Each task
runs in its own git branch and worktree; **nothing is merged, pushed or rebased**. In the morning you
read a report and bring back the changes you want.

<!-- video: placeholder for the Night Shift walkthrough (added by the site page) -->

## Turn it on

```bash
ragon-admin night enable                       # install the timer; window 01:00-07:00 by default
ragon-admin night enable --window 23:00-06:00  # another window (HH:MM-HH:MM)
ragon-admin night status                       # the timer, the window and every repository's queue
ragon-admin night disable                      # remove the timer
```

The timer runs `ragon-admin night run`, which works through the queue until the window ends. You can
start it by hand any time; it ends at the window's end too.

## Queue a task

In a repository, in `ragon`:

```text
/night add Fix the off-by-one in the pager and add a test
/night add --test "pytest tests/test_pager.py -q" Fix the off-by-one in the pager
/night list
/night show <id>
/night drop <id>
```

Queueing never calls the model. `--test` names the command that must pass for the task; without one
the model's own comparison decides between attempts. The same commands work from a shell as
`ragon night add …`, `ragon night list` and so on. `--best-of N` sets the number of attempts for one
task, and `--on <node>` sends it to a paired node (worked once there).

## What stops a night

- The **window** ends. An attempt after the first does not start when less of the window is left than
  the longest attempt took, and the report says when fewer attempts ran.
- An open `ragon` session or an outside request to the model: Night Shift gives way to you.
- A running benchmark holds the model. `ragon-admin night pause` (one hour; `--for 2h` for longer)
  lets you work meanwhile, and `ragon-admin night resume` hands it back.
- A task's own time limit, or its tests failing in every attempt, fails that task and the run goes on
  to the next.

Night Shift never starts or loads the model server.

## The morning report

`/night report` (or `ragon night report`) shows the latest report. Annotated sample:

```text
# Night Shift report                                  <- the night's report, one file per run
## example-repo                                       <- tasks grouped by repository
### Fix the off-by-one in the pager                   <- the task's first line
- Branch: `night/t1`, not merged; review it, then    <- the chosen result lives on this branch
  `ragon night apply t1`
- Tests: `pytest tests/test_pager.py -q` (task): passed
- Attempts: 1, nudges: 0, wall time: 14 min
- Best of 3: 3 ran; chose attempt 2 by judge         <- how the chooser decided
  - Why: attempt 2 also fixed the second caller       <- the judge's reason
  - attempt 1: done, 2 files, tests failed, 12 min    <- dropped: its tests failed
  - attempt 2: done, 2 files, tests passed, 14 min, chosen: the branch
  - attempt 3: done, 3 files, tests passed, 15 min
- Last message: the agent's closing note
## Notes                                              <- anything odd about the night
```

Every attempt keeps its diff and logs; the report names where.

## Bring a result back

```bash
git diff --stat main...night/<id>     # look first
ragon night apply <id>                # or /night apply <id>
ragon night apply <id> --attempt 2    # an attempt other than the chosen one
```

`apply` puts the changes into your checkout as **uncommitted changes**, all or nothing: if they
conflict with your own changes it refuses and changes nothing. Review and commit them yourself.
`--attempt N` takes attempt N instead of the chosen one, when you prefer another's approach.

## The chooser, in words

Each task is attempted three times by default, one after another (`[night] best_of` or
`--best-of N` change it; 1 is a single run). Then:

1. Attempts whose tests failed or timed out are dropped (if all failed, all stay).
2. One survivor is chosen outright.
3. Several survivors are compared in pairs by the model, and one wins.

Only the chosen attempt becomes the branch `night/<id>`. Three attempts take about three times as
long as one, so a short window may fit fewer.

## Repeating tasks

```bash
ragon-admin night every --daily -- Run the slow tests and fix what is flaky
ragon-admin night every add --weekly mon --test "pytest -q" -- Update the changelog
ragon-admin night every list
ragon-admin night every drop <id>
```

Cadences: `--daily`, `--weekly <day>`, `--days mon,thu`, `--every <N>` nights, `--monthly <D>`.
Each night's run queues the templates that are due, as ordinary tasks.

## Let it fill its own queue

Off by default. With `[night] autofill = true` in `dreamference.toml`, each night starts by queueing up
to five small tasks (`[night] autofill_max`, 1 to 5) from the failing tests of the latest failed CI
run, the desktop QA report and unchecked items in the repository's `specs/` documents.

```bash
ragon-admin night autofill --dry-run    # what it would queue
```

Autofilled tasks are marked `[autofill:...]` on their first line. Review them like any other result.

## Tidying up

```bash
ragon-admin night archive              # move runs older than four weeks away; branches stay
ragon-admin night archive --weeks 8
```

## Troubleshooting

- **Nothing ran.** `ragon-admin night status` shows the timer and the queue. A task that was queued
  after the window opened waits for the next night.
- **"The model is busy".** A benchmark or an open session holds it; see *What stops a night*.
- **A task failed in the sandbox.** Run from the timer, the sandbox needs the AppArmor profile that
  `ragon-admin host setup` installs.
- **`apply` refuses.** Your checkout has changes that conflict; commit or stash them and try again.
- **Fewer attempts than expected.** The window was too short; the report's `Why` line says so.

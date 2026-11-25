# Night Shift — 2015-10-01

Run started 01:00, finished <HH:MM>.

## Summary

1 done

- 20151001-0100-abc (ready to apply): Add hello.txt — finished, its changes are on a branch; `ragon night apply 20151001-0100-abc`

## <REPO>

### 20151001-0100-abc — done

- Task: Add hello.txt
- Branch: `night/20151001-0100-abc`, not merged; `ragon night apply 20151001-0100-abc` brings it into your checkout
- Changes: 1 file changed, 1 insertion(+)
- Tests: `grep -q hi hello.txt` (--test): passed, sandbox: workspace-write
- Attempts: 1, nudges: 0, wall time: <T>
- Best of 3: 3 of 3 attempts ran; chose attempt 1 by judge skipped: no model server; points a1 0.05, a3 0.05
  - Why: judge skipped: no model server, so the tie went to the attempt whose tests passed, then the earliest
  - Attempt 1: candidate, 1 file changed, 1 insertion(+), tests passed, <T>, chosen: the branch, log `<NIGHT>/logs/20151001-0100-abc-a1.jsonl`, test log `<NIGHT>/logs/20151001-0100-abc-a1.test.log`
  - Attempt 2: candidate, 1 file changed, 1 insertion(+), tests failed (exit 1), <T>, diff `<NIGHT>/logs/20151001-0100-abc-a2.diff`, log `<NIGHT>/logs/20151001-0100-abc-a2.jsonl`, test log `<NIGHT>/logs/20151001-0100-abc-a2.test.log`
  - Attempt 3: candidate, 1 file changed, 1 insertion(+), tests passed, <T>, diff `<NIGHT>/logs/20151001-0100-abc-a3.diff`, log `<NIGHT>/logs/20151001-0100-abc-a3.jsonl`, test log `<NIGHT>/logs/20151001-0100-abc-a3.test.log`

## Review

- `git -C <REPO> diff <BASE12>..night/20151001-0100-abc`
- Nothing was merged into your checkout. `ragon night apply <id>` (or `/night apply <id>`), run in the repository, brings a task's changes there as uncommitted changes; it refuses, changing nothing, if they conflict with yours.
- `git worktree list` shows the worktrees kept for interrupted tasks.

## Notes

- a note

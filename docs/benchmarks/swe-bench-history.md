# SWE-bench: history of every published number

Every published SWE-bench number, with its history and its spread, on one page: one table per slice, every run on it in date order, and the spread between the runs.

## Slice 1 (39 tasks, `docs/dev/swe-bench/verified357-1.txt`)

| Date | Run | Build / engine | Recipe | Resolved | Timeouts | Gross flips vs previous |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-07 | 0.9.0 weekend | 0.9.0 / ragon-serve (SGLang then) | one engine session per agent, function-call exec tool, apply-patch off, 60-minute task limit | 30 of 39 | n/a | first |
| 2026-11-09 | 0.9.1 night (a) | 0.9.1 / ragon-serve | one engine session per agent, 60-minute task limit | 28 of 39 | n/a | 2 of 39 |
| 2026-11-10 | 0.9.1 night (b) | 0.9.1 / ragon-serve | one engine session per agent, 60-minute task limit | 24 of 39 | n/a | 4 of 39 |
| 2026-11-10 | 0.9.4 (1st) | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 28 of 39 | 3 timeouts, 1 starved | 4 of 39 |
| 2026-11-10 | 0.9.4 (2nd) | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 26 of 39 | 3 timeouts, 1 starved | 2 of 39 |
| 2026-11-11 | 0.9.4 (3rd) | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 27 of 39 | 3 timeouts, 1 starved | 1 of 39 |
| 2026-11-11 | 0.9.4 (4th) | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 30 of 39 | 3 timeouts, 1 starved | 3 of 39 |
| 2026-11-12 | 0.9.5 image | 0.9.6 / ragon-serve, engine image 47277016 (session-protect) | two session slots, 60-minute task limit | 28 of 39 | 0 timeouts, 0 starved | 2 of 39 |

Spread: its 8 runs resolve 24 to 30 of 39; 1 to 4 gross flipped tasks between its runs.

## Slice 2 (106 tasks, `docs/dev/swe-bench/verified357-2.txt`)

| Date | Run | Build / engine | Recipe | Resolved | Timeouts | Gross flips vs previous |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-07 | 0.9.0 weekend | 0.9.0 / ragon-serve (SGLang then) | one engine session per agent, function-call exec tool, apply-patch off, 60-minute task limit | 70 of 106 | n/a | first |
| 2026-11-09 | 0.9.1 night | 0.9.1 / ragon-serve | one engine session per agent, 60-minute task limit | 67 of 106 | n/a | 3 of 106 |
| 2026-11-11 | 0.9.4 | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 69 of 106 | 4 timeouts, starved not checked | 2 of 106 |

Spread: its 3 runs resolve 67 to 70 of 106; 2 to 3 gross flipped tasks between its runs.

## Slice 3 (106 tasks, `docs/dev/swe-bench/verified357-3.txt`)

| Date | Run | Build / engine | Recipe | Resolved | Timeouts | Gross flips vs previous |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-07 | 0.9.0 weekend | 0.9.0 / ragon-serve (SGLang then) | one engine session per agent, function-call exec tool, apply-patch off, 60-minute task limit | 76 of 106 | n/a | first |
| 2026-11-09 | 0.9.1 night | 0.9.1 / ragon-serve | one engine session per agent, 60-minute task limit | 73 of 106 | n/a | 3 of 106 |
| 2026-11-11 | 0.9.4 | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 69 of 106 | 4 timeouts, 1 starved | 4 of 106 |
| 2026-11-12 | 0.9.5 image | 0.9.6 / ragon-serve, engine image 47277016 (session-protect) | two session slots, 60-minute task limit | 69 of 106 | 0 timeouts, 0 starved | 0 of 106 |

Spread: its 4 runs resolve 69 to 76 of 106; 0 to 4 gross flipped tasks between its runs.

## Slice 4 (106 tasks, `docs/dev/swe-bench/verified357-4.txt`)

| Date | Run | Build / engine | Recipe | Resolved | Timeouts | Gross flips vs previous |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-07 | 0.9.0 weekend | 0.9.0 / ragon-serve (SGLang then) | one engine session per agent, function-call exec tool, apply-patch off, 60-minute task limit | 72 of 106 | n/a | first |
| 2026-11-09 | 0.9.1 night | 0.9.1 / ragon-serve | one engine session per agent, 60-minute task limit | 69 of 106 | n/a | 3 of 106 |
| 2026-11-11 | 0.9.4 | 0.9.4 / ragon-serve, integration build 7d831bf | one engine session per agent, 60-minute task limit | 75 of 106 | n/a | 6 of 106 |
| 2026-11-12 | 0.9.5 image | 0.9.6 / ragon-serve, engine image 47277016 (session-protect) | two session slots, 60-minute task limit | 69 of 106 | 0 timeouts, 0 starved | 6 of 106 |

Spread: its 4 runs resolve 69 to 75 of 106; 3 to 6 gross flipped tasks between its runs.

## Method

Every run on this page is a single `ragon-admin swe-bench run` of one slice of the 357-task validated SWE-bench Verified set on this machine's own GB10, graded by the harness in the instance's own arm64 container; the slice list is the file named under each table. Each number is the count of instances whose patch the grader judged as resolving the issue, out of the slice's tasks. Runs are appended to docs/swe-bench-runs.json after they are graded; this page is regenerated from that file by scripts/swe_bench_history.py. A timeout is a task that reached its time limit; a starved timeout is one that never received its first reply (a session-protect stall), counted apart. A gross flip between two runs of the same slice is an instance resolved in exactly one of the two. None of these numbers is a leaderboard score; the spread between runs of the same build is the noise floor, and differences inside it are not measurable.

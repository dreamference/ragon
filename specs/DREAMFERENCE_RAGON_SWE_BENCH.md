# Ragon on SWE-bench — `ragon-admin swe-bench`

**Status:** Phase 1 implemented in `dreamference/swe_bench/`, with `ragon-admin swe-bench {setup,smoke,run,eval,report,status,clean}` (since extended with `build-images`, `shard` and `merge`). §1–§11 are the design as specified; **§12 records what was built, what Phase 0 measured, and where the build departs from the design**, and wins where the two disagree. Built since §12: per-repository and chunked image cycling (`--remove-images`, `--image-chunk`, `SweBenchImageCache`, §15, §27, §30) and local arm64 image builds for Multilingual (`swe-bench build-images`, `SweBenchImageBuilder`, §31). Not built: local image builds for Verified (impossible on arm64 as upstream ships them), the mini-SWE-agent baseline column.
**Target:** the `ragon` terminal agent and the model it is served by, measured on the GB10 itself.
**Command:** `ragon-admin swe-bench {setup,build-images,smoke,run,eval,report,status,clean,shard,merge}`. It lives in `ragon-admin`, not in the `ragon` binary (§2 says why).
**Builds on:**
- `ragon exec --json` ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md));
- Night Shift's host probes, admission, memory-capped scopes and runner lock ([RAGON_NIGHT_SHIFT §5, §11](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md));
- the per-task compaction limit ([RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md));
- `VLLMServerManager.check_host_safety()` ([INFERENCE](./DREAMFERENCE_INFERENCE.md));
- the upstream harness, `swebench` 5.0.2 (PyPI), and its task repository `SWE-bench/swe-bench-tasks`.

---

## 1. Goal

One command runs `ragon` over SWE-bench instances on this machine and says how many it resolved:

- the agent gets an issue and a repository, and produces a patch;
- the upstream harness applies the patch and runs the instance's tests;
- a report gives the resolved rate, with everything needed to reproduce it.

**What the number is for.** Comparisons *on this machine*: one model against another, a prompt change, cave mode on and off (RAGON_CAVE_MODE (spec removed)), a retrained drafter ([SELF_SPEEDING](./DREAMFERENCE_SELF_SPEEDING.md)), a Codex bump. `benchmark_server` measures tokens per second; nothing today measures whether the agent's work is right.

**What the number is not.** A leaderboard score. §8 lists why, and the report prints those reasons beside every figure.

**Non-goals:**
- **Submitting to a leaderboard.**
- **Other benchmarks** (SWE-bench Pro, SWE-bench-Live, Multimodal). The layout of §4 leaves room for a second dataset; none is specified.
- **Running during interactive use.** A run gives way to it, as Night Shift does.
- **Starting, stopping or loading the model server.** A run measures whatever is being served.

---

## 2. Where the command lives

**`ragon-admin swe-bench`, in Python.** Three reasons:

1. **Long-running Docker orchestration already lives there:** `benchmark_server`, `night run`, `codex test`. The host probes, scopes and locks a benchmark run needs are Python classes (`NightShiftHost`, `NightShiftQueue.runner_lock`).
2. **`ragon` is Codex.** A subcommand of the binary is either a Codex patch, and the series stands at 26,933 of its 27,500 bytes, or launcher code that would have to reimplement those probes in Rust.
3. **The harness is a Python package** with heavy dependencies (`datasets`, `docker`), which belongs in a virtualenv of its own beside `ragon-admin`, not in a Rust binary.

**Open (§10.1):** the user asked for a "ragon command line command". The launcher already handles `ragon night …`, `ragon app` and `ragon update` before Codex parses its arguments, so `ragon swe-bench …` could exec `ragon-admin swe-bench …` from `ragon-rs/` at no patch cost. It is not specified here because it adds nothing but a second spelling.

---

## 3. The commands

| Command | What it does | Needs the model |
|---|---|---|
| `swe-bench setup` | Creates the harness virtualenv, clones the task repository, downloads the dataset, checks Docker, disk and architecture. Builds or pulls no instance image. | no |
| `swe-bench build-images [--dataset multilingual] [--instances …]` | Builds arm64 instance images locally from the multilingual task repository (`SweBenchImageBuilder`, §31). | no |
| `swe-bench smoke` | Proves the whole pipeline on a handful of instances (§7.1). `run` refuses until a smoke has passed on this machine with the current harness version. | yes |
| `swe-bench run [--dataset verified] [--instances …] [--limit N] [--name <run>]` | The agent phase: one `ragon exec` per instance, producing `predictions.jsonl`. Resumes a run of the same name. | yes |
| `swe-bench eval <run>` | The grading phase: the upstream harness applies each patch and runs the tests. | no |
| `swe-bench report [<run>] [--against <run>]` | Prints the resolved rate and the manifest; with `--against`, the per-instance difference between two runs. | no |
| `swe-bench status` | Runs, their progress, images and disk used. | no |
| `swe-bench clean [<run>] [--images]` | Removes a run's containers and scratch; with `--images`, the instance images. | no |

**Why `run` and `eval` are separate:**
- `eval` uses no model, so it can run beside the server with different parallelism, or with the server stopped.
- A run can be graded again, after a harness fix or with a different image source, without paying for the agent a second time.
- An interrupted `run` loses nothing that was graded, and an interrupted `eval` loses no prediction.

`swe-bench run --eval` does both in order, for the unattended case.

**Datasets.** `--dataset verified` (500 instances, human-validated, all Python) is the default because it is the one most numbers elsewhere are quoted on. `verified` and `full` (2,294) are the harness's own aliases. `lite` (300) is not one: this command maps it to the dataset id `SWE-bench/SWE-bench_Lite`, which the harness passes through. Multimodal is out of scope; Multilingual (`--dataset multilingual`, `SWE-bench/SWE-bench_Multilingual` at revision `846e647b9f33`, 300 instances across 9 non-Python languages, built locally from `SWE-bench/swe-bench-multilingual-tasks` at `6e08cbcd763c` by `swe-bench build-images`) was added (§31).

**Selecting instances.** `--instances a,b,c`, `--limit N` (the first N of a fixed order: sorted by `instance_id`, so two runs with the same limit cover the same instances), or `--subset <file>` of ids. The default for `run` is every instance the machine can evaluate (§5.3).

---

## 4. Files

Everything is under `~/.local/share/dreamference/swe-bench/` (results) and `~/.cache/dreamference/swe-bench/` (rebuildable):

```text
~/.cache/dreamference/swe-bench/
  venv/                     the harness, pinned (swebench==5.0.2)
  swe-bench-tasks/          the task repository, at a pinned commit
  datasets/                 the dataset snapshot, at a pinned revision
  validated-<arch>.json     which instances pass their gold patch here (§5.3)

~/.local/share/dreamference/swe-bench/runs/<run>/
  manifest.json             what was measured (§6.4)
  instances/<id>.json       one instance's state, replaced whole
  logs/<id>.jsonl           ragon exec's events
  predictions.jsonl         appended, one line per finished instance
  eval/<n>/                 the harness's logs/evaluation/<run_id>/ for grading n
  report.md                 written by `report`
```

`predictions.jsonl` is the harness's own format: `instance_id`, `model_name_or_path`, `model_patch`. An instance whose agent run failed or produced nothing is written with an empty patch, so it counts as unresolved and is never silently missing from the denominator.

**`model_name_or_path`** is `ragon-<codex tag>-<patch series hash, 8 hex>/<served model id>`, so a result stays attributable to the build and the model that produced it.

---

## 5. The agent phase (`run`)

### 5.1. One instance

The agent runs **inside the instance's own image**, not in a checkout on the host:

- SWE-bench repositories need their pinned environment to run their tests, and an agent that cannot run tests is not the agent being measured. Night Shift's answer, the main checkout's `.venv`, has no analogue here.
- The image has the repository at `/testbed`, at the instance's `base_commit`, with its dependencies installed.

Steps:

1. **Start a container** from the instance image (§5.3):
   - network: `ragon-swe-bench`, an internal Docker network (§5.2);
   - mounts: the installed `ragon` binaries read-only at `/opt/ragon`, a fresh `CODEX_HOME` per instance, the instance's log directory;
   - limits: `--memory` and `--memory-swap` equal to `[swe_bench] task_memory`, `--cpus 4`, `--pids-limit`;
   - environment: `DREAMFERENCE_VLLM_HOST` pointing at the network's gateway, and `RAGON_NIGHT_RUN=1` so the run's own sessions are never taken for someone working.
2. **Remove what gives the answer away.** In `/testbed`: confirm `HEAD` is `base_commit`, delete every other ref and the reflog, and run `git gc --prune=now`, so the fixing commit cannot be found in the object store.
3. **Run the agent:** `ragon exec --json --dangerously-bypass-approvals-and-sandbox -C /testbed -c model_auto_compact_token_limit=<task_context> "<prompt>"`, with `stdin` from `/dev/null`.
   - **No Codex sandbox, deliberately.** Inside a container Codex's Linux sandbox cannot start (measured, §9: "bubblewrap … is missing here", and every shell command fails). The container is the sandbox: no network but the model, no mount but its own scratch.
   - `--skip-git-repo-check` is not needed: `/testbed` is a repository.
   - **Which user** runs the agent in the container is a Phase 0 question (§9): the images own `/testbed` and the conda environment as root, so the host's uid probably cannot write there, while root inside leaves root-owned logs in the mounted directories and needs `safe.directory` for git.
   - The prompt is §5.4.
4. **Nudge on a stall**, exactly as Night Shift does: if `/testbed` has no change and the last message announces work, resume the session with the nudge, at most `nudges` times.
5. **Collect the patch:** `git -C /testbed add -A`, then `git diff --cached <base_commit>` limited to text files, taken by the runner from outside the agent's session.
   - **Binary files are left out.** A repository whose `.gitignore` misses `__pycache__` (Night Shift's live run hit exactly this) yields "Binary files differ" stubs, which `git apply` rejects, failing the *whole* patch for a reason unrelated to the model. Files that `git diff --cached --numstat` reports as binary (`-	-`) are excluded from the diff, and their names are recorded in the instance state.
   - **Test files stay in the patch.** The harness applies `model_patch` and then the instance's `test_patch`; a model patch that edits the same test files can make `test_patch` fail to apply, and the instance is then unresolved. Stripping test hunks would hide that, and the reference agent (mini-SWE-agent) does not strip them, so neither does this. The report counts such instances separately (§6.3).
6. **Record and remove.** Append the prediction, write the instance state (`done`, `empty`, `stalled`, `timeout`, `error`), remove the container.

**Limits per instance:** `task_timeout` wall clock (default 45 min, nudges included), enforced by the runner with SIGTERM and then SIGKILL to the container; and the compaction limit above, which keeps one instance inside the KV budget parallelism was computed from.

### 5.2. Network

**The agent has no network except the model server.** With one, it can find the real fix: every instance is a public GitHub issue with a merged pull request, and `ragon`'s prompt tells the model to run `ragon-search` and `ragon-fetch`.

- **Mechanism:** `docker network create --internal ragon-swe-bench`. The model server listens on every interface by design, so the container reaches it at the network's gateway address. The network has no route out, but the host's INPUT chain is not filtered, so the container also reaches every other host service listening on all interfaces through the gateway address, not only the model gate (accepted risk, owner; no firewall rule is installed). Measured (§9): from such a network `http://<gateway>:8000/v1/models` answers 200, and both `https://1.1.1.1` and `https://github.com` fail.
- **Cost:** the prompt still names the web commands, and they fail. The model may spend turns finding that out until `/airgapped on` exists ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md)), which a run then sets.
- **No package installs:** an instance whose tests need a dependency the image lacks cannot fetch it. That is the benchmark's rule too.
- The web binaries and `ragon-admin` are **not** mounted into the container.

### 5.3. Images, and the architecture problem

The GB10 is aarch64. The official instance images are x86_64 only, and this machine has no x86 emulation registered (§9). Three sources, in the order `setup` and `run` try them:

| Source | What | State |
|---|---|---|
| **Local build** | The v5 harness builds images from `swe-bench-tasks` with Buildx (`--task-repo`), which upstream calls "experimental" on arm64. | Supported path upstream. How many of the 500 build here is unmeasured. |
| **Community arm64 images** | `greynewell/swe-bench-arm64` on Docker Hub: 1,721 tags, arm64, about 0.9 GB each as listed. | Third-party, unaudited. Its author reports 1,798 of 2,294 full-set instances building natively and the rest needing x86 emulation. Used only with `--images community`, and recorded in the manifest. |
| **x86 emulation** | The official images under QEMU. | Not available here without installing `qemu-user-static`; reported about six times slower. Out of scope. |

**Only validated instances are run.** An image that builds is not evidence that the instance works on arm64: a test can fail for reasons that have nothing to do with the patch, or pass without one. An instance is **validated** when both hold here:

- the reference patch resolves it (the harness with `--gold`);
- an empty patch does not. An instance whose `FAIL_TO_PASS` tests already pass on arm64 would otherwise count as resolved for every run.

So:

- `swe-bench setup --validate` (and `smoke`, for its handful) grades each instance twice, with the gold patch and with an empty one, and writes the instances that pass both to `validated-<arch>.json`, with the image source and digest. The second pass doubles the test time of validation; the image builds, which dominate, are shared.
- `run` defaults to that list. An instance outside it is skipped and reported as **excluded**, never as unresolved.
- The report's denominator is the validated set, and it prints both numbers: "resolved R of V validated (500 in the dataset, 500 − V not evaluable on arm64)".

Validating all 500 is itself a long job (image builds plus two test runs each). It is resumable, runs with the model server up, and is admitted like `eval` (§6.2).

### 5.4. The prompt

```text
This is an unattended task in the repository at /testbed. Nobody will answer questions:
where something is unclear, make the reasonable choice.

- Fix the issue below by changing the repository's source files.
- You may run the repository's tests. There is no network.
- Do not commit. Your changes are collected when you stop.

Issue:
<problem_statement>
```

**The prompt contains `problem_statement` and nothing else from the dataset row.** Never `hints_text`, `patch`, `test_patch`, `FAIL_TO_PASS` or `PASS_TO_PASS`; the dataset row carries all of them. A test asserts that the composed prompt and the container's files contain none of those fields' text.

Cave mode stays at whatever the configuration says, since that is the agent as shipped, and the level is recorded in the manifest; comparing levels is one of the uses of §1. (Cave mode was removed.)

### 5.5. Coexistence with the model server

Reused from Night Shift wherever the mechanism is the same, with the one difference named below:

- **Admission** (`NightShiftRunner.admit`): the server answers, host-safety checks pass, the memory reserve holds, no heavy job is running, and the model has been idle for `idle_minutes`. A SWE-bench run adds one check: free disk above `disk_reserve` (§6.2). **Now an active index run** (a `ragon-index-*` scope, `ragon-code` or `ragon-docs` indexing a session started) **is waited for, not refused:** `SweBenchRunner.wait_for_index_runs` polls the host's heavy jobs every 15 s for up to 30 min, prints one line when it starts waiting and one when it goes on, and only then asks for admission, which refuses as before if the scope is still active or another heavy job (a build, an upstream test run) is there. The first night's second arm had refused at 16:40 for a `ragon-docs` index a session had started and lost five minutes to a restart by hand.
- **One runner at a time:** a SWE-bench run takes `runner.lock`, so it and a night run exclude each other, and `server start`, `codex build` and `index` refuse while it holds the lock. `_refuse_during_night_run` prints a fixed "A Night Shift run is in progress" today; the lock file must record who holds it, and the message must name the holder.
- **Parallelism:** `max(1, min(max_parallel, floor(KV pool / task_context)))`, 3 today (since `max_parallel` is first capped at the served model's `swe_bench_max_parallel` from `ModelMatrixRegistry.swe_bench_max_parallel()` when the model entry sets one; `SweBenchRunner.max_parallel`).
- **Interactive use wins:** an open `ragon` session or an outside request stops new instances from starting; running ones finish. **Now the benchmark wins instead (§18):** behind the model gate, other requests are refused while a run lasts, and this rule applies only while the gate is paused or absent.
- **Memory:** each agent container is capped by Docker (`task_memory`), and a start is admitted only if `MemAvailable`, less what the running containers may still grow into, leaves the 8 GiB reserve plus one more cap. Night Shift's probe reads a systemd scope's `MemoryCurrent`, which says nothing about a container: a container's processes belong to dockerd's cgroup, not to the scope of the `docker run` client. This run reads the container's own cgroup (`docker stats --no-stream`, or `memory.current` under the container's cgroup path). That probe is new; the admission arithmetic around it is the shared one.

The shared pieces move out of `dreamference/night_shift/` into a neutral module both import, in the same change; the spec does not allow a second copy of the admission logic.

**An unattended run fits the night.** `swe-bench run --until 07:00` stops starting instances at that time and resumes the next time it is called. Queueing a run through `/night` is not specified.

---

## 6. The grading phase (`eval`)

### 6.1. The harness

`eval` calls the upstream harness from its own virtualenv, never a reimplementation:

```text
swebench eval <dataset> -p runs/<run>/predictions.jsonl --run-id <run>-<n> -j <workers> --task-repo <tasks>
```

- **`--run-id` is `<run>-<n>`, and `n` names a grading, not a set of patches.** Within a run a prediction never changes: `predictions.jsonl` is append-only and a resumed `run` skips finished instances. What can change is the grader: `n` increases when the harness version, the task-repository commit or the image source or digests differ from those grading `n` recorded, and a new grading then covers every instance again. This matters because the harness caches by `run_id` and `instance_id` only and would otherwise return the old result.
- **`-i`** limits a call to the instances not yet graded under the current `n`, which is what makes `eval` resumable and lets it follow a `run` that is still producing predictions.
- The harness's `logs/evaluation/<run_id>/` is moved into the run's `eval/<n>/`. `report` reads `results.json` of the highest `n` only, and names `n` and what it was graded with.

### 6.2. Resources

Grading uses no model, but it runs beside one:

- **Workers:** `eval_workers`, default 4. Upstream's guidance is fewer than `min(0.75 × cores, 24)`, 15 on this machine's 20 cores; memory, not cores, is the limit here, with roughly 27–40 GB available while the server is resident.
- **Memory:** whether the harness caps its containers is unverified. Until it is, `eval` runs its workers under one systemd scope with `MemoryMax`, and admission requires the 8 GiB reserve on top.
- **Disk is the scarce resource.** 388 GB are free, shared with the model caches. Docker Hub lists the community images at about 0.9 GB each, which is the **compressed** size: 500 of them are 450 GB before unpacking, and unpacked layers are commonly two to three times larger. Layers shared within a repository pull the other way, by an amount Phase 0 has to measure (`docker system df` after ten instances of one repository). Until then the whole set must be assumed not to fit, so the rules below are what makes a full run possible, not a precaution:
  - admission for `setup --validate`, `run` and `eval` requires `disk_reserve` (default 100 GB) free, and each stops cleanly, resumable, when the reserve is reached;
  - images are built and removed **per repository**: the instances of one repository are run and graded together, then their instance images are removed and the shared base and environment layers kept;
  - `swe-bench status` prints `docker system df` for the benchmark's images.

### 6.3. The report

`report.md`, and the same on the terminal. The layout only (R, V, P and the times are placeholders):

```text
ragon-0.158.0-3fa9c21e / RadixArk/Qwen3.8-27B-NVFP4       run <name>
SWE-bench Verified, arm64, local images

Resolved            R / V validated   P%
Not evaluable        N of 500 (no arm64 image, or gold patch fails here)
Empty patch          a      Stalled b      Timeout c     Agent error d
Patch broke test_patch application     e
Median wall time    M per instance, 3 at once; T in all

Not comparable with published SWE-bench scores: see §8 of the spec.
```

- **Per repository:** resolved over validated, since the validated set is not spread evenly.
- **Why the unresolved failed** (§24): `Broke a passing test` (a PASS_TO_PASS test fails) is counted apart from `Did not fix` (a FAIL_TO_PASS test fails, every PASS_TO_PASS test passes), and `--against` sets both side by side.
- **Compactions** (§25): their number, time and share of agent time, and resolved by compaction count (0, 1, 2, 3+), counted from the rollouts kept in each instance's scratch folder.
- **`--against <run>`:** instances resolved by one run and not the other, and the difference with a paired confidence interval (McNemar on the instances both runs attempted). A difference inside the interval is reported as "no measurable difference", in those words.
- **Variance:** a single run is one sample. The spec's acceptance criteria (§7.3) include running the smoke set three times to see how much the same configuration varies before any A/B is read.
- **Optional baseline column:** `swebench infer` (mini-SWE-agent against the same endpoint) measures the model without `ragon`. It is not run by default.

### 6.4. The manifest

Written when a run starts, never edited: harness version, task-repository commit, dataset revision, image source and the digests of the images used, `ragon` version, Codex tag and patch-series hash, served model id and registry alias, the launch recipe's context length and speculative settings, cave-mode level (until cave mode was removed; old manifests' `cave_mode` key is ignored), `task_context`, timeouts, nudges, parallelism, the instance list, the start time, and the git commit of this repository. `report --against` refuses to compare two runs whose datasets or validated sets differ, and lists every other field that differs.

---

## 7. Tests

### 7.1. The smoke set

`swe-bench smoke` is the gate for everything else:

1. **Gold:** the harness's own installation check, `swebench eval verified --gold -i sympy__sympy-20590 --task-repo …`, extended to five instances from five repositories. Every one must resolve. This proves images build and grade correctly on arm64.
2. **Empty:** the same five with an empty patch must all be unresolved. This proves the grader can fail.
3. **Agent:** one instance, chosen for a small fix, run through §5.1 in full, and graded. It need not resolve; it must produce a prediction, a log and a graded result.

The five ids are fixed in the code, chosen in Phase 0 from instances that validate here. A smoke takes minutes after the first image build, and its pass is recorded with the harness version; a harness upgrade invalidates it.

### 7.2. Offline tests

`tests/test_swe_bench.py`, with a scripted stand-in for `ragon` and for `docker`, as `tests/test_night_shift.py` has for `ragon`: nothing in the suite starts a container, pulls an image or reaches the network (`tests/conftest.py` already fails any real `docker` command that changes something). The cache and results directories of §4 are module-level constants, because the fixture that gives each test its own home re-points module attributes; a path computed inside a function would escape it and a test could write the user's real cache.

- the prompt holds the issue and none of the forbidden fields;
- an agent failure writes an empty prediction, not a missing one;
- a resumed `run` skips finished instances and appends;
- `eval` raises `n` when a graded prediction changed, and passes `-i` for the ungraded;
- excluded instances are counted as excluded;
- binary files are left out of a collected patch and named in the instance state;
- an instance whose empty patch resolves is not validated;
- admission refuses on the disk reserve;
- `run` refuses without a passed smoke, and while a night run holds the lock;
- the manifest's fields, and `--against` refusing mismatched sets.

### 7.3. Acceptance

1. `setup` on a clean machine leaves a working harness virtualenv and changes nothing in `.venv`.
2. `smoke` passes on the GB10 with the model server resident, and earlyoom stays silent.
3. The smoke set's agent step, run three times, gives the spread of one configuration.
4. A 25-instance `run` and `eval` complete unattended, are interrupted once on purpose and resume, and the report's counts add up to 25.
5. No connection from an agent container to anything but the model server (checked with the trace method of [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) on one instance).

---

## 8. What the number means, and what it does not

Printed in short form on every report, and to be repeated wherever a figure from this command is quoted:

1. **The benchmark is contaminated.** the upstream vendor stopped reporting SWE-bench Verified in early 2026: frontier models could reproduce gold patches verbatim, and many of the hardest unsolved tasks had flawed tests. Its replacement recommendation, SWE-bench Pro, was itself withdrawn in July 2026 after an audit estimated about 30% of its tasks broken. An open-weights model has very likely seen these repositories and their fixes.
2. **The images are not the leaderboard's.** Scores elsewhere are graded in the official x86_64 images. These are arm64 images built locally or by a third party, and a subset: the denominator is what validates here, not 500.
3. **The agent is restricted** in ways others may not be (no network), and the model is a quantised build (NVFP4) with a speculative drafter.
4. **One run is one sample.**

So the number supports "configuration A resolves more of these instances than configuration B on this machine". It does not support "Ragon scores N% on SWE-bench Verified" without all four qualifications, and the report says so in its last line.

---

## 9. Checked on this machine, and not

**Checked:**

| What | Result |
|---|---|
| Harness | `swebench` 5.0.2 on PyPI, Python ≥ 3.10; the v5 CLI is `swebench eval <dataset> -p … --run-id … -j …`, with `--gold`, `-i` and `--task-repo`; the older `python -m swebench.harness.run_evaluation` form still works. |
| Dataset | `princeton-nlp/SWE-bench_Verified`: one split, `test`, 500 rows; fields `repo`, `instance_id`, `base_commit`, `patch`, `test_patch`, `problem_statement`, `hints_text`, `created_at`, `version`, `FAIL_TO_PASS`, `PASS_TO_PASS`, `environment_setup_commit`, `difficulty`. |
| Official images | `swebench/sweb.eval.x86_64.django_1776_django-11099` exists and is amd64 only; the `arm64` name does not exist. |
| Community images | `greynewell/swe-bench-arm64`: 1,721 tags, arm64, e.g. `sympy-sympy-22005` at 908 MB, last pushed. |
| Emulation | `/proc/sys/fs/binfmt_misc` has no QEMU entry: x86 images cannot run here as installed. |
| Host | aarch64, 20 cores, Docker 29.2.1 with Buildx 0.31.1, 388 GB free on the one filesystem Docker and the model caches share. |
| Internal network | A container on `docker network create --internal` got 200 from `http://172.20.0.1:8000/v1/models` and no connection to `https://1.1.1.1` or `https://github.com`. |
| `ragon` in a container | The installed binary, mounted read-only into `python:3-slim` on that network with a fresh `CODEX_HOME` (which must exist beforehand) and `DREAMFERENCE_VLLM_HOST` set to the gateway, completed a turn against the served model. With `-s workspace-write` every shell command failed (no bubblewrap in the container) and nothing was written; with `--dangerously-bypass-approvals-and-sandbox` the command ran and the file was created. With `stdin` from `/dev/null`, `exec` still printed "Reading additional input from stdin..." and went on. |
| Upstream arm64 work | Pull request 521 (arm64 support across the harness's build and evaluation) was closed unmerged, when v5 moved dataset-specific attributes into the task repository. |

**Not checked when the spec was written** (§12.1 answers the first six; wall time and the resolved rate are in §12.5):

- How many Verified instances build with `--task-repo` on this machine, and how many of those pass their gold patch.
- Whether the community images agree with local builds, and how their tag names map to instance ids (`sympy-sympy-22005` against `sympy__sympy-22005`).
- Disk actually used per repository after layer sharing.
- Whether the harness caps its containers' memory.
- Whether `ragon` runs in every instance image: the images are older distributions, and the binary needs a compatible glibc. `python:3-slim` is not evidence for an Ubuntu 22.04 conda image.
- Which user the agent must run as inside an instance image: the probe ran as the host's uid against an empty mounted directory, which says nothing about a root-owned `/testbed` and conda environment, or about git's "dubious ownership" check.
- Wall time per instance and for 500, the resolved rate, and its run-to-run spread.
- Whether removing refs and running `git gc` in `/testbed` is enough to hide the fix in every image, or whether some carry it elsewhere (a pip cache, a second clone).
- How many turns the model wastes on the web commands the prompt names.

---

## 10. Phases

- **Phase 0, measurements (no product code):** ten instances from ten repositories, validated (gold and empty) by local build and by community image; disk per repository, unpacked; harness memory behaviour; `ragon` started in each of the ten images, as root and as the host's uid; one full instance by hand. The results decide the default image source and replace the estimates in §5.3 and §6.2.
- **Phase 1:** `setup`, `smoke`, `run`, `eval`, `report`, `status`, `clean`; the shared admission module; the tests of §7.2.
- **Phase 2:** `--against` with its interval, the mini-SWE-agent baseline column, `--until`.

---

## 11. Open questions

1. **`ragon swe-bench` as a second spelling** (§2): add the launcher's pass-through or not?
2. **Community images:** acceptable as a default if Phase 0 shows they agree with local builds, or opt-in for good because they are third-party binaries?
3. **Installing QEMU** to reach the instances with no arm64 path: the denominator becomes 500, at about six times the grading time for those instances and one more system package.
4. **Which dataset should be the default once Phase 0 is done.** Verified is the common reference and is known to be contaminated; a held-out or newer set would mean more and compare with less.
5. **Should a run be queueable through `/night`,** so the timer starts it in the window, or is `--until` enough?
6. **Reasoning effort and cave mode for the benchmark:** as configured (the agent as shipped), or pinned per run so results do not move when a default changes? The manifest records them either way.

---

## 12. As built

### 12.1 What Phase 0 found, and what it changed

| Finding | Consequence |
|---|---|
| **The task repository cannot build arm64 images.** Every `tasks/<id>/Dockerfile` starts `FROM --platform=linux/amd64` and installs `Miniconda3-…-Linux-x86_64.sh` with x86 conda builds pinned. | §5.3's "local build" source does not exist on this machine. `--task-repo` is not used; the only source is the community repository, and the manifest says so. The clone of `swe-bench-tasks` is not needed and `setup` does not make one. |
| **400 of the 500 Verified instances have a community arm64 image**, tagged with the instance id with `__` written `-`. Missing: matplotlib 33 of 34, scikit-learn 25 of 32, xarray 22 of 22, django 9, astropy 6, sympy 3, pytest 1, sphinx 1. Docker Hub's web API stops anonymous paging at 1,000 tags, so the list is read from the registry's own `tags/list`. | The denominator can be at most 400 before validation. |
| **The harness takes a local `.jsonl` as its dataset** and uses whatever image the row's `image` column names, pulling only if it is absent. | The dataset is downloaded once (`datasets/SWE-bench_Verified.jsonl`, revision `78f471bf655a`), and each harness call gets a file with `image` rewritten to the arm64 image. No fake `x86_64` tags, no patch to the harness, and `run` and `eval` need no network once the images are here. |
| **The harness never runs an empty patch**: it files it under "empty patches" without starting a container. | §5.3's "an empty patch must not resolve" would test nothing. Validation uses a **no-op patch** (one new unrelated file) instead: gold must resolve and the no-op must not. Empty predictions are recorded as unresolved by the runner and never handed to the harness. |
| **An image that exists is not an instance that works.** Of 30 instances tried, 28 validated; `sphinx-doc__sphinx-8721` and `sphinx-doc__sphinx-8056` fail their own gold patch in the community image (8721: `No module named 'roman'`). | Confirms the rule of §5.3. The smoke set uses `sphinx-doc__sphinx-9230`. |
| **`HEAD` is not `base_commit`** in these images: one commit named "SWE-bench" sits on top, with the same tree. | The runner checks tree equality (recorded as `base_tree_equal`), not the hash, and collects the patch against **the tree the agent started from** (the checked-out commit plus whatever the image build left untracked), which is what the harness's container also has. |
| **The object store already lacks the fix** in the image inspected (`git log --all -S` for the fixed line found nothing; scrubbing removed 16 objects of 211,142, all old tags). `.git` belongs to root. | The scrub of §5.1 step 2 still runs, as **root** (`docker exec -u 0`), because a repack as the agent's user cannot replace root's pack files; `.git` is then made writable again. It took under a second. |
| **The installed `ragon` does not start in an instance image**: it needs glibc 2.38/2.39, the images are Ubuntu 22.04 with 2.35. | `setup` builds a **relocated runtime** (`~/.cache/dreamference/swe-bench/runtime`): copies of `ragon` and `codex-code-mode-host` whose ELF interpreter and rpath point at copies of the host's loader, `libc`, `libm` and `libgcc_s`, mounted read-only at `/opt/ragon`. Only those two binaries use the copied libraries; the repository's Python uses the image's. The runtime is stamped with the hash of the binaries it was copied from, rebuilt when `codex build` replaces them, and the hash is in the manifest; a run refuses to resume with a different one. `ragon-code` is not in the runtime (it needs bubblewrap), so the agent in a container works **without the code index**. |
| **Which user:** `/testbed` is mode 0777 and the image has a `nonroot` user with uid 1000. | The agent and every collecting command run as the **host's uid**, so logs and scratch files belong to the user. Git's ownership check is answered with `GIT_CONFIG_COUNT/KEY_0/VALUE_0` (`safe.directory=/testbed`) in the container's environment, which writes nothing. |
| **The image's default `PATH` puts conda's base environment first**; `conda activate testbed` happens only in the grading script. | The container's `PATH` starts with `/opt/miniconda3/envs/testbed/bin`, or the agent's `python -m pytest` fails on imports. |
| **The harness sets no memory limit** on its containers, and they belong to dockerd's cgroup, so §6.2's systemd scope would not contain them. | While the harness runs, a thread puts `docker update --memory` (`eval_memory`, default 4G) on each `sweb.eval.*.<run id>` container as it appears. Measured peaks on ten containers: 28–160 MiB. |
| **Disk:** an image is about 0.9 GB to pull (33–38 s each here) and **2.1–2.4 GB unpacked**, with little shared between repositories: 32 images took about 70 GB. | 400 images would need roughly 900 GB; 344 GB were free. The whole set does not fit at once (§12.3). |

### 12.2 Where the code is

| Piece | Path |
|---|---|
| Subcommands (`setup`, `build-images`, `smoke`, `run`, `eval`, `report`, `status`, `clean`, `shard`, `merge`) | `dreamference/swe_bench/swe_bench_command.py` |
| Settings (`[swe_bench]`), directories, pins, the smoke set | `dreamference/swe_bench/swe_bench_settings.py` |
| Every `docker` call | `dreamference/swe_bench/swe_bench_docker.py` |
| The relocated `ragon` | `dreamference/swe_bench/swe_bench_runtime.py` |
| The upstream harness: virtualenv, dataset snapshot, `swebench eval`, its reports | `dreamference/swe_bench/swe_bench_harness.py` |
| Image names, the validated list | `dreamference/swe_bench/swe_bench_images.py` |
| Local arm64 image builder for Multilingual (§31) | `dreamference/swe_bench/swe_bench_image_builder.py` |
| LRU cache for instance images across nights (§30) | `dreamference/swe_bench/swe_bench_image_cache.py` |
| One instance: container, scrub, agent, nudges, patch | `dreamference/swe_bench/swe_bench_instance_run.py` |
| Host-built `ragon-code` indexes for `--code-index` (§13) | `dreamference/swe_bench/swe_bench_code_index.py` |
| Stripping gold-patch names for `--strip-names` (§16) | `dreamference/swe_bench/swe_bench_name_stripper.py` |
| Model-gate hold during a run (§18) | `dreamference/swe_bench/swe_bench_gate_hold.py` |
| Hook registration, target extraction and gate script for `--hooks issue-v1` (§20) | `dreamference/swe_bench/swe_bench_hooks.py`, `swe_bench_issue_targets.py`, `swe_bench_issue_gate.py` |
| Dropping test hunks and detecting weakened tests (§15, §29) | `dreamference/swe_bench/swe_bench_patch_filter.py` |
| Frozen difficulty tiers, shards and shard merging (§27) | `dreamference/swe_bench/swe_bench_difficulty.py`, `swe_bench_shards.py`, `swe_bench_shard_merge.py` |
| Gateway TCP relay to a paired or remote node's model server (§12.6, §31) | `dreamference/swe_bench/swe_bench_relay.py` |
| Per-arm smoke gate before a night starts (§31) | `dreamference/swe_bench/swe_bench_arm_smoke.py` |
| A run's files | `dreamference/swe_bench/swe_bench_run_store.py` |
| Admission, scheduling, manifest, resume | `dreamference/swe_bench/swe_bench_runner.py` |
| Validation and grading | `dreamference/swe_bench/swe_bench_evaluator.py` |
| Report and `--against` | `dreamference/swe_bench/swe_bench_report.py` |
| The shared lock now names its holder | `NightShiftQueue.runner_lock(holder=…)`, `runner_holder()`; `_refuse_during_night_run` prints it |

`[swe_bench]` in `dreamference.toml`: `max_parallel` 3, `task_timeout` 45m, `task_memory` 8G, `task_cpus` 4, `nudges` 2, `idle_minutes` 10, `task_context` 49152, `eval_workers` 4, `eval_memory` 4G, `eval_timeout` 30m, `disk_reserve` 100G, `image_chunk` 25, `image_cache_lists` `()`, `image_cache_floor` `""` (`disk_reserve` + 20G when empty), `build_memory` 24g, `build_cpus` `0-11`, `nodes` `"paired"`.

### 12.3 Departures from the design

- **Admission is imported, not moved.** §5.5 asked for the shared pieces to move to a neutral module. `SweBenchRunner` calls `NightShiftRunner.admit` and `NightShiftRunner.start_blocker` and takes `NightShiftQueue.runner_lock`: one implementation, no copy, no new module. `start_blocker` reads a running task's systemd scope; an instance run has none (`current_unit` is `None`), so each running container is assumed to grow to its full `task_memory`, which is the conservative reading and needs no container probe. With 28 GiB available and 8G caps, that admits **two** containers at once although the KV pool allows three; the measured agent container peaked at 113 MiB, so the cap is far too generous and is left for a run with more data to retune.
- **`/airgapped on` cannot be used.** §5.2 planned to set it once it existed. As built, `ragon` refuses to start at `on` together with `--dangerously-bypass-approvals-and-sandbox` ("nothing would keep them off the network"), and inside a container the bypass flag is the only way to run. The run sets `DREAMFERENCE_RAGON_AIRGAPPED=off` explicitly: the internal Docker network is the enforcement, the web binaries are not mounted, and the task prompt says there is no network. The system prompt still names the web commands. Open for the airgapped module: a way to say "the network is absent by other means".
- **Validation happens when a run starts, for the instances it selected**, and the manifest's instance list and exclusions are then fixed. An instance that could not be validated because the disk reserve was reached is excluded from that run. `setup --validate` does the same ahead of time.
- **Per-repository image cycling is not built** (as; subsequently built in §15, §27 and §30: `run --eval --remove-images` and `eval --remove-images` cycle images in chunks of `image_chunk` and release them through `SweBenchImageCache`). `run --eval --remove-images` removes a repository's images after grading it, but validation still pulls every selected image first. A run over all 400 instances therefore needs the instances validated repository by repository with `clean --images` in between, and does not fit as one command today. Runs of a few dozen instances do.
- **A timeout still submits what was changed.** §5.1 lists `timeout` among the states without saying what is predicted. The container is stopped, started again, and the partial patch collected; the status stays `timeout`.
- **An interrupted run (Ctrl-C or SIGTERM) writes no prediction for the instances it cut off**; their state is `interrupted` and they run again on resume. `--until` only stops new starts.
- **The grading number rises on a changed grader**, as §6.1 says, not on a changed prediction, as one line of §7.2 said: predictions never change within a run.
- **Cave mode is passed in explicitly** (removed: `DREAMFERENCE_RAGON_CAVE_MODE` is no longer set or recorded in `manifest.json`, and old manifests' `cave_mode` key is ignored). The container has no `dreamference.toml`, so the level configured on the host is resolved by the runner, set as `DREAMFERENCE_RAGON_CAVE_MODE`, and recorded.
- **`--against` (Phase 2) is built**: per-instance differences, a 95% Wald interval for paired proportions and the exact McNemar p-value; "No measurable difference." when the interval contains zero. The mini-SWE-agent baseline column is not built.
- **`swe-bench eval` checks memory, not idleness:** `eval_workers × eval_memory` plus the 8 GiB reserve must be available.
- **Every agent log starts with a line that is not JSON** (`Reading additional input from stdin...`, printed by `ragon exec`); readers skip it.

### 12.4 Tests

`tests/test_swe_bench.py`, 41 tests (191 as, plus `test_swe_bench_night_script.py` 64, `test_swe_bench_multilingual.py` 18, `test_swe_bench_shards.py` 10 and `test_swe_bench_image_cache.py` 7). A stand-in plays `docker`: a container is a scratch git repository on the host, and the scripts the runner executes in a container (scrub, record the starting tree, detect a change, collect the patch) run for real against it with bash and git. Another stand-in plays the harness and writes the per-instance reports the real one writes. Covered: the prompt and everything handed to the container contain none of the answer-bearing fields; the container's limits, network, user and environment; refs removed before the agent starts, as root; a change, an error, an empty answer, a stall, a nudge that works, a timeout with its partial patch; binary files left out and named; the collected patch applying to a fresh checkout, with files the image left untracked kept out of it; resume, interruption, a torn predictions line, a resume under another model; exclusion and its counts; the no-op rule; validation not repeated; an unpullable image not recorded as rejected; grading only the ungraded, never an empty patch, a new grading for a new harness, an instance without a verdict staying ungraded; the smoke gate, the lock and its holder's name, the disk reserve, Night Shift's admission, `--until`; the manifest, the report, `--against`, the statistics; `smoke` passing and failing; the runtime's library list; selection; settings; the command line; `status` and `clean`.

### 12.5 Checks run

All on the GB10 with the default model resident (Qwen3.8-27B NVFP4 on SGLang), cave mode `ultra`. The benchmark results themselves are not kept in this repository.

- **`setup`:** harness virtualenv, dataset snapshot (500 rows), tag list (400 images), runtime. Seconds once the virtualenv exists; the project's `.venv` is untouched.
- **`setup --validate`** on a sample plus the smoke set: most of the time is image pulls; instances whose gold patch does not resolve in the community image are rejected (`sphinx-doc__sphinx-8056`). Grading the five smoke instances twice takes about 90 s once the images are here.
- **`smoke`:** passed in a few minutes with a one-minute idle wait. The five resolve with their gold patch and none with the no-op; the agent produced a prediction, a log and a graded result; the agent container peaked at 113 MiB.
- **The sample.** `~/.cache/dreamference/swe-bench/sample-25.txt`: the 25 Verified instances with an arm64 image whose `sha256(instance_id)` is smallest, so it is fixed and not chosen by difficulty.
- **A 25-instance run, without the code index,** completed unattended: one instance was excluded by validation, the counts added up, agent containers peaked at 88-184 MiB and grading containers at 122 MiB, earlyoom logged nothing but its periodic memory line, and the model server answered throughout.
- **Two at a time, not three.** With 8G caps and about 28 GiB available, Night Shift's start check admitted two containers (§12.3); the run said so each time it waited.
- **Interrupted on purpose and resumed.** After four predictions, SIGTERM to the `ragon-admin` process: it exited in 4 s, left no container, four parseable predictions, and the instances it cut off in state `interrupted` with no prediction. The same command with the same `--name` then ran the remaining ones, those among them.
- **No turn spent on the web commands.** The logs mention `ragon-search` and `pip install` only inside quoted text, so §5.2's worry did not show at this size.
- **Not run as designed:** the run-to-run spread of §7.3 item 3, and the connection trace of item 5 (the internal network was checked by hand: the model server answers, `github.com` does not).

---

### 12.6 A replica's model server

A paired node serving the same model adds a lane (`[swe_bench] nodes`, default `"paired"`; [RAGON_NODE §18.8](./DREAMFERENCE_RAGON_NODE.md)). The containers still run here; the run listens on the internal network's gateway at a port of its own and relays each connection to that node's model port, so the network stays closed to everything else. Each instance's state notes which server answered it when it was not this machine's. With no paired node nothing changes. Not run with a second node; the relay was run live from a container on `ragon-swe-bench`.

## 13. The code index as an arm

`swe-bench run --code-index universal` gives the agent `ragon-code` ([RAGON_CODE_INDEX](./DREAMFERENCE_RAGON_CODE_INDEX.md)); the default, `off`, is the agent of §12, which navigates with `grep` and `find` because the runtime carries nothing of the index. The arm is recorded in the manifest (`code_index`), a resumed run keeps the arm it started with, and `report --against` lists it among the fields that differ.

### 13.1 How it works

1. **The repository is indexed on the host, before any agent starts.** For each instance, `/testbed` is copied out of the instance image (`docker create`, `docker cp`, no container ever runs), `ragon-code index --wait` is run in the copy with `RAGON_CODE_STATE_DIR` and `CBM_CACHE_DIR` pointing into `~/.cache/dreamference/swe-bench/index/<repo>@<base commit>/`, and the copy is deleted. The index run is admitted against the host's memory budget and runs in `ragon-index.slice`, like any other. All indexes are built before the first agent container starts: Night Shift's admission refuses to start beside an index run, and index time is not the agent's time. It is recorded per instance (`index.seconds`, `index.cached`) and reported separately.
2. **The index is mounted read-only**, with a relocated `ragon-code` (`runtime-code/`, the same loader-and-libc treatment as `ragon`, kept in a directory of its own so the `ragon` runtime's hash, which a manifest pins, does not move). Four variables tell `ragon-code` where things are: `RAGON_CODE_BIN` (the launcher then appends `ragon-code prompt-block` to the model's prompt by itself), `RAGON_CODE_STATE_DIR`, `RAGON_CODE_GRAPH_DB` and `RAGON_CODE_PROJECT` (the graph names its project after the host path it was built at). `ragon-code` is put first on the container's `PATH`.
3. **Queries only read**, which is the code-index spec's own rule. A file the agent edits is answered by text search and tagged `heuristic (text)`, as on the host. The launcher also starts `ragon-code session`, which cannot write its lock on the read-only mount and exits; nothing depends on it.
4. **A run whose index cannot be built does not start:** a run measures one arm.

### 13.2 Only the universal layer

The exact (SCIP) layer is left out, by pointing `RAGON_CODE_INDEXERS_DIR` at an empty directory so scip-python counts as not installed. Measured on `sympy__sympy-13480` with both layers: 1,022 s, scip-python at 1.6-1.7 GiB per directory, and the run for the `sympy/` package itself ended with Node's "JavaScript heap out of memory" at 2 GB, leaving exact data only for `bin/`, `doc/` and `examples/`. Every instance is a different commit, so nothing is shared between instances. The universal layer alone took 19-29 s per repository with the model idle (79 s for django with two agents running). So every answer the agent gets in this arm is tagged `heuristic`, and the result says nothing about the exact layer.

### 13.3 Checked

- A host-built index is accepted in the container: `status`, `refs`, `callers` and `prompt-block` answered there, in milliseconds, and after an edit the changed file's rows turned to `heuristic (text)`.
- The prompt block arrives: the with-arm's `model_catalog.json` contains `# Code navigation`; the without-arm's 24 logs and catalogs contain neither that heading nor one mention of `ragon-code`.
- `ragon` was not rebuilt between the arms: both manifests carry the same `runtime_hash`.

### 13.4 The report

`report` prints, for one run, the tokens, the command count and a `Code index` line: the arm, what the indexes cost, and in how many instances the agent called `ragon-code` at all (counted from `ragon exec`'s command events). `report <run> --against <other>` sets two runs side by side on the instances both graded: resolved in each, median and total agent time, input and output tokens, commands, `ragon-code` calls, instances that used it, index time, then how many were resolved in both, only one or neither, and one row per instance. If the agent never called `ragon-code`, the report says the comparison says nothing about the index; otherwise it says in how many instances it was used. The paired interval and "No measurable difference." of §6.3 apply unchanged.

### 13.5 The first arm: the tool went unused

Two runs on the same validated instances, the same `ragon` build, the same model, one after the other: one without the index and one with the universal layer. The arm said nothing about whether the index helps, because the agent never used it: every prompt carried the `# Code navigation` block and `ragon-code` answered in the container, yet no command in the logs asked it anything. The one command that named it was `ls /opt/ragon-code/bin`. The first version of the counter counted that as a call; it now counts only `ragon-code <query verb>`, and a test fixes the difference.

- **Two arms with the tool unused still differed on a few instances** with the model, build and instances unchanged. That is this benchmark's run-to-run noise at this size, and the nearest thing to the spread measurement §7.3 asked for: a difference of an instance or two either way is not a finding.
- **The isolation held where the agent itself tested it.** In one instance the agent tried `ragon-fetch` on the upstream file at `raw.githubusercontent.com`, which would have shown it the fix; the command does not exist in the container and there is no route out.
- **The manifest's `repository_commit` is `HEAD` when the run started, not proof of the code that ran:** both runs were made from a working tree with uncommitted changes, and `--against` lists the two commits as differing for that reason only.
- **The decision it led to:** the model does not reach for `ragon-code` on its own in this setting (a bare issue text and an unattended run). Whether the task prompt should name it, or the block's wording should change, is a question for the code-index spec; an A/B where the tool is actually used needs one of those first.


### 13.6 Making the tools reach the model

After §13.5 the index became tools (`code_*`, CODE_INDEX §15). The next arm was the first in which the agent used them. What had to change first:

- **The agent could not write `/testbed`** (RAGON_PROMPT §6.1 item 1): the scrub step now opens it all, in both arms.
- **The tools were missing under load.** In a first attempt (stopped early and marked `INVALID.txt`), `code_search` came back `unsupported call: code_search`: Codex waits 1 s for an optional MCP server before the first request and leaves out the tools of one that is not up (CODE_INDEX §15.4). The runner now declares `ragon_code` itself with `required = true`.
- **The task prompt names the tools in this arm only** (`CODE_INDEX_HINT`, two sentences: `code_search`/`code_def` before grep, `code_impact`/`code_callers` before an edit). The system prompt's block alone had left the index unused (§13.5). A test pins the plain arm's prompt as it was.
- **A pair is run back to back on one `ragon` build** and the installed `ragon-code`, on the instances not resolved by both earlier arms, with the same departures from §12 in both arms (`task_context` 44,000, three at once, no idle wait, open sessions ignored).
- **The tools were then used** (`code_show` most, then `code_search`, `code_impact`, `code_callers`, `code_refs`, `code_def`), and the run store's `ragon_code_calls` and a count from the session files agree for every instance. Empty answers had three causes: an absolute `path` passed to a search, `refs`/`impact` for methods the graph cannot see called (both fixed afterwards), and a `show` of a name not found.
- **Decision:** the score of one pair is inside the noise and settles nothing; the next pair should run on a rebuilt `ragon` and `ragon-code`, on the full validated sample, with the plain arm run twice so the floor is measured in the same session.
---

## 14. Refine, then fix (`--refine`)

**Why.** The failures of the index-on round were mostly not about where the agent edited (it edited a file the reference patch edits) but about the change. The fix covered the issue's example but not its stated use case, stopped at direct parents where grandparents also apply, guarded a symptom instead of fixing the rule, or missed a second code path the hidden test checks. A first step that only studies the issue targets these.

**What it does.** `swe-bench run --refine` (a new run only, recorded as `refine` in the manifest) runs each instance in two `ragon exec` sessions in the same container:

1. **Refine** (`REFINE_PROMPT`, at most `REFINE_TIMEOUT_S`: 15 minutes in the first round; `None`, the task's own limit, to match the product's uncapped study): read, run and test, change nothing under `/testbed`, and write `/ragon-scratch/refined.md` in six sections: intent, requirements as observable results, every code path (by callers and references), edge cases, what must not change, acceptance checks. With the code index the prompt names `code_callers` and `code_refs` for the paths.
2. The runner records whether the step changed the tree (it is told not to; this is measured), then puts `/testbed` back to the tree the agent started from (`RESET_SCRIPT`: `git read-tree -u --reset` to the recorded base tree, `git clean -fd`), keeping the description.
3. **Fix** (`FIX_PROMPT`, a new session, the full task timeout): the issue verbatim, then the description, marked as possibly wrong. The issue is authoritative where they disagree; no guard that only hides the symptom; every acceptance check is run before stopping. Nudges apply to this session as before.

The instance's state keeps `refine`: the description (up to 40,000 characters), its size, both steps' times, the first step's outcome and session, whether it changed the tree, and the log offset where the fixing session starts, so the report counts each step's tokens apart. `report` prints a "Refine first" line, and `--against` lists `refine` among the differing fields.

**refine-v2.** `--refine --refine-version v2` gives both steps the second version of the texts ([RAGON_REFINE §10](./DREAMFERENCE_RAGON_REFINE.md)): the same prompts with v2's six sections and fix rules in place of v1's. The manifest records `refine_version` (`v1` for every run without the flag and for runs made before it existed), the "Refine first" line says `on (v1)` or `on (v2)`, and `--against` lists `refine_version` when it differs. `REFINE_PROMPT` and `FIX_PROMPT` are v1's and stay pinned.

**Decision.** `--refine` stays an opt-in arm: one 24-instance pair does not settle whether it helps, and it costs a great deal of agent time (the first step alone is a large share of it). The first step's tokens are undercounted, since a stopped session writes no `turn.completed` event, so the log has no usage for it. The first step must change nothing under `/testbed`; the runner measures that. Where the description was wrong, it was followed: the "the issue is authoritative" sentence did not prevent it.

---

## Sources

Fetched.

- [SWE-bench README](https://github.com/SWE-bench/SWE-bench/blob/main/README.md): the v5 CLI, `--task-repo`, the resource guidance, "Support for `arm64` machines is experimental", result caching by `run_id`.
- [SWE-bench evaluation guide](https://www.swebench.com/SWE-bench/guides/evaluation/): the predictions format and the layout of `logs/evaluation/<run_id>/`.
- [swebench on PyPI](https://pypi.org/project/swebench/): version 5.0.2.
- [SWE-bench pull request 521](https://github.com/SWE-bench/SWE-bench/pull/521) and [issue 520](https://github.com/SWE-bench/SWE-bench/issues/520): arm64 support, closed unmerged, and the x86 assumptions it listed.
- [`greynewell/swe-bench-arm64` on Docker Hub](https://hub.docker.com/r/greynewell/swe-bench-arm64), with its author's [write-up](https://greynewell.com/blog/swe-bench-arm64-native-containers-6x-faster/) and [data](https://gist.github.com/greynewell/497005bb33641503f1a5874f16578088): 1,798 of 2,294 instances native on arm64, 11 instances compared against x86, about six times faster than emulation. These are the author's figures, not checked here.
- [princeton-nlp/SWE-bench_Verified](https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified): 500 rows and the field list.
- [Why SWE-bench Verified no longer measures frontier coding capabilities](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/) (the upstream vendor): the contamination findings of §8.
- [SWE-Bench Pro Verified (arXiv 2609.08149)](https://arxiv.org/pdf/2609.08149): the July 2026 withdrawal of the SWE-bench Pro recommendation and the estimate of about 30% broken tasks, as reported by a web search summary; the paper itself was not read.

### 13.8 The exact arm: `--code-index exact`

The third arm indexes each instance with the SCIP stores alone and runs the agent with `RAGON_CODE_LAYERS=exact` (code-index spec §16). Nothing in it comes from the graph.

- **The index.** It is built on the host from the same copy of `/testbed` as the universal arm, with the real static indexers (scip-python; scip-typescript where a repository has TypeScript). It runs under the machine's own admission rules, with a 16 GiB memory cap per run, which gives Node a 12 GiB heap.
- **A busy model.** A run the model kept busy past `ragon-code`'s wait is deferred, and the arm tries again, three attempts in all.
- **Caching.** The stores are cached per repository and commit under `index-exact/`, apart from the universal cache.
- **Failures.** An instance whose indexers did not all finish still runs. Its state records `stores`, `failed` and `peak_mb`, and the report counts such instances and those with no store at all.
- **A `ragon-code` under test** (`DREAMFERENCE_SWE_BENCH_RAGON_CODE`) is relocated into a runtime directory of its own, so preparing it never replaces the one a running arm has mounted.

## 15. Fixes from the failure analysis

The failure analysis of earlier rounds led to these. The eval script's test-file reset is made per file in the dataset file each harness call gets, so a test patch that adds a file no longer leaves the agent's test edits in place (the grader records `eval_reset: per-file`, so a run graded before is graded again). "Test patch failed" counts every refusal of the test patch. A turn that changed the tree and stopped mid-work gets a completion nudge. `run --task-rules tests` is the test-discipline arm. `eval --drop-test-hunks` regrades a run's predictions with their test files left out, as a separate grading series, and `eval --remove-images` cycles images by repository. With `run --eval --remove-images` the code-index pass no longer keeps every image it pulled.

**The first A/B runs `tests-v2`, not `tests`.** `scripts/swe_bench_ab.sh` compares `default` with `--task-rules tests-v2`. The failure analysis' read of every run found that `tests`' "if a test that passed before your change fails after it, your change is wrong" is false for many resolved tasks: their correct fix fails an old test because the issue asks for new behaviour, so the rule could talk the agent out of a right answer. `tests-v2` keeps the `/tmp` line and the comparison by name, forbids editing a test only to make it pass, and has the agent decide from the issue whether the old test or the change is wrong. `tests` stays available, unchanged.

**`issue-v1`, the issue and the code around the fix.** The failure analysis' first proposal (contract and sweep) is the task rule `issue-v1`, three lines in the task prompt: before editing, read the whole issue and work out exactly what behaviour it asks for (the result it expects, any function, type or code path it names, every edge case it mentions), and change no more than that; find the code nearby that does the same thing (sibling functions and classes, other backends and entry points) and follow its pattern, fixing a sibling that has the same defect; and if the issue shows an example with its expected output, run it after the last edit and check the output matches. The wording comes from the first wrong moments of the analysed runs, not from any one task. Rules stack: `--task-rules tests-v2,issue-v1` adds both blocks, each once, and the prompt always has them in `TASK_RULES`' order, whatever order the option names them in. `issue-v1` comes first there, because it is the order of the work (understand the issue, edit, then the test discipline before stopping); the manifest records the set sorted (`["issue-v1", "tests-v2"]`). An arm with several rules is named by joining them with `-`, the record arm's first (`<run>-tests-v2-issue-v1`, reports `report-tests-v2-issue-v1.txt`). A new run's `--name` must now be letters, digits, `.`, `_` and `-`, because grading passes `<name>-<n>` to the harness as its run id and the harness names Docker containers after it, so a `+` or a `,` would have failed only at grading; a run already on disk under another name is still resumed.

## 16. The other arms, and fresh tasks (recorded)

Two arms of `swe-bench run` had no section here; both are in the code and in [CLI §4.23](./DREAMFERENCE_CLI.md).

- **`--mask on`** sends the agent's requests with old tool outputs masked ([RAGON_CONTEXT_BUDGET](./DREAMFERENCE_RAGON_CONTEXT_BUDGET.md) §4.1); the runner passes `DREAMFERENCE_RAGON_MASK` and the manifest records `masking` (a run made before the option existed counts as `off`). CONTEXT_BUDGET §8.1 records the decision it led to.
- **`--strip-names`** gives the agent the issue with the fix's names taken out (`SweBenchNameStripper`): every file path, module, function and class the gold patch touches becomes a numbered neutral phrase (`[file 1]`, `[function 2]`), the same phrase for the same name throughout, with English words replaced only where they read as code. It measures the code index where the issue does not say where to look. The run records the text its agent saw.

**Fresh tasks.** The fixes proposed by the failure analysis are measured on tasks outside the sample it was made from. `scripts/swe_bench_fresh.py` lists the candidates (Verified tasks with an arm64 image, not in `sample-100.txt`, not yet validated here, in `sha256(instance_id)` order), validates them as `setup --validate` does in batches that fit the disk reserve, removing each batch's images, and draws a list stratified to that sample's difficulty mix with a recorded seed. `scripts/swe_bench_ab.sh` runs the A/B from that list as a systemd unit. Neither had run when this was written.

**Task rotation (the owner's rule).** The pool is finite: SWE-bench Verified's tasks with an arm64 image, outside `sample-100.txt`, validated on this machine, less the PR-issue mismatch list. Once the unseen tasks run out, **a night may reuse the tasks of lists used three or more nights earlier; 50-task nights continue.** Both arms of a night see the same list, so a task seen before biases the two arms equally, and the night's question (candidate against record) is unchanged; what reuse costs is independence *between* nights, so a night's resolved count is not read against an earlier night's without saying which tasks they share. The draw records it: `scripts/swe_bench_fresh.py draw --reuse-from <list>` (repeatable) names each earlier list whose tasks may recur, the lists of the last two nights and the mismatch list stay under `--exclude`, an excluded list wins over a task it shares with a reused one, a list named for both is refused, and the header says which lists were reused, how many drawn tasks come from each and how many were seen in none of them (the first line then no longer calls the tasks fresh). The reused tasks are drawn on the same footing as the unseen ones, stratified as before.

**Compaction at the end of a turn in `exec` (`--turn-end-compaction`, prepared).** The launcher's turn-end compaction ([COMPACTION §12.1](./DREAMFERENCE_RAGON_COMPACTION.md), its switch `DREAMFERENCE_RAGON_TURN_END_COMPACTION`) never reaches `ragon exec`: `with_turn_end` sets Codex's `model_post_turn_compact_threshold_percent` only for sessions someone reads, so in a benchmark run the switch alone would measure the record arm twice. The arm therefore pins Codex's own key on every `ragon exec` command line of the run, after the limit: `run --turn-end-compaction on` at the launcher's rule for the run's limit (`SweBenchRunner.turn_end_percent`, the same arithmetic as `turn_end_percent` in `ragon-rs/src/compaction.rs`: 70% of the limit, Codex's 90%-of-window cap, as a percentage of the usable window, the window less its 5% headroom; 13 for the default model's 262,144 window and the 49,152-token `task_context`, that is a turn that ends past 32,374 tokens compacts), `off` at 0 (Codex reads 0 as off), or a number, that percentage of the usable window; absent, the key is not set and `exec` never compacts at a turn's end, as every earlier run did. The launcher's switch is set to `1` or `0` beside it, for the record. The manifest records `turn_end_compaction` (`auto` when absent; `--against` tells the arms apart) and every report prints a `Turn-end compaction` line with the percentage used. The night script's arm is `turn-end-compaction-on|off|N`; since `default` is a second off arm in `exec`, `start` refuses a night in which only one arm names the part. The A/B is `ARMS="turn-end-compaction-off turn-end-compaction-on"` at a 60-minute limit, conditional on the compaction bench v2 ([COMPACTION §14.3](./DREAMFERENCE_RAGON_COMPACTION.md)): its chain starts the night only when origin/main carries the merged behaviour (the cache-hot compaction request and the summary cap: `ragon-rs/tools/src/compaction.rs` and `ragon-rs/src/summary_cap.rs` in the tree) and skips itself otherwise. Both arms run the same binary and inherit whatever it has on by default; the night measures the turn-end key alone. What it measures is narrower than the bench's question: a `ragon exec` session pays the compaction after every turn that ends over the threshold, and only a resumed turn (a nudge, the review turn) reads the summary, where a TUI user reads every turn; the behaviour columns decide, and a loss here does not contradict a pass in the TUI.

## 17. A/B of two models: production against Minima (prepared, not yet run)

The first A/B of two models rather than two harness arms. The candidate is Minima, `qwen3.8-27b-minima-nvfp4-dflash2`: Qwen3.8-27B with every linear layer in NVFP4, served by production's recipe with only the checkpoint swapped. The two arms run on 50 fresh tasks drawn the way the first A/B's were, with a fixed seed. The model server is swapped between the arms by the normal `server stop` / `server start --model` path, both checkpoints are timed with `scripts/decode_speed.py`, and production is served again whatever happens. The findings (servable by the pinned SGLang as-is, the drafter applies, the fused-group scales checked), the fairness argument and the decision rule are in [MODELS §2.2](./DREAMFERENCE_MODELS.md).

## 18. The model gate: the benchmark first

**The user's decision.** During a benchmark run (the A/B nights, about 16 hours each) the benchmark has priority over the model server. Until now the run gave way instead (§5.5): whenever the server was serving a request that was not the run's, no instance started, and on one night the run waited fifteen times. Now one gate in front of the model server refuses, while a run lasts, every request that is not the run's: `ragon` in a terminal, `ragon web`, the desktop app, the messenger bridges, `ragon-docs`, the Gmail and image-search sidecars, anything. `ragon-admin night pause` lets them through for a while.

### 18.1 Where the gate is, and why there

How the model server is reached today decides it. The engine (SGLang, from the registry) runs in `dreamference-vllm-<port>` with `--network host` and listens on every interface, and every client comes to that one port: `ragon` and the host's tools on loopback, sidecars through Docker's bridge gateway, the benchmark's containers through the gateway of `ragon-swe-bench`, other machines over the LAN, and a replica lane through the run's relay, which connects to the other node's public port.

So `server start` now moves the engine to **loopback at the public port plus 10,000** (`127.0.0.1:18000` for 8000) and starts the gate on the public port, on every interface, as the engine served it. The container is still named after the public port, so the PSI watchdog, `server stop|logs` and every client address nothing new. Nothing in a container or on the LAN can reach a loopback port, so the gate is the one way in; a process on this machine that names the internal port deliberately is an override, not a leak.

The gate is a **container of its own**, `dreamference-gate-<port>`, started by `server start` (`ModelGate`, `dreamference/vllm_server/model_gate.py`) and stopped and removed with the engine:

- **Not the SWE-bench relay.** The relay exists only while a run lasts and only on the run's gateway; the gate must stand in front of every client all the time, so that a run can close it.
- **Not a host process or a user unit.** The engine comes back at boot through Docker's restart policy; a user unit would need lingering to do the same, and a process started by `server start` would not come back at all. The gate has the engine's policy (`unless-stopped`), so the two come back together.
- **Not inside the engine's container.** In a container of its own, a crashed gate is restarted by Docker in about a second without touching the engine; its log is not the engine's, which the model-loading monitor reads for its readiness words; and the engine's launch command changes only in `--host` and `--port`.
- **In the engine's image**, which carries a Python, so nothing is pulled for it (as the diffusion sidecar's service runs in the main model's image), with `--user` the user's, 256 MiB, two CPUs, and its state mounted read-only.
- **Python, standard library only** (`model_gate_service.py`, plus `model_gate_adapters.py` and `model_gate_guard.py`, asyncio). It needs no change to `codex-patches/`, whose series stands at 42,741 of its 43,000 bytes. The files are copied beside the gate's state (`~/.local/state/dreamference/model-gate/`) at every start (with `AdapterServing.gate_mounts()` added when adapters are served), so the container never mounts a path inside an installation an upgrade removes.

If the gate cannot start, or started but does not answer its probe within 10 seconds (the port held, say), it is removed and the engine serves the public port itself, as before, and `server start` says so; `--no-gate` does the same on purpose and removes a gate an earlier start left.

### 18.2 What passes

- **No run holds the gate, or a pause is in force:** everything, as it was.
- **A run holds it:** the run's own requests (§18.3); read-only probes that do not run the model (`GET` of `/v1/models`, `/metrics`, `/health`, `/get_model_info`, `/get_server_info`, `/server_info`, `/model_info`, `/version`, `/ping`), so `ragon` still starts and says what the model is, and the run's admission and `ragon-code` can read `/metrics`; and `GET /ragon-gate`, which the gate answers itself with what it is doing. Everything else is refused (§18.4), and the engine never sees it.
- **One request per connection.** The gate decides on the request's head, sends it to the engine with `Connection: close` in place of the client's connection headers, forwards that one request's body (by `Content-Length` or chunked), and never forwards anything the client sends after it. The engine's answer goes back with `Connection: close` too, whatever the engine said, so a client never pools a connection the gate let through. A kept-alive connection opened while the gate was open therefore cannot carry a later request past it once it closes; the client opens a new connection, which is decided anew.
- **Streaming is untouched.** The answer is copied as it arrives, never buffered. Measured on scratch ports against a stand-in that sent one SSE event every 400 ms: each arrived through the gate 3-4 ms after it was sent. 200 small `GET`s took 6.65 ms each directly and 7.02 ms through the gate (curl's own start included). When the client goes away, the gate closes the engine's connection, so the engine stops generating.

### 18.3 How the run's requests are known

**By their source address: the run's internal network's subnet.** The run reads it from `docker network inspect ragon-swe-bench` and writes it into its record. The gate is on the host's network, so it sees the instance container's own address: checked from a container on a scratch internal network, the gate passed it with that subnet recorded and refused it (logging `172.21.0.2`) with another. No other client can send from that subnet: a process on the host or a container elsewhere has another address, the LAN cannot route to it, and only the Docker socket (which can do anything anyway) could put a container on that network. A port, a path or a header would be guessable or visible to any client; no token is used.

A replica lane is reached through the relay, which connects to the replica's public port, so its requests meet the replica's gate, which is open unless the replica runs a benchmark of its own. The run then gives that lane no instance (it asks the replica's `/ragon-gate`). The gate protects the machine whose run it is.

### 18.4 The refusal, and what a refused user sees

HTTP **503** in the API's own error shape, `Content-Type: application/json`, `Connection: close`, **`Retry-After: 0`**:

```json
{"error": {"message": "The model is running a benchmark (<label>, default arm, N/M done, about H h left). Try later or run `ragon-admin night pause`.", "type": "service_unavailable", "code": "benchmark_running", "param": null}}
```

The label is `swe-bench run --label` (the night script passes `<night>, <arm> arm`; default `SWE-bench run <name>`); the progress is the run's finished instances over its instances, and the time left is the instances left times the median instance's time over the parallelism, refreshed every 15 seconds.

**Why `Retry-After: 0`.** The agent retries a 5xx at two layers, five HTTP attempts with a backoff from 0.2 to 1.6 s and then five turn retries, about 30 requests and 25 seconds before the error shows, and it honours `Retry-After` with no upper bound. Zero makes those retries immediate, so the message shows in about a second; any real delay would hang the turn (an hour would hang it for more than a day). The code is not `server_is_overloaded` or `slow_down`, which the agent replaces with a generic message of its own.

**What each client shows.** Checked with the installed build against a scratch gate:

- **`ragon exec`** prints `Reconnecting... 1/5` to `5/5`, then `ERROR: unexpected status 503 Service Unavailable: The model is running a benchmark (<label>, default arm, N/M done, about H h left). Try later or run `ragon-admin night pause`., url: http://…/v1/responses`, 1.1 s after it started.
- **The TUI**, by reading the code: the same text in an error cell, after a `Reconnecting… n/5` status line.
- **The app-server** sends an `error` notification carrying that message; the **desktop app** and **`ragon web`** (the same UI build) show the turn's `error.message`, and `ragon web ask` prints the error's parameters, the message included.

No client swallows it, so no display was changed. `ragon-admin run` with another agent (Cline, Continue, OpenHands) waits for the server with a one-token completion, which the gate refuses: it now prints the gate's message and stops instead of waiting out the run, and `ragon-admin status` shows the server as not answering completions. The wrapper `unexpected status 503 Service Unavailable: …, url: …` is the agent's own and stays: removing it would need a patch. The bridges and the sidecars show whatever their client libraries make of a 503 with that body; they were not checked one by one.

### 18.5 The run's side

`SweBenchGateHold` (`dreamference/swe_bench/swe_bench_gate_hold.py`) closes the gate once the run holds the runner lock and has its network, before admission, and opens it when the run ends:

- It writes `run.json` beside the gate's state: the run's name, a random id, the label, the subnet, progress and time left, its pid, and a heartbeat, refreshed every 15 seconds from a thread of its own. It removes the file at the end, only if the file is still its own.
- **While the gate is in force**, nobody else can reach the model, so admission skips the open-session check and the idle wait (it waits only for requests already in flight to finish), and an open session or another request no longer holds a start back. Memory and disk still do.
- **While paused, or with no gate answering** (a server started before the gate, or with `--no-gate`), everything is as before the gate: an open session or another request holds new starts back, and running instances finish. A run says when it finds no gate.
- **Why the run waits during a pause** rather than going on starting instances: an instance that shares the model with someone's work is not timed like the others, and a pause is meant to give the model back for a while.
- **The record:** `runs/<run>/gate.json` keeps, per run session, whether the gate was in force, and every pause interval clipped to the run. The report prints a `Model gate` line, and for pauses `Gate paused  N time(s), X in all (spans); K instance(s) ran during a pause … not comparable: <ids>`. `report --against` notes a differing gate and a paused run.

### 18.6 Pause and resume

`ragon-admin night pause [--for DURATION]` (default one hour; `90m`, `2h`, `45s` or minutes) writes `pause.json` (`since`, `until`). A second `pause` while one is in force extends it from now and keeps its start, so a run records one interval. `ragon-admin night resume` sets its end to now. A pause given with no run holding the gate also covers a run that starts before it ends. The gate reads the file on every request; the run, at every heartbeat. `night status` and `swe-bench status` say what the gate is doing.

### 18.7 Failure modes

- **The gate crashes:** Docker restarts it in about a second; the engine is untouched, and since its state is in files nothing is lost. Clients see a refused connection meanwhile.
- **The run dies without cleaning up** (SIGKILL, out of memory, a reboot): its heartbeat stops and the gate ignores a record older than 180 seconds, so it opens on its own. A missing or unreadable record means open. A run that ends normally removes its record at once.
- **A run hangs but its process lives:** the heartbeat thread keeps the gate closed. `ragon-admin night pause --for 24h`, or stopping the run, opens it. Not detected automatically.
- **The engine is down or loading:** the gate answers 502 ("not answering behind its gate; it may still be loading"); readiness polls see a non-200, as they saw a refused connection before.
- **Egress:** the gate connects only to the engine on loopback. `ragon` still connects to the model server's public port, so the egress audit's allowlist is unchanged.

### 18.8 At the switch to the new install

- The gate exists only once `ragon-admin server start` of the new install has started the model server. The server running before was started without one and keeps the public port itself; a run against it prints that no gate answers and waits as before. The two-model A/B (§17) swaps models with `server stop` and `server start` from its checkout, so its first swap brings the gate; the other night script swaps nothing and needs the server restarted before it starts. Both scripts label their runs (`--label "<night>, <arm> arm"`).
- `server start` refuses while a run holds the runner lock under the new `CODEX_HOME`. A run of the old install holds its lock under the old home, which the new code does not see, so the model server must be restarted only once such a run has finished.

### 18.9 Tests

`tests/test_model_gate.py` (32; 37 as): the rules (open without a live run, with a stale, unreadable or network-less record; closed by a live one; opened by a pause); the time-left wording; who passes a closed gate; the request head rewritten to one request per connection; the proxy live on loopback in front of a stand-in, streaming through without buffering (the stand-in sends its second event only once the client has read the first), refusing with the message, `Retry-After: 0` and the engine never seeing the request, passing the run's network and a pause, a chunked body reaching the engine whole, a kept-alive connection unable to carry a second request past a gate that closed, the answer's head rewritten to `Connection: close` after an interim `100 Continue` from an engine that wanted to keep the connection, 502 with the engine down, and the probe; the service file run on its own with `python3 -I` as the container runs it, its log free of the loading monitor's words; the container command; the copy at start and the probe that must answer, a gate that never answers removed; the readiness waiter of the other agents printing the message; pause, extension and resume; a hold removing only its own record; `server start` starting the gate after the old engine is removed and before the new one, falling back without it, and `--no-gate`; `night pause|resume` from the command line. `tests/test_swe_bench.py` (+7): a run writing its subnet and label into the record, admission waiting only for requests in flight, starts no longer held back, the gate open again after the run, the report line; the time left; a pause making the run wait and its report naming it; no subnet, no gate; the instances that ran during a pause; a replica with a closed gate getting no instance. `tests/test_night_shift.py` (+1): with priority only memory holds a start back. `tests/conftest.py` answers the gate probe with "no gate" for every test, since the real probe is an HTTP request to the configured server.

### 18.10 Not built

A per-run token as a second identification; removing the agent's `unexpected status …, url: …` wrapper (a patch); checking each bridge's display one by one; detecting a run that hangs while alive.

## 19. The review turn (`--review-turn`)

**Why.** The failure analysis found that in a number of failures the run's own output contradicted the fix and the agent carried on. A second look at the finished diff targets those. The user chose *review and test*: re-read the issue, read the diff, run the tests of the changed modules, fix what does not hold. It is an arm, off by default.

**What it does.** `swe-bench run --review-turn` (a new run only; the manifest records `review_turn`, and a resumed run keeps it). Once the agent's turns have ended (after its nudges) and before the patch is collected, the runner resumes the same session in the same container, `ragon exec … resume <session> <prompt>`, as a nudge is resumed, with `REVIEW_PROMPT` (`swe_bench_instance_run.py`):

```text
Before you finish, review your work:
- Re-read the issue.
- Read your own diff: `git status`, then `git diff` (a file you added shows only in `git status`).
- Run the test files of every module you changed.
- If the diff does not do what the issue asks, fix it.
- If a test that passed before your change now fails, decide from the issue whether the issue asks
  for the behaviour that test rules out: if it does, leave the test as it is and say so; if not,
  fix the source. Never edit an existing test to make it pass.
Then stop with a short summary.
```

- **One turn, outside the `nudges` budget**, and no nudge after it. It is not counted among the nudges.
- **Only on a changed tree after a turn that ended normally.** With no change there is no diff to review: a turn there would be a second attempt, not a review, and its last message would decide between `empty` and `stalled` in place of the agent's. After an error or a timeout, or with no time left in the task, the turn is skipped too. The state says why (`review.skipped`).
- **The same container, so the model gate (§18) passes it**: the gate decides by the source subnet, and the turn comes from the instance's own container.
- **No session to resume** (`ragon exec` reported no thread): a fresh session gets `REVIEW_FRESH_PROMPT`, the unattended preamble, the same rules, the issue and the diff as the agent's turns left it (cut at 40,000 characters). Recorded as `resumed: false`, and counted in the report.
- **The task's time limit covers it.** The deadline is not reset. If the limit is reached during the review, the container is stopped as for any timeout, started again, and the tree is collected as it stands; the instance's status is the one its patch earns (`done`), not `timeout`, because the agent itself finished. The state's `review.exec` is `timeout` and a note says the patch was collected as it stood.
- **The patch collected is the tree after the review turn.** The patch before it is kept as `scratch/<id>/patch-before-review.diff`, so the two can be graded apart (not built as a command). Collecting it stages everything, so `COLLECT_SCRIPT` now ends by putting the index back to `HEAD`, as the other scripts do: otherwise the agent's `git diff` would show nothing.

**The record.** Per instance, `state.review`: `resumed`, `exec` (`ok`, `error`, `timeout`), `seconds`, `tokens` (the turn's own, from the log between its offsets, counted as everything else is), `log_offset`, `patch_bytes_before`, and what the turn changed: `changed`, `added` and `removed` lines (`git diff --numstat` from the tree before the turn to the tree after it), `files` (up to 50). The report prints a `Review turn` line in every run, `off` when the arm was not on; with it on, in how many instances it ran and why not in the rest, in how many it changed the patch and by how many lines, how many reached the time limit, its time and tokens. `report --against` lists `review_turn` among the differing fields (a manifest written before the option counts as `false`), and the side-by-side table has a `review turn` row.

**Risk.** A second look can undo a correct fix. "A test that passed before your change now fails, fix it" is the sentence the failure analysis found false for many resolved tasks, whose correct fix fails an old test, so the review weighs such a test against the issue, in tests-v2's words, instead of always fixing the source. The measures are the patches the review changed and the instances resolved without it and unresolved with it; the second needs `patch-before-review.diff` graded.

**Tests** (`tests/test_swe_bench.py`, +9; the scripted agent checks, when reviewing, that the index is at `HEAD` and `git diff` shows its change): the prompts (terse, no benchmark words; the fresh one carries the issue, then the diff, cut when long); off by default and the report saying so; on, resuming the fix's session in the same container before collecting, with its tokens and the pre-review patch; a review that changes the diff, collected, with its lines counted; a review that reaches the time limit, its tree submitted, the status `done`, the note; no review with no time left, after no change or after an error, and the report's reasons; the fresh-session fallback; the arm kept by a resumed run, told apart by `--against`, `false` for an older manifest, and the option reaching the runner.

**Not checked without a real container:** that `ragon exec resume` in a container finds the session it wrote there (the nudges make the same call); how long the turn takes and what it costs in tokens; whether the agent follows the prompt; and whether a resumed process's `turn.completed` usage counts only the new turn (the report sums it as it sums the nudges').

## 20. `issue-v1` enforced by hooks (`--hooks issue-v1`)

**Why.** The task rule `issue-v1` (§15) only asks. An evidence gate that *holds* an action whose conditions are unmet: in upstream Codex, through its hooks, it beat the same conditions given as a paragraph, which did worse than no guidance at all. `--hooks issue-v1` enforces two of `issue-v1`'s lines, once each per task, with no model call and no Codex patch.

**It works under `ragon exec` with the sandbox bypass, from the source at the pin (rust-v0.158.0).** Hooks are the stable feature `hooks`, on by default (`codex-rs/features/src/lib.rs`, `Feature::CodexHooks`). `--dangerously-bypass-approvals-and-sandbox` and `--dangerously-bypass-hook-trust` are separate options (`utils/cli/src/shared_options.rs`); nothing ties hooks to the sandbox or the approval policy. `PreToolUse` runs for every tool call before its handler (`core/src/tools/registry.rs`, `run_pre_tool_use_hooks`): `exec_command` as `Bash` with `{command}`, `apply_patch` with the patch as `command`, an MCP tool with its arguments. A denial reaches the model as the tool's result, `Command blocked by PreToolUse hook: <reason>. Command: <command>` (`core/src/hook_runtime.rs`). `Stop` runs when a turn would end (`core/src/session/turn.rs`); a block records the reason as a prompt and the turn goes on. The one gate is trust: a hook runs only with `hooks.state."<config path>:<event>:<group>:<handler>".trusted_hash` equal to its hash (`hooks/src/engine/discovery.rs`, `hooks/src/lib.rs` `hook_key`), and an untrusted one is skipped in silence. `exec --json` drops hook events from its output (`exec/src/event_processor_with_jsonl_output.rs`), so the gate's own state file is the only record that a hook ran.

**Registration.** When the container starts, before the launcher's first run, the runner writes three groups into the instance's `codex-home/config.toml`: `PreToolUse`, `PostToolUse` and `Stop`, no matcher (every tool, MCP tools included), one command each, timeout 30 s, and their trust entries under `/ragon-scratch/codex-home/config.toml:<event>:0:0` (`SweBenchHooks`). The hash is the launcher's scheme (canonical JSON, SHA-256), pinned by a test to the value a live session accepted for the ledger hook. The launcher edits the same file with `toml_edit` and keeps groups and trust entries that are not its own. The gate, `swe_bench_issue_gate.py`, is mounted read-only at `/opt/ragon-issue-gate/issue_gate.py`; the command tries conda's base Python, then the testbed environment's, then `/usr/bin/python3` (a login shell may reset `PATH`), and does nothing if none is there. The file is standard library only and runs on Python 3.6 or later (a test parses it as 3.6). The manifest records `hooks` and `hooks_gate_sha256`.

**Conditions.** Once `/testbed` is prepared (in the refine arm, only after the study step, which is neither held nor counted), the runner extracts from the issue as the agent sees it (`SweBenchIssueTargets`): code-file paths in the prose, dotted names in the prose and inline code, names written `name(` or in backticks, paths in code blocks, and traceback frames, deepest first. It resolves them in the container with one script: `git ls-files`, then `git grep` for the names' `def`/`class` lines in tracked Python files. A path is the tracked file it names, or the longest tail of it that names exactly one (`/usr/lib/.../site-packages/django/db/models/query.py`, a GitHub URL); a bare base name must be unique. A dotted name is a module file (`a.b.c` is `a/b/c.py` or `a/b/c/__init__.py`), or a module and a definition in it, or `Class.method` narrowed to the files that define the class. A name defined in more than 3 non-test files, or nowhere, is dropped, as is a definition whose file is already a target; at most 6 targets, in the order above. Examples: a fenced block that parses as Python, calls something and is more than one bare expression; `>>>` sessions, fenced or not; or a shell command after `$ ` (or in a shell block) that is not setup (`pip`, `cd`, `git clone`). Output, tracebacks and Python 2 are not examples, so when unsure there is none. Everything is written to `scratch/<id>/issue-gate/conditions.json`, and the targets, what was dropped and why, and each example's first line go to `state.hooks.conditions`.

**The edit hold** (`PreToolUse`). An edit is a change to a file that already exists in the repository: `apply_patch` (the tool, or typed into the shell) updating or deleting a file, `sed -i`, `perl -i`, `>`, `>>` or `tee` onto one, `git apply`, `git am` or `patch`. A new file (a script in `/tmp` or the repository) is not an edit, and neither is `git stash`; a program that writes the file itself (`python -c "open(...,'w')"`), `cp` and `mv` are not detected. A target is read when a tool call's command or output names its path (or the command names its directory and then its base name), or, for a definition, when the output shows `def <name>`/`class <name>` or the call touched a file defining it. Naming it in a search command is not reading it. If anything is unread at the first edit, that edit is denied once:

```text
Not yet: read what the issue names before your first edit. Still unread: <path>; `<name>` (<file>). Open each one (cat, sed -n, or grep -n for its definition), then make the edit again
```

**The stop hold** (`Stop`). If the issue has an example, the agent has edited, and no command after the last edit ran the example, the first stop is blocked once:

```text
Before you stop: the issue shows an example, and no command since your last edit has run it (it starts `<first line>`). Run the issue's example now and check that its output is what the issue expects; if it is not, fix the code. Then stop.
```

A command ran a Python example when code it runs (a `-c` argument, a here-document fed to Python, or a script run with Python or pytest, read from disk) contains one of the example's lines with a call in it (whitespace ignored), or calls at least half of the example's distinctive functions; a script that cannot be read counts, since it may be the example. A shell example ran when the command runs the same program on the same file. No edit, or no example, means no hold.

**At most one hold of each kind per task**, across the fix, the nudges and the review turn: the gate's state lives in the instance's scratch, so a wrong condition (an example that is itself wrong, as in 13798) costs one turn and can never trap the agent. A fault of the gate's own (bad conditions, an exception) allows the call and is recorded in `state.hooks.errors`; the process exits 2 with the reason on stderr only to block.

**The record.** Per instance, `state.hooks`: `sets`, `conditions`, `ran` (any hook ran once the conditions existed), `invocations` per event, `targets`, `examples`, `edit_hold` (`unread`, `kind`, `complied`: whether by the next edit the agent had read what it was told was unread, `null` if it never edited again, `read_after`), `stop_hold` (`complied`: whether a command after the hold ran the example before the next stop), `first_edit` (`read` of `of` targets before the first edit attempt), `edits`, `example_run_after_last_edit` at the last stop, `errors`. The report prints a `Hooks` line in every run (`off` when the arm was not on), and with it on, where the hooks never ran (those instances ran as if there were none), and for each hold where it fired and whether the agent complied, the first edits that came after reading everything named, and the example runs after the last edit. `report --against` lists `hooks` among the differing fields (an older manifest counts as `[]`), the side-by-side table has a `hooks` row, and a comparison in which the hooks never ran says it proves nothing about them.

**The arm.** `--hooks issue-v1`, beside `--task-rules` and independent of it: the survey's test is `--task-rules issue-v1` against `--task-rules issue-v1 --hooks issue-v1` on the same list (name the second arm `…-issue-v1-hooks`). With `--strip-names`, the names the reference fix touches are gone from the issue, so the edit hold has less to ask for; that is recorded per instance, not prevented.

**With a restart arm** (branch `bench/restart-after-compactions`, merged into `main` as §21): the two stack; the gate's conditions and its one-hold-per-kind state live in the scratch folder, so a hold that fired before a restart does not fire again in the fresh session, and the hook measures count across both sessions. The replay count for that arm's trigger is `scripts/context_budget_replay.py compactions <run dir>`.

**Tests** (`tests/test_swe_bench.py`, +24). The hash against the pinned value, and each hook's trust entry against its own command; the gate parsed as Python 3.6 and importing nothing from the package; extraction and ranking, resolution by tail and by module, ambiguity and test files, the target cap; what is and is not an example; the light index run against a scratch repository; what counts as an edit, a read and an example run; both holds firing once, never twice, with compliance recorded both ways; no hold without targets, an example or an edit; the gate process (exit 2 and stderr, no conditions yet, its own fault never blocking); and through the scripted agent, which calls the gate as Codex would: registration and mount, both holds and their messages, the state and the report, an agent that ignores both holds, hooks that never ran and the comparison saying so, the refine arm's study step running before the conditions exist, a resumed run keeping the arm, and the option reaching the runner.

**Not checked without a real container:** that Codex accepts the trust entries the runner writes (the scheme is the one the ledger's live check confirmed, but these commands were never run by Codex); that the launcher in the container leaves the `[hooks]` tables as written; that conda's base Python is at `/opt/miniconda3/bin/python3` in the arm64 images; that `apply_patch` reaches the hook as the `apply_patch` tool (the shell form is detected too); how often the holds fire, what they cost in turns, and whether the agent complies.

## 21. Restart instead of another compaction (`--restart-after-compactions N`)

*Built on branch `bench/restart-after-compactions`, off by default; merged into main (`scripts/swe_bench_ab.sh` with `ARMS="default restart-2"`).* Literature: FailFast-RestartSmart's gain on a 27B model came from a fresh rollout with the interrupted diff offered as an overlay, and the failure analysis saw compaction harm several runs. There is no monitor model here, so the trigger is a rule on the count of compactions.

**What it does.** While the fix session's turns run (the first turn and its nudges; the refine arm's study step and the review turn are outside it), the runner's 2-second poll counts the `compacted` records in the instance's rollouts under `scratch/<id>/codex-home/sessions/` (the records `scripts/context_budget_replay.py` counts), minus the count when the fix session started, so the study step's compactions do not count. At `N` it stops the container as the time limit does, writes the stopped session's diff to `scratch/<id>/restart-diff.patch` (`COLLECT_SCRIPT`), resets the tree to the base (`RESET_SCRIPT`, so the patch is **not** applied for the agent), and starts **one** fresh session in the same container: `RESTART_PREAMBLE` (where the patch is; read it; `git apply` it only if it is right, else take what helps or ignore it) and then the task prompt again, task rules and refined description included. The fresh session's turn then goes on as the fix session's did: the nudge budget and the task's deadline are not reset, and a restart is not a nudge. A second burst of compactions does not restart again. The survey's "instead of a third compaction" is `--restart-after-compactions 2`.

**Recorded.** The manifest's `restart_after_compactions` (0 when off; a run resumed keeps it; a manifest from before the option reads as 0, and `--against` prints `differs: restart_after_compactions`). The instance's state gets `restart`: compactions counted, seconds into the task, nudges before, the stopped session's id, the diff's bytes and the log offset where the fresh session's events start; `exec` stays the fix session's final outcome. The report prints a `Restart` line every time (off, or how many of the finished instances restarted, the median time into the task, how many had a diff to hand over, and their ids), so two reports give the survey's measure: resolved among the restarted tasks against the same tasks in the record arm.

**With `--hooks issue-v1`.** The two stack. The gate's conditions and its one-hold-per-kind state live in the scratch folder, so a hold that fired before the restart does not fire again in the fresh session, and the hook measures count across both sessions. Said in the report's hook line when a restart happened: not yet built; read the `restart` record beside the `hooks` one.

**Not built (v2).** The ledger as notes for the fresh session (the launcher's compaction ledger, `ragon-rs/src/compaction.rs`): the fresh session gets the diff only. The turn-count trigger the survey also names. Mid-turn the compaction count is read from files the agent's process writes; a rollout whose last line is still being written is skipped and read on the next poll.

**Tests** (`tests/test_swe_bench.py`, +5, with a `compacting` mode of the fake agent that records compactions in a rollout as Codex writes them and never finishes): the restart happens once at `N` with the diff in the file, the tree at the base and the prompt as specified, and the patch is the fresh session's; below `N`, and with the arm off, the session runs to the time limit; a second burst does not restart again; the study step's compactions do not count; the arm is kept on a resume, told apart by `--against`, and parsed from the command line.

**Decision.** The A/B showed no measurable difference from the default and a longer wall time, so the arm is dropped and the default stays (a session compacts as often as it needs).

## 22. `apply_patch` as a function tool (`--apply-patch`)

*Built on branch `bench/apply-patch-function`, off by default; merged into main (the user's decision: record against record plus `--apply-patch function`, same list, no change to the default; `scripts/swe_bench_ab.sh` with `ARMS="default apply-patch-function"`, whose `start` refuses an installed `ragon` that does not read `DREAMFERENCE_RAGON_APPLY_PATCH`: the launcher's override reached main after the release was cut). The A/B therefore needs a source build of its harness commit, not the release: the chain runs `ragon-admin codex build` from the worktree at the pinned commit, requires `ragon-admin audit egress` to pass and the new `ragon` to read the variable, and only then starts the night; any failure writes a `blocked` file and starts nothing.* Codex offers its `apply_patch` tool only when the model catalog's `apply_patch_tool_type` is set, and the launcher sets it only on ragon-engine, in the freeform form, because production SGLang drops a `custom` tool silently ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md), the catalog entry; [INFERENCE](./DREAMFERENCE_INFERENCE.md) §5.3). So on SGLang the agent edits through the shell. The function form of `apply_patch` is an ordinary function tool SGLang serves; whether offering it helps a 27B model is the question this arm was built to measure.

**What it does.** `run --apply-patch function|freeform|off` sets `DREAMFERENCE_RAGON_APPLY_PATCH` for every session of the run (the launcher's override, `ragon-rs/src/lib.rs`), and the launcher writes that form into the catalog; with no flag the variable is not set and the launcher decides as it does for every user. The manifest records `apply_patch` (`auto` when the flag was absent; a resumed run keeps it; a manifest from before the option reads as `auto`; `--against` prints `differs: apply_patch`). When a task ends, the instance's state gets `apply_patch`: the form, and the number of `apply_patch` calls counted from its rollouts (`function_call` records named `apply_patch`, and `custom_tool_call` ones for the freeform form; an `exec_command` that mentions the name is not a call). Every report prints an `Apply patch` line: the form and, over the finished instances, in how many the agent called the tool and how many calls, with their ids, so the behaviour measure (did the model use the tool at all, and where) is read before the resolve count.

**Decision rule (the user):** a clear win for the function form on SGLang makes it the launcher's SGLang default, said in the notes; a tie leaves the launcher's choice as it is.

**Tests** (`tests/test_swe_bench.py`, +3, with the fake agent editing through `apply_patch` and recording the call as Codex does when the override names a form): the variable is set only when the flag is, the call is counted for the function and the freeform forms and not for a shell command that mentions the tool, `off` is passed through, the report line and `--against` tell the arms apart, a resumed run keeps the arm, an old manifest reads as `auto`, and the command line rejects any other form.

**Outcome: the arm could not run.** Codex rust-v0.158.0's `ApplyPatchToolType` has only `freeform`, so a catalog that says `"function"` makes `ragon` exit at start ("failed to parse model_catalog_json … unknown variant `function`, expected `freeform`"): every task of the arm ended as an agent error in about 7 s. Nothing was measured about the function form. The default is unchanged (no `apply_patch` tool on SGLang); what to do with the `function` value of `DREAMFERENCE_RAGON_APPLY_PATCH` (remove it, or translate it as `exec` is in §23) waits for an owner decision, and the code is unchanged.

**The function form is refused.** `run --apply-patch` takes `freeform` or `off`; the night script ends a night whose arm says `apply-patch-function` before any round; and the launcher stops at start with a line saying the pinned Codex offers apply_patch only as freeform (`check_apply_patch_setting`), instead of writing a catalog Codex cannot read. A function form needs a Codex that has one.

## 23. Code Mode's `exec` as a function (`--exec-tool`)

*The owner's decision: the Code Mode A/B on production SGLang, unattended, on a source build of its pinned commit (`ragon` rebuilt between runs only if the tests and the egress audit pass).* Codex offers Code Mode's `exec` as a `custom` tool, which SGLang drops, so before the translation the model on SGLang never saw it. With the translation ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md); `ragon-rs/src/exec_tool.rs`, `ragon-rs/tools`) the launcher offers `exec` as a function on every server that is not ragon-engine, and turns the model's call back into the custom tool call Codex records. `DREAMFERENCE_RAGON_EXEC_TOOL=freeform` turns that off, `function` forces it; unset, the launcher decides.

**What it does.** `run --exec-tool function|freeform` sets `DREAMFERENCE_RAGON_EXEC_TOOL` for every session of the run (the runner's extra environment of each container); with no flag the variable is not set and the launcher decides as it does for every user. The manifest records `exec_tool` (`auto` when the flag was absent; a resumed run keeps it; a manifest from before the option reads as `auto`, which for a `ragon` older than the translation meant Codex's custom tool; `--against` prints `differs: exec_tool`). When a task ends, the instance's state gets `exec_tool`: the form, and the number of `exec` calls counted from its rollouts (`custom_tool_call` and `function_call` records named `exec`: a translated call is recorded as the custom tool call, so both forms count alike; an `exec_command` that mentions the name is not a call). Every report prints an `Exec tool` line beside the `Apply patch` line.

**The A/B.** `scripts/swe_bench_ab.sh` with `ARMS="exec-tool-freeform default"` on `docs/dev/swe-bench/no-mismatch.txt`: the record arm first, as always, is `exec` as Codex offers it (`--exec-tool freeform`, which SGLang drops); the candidate is `default`, the launcher's own choice, which on SGLang is now the function. An arm part `exec-tool-function|freeform` stacks with the others; a misspelt one writes `failed` before either round; `start` refuses a worktree without `--exec-tool` and an installed `ragon` that does not read the variable (an older build would run the record arm twice). The chain that starts it builds `ragon` from the worktree, requires the egress audit to pass and the binary to read the variable, and runs one live `ragon exec` through a recording loopback proxy in front of the model server before the night: the request must offer `exec` as a function (one string parameter `input`) and no `custom` `exec`, and the session must end without an error; whether the model called `exec` is recorded, not required. Any failure writes a `blocked` file and waits for the owner.

**Tests** (`tests/test_swe_bench.py`, +3, with the fake agent fixing the issue in one `exec` cell recorded as Codex records a translated call when the switch says `function`; `tests/test_swe_bench_night_script.py`, +3): the variable is set only when the flag is, the call is counted for the function form and not for a shell command naming the tool, `freeform` is passed through, the report line and `--against` tell the arms apart, a resumed run keeps the arm, an old manifest reads as `auto`, the command line rejects any other form; the arms reach the right rounds, a misspelt arm ends the night before either, and `start` refuses a `ragon` without the variable.

**Decision:** the translation stays on.

## 24. Broken passing tests, counted apart

*Built, approved by the owner the same day ([BACKLOG](./DREAMFERENCE_BACKLOG.md) item 2).* "Resolved" requires every FAIL_TO_PASS test to pass and every PASS_TO_PASS test to keep passing, so an unresolved patch failed in one of two ways with different remedies: it did not fix the issue, or it broke something that worked (the failure `tests-v2`'s discipline rules aim at).

**What the report shows.** Each unresolved verdict gets the first cause that applies: empty patch, patch did not apply, the instance's test patch did not apply on top of it, **broke a passing test** (a PASS_TO_PASS test fails; how many of those also left a FAIL_TO_PASS test failing is said), **did not fix** (a FAIL_TO_PASS test fails, every PASS_TO_PASS test passes), or no test results on disk. `report` prints the broke line with its share of graded instances and the instances with their failing PASS_TO_PASS counts; `--against` adds both counts to the side-by-side table and marks `broke P2P` in the per-instance rows.

**Where the numbers come from.** `eval` now records the two lists' sizes and failures in `grading.json` (`f2p`, `f2p_failed`, `p2p`, `p2p_failed`; counts only, the names stay in the harness's logs). A grading made before that is split from the harness's per-instance `report.json` (`tests_status`) in the grading's directory, read only, with no regrade; a `drop-test-hunks` verdict taken over from the plain grading is read from the plain grading's logs.



**Tests** (`tests/test_swe_bench.py`, +3; the stand-in harness writes `tests_status` for the instances a test names): the three outcomes counted and printed, the side-by-side rows; a grading without the counts split from the logs with no harness call, the regrade series reading the plain grading's logs, a missing report counted as no test results rather than guessed; the first-cause order.

## 25. Compaction columns

*Built, approved by the owner the same day ([BACKLOG](./DREAMFERENCE_BACKLOG.md) item 11, its report half; the probe questions are not built; [compaction survey](./research/compaction-survey.md) §1 and §3.5).* The survey found compaction to be the largest cost in a long session that is not model work, and sessions that compacted often resolved fewer tasks. The report now shows this for every run, so an arm that changes compaction (BACKLOG item 7, drop-first compaction) is read on it.

**What the report shows.** `Compactions`: how many, in how many of the instances with rollouts, their time in all, its share of those instances' agent time, the median per compaction; and `resolved by count`: resolved over instances for sessions with 0, 1, 2 and 3 or more compactions. `--against` adds `compactions` and `compaction time` to the side-by-side table.

**How it is counted** (the survey's method, `SweBenchRunStore.compactions`): one compaction per `compacted` record in the rollouts under `scratch/<id>/codex-home/sessions/` (every session of the instance: the refine arm's study step and the restart arm's fresh session included). Its time is the gap from its record back to the last earlier record more than 2 s before it, which covers the compaction request's prefill and the summary's decode, an upper bound by a second or two. A record with no time is counted, not timed. An instance whose rollouts are gone is named as not counted.


**Tests** (`tests/test_swe_bench.py`, +1): the count, the durations (the 2 s rule, a record with no time counted but not timed, a torn last line skipped), an instance without rollouts named, resolved by count, and the side-by-side rows.

## 26. Drop-first compaction (`--drop-old-outputs`)

*Built on branch `compaction/drop-old-outputs`, off by default (merged into `main`); the owner approved it as the first compaction change to test, as a night arm. Not yet run.* What it does and why is [RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md) §13: when a session is about to compact, the output of its older tool calls is dropped from the history (the calls stay; the last 10 outputs and those under 600 characters are kept) and Codex summarises only if that does not leave 15% of the limit free.

**What it does here.** `run --drop-old-outputs` sets `DREAMFERENCE_RAGON_DROP_OLD_OUTPUTS=1` for every session of the run; without the flag the variable is not set. The manifest records `drop_old_outputs` (a resumed run keeps it; an older manifest reads as `False`; `--against` prints `differs: drop_old_outputs`). Every instance's state gets `compaction` in **every** run, so the record arm is measured the same way: `compactions` and `seconds` (each compaction's time from the last rollout line more than 2 s before its `compacted` record, the compaction survey's §1 method), and from `codex-home/masking/dropped/log.jsonl` `drops` (a drop that replaced a summary), `declined` (a drop that would not have been enough, so the summary ran) and `dropped_outputs`. Every report prints a `Compaction` line.

**The night.** `scripts/swe_bench_ab.sh` with `ARMS="default drop-outputs"` (record arm first); the part stacks with the others (`drop-outputs+exec-tool-function`); a misspelt one writes `failed` before either round; `start` refuses a worktree without `--drop-old-outputs` and an installed `ragon` that does not read the variable (a `ragon` without patch `0026` would run the record arm twice). The `ragon` the night runs must be built from a commit that has the patch.

**Acceptance (survey §3.1):** resolved not lower than the record arm, compaction seconds down.

**Tests** (`tests/test_swe_bench.py`, +3, with the fake agent writing a drop log and a timed compaction when the variable is set; `tests/test_swe_bench_night_script.py`, +3): the variable only with the flag, the record and the report line in both arms, untimed compactions counted without seconds, the manifest field told apart by `--against` and read as `False` when absent, the command line; the arm reaches only its round and stacks, a misspelt arm ends the night, `start` refuses a `ragon` without the variable.


## 27. Image chunks, shards and their merge (150-task nights)

*The owner's decisions: a night list holds about 150 tasks (`docs/dev/swe-bench/fresh-150k.txt`: tasks never used before and tasks reused under §16's rotation rule), its images are pulled, run and graded in chunks, and one list can be split over several machines.* Built on branch `bench/chunked-images` (merged into `main`).

**Image chunks.** With `--eval --remove-images` a run used to hold a whole repository's images while it ran and graded it: fresh-150k's 86 django tasks would need about 200 GB at once, the whole free disk. Now a repository of more than `image_chunk` instances (`[swe_bench] image_chunk`, default 25, `run --image-chunk N` and `eval --image-chunk N` override it, 0 is the whole repository) is cut into as few chunks of at most that many as hold it, as equal as can be and in run order (86 at 25: 22, 22, 21, 21; `SweBenchRunner.image_chunks`). Each chunk is run, graded and its images removed before the next starts. The chunks are cut from the run's whole instance list, so a resumed run has the same chunks and skips those with nothing left to run. `eval --remove-images` (the drop-test-hunks regrade) grades in the same chunks. The night script's `IMAGE_CHUNK` (default 25) reaches every round and regrade, and `start`'s disk check counts one chunk of the largest repository instead of the whole repository: `min(largest, IMAGE_CHUNK) × 3 GB + RESERVE_GB`, 175 GB for fresh-150k or any of its thirds at the default reserve.

**Shards.** `run --shard I/K` makes a new run of the I-th of K shards of the selected instances (`SweBenchShards`). The split depends only on the set of ids: the largest repository is dealt first, each repository tier by tier in the order of the ids' SHA-256, and each task goes to a shard with room left (shards end `n // K` or one more) holding the fewest of its repository and tier, then of its tier, then of its repository. The tiers are frozen in `SweBenchDifficulty` (computed from the leaderboard data the fresh lists use), so a machine without that data splits the same way. Over fresh-150k each third comes out 30/8/12 or 29/8/13 easy/medium/hard, with repositories within two of each other. The manifest records `shard` (`index`, `of`, `list_instances`, `list_sha256` of the sorted ids). A resumed run keeps its shard, and naming another is refused. `swe-bench shard LIST --shards K` writes the same shards as lists of their own (`<list>-<I>of<K>.txt`, with a header naming the list's digest and each shard's mix), for a night that runs one shard as its list. `--balance-by OTHER` (repeatable) adds a stratum: each task's group is the set of named lists holding it, and the shards hold each group as evenly as the split allows. Such shards differ from `run --shard`'s, and their headers say so. fresh-150k's thirds (`fresh-150k-1of3.txt` to `-3of3.txt`) are made with `--balance-by fresh-61j.txt`: 21, 20 and 20 of the 61 fresh tasks, easy/medium/hard 30/8/12, 30/8/12 and 29/8/13, django 28, 29 and 29. The night script's `SHARD=I/K` passes `--shard` to every round, never to a smoke.

**A pinned series.** The nights of the thirds run from one commit and one `ragon` build, since the merge is not relaxed. The night script's `REF` as a full commit makes the first `start` build `ragon` at it (`codex build`, never while a `ragon-swe-*` unit runs) and record the build's sha256 in `$PINS/ragon-<REF>.sha256`. A later `start` at that REF fetches nothing when the commit is here, skips the build, and refuses an installed `ragon` that is not the recorded one. The unit checks the build again before every round (`RAGON_PIN_SHA256`).

**The merge.** `swe-bench merge NAME RUN…` writes a run of its own from the runs of one list's shards, which `report` and `report --against` read like any other run. Instance states, agent logs and predictions are copied, each instance's scratch is linked (the rollouts stay where they are), the gate records are joined, and each grading series' latest grading of every shard becomes the merged run's grading 1, with each verdict's test counts read in from the shard's harness logs. The manifest is the first shard's with the instances and per-instance tables joined, `merged_from`, and every shard under `shards`. A merge is refused when any manifest key other than where and when a shard ran differs (a merged run measures one configuration), when an instance is in two runs, when shards and runs that are not shards are mixed, when the shards are of different lists or one shard is given twice, or when one series' gradings have different graders. A missing shard or an unfinished instance is a warning. `run` refuses a merged run. A shard made on another machine is copied into this machine's runs directory first.

**Tests** (`tests/test_swe_bench_shards.py`, new; `tests/test_swe_bench.py`, +13; `tests/test_swe_bench_night_script.py`, +8): the frozen tiers cover Verified once each; shards partition the list and balance size, tier and repository for 2 to 5 shards, and are the same whatever the ids' order; the shard lists equal `pick`. A run with chunks of one grades and removes one image at a time, the configured chunk applies with no flag, and `eval` grades in chunks. Two shards cover the list once and record what they are, a resumed shard keeps its shard, the merged run reports the shards' verdicts as one run, and the refusals hold. Every round and regrade takes the chunk and a round its shard, `start` needs disk for one chunk, and the shard reaches the unit but never a smoke. `--balance-by` spreads the other list within one task while keeping tier and repository balanced, and its lists say they are not `--shard`'s. A pinned series builds once, records the build and never rebuilds, refuses another `ragon`, an unpinned REF neither builds nor records, and each round of a pinned night checks the build.

## 29. The guards: the failure analysis in the product (`--test-guard`, `--finish-check`)

*Built on branch `bench/failure-arms` (merged into `main`; §28 was reserved for `bench/swe-rebench`), off by default; the owner's decision of the same day: build the failure analysis' proposals as switchable night arms, A/B them on `fresh-150k`'s thirds, make only winners defaults. Not yet run.* Most of those proposals were already arms, but benchmark-side ones: lines in the task prompt (`--task-rules`), a resume by the runner (`--review-turn`, the completion nudge), a gate mounted into the container (`--hooks issue-v1`), a regrade (`--drop-test-hunks`). None of them reaches a user. The two arms here are the launcher's, so whatever wins is what `ragon`, `ragon exec` and Night Shift do: the run only sets the launcher's switches, as `--drop-old-outputs` does. Both are rules on the agent's own work in the repository, never on the benchmark: no hidden test, no gold patch, no `FAIL_TO_PASS` list is read. They are enforced, not asked for, since the literature found a hold beat the same conditions given as a paragraph.

**The test guard** (`DREAMFERENCE_RAGON_TEST_GUARD`, `ragon_test_guard`; the failure analysis found agents rewriting an existing test that their own change had broken, or changing expectations to match). When a tool call leaves an existing test file with an assertion it had at `HEAD` gone or rewritten, a test function deleted, or a skip added (`@pytest.mark.skip`, `xfail`, `@unittest.skip`, `#[ignore]`, `it.skip`, …), the call's result carries a note, once per file:

```text
Note: this changes an existing test: tests/x.py (`- assert f() == 1`). Change an existing test only when the task asks for the behaviour it checks to change. If it does not, restore the test (`git checkout -- tests/x.py`) and fix the code instead.
```

and the first stop of the session while such a change stands is held once ("Your change weakens existing tests: … Unless the task asks for the behaviour those tests check to change, restore them and fix the code; if it does, keep them and say in your summary which behaviour the task changes"). It asks and never refuses: in many resolved runs whose patch removed test lines, the reference test patch changes the same assertions, so the issue decides, in `tests-v2`'s words. A line counts as lost only if no equal line (indentation aside) is left in the file, so a moved or re-indented assertion is not a change, and a test file the session created is never one.

**The finish check** (`DREAMFERENCE_RAGON_FINISH_CHECK`, `ragon_finish_check`). Three holds, each at most once per session, combined into one message when several apply at the same stop, and only in a session whose tool calls changed the tree:

- **untested** (the failure analysis' "broke other tests"; the owner's "run the touched modules' existing tests before stopping"): the session changed tracked source files (not tests, not docs) and no test runner ran on the tree as it now is. The message names up to three changed files and up to three existing tests that look like theirs (`test_<module>`, `<module>_test`, a `tests/<module>/` folder), and asks for them to be run, compared by name without the change (`git stash`) when any fail, and the failures the change caused fixed.
- **pre-existing** (the failure analysis' "'pre-existing' is said without comparing names"): the last message calls failures pre-existing (or "already failing", "unrelated to my change", "fails on the base") and no test ever ran on the code without the change (a test run at the session's starting tree or a clean one, or a command with `git stash` or `git worktree`).
- **mid-work** (the completion nudge, as a product rule): the last 400 characters of the last message announce a next step (Night Shift's `ANNOUNCES_WORK`), "let me know" aside, and they are not an offer to the reader ("If you'd like, I'll also …", a question at the end), which an interactive session ends with legitimately.

**How it works** (`ragon-rs/src/guard.rs`; no Codex patch). With either switch on, the launcher registers two hooks in `config.toml` with their trust entries, one `PostToolUse` and one `Stop` group, no matcher, both `<ragon> guard <names>` (`tests`, `finish` or `tests.finish`: the guards are on the command line, because Codex gives a hook the session's environment snapshot, which may leave a variable out). It does so for every session, `exec` included; with both off it removes them. The `PostToolUse` hook records after every tool call a fingerprint of the working tree (SHA-256 of `git diff HEAD` and of the untracked files' names, sizes and times), so an edit is seen whatever made it (the `apply_patch` tool, `sed -i`, a script, a call from Code Mode), and whether the call ran a test runner (`pytest`, `runtests.py`, `manage.py test`, `bin/test`, `tox`, `cargo test`, `go test`, `npm test`, `python …test….py`, …; the patch tool's and Code Mode's own input are not read, since their text is a file's or a script's, and Code Mode's inner calls reach the hook on their own). A call that leaves the tree in a state seen before (a `git stash` and its `pop`) is not an edit, so testing without the change does not count as untested afterwards. The test guard's note goes back as the call's additional context (`hookSpecificOutput.additionalContext`), never as a block: a `PostToolUse` block replaces the call's output, and the edit has happened anyway. The holds are a `Stop` block (`{"decision":"block","reason":…}`), which Codex turns into a prompt and the turn goes on; a stop with `stop_hook_active` set is never held, so one stop is held at most once. State is `$CODEX_HOME/guards/<session>.json` (the folder above `sessions/` in the hook's `transcript_path`), with a lock file for parallel tool calls; it is also the record a run reads. The baseline is `HEAD`: a Night Shift worktree starts there, and the instance images start with a working tree equal to `HEAD` (checked in the django and sympy images on this machine: no file differs; where `base_tree_equal` is false it is `HEAD` against `base_commit`, not the working tree), so a change made before the session counts as the session's in an interactive session started on a dirty tree, and only there; the holds still need an edit made during the session. Outside a git repository, without `HEAD`, or on any fault of its own (git missing, an unreadable state file), the guard lets the call and the stop through. Night Shift's process check counts `ragon guard` as a hook, not a session (`NON_INTERACTIVE`). In the TUI a start-up line says which guard is on.

**What the arm does here.** `run --test-guard` and `run --finish-check` (stackable) set `DREAMFERENCE_RAGON_TEST_GUARD=1` and `DREAMFERENCE_RAGON_FINISH_CHECK=1` for every session of the run; without the flag the variable is not set, so the record arm measures what a user gets. The manifest records `test_guard` and `finish_check` (a manifest from before the options reads as `False`; `--against` prints `differs: …`). Every instance's state gets `guards` in every run, from the state files in its scratch `codex-home`: `sessions`, the totals `calls`, `edits`, `test_runs` and `notes`, `holds` (per kind: `fired`, `complied`: a test ran after an untested hold, a run without the change after a pre-existing hold, a tool call after a mid-work hold, every named line back after a test-edits hold), and the last stop's view (`edited`, `code_changed`, `tested_now`, `weakened_files`). Every report prints a `Guards` line (both switches, and when one is on: where the hooks ran, notes, holds and compliance per kind, and in how many instances whose last stop saw changed code that code had been tested) and a `Weakened tests` line in every run, arm or not, from the patches alone (`SweBenchPatchFilter.weakened_tests`: the guard's line rule, an assertion removed or rewritten, a test deleted or skipped, on the harness's test-path rule, `tests/`, `testing/` and Python test names, in a test file the patch did not create; `guard.rs` also counts a top-level `test/`, `src/test/`, `spec/` and other languages' names, which agree with it on Verified's Python repositories), so the record arm is measured by the same rule. A run with a guard on and no state file anywhere says its hooks never ran, and `--against` that the comparison says nothing about them. The side-by-side table has `guards` and `weakened tests` rows.

**The night.** `scripts/swe_bench_ab.sh` arm parts `test-guard` and `finish-check` (`ARMS="<record> <record>+test-guard"`; a misspelling writes `failed` before either round); `start` refuses a worktree without the flags and an installed `ragon` that does not contain the variables' names (a `ragon` without `guard.rs` would run the record arm twice). The `ragon` the night runs must be built from a commit that has `guard.rs`.

**Decided by** (the behaviour first, as every night): for the test guard, `Weakened tests` down against the record arm and `broke a passing test` not up, then the resolve count; for the finish check, `broke a passing test` down, holds fired and complied (an untested hold that leads to a test run is the measure), the tested-at-last-stop share up, wall time not more than about 10% up, then the resolve count. Exposure: a hold costs a turn; the test guard may talk the agent out of an assertion change the issue asked for, which the `did not fix` count catches.

**Tests.** `ragon-rs/src/guard.rs` (+14, in a scratch repository with a real `git`): the switches; test and code paths (`django/test/` is framework code); what weakens a test and what does not (moved, re-indented, comments); test commands and last messages (mid-work, offers, pre-existing); nothing outside a repository or with no guard; an untested change held once, the following stop let through, a later test run complying; a tested change, a read-only session and a stash-and-pop not held; a pre-existing claim with no baseline and a mid-work stop held together; a weakened test noted once, its stop held, restored and complying; the test guard alone holding no untested change; the state's round trip; the two hooks registered side by side, rewritten and removed together, after another hook under `Stop` without moving it; and both trust hashes against Codex's own types. `tests/test_swe_bench.py` (+6, the fake agent writing a guard state when a variable is set): both variables only with the flags, the manifest, the per-instance record and the report lines in both arms; one guard alone; hooks that never ran; `--against` and the command line; the patch rule; and the `Weakened tests` line in a run without the arm. `tests/test_swe_bench_night_script.py` (+10): the arms reach only their round and stack, misspellings end the night, `start` refuses a `ragon` without either variable. `tests/test_night_shift.py`: `ragon guard` is not a session.

**Checked in the source at the pin:** Code Mode's inner calls go through the same `handle_tool_call_with_source` and registry dispatch as any call, so each reaches `PostToolUse` (`core/src/tools/code_mode/mod.rs`, `submit_nested_tool`); `PostToolUse` runs only for a call that succeeded, and a shell command always counts as one, whatever its exit status (`ExecCommandToolOutput::success_for_logging`), so a failing test run is recorded as a test run; a failed `apply_patch` changed nothing.

**Not checked without a real session:** that Codex accepts the trust entries for `PostToolUse` and `Stop` without a matcher (the hashes are computed from Codex's own types, as for refine's `UserPromptSubmit` hook, but these two were never run by Codex); what the hook costs per tool call (a login shell and the `ragon` binary, then three `git` commands) in a container beside a turn; how often each hold fires, what it costs in turns, and whether the agent complies.

## 30. Nights win, and the image cache

*The owner's decisions: nights run on every night machine and win over interactive use there, three at once, handed out by a scheduler; each night machine keeps the instance images of the task lists in use.*

**Nights win.** With `NIGHT_WINS=1` (the scheduler's nights set it) the night script's smokes and rounds run `swe-bench run --ignore-open-sessions`, so an open `ragon` session or a desktop turn neither delays nor refuses them, and before each smoke and each round an index run (`ragon-index-*` scope: `ragon-code` or `ragon-docs` indexing) is waited out for up to `INDEX_WAIT_S` (15 min) and then stopped; it resumes on a later scan. Both are logged. A night started by hand, without the setting, waits and refuses as before.

**The image cache.** `[swe_bench] image_cache_lists` (task-list paths) and `image_cache_floor` (free disk to keep; empty: the disk reserve plus 20 GB) make `run --eval --remove-images` and `eval --remove-images` keep the images of those lists' instances after their chunk is graded, instead of removing them (`SweBenchImageCache.release`). Below the floor, instance images go least recently used first (`SweBenchImageCache.evict`, the last use of each recorded in `~/.cache/dreamference/swe-bench/image-cache.json`, an unrecorded one oldest), the chunk just graded last. Pulls already skip a present image, so a cached image costs the next run nothing. The night script's `IMAGE_CACHE_LISTS` and `IMAGE_CACHE_FLOOR` write those settings for the night and its smokes, and `start` evicts cached images to make room for its disk check instead of refusing. `swe-bench status` shows how much of the cache is present.

**Tests** (`tests/test_swe_bench_image_cache.py`, new; `tests/test_swe_bench_night_script.py`, +4): a cached list's images stay and others go; without a list everything goes as before; below the floor the least recently used go first and a just-released one last; the floor's default; a night's own floor; an unreadable list keeps nothing. A winning night ignores sessions in every round and a night by hand does not; an index run is waited out and stopped only by a winning night; the cache reaches the night's and the smokes' settings.

## 31. Per-arm smoke, loop-guard, compat and remote-server arms, and SWE-bench Multilingual

- **Mandatory per-arm smoke before a night starts.** `SweBenchArmSmoke` (`dreamference/swe_bench/swe_bench_arm_smoke.py`) judges one real task run with an arm's exact settings before the night starts (`scripts/swe_bench_ab.sh start`, default instance `sympy__sympy-13480`), read from the run's files. It asks only whether the arm runs: a wrong or empty patch or the smoke's short time limit are fine; an agent that errors, exits non-zero or never reaches the model is not. A failure like §22's (an arm that cannot start) would have stopped here.
- **Loop-guard arms (`--loop-guard`, `--output-budget TOKENS`, `--sampling default|nonthinking`).** `run --loop-guard` sets `DREAMFERENCE_RAGON_LOOP_GUARD=1` (`ragon-rs/src/loop_guard.rs`, [RAGON_LOOP_GUARD](./DREAMFERENCE_RAGON_LOOP_GUARD.md)); `--output-budget TOKENS` (positive integer, or `0`/`off`) and `--sampling default|nonthinking` (`nonthinking` applies the model card's non-thinking values, filled in by the model gate: `temperature=0.7`, `top_p=0.8`, `top_k=20`, `presence_penalty=1.5`; the server's own defaults otherwise) are enforced by the model gate (`model_gate_guard.py`) through the run's gate hold (`SweBenchGateHold`) and recorded in `manifest.json` (`loop_guard`, `output_budget`, `sampling`) and per instance (`gate_guard`, `loop_guard`).
- **Claude Code compatibility arm (`--compat claude|off`).** `run --compat claude|off` sets `DREAMFERENCE_RAGON_COMPAT` in the container's environment (`swe_bench_runner.py`, [RAGON_CLAUDE_CODE_COMPAT](./DREAMFERENCE_RAGON_CLAUDE_CODE_COMPAT.md)) and records `compat` in `manifest.json` (`auto` when omitted; `--against` compares it).
- **Remote model server for engine A/B.** When `DREAMFERENCE_VLLM_HOST` points at another host, `SweBenchRunner.remote_server()` and `lane_relays()` start a `SweBenchRelay` on the internal Docker network's gateway that forwards containers' model requests to the remote server while keeping the containers on the internal network (`scripts/engine_ab_remote.sh`). Host-local memory and idle admission checks are skipped for the remote server, while local disk and container memory checks remain in force.
- **SWE-bench Multilingual on arm64 (`--dataset multilingual`, `ragon-admin swe-bench build-images`).** `SWE-bench/SWE-bench_Multilingual` (pinned revision `846e647b9f33`, 300 instances across 9 non-Python languages) has no prebuilt arm64 community images, and its upstream task repository (`SWE-bench/swe-bench-multilingual-tasks`, pinned commit `6e08cbcd763c`, cloned to `~/.cache/dreamference/swe-bench/swe-bench-multilingual-tasks`) ships `FROM --platform=linux/amd64` Dockerfiles with x86_64 toolchains. `SweBenchImageBuilder` (`dreamference/swe_bench/swe_bench_image_builder.py`, invoked by `ragon-admin swe-bench build-images`) rewrites each task's Dockerfile for a native `linux/arm64` build (`SweBenchImageBuilder.arm64_dockerfile`) and builds the images, tagged `ragon/swe-bench-built-arm64:<id>`, under `[swe_bench] build_memory` (`24g`) and `build_cpus` (`0-11`). Tests: `tests/test_swe_bench_multilingual.py` (18 tests).

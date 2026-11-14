# Adapter fine-tuning from a user's repository: the plan

Owner Q25 (2026-11-10): a first-party recipe that fine-tunes a LoRA adapter for the served model on a person's own repository ships before 1.0. Training runs on an RTX 5090 or a rented GPU. This page is the plan and the first lane briefs. The coordinator's backlog rows (`plan-adapters-recipe`) mirror the lane section. The serving, signing and install side is already built: [adapters.md](adapters.md) and `specs/DREAMFERENCE_RAGON_ADAPTERS.md` (§7 is the training and publishing contract this plan implements).

Nothing here is measured yet. Every number of hours below is an estimate derived from stated assumptions, and the pilot in lane `adt-trainer-parity` replaces it.

## What already decides the shape

- **Adapters on the served base.** A LoRA adapter is pinned to the base checkpoint's digest, and a base change retires every adapter (§7.4).
- **Signing.** Published adapters are signed by CI with the release key (`trust = dreamference`). Anything else installs only as `third-party`, and the person types its name. This plan changes neither rule (decision D1 below).
- **Drafters.** Every *published* adapter ships a drafter adapter (§7.2). Its publishing bar (§7.3): a clear gain on its language, at most one task lost on the Python control night, at most 5% decode speed lost with its drafter adapter, and the egress audit passing.
- **The trainer box.** Training runs on the RTX 5090 PC (training only, owner Q14), or on a rented GPU. The 5090 is not up yet (Q219), and the drafter retrain waits for it too.
- **Air gap.** The product is air-gapped: anything that sends repository-derived data to another machine is egress. It needs explicit consent and is refused at `/airgapped on`.
- **Non-goals (§1).** Training many adapters, automatic per-task adapter choice, a marketplace.

## The base checkpoint and the LoRA shape the gate serves

**Base.** `qwen3.8-27b-nvfp4-dflash2`, the default model on ragon-serve (`QWEN38_DEFAULT_ENGINE`), at the repository revision pinned in `ModelMatrixRegistry.MATRIX`. It is a hybrid: Gated DeltaNet layers with a full-attention layer every fourth. The attention and GDN projections are FP8, the MLPs NVFP4, and `in_proj_a`/`in_proj_b` BF16.

Training reads exactly that snapshot (§7.1), never a BF16 re-release. The manifest's base digest is computed by `AdapterBaseDigest` over the same snapshot directory the trainer loaded.

**Shape.** The source of truth is `QWEN38_LORA_RECIPE` in `dreamference/hardware/model_matrix_registry.py` and the checks in `AdapterVerifier` (`dreamference/hardware/adapter_verifier.py`). Today they say:

- PEFT LoRA, BF16 weights, unmerged. One rank and one alpha for every module, at most the recipe's `max_lora_rank` (16). No `rank_pattern`, `alpha_pattern`, DoRA, biases or `modules_to_save`.
- Targets are the FP8 projections only: attention `q_proj`, `k_proj`, `v_proj`, `o_proj`, and GDN `in_proj_qkvz` (tensors split as `in_proj_q/k/v/z`) and `out_proj`. The MLP (NVFP4), `in_proj_a`/`in_proj_b`, embeddings, head, vision tower and MTP head are refused by the verifier.
- Exported in the "gdn-split" layout that both ragon-serve (the `lora-adapters` feature) and the SGLang fallback load. The drafter adapter, when there is one, goes in the optional `drafter/` part.

**Trainer defaults.**

| Setting | Default |
| --- | --- |
| Rank / alpha | 16 / 32 |
| Dropout | 0.05 |
| Optimizer | AdamW, lr 1e-4, cosine schedule, 3% warmup |
| Epochs | 2 |
| Sequence length | up to 32K tokens |
| Loss | assistant tokens only, in the served chat template |
| Targets | all of the recipe's target modules |

If the recipe changes, the trainer reads the new one; it never hard-codes the module list.

## The data pipeline from a person's repository

All of it runs on the person's own node (the GB10 with the served base), so no data leaves the machine until the train stage (see "Where training runs").

1. **Mine tasks from history (`data` stage).** Walk the default branch's first-parent history and keep a commit when it:
   - is not a merge and not a bot's commit;
   - has a message of at least a line of prose;
   - touches code and at least one test file;
   - has a diff within bounds (default 400 changed lines, 15 files);
   - is not vendored or generated code.

   Each task records the parent sha, the message as the task text, and the touched tests.

   The newest 10% by commit date (at least 20 and at most 60 tasks) is the **hold-out**, never trained on. The split is by time, so later tasks cannot leak into earlier training.
2. **Make each task gradable.** At the parent, the repository's test command (`--test-cmd`, else detected from the repository's build files; the lane picks the rule) runs the commit's touched tests. A test that fails at the parent and passes at the commit is a FAIL_TO_PASS target. A task with none is dropped from training and listed in the stage report.
3. **Generate on-policy trajectories (`generate` stage).** For each task, run `ragon exec` against the served base in its own worktree and memory-capped scope (Night Shift's machinery: sandboxed, nothing pushed), best-of-k with k = 2 by default. Grade each attempt by its FAIL_TO_PASS tests plus the touched suite not regressing. Keep the best passing trajectory per task: the full rollout of tool calls and outputs, rendered in the served chat template.

   The stage is long. It runs in the person's night window, or whenever they start it; one night at the default budget of about 300 tasks × 2 attempts is the planning figure.
4. **Optional repository text.** A low-weight slice of the repository's own source, a tenth of the tokens at most, for identifiers and style. Off by default and decided after the first eval (D5).
5. **Pack.** Write `train.jsonl`, `holdout.jsonl` and `source.json` under `~/.local/share/dreamference/adapters/train/<name>/`, never in the repository. `source.json` records the commit range, counts and the k used, and becomes the manifest's provenance.

Without tests the recipe cannot grade attempts. A repository with fewer than about 50 candidate tasks gets a clear refusal from the `data` stage, and the generate stage refuses again if fewer than 50 turn out gradable. There is no fallback to diff similarity in v1 (D6).

**The first-party Rust adapter** uses the same pipeline over a list of permissively licensed Rust repositories. Every repository in SWE-bench Multilingual and every SWE-bench Verified instance are excluded, because they are the eval (MULTI_DRAFTER §3.1).

## Where training runs, and the GPU hours on one 5090

**Trainer hosts.**

- **The default trainer** is a Linux box (or WSL2) with one NVIDIA GPU of at least 32 GB, reachable by SSH key: the RTX 5090 PC, or a rented A100/H100/L40S-class instance.
- **The GB10 itself** is out of v1. The trainer holds the base in its served form plus LoRA state and activations, which means stopping the resident model server for the whole run; a training load is a model load under the host-safety rules. Revisit it as a `local` trainer after the pilot measures both machines.
- **Sending the data off the node** (the packed jsonl and nothing else, over scp to the named host) is egress:
  - it is refused at `/airgapped on`;
  - the first upload to a host asks the person to type the host name;
  - the run's report names everything that left the node.

**Forward.** The trainer loads the snapshot's FP8 and NVFP4 weights as they are and dequantizes one layer's blocks to BF16 on the fly inside a "served-weights" linear. The LoRA path adds `B(Ax)·alpha/r` to its output, as the engine does. A dequantized BF16 copy does not fit 32 GB.

Supporting choices:

- gradient checkpointing, with activations offloaded to host RAM when needed;
- chunked cross-entropy over the large vocabulary;
- the GDN layers through the `fla` kernels;
- PEFT on the transformers model class.

The parity probe (lane `adt-trainer-parity`) must show the trainer's teacher-forced logits match ragon-serve's on the same tokens before any training run counts.

**GPU hours: the estimate the pilot replaces.**

- **Compute.** About 6 × 27e9 ≈ 1.6e11 FLOPs per trained token (frozen base, LoRA gradients, one recompute).
- **Throughput.** Assume a dense BF16 tensor peak of about 100 TFLOPS with FP32 accumulation on a 5090, sustained at 25–40%. That gives roughly 150–300 tokens per second.
- **Memory.** 32 GB is the binding constraint. Whether the served snapshot, the LoRA state and the activations of a 32K-token sequence fit is the pilot's first answer; if not, the sequence cap drops or activations go to host RAM.
- **Data.** About 150–250 kept trajectories at about 15–25K tokens each, capped by default at 3M sequence tokens per epoch. Two epochs make 6M tokens.
- **Result: about 6–11 GPU hours on one 5090**, one overnight run.
  - The pilot (200K tokens, under half an hour) measures the real rate.
  - `--budget-hours` then trims the token cap to fit.
  - A rented H100-class GPU is a few times faster, so the same run takes a couple of hours.
- **The drafter adapter** is a separate run and only needed for publishing (D2). Teacher features are dumped from ragon-serve on the GB10 with the adapter on (the drafter-retrain recipe's `RAGON_DUMP_FEATURES` hook). The SpecForge DFlash2 trainer then runs on the same few million tokens. Estimate: 1–2 more GPU hours on the 5090, also replaced by a measurement.

## The eval

Each check compares the base against base plus adapter on the same node, with the same attempts and seeds. The `eval` stage writes one report.

| | Check | Local adapter (a person's repo) | Published adapter (§7.3) |
| --- | --- | --- | --- |
| E1 | Hold-out resolve rate | Reported. "Gain" means at least 2 more hold-out tasks resolved; with no gain, the adapter is not made the default | Its language's set (Rust: the SWE-bench Multilingual Rust subset), a clear gain |
| E2 | Python control | A fixed 20-instance slice when its images are present, else skipped with a note; at most 1 lost | The Python control night; at most 1 task lost |
| E3 | Decode speed | `scripts/decode_speed.py`, single stream, base-drafter row; a warning above 5% loss (D3) | At most 5% lost with its drafter adapter; the base-drafter row recorded |
| E4 | Exactness | Outputs identical with speculation on and off (the engine LoRA bar's agreement check) | Same |
| E5 | Egress | `ragon-admin audit egress` with the adapter selected passes | Same |

E1 and E2 reuse the generate stage's runner and grading, so the train and eval numbers cannot drift apart.

## The CLI surface

A new `train` subcommand under `ragon-admin adapter` (`dreamference/cli/adapter_command.py`). Every stage can be run alone, can be resumed, and writes only under the run's directory:

```text
adapter train <name> --repo PATH [--test-cmd CMD] [--language LANG]
                     [--trainer ssh://HOST] [--budget-hours N] [--rank R] [--epochs N]
                     [--stage data|generate|train|eval|install] [--resume] [--dry-run]
```

- **`--dry-run`** prints the plan and changes nothing. It shows:
  - the candidate task count and hold-out size (gradability is known only after the generate stage's FAIL_TO_PASS check);
  - the token estimate;
  - the GPU-hour estimate from the pilot's measured rate, or from the default above;
  - the trainer host;
  - exactly what would leave the node.
- **Without `--stage`**, the command runs the next unfinished stage and stops at the first one that needs the person: the night for `generate`, consent for an upload.
- **`install`** writes the manifest (schema 1, the base digest, `source.json`) and runs `AdapterVerifier`. It then installs the adapter through the existing third-party path (D1), after which the existing surface takes over: `adapter list`, `ragon --adapter <name>`, `/adapter` in the TUI, and `ragon_adapter` in the config. Paired-node sync (`NodeAdapterSync`) carries only adapters Dreamference signed, so a person's adapter stays on the node that trained it unless D1 changes.
- **The exact run is reproducible:** the run directory keeps `train-config.json` (the recipe, hyperparameters and the snapshot revision).

## Decisions this plan needs, with defaults

| # | Question | Default if no answer |
| --- | --- | --- |
| D1 | Trust for an adapter the person trained: reuse `third-party` (typed name), or add a `local` class signed by a node-local key | Reuse `third-party`; no change to the signing model. `local` is a later lane if the owner wants it |
| D2 | Must a local adapter ship a drafter adapter (§7.2 says every *published* one does) | No: local adapters run with the base drafter, and the drafter stage is opt-in (`--drafter`); required only to publish |
| D3 | Local adapters losing more than 5% decode speed | Warn in the eval report and at install; refuse nothing. Publishing keeps the 5% bar |
| D4 | Rented GPUs | Allowed only as an SSH host the person names, with typed consent on the first upload; refused at `/airgapped on`; Ragon integrates with no cloud provider |
| D5 | Repository text in the mix | Off in v1; turned on only if E1 shows it helps on the first-party run |
| D6 | Repositories without tests | Refused by the `data` stage in v1 (no ungraded data) |

## The lanes

The lanes that need no GPU go first. Briefs are written as dispatch-ready rows in the coordinator's backlog. Code under `/dreamference/hardware/` and `/scripts/adapters/` already belongs to the models team in CODEOWNERS; new test files need their own line.

1. **`adt-data`**: the commit miner and the hold-out split.
   - Files: `dreamference/hardware/adapter_train_tasks.py`, `tests/test_adapter_train_tasks.py`.
   - Accepted when: a fixture repository built in the test yields the right tasks, drops merges, bots and test-less commits, splits by date, and the stage refuses below the minimum.
   - No GPU.
2. **`adt-cli`**: the `train` subcommand.
   - Scope: the parser, `--dry-run`, stage dispatch to the lanes' classes (a missing stage says which lane brings it), and the run directory layout.
   - Docs: [adapters.md](adapters.md) gets the user-facing section; `docs/dev/cli.md` gets the row.
   - Order: after `adt-data`.
3. **`adt-generate`**: trajectories.
   - Scope: per-task worktree and scope through Night Shift's machinery, best-of-k, FAIL_TO_PASS grading, and rendering into the chat template.
   - Tests: a stand-in agent (`FAKE_RAGON`).
   - Order: after `adt-data`.
4. **`adt-trainer-parity`**: the trainer skeleton on the 5090.
   - Files: `scripts/adapters/train/` with its own pinned requirements, run in its own venv on the trainer host.
   - Scope: the served-weights forward, the parity probe against ragon-serve's logits in a day window on a GB10, and the 200K-token pilot.
   - Output: measured tokens per second and peak VRAM, which replace this page's estimate.
   - Held until the 5090 is up and not running the drafter retrain (Q219).
5. **`adt-gen-data`**: the first real data.
   - Scope: the `data` and `generate` stages on one permissively licensed open-source repository with tests (not in either SWE-bench set), on a GB10 in a window the GPU split allows.
   - Output: candidate, gradable and kept counts and the stage's wall time, as a results note on this page.
   - Order: after `adt-cli` and `adt-generate`; held until the coordinator names the box and window.
6. **`adt-train-export`**: the full LoRA run, the gdn-split export, the manifest and `AdapterVerifier`.
   - Acceptance: the adapter serves on ragon-serve with outputs identical with and without speculation.
   - Order: after `adt-trainer-parity`, `adt-gen-data` and `adt-cli`; held with them.
7. **`adt-eval`**: E1 to E5 as one stage and one report.
   - Order: after `adt-generate` (E1 and E2 reuse its runner); runs with the adapter once `adt-train-export` lands.

After these: the first-party Rust adapter is trained with this recipe and taken through the §7.3 bar and CI signing (its own plan once `adt-eval` lands). D1's `local` trust class is a lane only if the owner chooses it.

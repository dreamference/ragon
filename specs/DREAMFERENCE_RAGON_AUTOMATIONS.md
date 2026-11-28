# Ragon Scheduled Automations

**Status:** draft for the owner (2026-11-28), on branch `spec/automations`; nothing is built. The owner asked the same day for "fully compatible scheduled automations for 0.9.11" (release on Fri 2026-12-18). §0.2 lists nine questions, each with a recommended default; the design below follows the defaults.

**In one sentence:** an automation created by Codex runs on Ragon unchanged. Ragon uses the same folder and file (`automations/<id>/automation.toml` and `memory.md` under the Codex home), the same two kinds (a standalone scheduled task and a task inside a chat), the same RFC 5545 schedule, the same `automation_update` tool and the same turn triggers, heartbeat envelope and thread source on the app-server protocol. The run itself happens on the user's own node, which is always on, so the desktop app does not have to be open.

**Names:** post-rename (`ragon`, `ragon-admin`, `~/.ragon`, `ragon web` on port 3100). "Upstream" means Codex: the open-source code at the submodule pin `codex/` (commit `064c6b8c737f`) and the public documentation of the Codex desktop app, whose scheduler is not open source.

**Builds on:**
- [RAGON_NIGHT_SHIFT](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md): worktrees, memory-capped scopes, the runner lock, the morning report and the repeating templates (`dreamference/night_shift/night_shift_recurring.py`);
- [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md) §§ on node jobs: the job cap, the admission checks and `ragon-job-<id>` units (`dreamference/node/node_job.py:161, 398-413`);
- [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md): the hub in `ragon web` and the rule that databases never travel between devices (§0 point 3);
- [RAGON_CHAT](./DREAMFERENCE_RAGON_CHAT.md) and [RAGON_SIGNAL](./DREAMFERENCE_RAGON_SIGNAL.md): the bridges, which poll the node for reports;
- [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md) and [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md);
- [RAGON_DESKTOP](./DREAMFERENCE_RAGON_DESKTOP.md) §6.8 and [RAGON_DESKTOP_PARITY](./DREAMFERENCE_RAGON_DESKTOP_PARITY.md) §2.12: the Scheduled view, which today shows Night Shift;
- [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) and [MAC_NODE](./DREAMFERENCE_MAC_NODE.md): Task Scheduler, launchd and the node services (0.9.10);
- the remote-sessions draft (`specs/DREAMFERENCE_RAGON_REMOTE_SESSIONS.md` on branch `spec/remote-sessions`): placement through the primary (§7.2).

**Supersedes on approval:** DESKTOP §6.8 ("no new scheduler") and DESKTOP_PARITY §2.12 rows 18 and "RRULE and thread tasks". Both documents are updated to point here when the owner approves; this branch does not edit them.

---

## 0. Decisions

### 0.1 Decisions this spec makes (the owner may reverse any)

1. **The automation's file is upstream's file, byte for byte.** `~/.ragon/automations/<id>/automation.toml` and `memory.md` have upstream's layout and keys. Ragon reads and writes them with a format-preserving TOML editor: every key Ragon does not know, every comment and the key order survive an edit. Ragon never rewrites the file from a narrower schema (§3.1).
2. **Ragon-only settings go in a sidecar, never as new keys in the upstream file.** They live in `~/.ragon/ragon-automations/<id>/ragon.toml`: the owning machine, catch-up, notification, limits and the Night Shift link. Run records go in the same folder. An upstream file copied in without a sidecar runs with the defaults.
3. **Runs go through the app-server protocol, with upstream's markers.** A run starts a thread with `threadSource` `automation` and a turn with `turnTrigger` `automation_cron_scheduled` or `automation_heartbeat_scheduled`. A heartbeat's input is upstream's `<heartbeat>` envelope, and `additionalContext` carries `automation_info`. Upstream's own guards then apply unchanged: the guardian treats a replay as "not a new human instruction", heartbeat versions coalesce, and a full-access MCP elicitation is declined. Ragon adds no protocol method; upstream has none either.
4. **The scheduler is headless and lives on the machine that holds the project.** On a node it is a module of `ragon web`. On a client it is a small user service. Each platform keeps it alive with its own service manager (§4.2). The desktop app may be closed.
5. **Exactly one machine fires each automation.** That machine, the owning machine, is recorded in the sidecar. The primary keeps the list for every paired device, but there is no failover in v1 (§4.3).
6. **Unattended runs are never less safe than the user's interactive default, and never more open than `workspace-write`, unless the user opts in for that one automation** (Q1). Upstream runs unattended with the user's default sandbox, even when that default is full access. Ragon caps the sandbox: owner rule, not unsafe by default.
7. **Night Shift stays, and its queue is not touched.** Its repeating templates become automations that queue into the night window. Night Shift's results and the automations' runs share one inbox, the Scheduled view (Q2, §4.8).
8. **No telemetry.** Scheduling, runs and the inbox stay on the user's machines. Notifications use only the bridges the user has already set up.

### 0.2 Questions for the owner (each with the recommended default)

| # | Question | Recommended default |
|---|---|---|
| Q1 | An unattended run whose user default is full access: run with full access as upstream does, or cap it? | **Cap at `workspace-write`, network as configured, `approval_policy = "never"`.** Full access only when the automation's sidecar says `allow_full_access = true`, set by the user in the edit form behind a warning; never at `/airgapped on`. |
| Q2 | Night Shift and automations: merge, keep side by side, or replace? | **Side by side, one inbox.** The queue (`night/tasks/`) stays as it is. The repeating templates (`night/recurring/`) are migrated to automations with `runner = "night"`, which queue into the night window as before (§4.8). |
| Q3 | Automations that already exist in `~/.codex/automations`: import them, and how? | **Offer, never automatic.** `ragon automations import` and a banner in the Scheduled view copy them as PAUSED, so the upstream app and Ragon never both run one; the originals are not touched. |
| Q4 | A run missed because the machine was off or asleep | **Catch up once.** A missed standalone run fires once on wake-up when it is less than 12 h late (or less than one period, if that is shorter); otherwise it is recorded as missed. Heartbeats are never caught up. |
| Q5 | Heartbeats every minute on the local model | **Floor of 5 minutes on Ragon.** A 1-minute rule from upstream is kept in the file and runs every 5 minutes; the Scheduled view says so. The floor is a setting. |
| Q6 | Which machine runs an automation when several are paired | **The machine that holds the project, fixed when the automation is created**, shown in the Scheduled view and changeable there. No automatic failover in v1. |
| Q7 | Notifications | **Runs with findings only**, to the bridges in `[chat] night_report_to` (today Signal and Matrix). Can be changed per automation (`notify = "findings" \| "always" \| "never"`). Runs without findings are archived silently. |
| Q8 | What ships in 0.9.11 on 12-18 | **Standalone automations on Linux machines**, editable from the terminal, Work and the phone, with the Night Shift migration and import. Heartbeats inside a chat are a stretch. Mac and Windows scheduling, heartbeats on remote-session threads and the TUI tool follow in the release after (§8). |
| Q9 | Archiving the chat a heartbeat belongs to | **Pause the heartbeat, keep it.** Upstream deletes it (openai/codex#24294, reported as a bug); unarchiving the chat offers to resume it. |

### 0.3 Owner steps

1. Answer Q1–Q9 (or accept the defaults).
2. Optional, if the owner uses the upstream desktop app: create one standalone automation in worktree mode with a custom RRULE, and one automation inside a chat. Send the two `automation.toml` files with the project paths replaced by `/x`. They settle the rows of §1 marked *unverified* (the worktree key, `destination`, the id format, DTSTART). Without them, Phase 0 reads the keys from public issue reports and keeps unknown keys intact, so nothing breaks if they differ.
3. Nothing else: no credentials, no store or signing steps.

---

## 1. Compatibility with upstream

**Sources.** Three tiers, marked in the Source column:
- **[C]** open source at the pin, path relative to `codex/codex-rs/` unless it starts with `codex/`;
- **[D]** the public documentation:
  - "Scheduled tasks", <https://developers.openai.com/codex/app/automations> (it redirects to the current docs site), cited as [D-auto §section];
  - the worktrees page (<https://learn.chatgpt.com/docs/environments/git-worktrees>) [D-wt];
  - the sandboxing page (<https://learn.chatgpt.com/docs/sandboxing>) [D-sb];
  - the app commands page (<https://learn.chatgpt.com/docs/app/commands>) [D-cmd];
  - the what's-new page (<https://learn.chatgpt.com/docs/whats-new>) [D-new];
- **[I]** public issue reports on openai/codex, which show files the closed app wrote. They are **unverified**: the issue number is the source, and the row says so.

**Verdicts.**
- **Identical:** same name, same bytes, same behaviour; in most of these rows Ragon inherits the code because Ragon is built from the same Codex.
- **Interface identical:** same interface, Ragon's own implementation; Ragon writes it from the specification, never copying code.
- **Different:** the reason is given in the row.

### 1.1 Concepts

| # | Upstream | Source | Ragon | Verdict |
|---|---|---|---|---|
| 1 | Two kinds. A **standalone** task starts a new chat for each run and reports in Scheduled. A task **inside a chat** returns to that chat (the "heartbeat") | [D-auto §Manage scheduled tasks, §Schedule a task inside a chat]; [C] `core/src/session/turn_input_tests.rs:912, 1018-1019` | The same two kinds, the same names in the file (`kind = "cron"` and `"heartbeat"`) | Identical |
| 2 | Feature gate `in_app_local_automation`: Stable, on by default. It is a requirements-only gate: an enterprise requirement `[features] in_app_local_automation = false` turns it off | [C] `features/src/lib.rs:265-268, 1480-1483`; `core/src/config/config_tests.rs:11721-11738`; `core/config.schema.json:891` | Ragon's scheduler and the `automation_update` tool honour the same key. If requirements turn it off, nothing is scheduled and the tool is not offered | Identical |
| 3 | Schedule as an RFC 5545 recurrence rule, e.g. `RRULE:FREQ=MONTHLY;BYMONTHDAY=1;BYHOUR=9;BYMINUTE=0`; presets and custom controls in the UI | [D-auto §Manage scheduled tasks] | The same RRULE text in the same key. Ragon's own parser (Rust, `ragon-rs/automations`) is tested against shared vectors (§6.1) | Interface identical |
| 4 | Minute intervals for tasks inside a chat; daily or weekly times for check-ins | [D-auto §Schedule a task inside a chat] | Accepted as written; Ragon runs heartbeats no more often than every 5 minutes (Q5) | **Different:** one local GPU serves every user turn, so a 1-minute loop would hold it continuously. The file keeps the user's value, so the automation is unchanged if it moves back |
| 5 | Run in the local project or on a dedicated background worktree (Git); non-Git projects run in the directory | [D-auto §Manage scheduled tasks]; [D-wt] | The same two modes. Worktrees use Ragon's worktree mode (`$CODEX_HOME/session-worktrees/<id>`, AGENTS.md) on branch `automation/<id>/<run>` | Interface identical |
| 6 | One automation can run on several projects | [D-auto §Manage scheduled tasks]; `cwds = [...]` [I] #19960 | Same: one run per project per firing, run one after another | Identical |
| 7 | Model and reasoning effort: default or explicit; a retired model must be replaced | [D-auto] intro | Same keys. A model the node does not serve runs on the node's default model, and the run record and inbox say so | **Different** (only for model names): a node serves its own local models. The file keeps the original value |
| 8 | Skills in prompts (`$skill-name`); plugins | [D-auto §Manage scheduled tasks, §Test scheduled tasks] | Skills: the same (Ragon runs Codex's skills). Plugins and apps: whatever Ragon's apps mechanism provides on that machine | Identical for skills; plugins as far as Ragon supports them |
| 9 | Create and update automations from a chat; skills can create them too | [D-auto §Ask … to create or update scheduled tasks] | The same, through the `automation_update` tool (rows 31-33) | Identical |
| 10 | The Scheduled view is the inbox: runs with findings appear there with an unread indicator | [D-auto §Manage scheduled tasks] | Ragon's Scheduled view does the same, for automations and Night Shift together (§4.6) | Interface identical |
| 11 | Runs without findings do not appear in the inbox | **unverified**: the docs say only that runs *with* findings appear [D-auto §Manage scheduled tasks]; no source found for what happens to the rest | Runs without findings are archived: kept in the run history, not in the inbox, no notification | Interface identical (assumed) |
| 12 | Archive runs; pinned runs keep their worktrees | [D-auto §Worktree cleanup for scheduled tasks] | Same | Identical |
| 13 | Managed worktrees: the 15 most recent are kept (a setting); a pinned or running one is never deleted; a snapshot is saved before deletion and can be restored | [D-wt §Worktree cleanup] | Same default of 15 and the same rules. The snapshot is a ref `refs/ragon/snapshots/<run>` in the project's repository | Interface identical |
| 14 | Test the prompt by hand first; run now | [D-auto §Test scheduled tasks]; [I] #26954 (Run Now) | "Run now" in the Scheduled view, `ragon automations run <id>` and `/auto run` on the phone | Interface identical |
| 15 | Unattended runs use the default sandbox settings and `approval_policy = "never"` where the organisation allows it; otherwise they fall back to the selected permission mode | [D-auto §Permissions and security model] | `approval_policy = "never"` as upstream, but the sandbox is capped at `workspace-write` (§4.4) | **Different** (Q1): the owner's rule that no feature ships unsafe by default. An automation that needs more says so in its own sidecar |
| 16 | Admins restrict this with `requirements.toml` (forbid `never`, constrain sandbox modes) | [D-auto §Permissions and security model] | Same file, same effect: Ragon reads requirements through the Codex config it is built from | Identical |
| 17 | "Lower-risk local automation preset": `workspace-write` with `on-request`, optionally with `approvals_reviewer = "auto_review"` | [D-sb] | When requirements forbid `never`, Ragon uses this preset. An approval request with no one to answer it is declined after 10 minutes and shows in the inbox as "needs approval" (§4.4) | Interface identical |
| 18 | The computer must be on, the desktop app running, and the project on disk | [D-auto] intro, §Manage scheduled tasks | The machine must be on and the project on disk. The desktop app may be closed (§4.2) | **Different, in the user's favour:** the node runs around the clock, and requiring an open window would lose runs for no reason |
| 19 | Missed runs while the computer is off | **unverified**: not documented | Catch up once within a grace period, otherwise skip and record (Q4, §4.5) | Ragon's own rule, stated |
| 20 | Team tasks and event triggers (mail, chat and code-hosting events) | [D-new]; [D-auto] intro: web and mobile only, on eligible plans | Not in v1 (§7) | **Different:** these run on the vendor's servers. Ragon has no cloud, and an event source needs a connector that polls (later) |
| 21 | Deep link `codex://automations` opens Scheduled with the create flow | [D-cmd] | `ragon://automations` with the same meaning (Ragon's scheme, DESKTOP_PARITY §2.21) | **Different scheme:** `codex://` belongs to the other app on the same machine; taking it over would steal its links |

### 1.2 Files

| # | Upstream | Source | Ragon | Verdict |
|---|---|---|---|---|
| 22 | `~/.codex/automations/<id>/automation.toml` | [I] #19960, #23798, #36256, #30439 (unverified tier) | `~/.ragon/automations/<id>/automation.toml`: the same path under Ragon's Codex home (`CODEX_HOME=~/.ragon`, `docs/dev/launcher.md:47`) | Identical relative to the Codex home |
| 23 | `memory.md` beside it: the automation's notes between runs | [I] #14742 | Same file. The run gets the folder as its only extra writable root, and its path in `automation_info` | Identical |
| 24 | Keys `version = 1`, `id`, `kind`, `name`, `prompt`, `status` (`"ACTIVE"`/`"PAUSED"`), `rrule`, `cwds`, `model`, `reasoning_effort`, `created_at`, `updated_at` (epoch ms) | [I] #19960, #23798, #30439, #36256, #39899 | Read and written with the same names, types and values | Identical (keys unverified) |
| 25 | `rrule` is seen both with and without the `RRULE:` prefix | [I] #21342, #36225 | Both are read; Ragon writes what the file had, or the prefixed form for new files | Identical |
| 26 | Heartbeat keys `target_thread_id` and `destination = "thread"` | [I] #23798, #24294 | Same | Identical (unverified) |
| 27 | The key choosing local or worktree mode | **unverified**: not found in any public source | Phase 0 settles it from owner step 2 or a newer issue report. Until then Ragon reads the mode from the sidecar and leaves any upstream key untouched | Identical once known |
| 28 | Unknown and future keys | no upstream source (how the upstream app treats unknown keys is unverified) | Kept byte for byte (§3.1); the format-preserving round trip is tested | Identical |
| 29 | Run state in an SQLite file `codex-dev.db` with the tables `automations`, `automation_runs`, `inbox_items` | [I] #19960, #23895 | Not written. Ragon keeps run records as JSON files in the sidecar folder (§3.2) | **Different:** the database is the closed app's private state, with an unpublished schema. Writing it would risk corrupting the other app's data. Ragon also never moves databases between machines (SYNC §0 point 3). No automation depends on it: the definition is the TOML file |
| 30 | First-run copy from `~/.codex` to `~/.ragon` (Ragon's own, an allow-list) | `docs/dev/launcher.md:47` | `automations/` is **not** added to the automatic copy; `ragon automations import` copies them as PAUSED (Q3) | Ragon's own; it prevents double runs |

### 1.3 Tool and protocol

| # | Upstream | Source | Ragon | Verdict |
|---|---|---|---|---|
| 31 | Dynamic tool `automation_update`, namespace `codex_app`, description "Create, update, view, or delete recurring automations.", loaded lazily (`defer_loading`), schema with a required `mode` | [C] `core/src/tools/handlers/tool_search.rs:372-389`; `core/tests/suite/search_tool.rs:1073-1124` | The same name, namespace, description and lazy loading, offered by every Ragon client that runs threads (desktop, web, phone bridges; TUI later, §7) | Interface identical |
| 32 | Modes `view`, `create`, `update`, `delete`; the other arguments carry the file's keys | [I] #23798 (unverified beyond `mode`) | Same modes. The arguments are the keys of row 24, unknown arguments are kept, and an update never drops a key it was not given (fixes the drop reported in #23798) | Interface identical |
| 33 | The client injects the tool with `thread/start.dynamicTools`; calls come back as `item/tool/call` | [C] `app-server-protocol/src/protocol/v2/thread.rs:145-151`; `app-server-protocol/src/protocol/common.rs:1796` | Same. `policy.json` gains the tool so the desktop app may send it | Identical |
| 34 | No app-server method for automations | [C] `app-server-protocol/src/protocol/common.rs` (the method list: none named for automations) | No new method. Ragon's own management goes through `ragon web`'s HTTP API (`/api/automations`, §4.1), which is no part of the Codex protocol, just as `/api/night/*` is not | Identical (no method) |
| 35 | `turn/start.turnTrigger` with `automation_cron_scheduled` and `automation_heartbeat_scheduled`; the value is stored in the turn's metadata | [C] `app-server-protocol/src/protocol/v2/turn.rs:176-179`; `core/tests/suite/subagent_notifications.rs:1153, 1173`; `core/src/session/turn_input.rs:172-175` | Ragon's runner sends exactly these strings | Identical |
| 36 | A heartbeat turn's input is the envelope `<heartbeat>` / `<automation_id>` / `<current_time_iso>` / `<instructions>`. The id is `[A-Za-z0-9_-]`, at most 128 characters; the envelope has a size limit (3,600 with the per-line allowance); it is stored with content kind `user.heartbeat` | [C] `history/src/heartbeat.rs:10, 24-28, 76-110`; `history/src/heartbeat_tests.rs:7-26`; `core/src/session/mod.rs:4925-4935` | Ragon builds the same bytes. A heartbeat prompt that would overflow the envelope is refused when the automation is saved, not when it runs | Identical |
| 37 | Heartbeat versions coalesce in retained context | [C] `history/src/reconciled_retained_context.rs:5, 116-137` | Inherited | Identical |
| 38 | The guardian reads a replay as "Scheduled automation … ran at … not a new human instruction"; human overrides are preserved | [C] `guardian-context/src/transcript.rs:315-336`; `core/tests/suite/scenarios_guardian_heartbeat.rs:1`; `core/tests/suite/guardian_heartbeat_authorization.rs:1` | Inherited | Identical |
| 39 | Thread source `ThreadSource::Feature("automation")`, sent in `thread/start.threadSource` | [C] `app-server-protocol/src/protocol/v2/thread_data.rs:106-111`; `app-server-protocol/src/protocol/v2/thread.rs:124`; `protocol/src/protocol.rs:4604-4608`; the literal `"automation"` in `app-server/tests/suite/v2/mcp_tool.rs:537-542` | Ragon's runner sends it on every standalone run | Identical |
| 40 | `additionalContext` with the key `automation_info` (kind Application), shown as `<automation_info>…</automation_info>` | [C] `app-server-protocol/src/protocol/v2/turn.rs:193-195`; `core/tests/suite/additional_context.rs:55, 86, 100-104`; `core/tests/suite/context_annotations.rs:134, 215` | Same key. The content is Ragon's own: automation id, name, run id, scheduled time and the path of `memory.md` | Interface identical (upstream's content unverified) |
| 41 | An MCP tool asking for full access is declined in an automation thread | [C] `app-server/tests/suite/v2/mcp_tool.rs:537` | Inherited; that test is part of Ragon's codex-tests run | Identical |
| 42 | Remote control lacks `automation_update` | [I] #26489 | Ragon's phone and web clients offer it | **Different, in the user's favour** |
| 43 | `CODEX_HOME` is not set in an automation's shell, so `memory.md` is lost | [I] #14742 | The runner exports `CODEX_HOME` (and `DREAMFERENCE_VLLM_HOST`, per AGENTS.md) | **Different:** reported as a bug upstream |
| 44 | Pausing is saved as ACTIVE; a stale prompt is used after an edit; a custom model id is overwritten | [I] #36256, #39899, #24007 | Each becomes a test (§6.3) | Ragon avoids the reported bugs |
| 45 | A weekday written `MON` instead of `MO` crashes the app | [I] #36225 | Refused with a message naming the rule; the automation is marked invalid and never runs | **Different:** an error, not a crash |

### 1.4 Commands

| # | Upstream | Source | Ragon | Verdict |
|---|---|---|---|---|
| 46 | No CLI or TUI command for automations: management is in the desktop app and the tool | [C] no `automation` command in `cli/src` or the TUI's slash commands (searched at the pin) | `ragon automations list\|show\|add\|edit\|pause\|resume\|run\|rm\|import\|doctor` (§4.1). It is additive and reaches only Ragon's own store | Additive; nothing upstream to match |
| 47 | Issue label "automations - scheduled automation tasks or heartbeats" | [C] `codex/.github/workflows/issue-labeler.yml:78` | The same word in Ragon's UI and docs ("Scheduled" in the sidebar, "automation" for the object) | Identical naming |

---

## 2. Scope

**In v1 (0.9.11, Q8 default):**
- standalone automations on Linux machines: on the node and on Linux clients;
- the upstream file format with round trip, and import from `~/.codex`;
- the RRULE engine with shared vectors;
- the scheduler in `ragon web`, with catch-up, admission and limits;
- runs over the app-server protocol, in a worktree or locally, sandboxed;
- the Scheduled inbox: findings, archive, apply, discard, pin;
- editing from the terminal, from Work (form and tool) and from the phone (commands and tool);
- notifications through the bridges;
- migration of the Night Shift templates;
- airgapped behaviour;
- docs.

**Stretch for 0.9.11:** heartbeats inside a chat for threads on the same machine.

**After v1:** §7.

## 3. Storage

### 3.1 The upstream file

- **Location.** `~/.ragon/automations/<id>/automation.toml`, plus `memory.md` created empty on first run.
- **Ids.** Ragon creates ids in the envelope's character set (`[A-Za-z0-9_-]`, at most 128 characters): a slug of the name plus 6 hex characters. It accepts any id in that set.
- **Format-preserving.** It is edited with `toml_edit` (or an equivalent): Ragon changes only the keys it sets, and keeps comments, order and unknown keys. `updated_at` is set on every change.
- **Atomic.** Writes go to a temporary file followed by a rename. A per-machine lock (`~/.ragon/ragon-automations/.lock`, `flock`, outside the upstream folder) gives one writer.
- **Validated on save.** The RRULE parses; the `status`, `kind` and `cwds` values are known; a heartbeat names its thread and fits the envelope. An invalid file found on disk is shown in the Scheduled view as invalid and never runs.

### 3.2 Ragon's sidecar

`~/.ragon/ragon-automations/<id>/`:
- `ragon.toml` (all keys optional):

  ```toml
  node = "<node id>"          # owning machine (Q6); default: where it was created
  run_in = "worktree"         # "worktree" | "local", until row 27 is settled
  catch_up = "once"           # "once" | "skip"           (Q4)
  catch_up_hours = 12
  notify = "findings"         # "findings" | "always" | "never" (Q7)
  max_minutes = 30            # 1 … 90
  allow_full_access = false   # Q1
  network = "default"         # "default" | "off"
  runner = "app-server"       # "app-server" | "night" (Night Shift templates, §4.8)
  ```

- `runs/<run id>.json`: scheduled time, start, end, state, machine, thread id, worktree, branch, diff stat, final message, `findings`, `read`, `archived`, `pinned`, skip reason.
- `scheduler.json` (one per machine, at the folder's top): last tick and the next due time of each automation.

A copied upstream folder without a sidecar runs with the defaults on the machine that finds it. Removing an automation moves both folders to `~/.ragon/automations-trash/` for 30 days.

### 3.3 Editing from each place

| Place | How | Writes through |
|---|---|---|
| Terminal | `ragon automations add --name … --rrule … --cwd … [--worktree] [--model …] [--effort …] --prompt-file …`, `edit` (opens `$EDITOR` on the TOML, validated on close), `pause`, `resume`, `run`, `rm`, `list`, `show`, `import`, `doctor` | Local store; for an automation owned by another machine, the primary's `/api/automations` |
| Work (desktop) | The Scheduled view gets a create and edit form: name, prompt, a schedule (presets plus a raw RRULE field showing the next five times), projects, local or worktree, model, effort, status and the Ragon settings. In chats, the `automation_update` tool; `ragon://automations` opens the form | The machine's `ragon web` API |
| Phone | `/auto list\|show\|pause\|resume\|run <id>` in the bridges. In phone threads, the `automation_update` tool; any create, update or delete from the phone asks YES/NO first, in the bridge's existing question form (SIGNAL §19) | The node's `ragon web` API |

## 4. Design

### 4.1 Components

- `ragon-rs/automations` (new crate): the file model with round trip, the RRULE engine, the due-time calculation and the run records. Rust, with no Codex patch: it reads and writes files in Codex's own format, as the sync agent does (SYNC §3).
- The scheduler: a module of `ragon web` on a node (`ragon-rs/web/src/automations.rs`), and `ragon automations serve` on a client.
- The HTTP API in `ragon web`, behind the same device credentials as `/api/night/*`:
  - `GET /api/automations`;
  - `POST /api/automations`;
  - `PATCH /api/automations/<id>`;
  - `DELETE /api/automations/<id>`;
  - `POST /api/automations/<id>/run`;
  - `GET /api/automations/runs?after=`;
  - `POST /api/automations/runs/<run>/{read,archive,pin,apply,discard}`;
  - `GET /api/automations/reports?after=` (for the bridges, §4.7).
- The runner: `ragon automations run-one <run id>`, the process that talks to `ragon app-server` (§4.4).
- Clients:
  - the desktop app: the Scheduled view and the tool;
  - the bridges: `/auto` and the tool;
  - the CLI.

### 4.2 The scheduler on each platform

- **The loop.** The scheduler is one loop. It sleeps until the earliest next due time or until an edit wakes it (the API, or a file watch on `automations/`). At each wake it:
  1. computes the due runs since its last tick;
  2. applies admission and catch-up (§4.5);
  3. starts the runs;
  4. writes `scheduler.json`.
- **Why not one OS timer per automation.** A single loop is precise to the second for heartbeats, needs no OS timer per automation and behaves the same on all three platforms. The platform's service manager only keeps the loop alive and starts it at boot or login.

| Platform | Node | Client (no `ragon web`) | Status |
|---|---|---|---|
| Linux | Inside `ragon web`, already a systemd user service with linger | `ragon-automations.service`, a systemd user unit (`Restart=on-failure`), enabled by `ragon automations enable` | v1 |
| Mac | Inside `ragon web`, kept alive by its LaunchAgent (`ai.dreamference.ragon-web`, `ragon-rs/web/src/cli.rs:229`), or by the node's LaunchDaemon `ai.dreamference.ragon-node` (MAC_NODE:110) once 0.9.10 ships it | A LaunchAgent `ai.dreamference.ragon-automations`, `RunAtLoad` + `KeepAlive` | After 0.9.10 Mac nodes |
| Windows | Inside the `RagonNode` service (WINDOWS_NODE, the node service) | A Task Scheduler task "Ragon automations" with an at-logon trigger, `/RL LIMITED`, restart on failure | After 0.9.10 Windows nodes |

- **WINDOWS_NODE:111.** A scheduled task did not run when created over SSH. The task is therefore registered only by the interactive installer or by the desktop app, never over SSH. `ragon automations doctor` reports the age of the loop's last tick, so a task that never started is visible.
- **The process keeps running across changes.** The loop survives an edit. An update restarts `ragon web`, and the restart is a short "machine off" case (§4.5).

### 4.3 Which machine runs it

- **Ownership.** Every automation has one owning machine (sidecar `node`). Only that machine's scheduler fires it. Another machine that finds the same id in its own store (a copied folder) treats it as foreign: it shows it, marked "owned by <node name>", and never runs it.
- **Default owner (Q6).**
  - The machine whose disk holds the project.
  - Created from Work on a client, for a project on the node: the node.
  - For a project on the client: the client (it must be on, as upstream requires).
  - Created by `automation_update` in a thread: the machine where the thread runs.
- **The primary's list.** The primary keeps the list of every machine's automations (the hub pattern, SYNC §3: an index, not a copy), so every device shows them all and forwards edits to the owner.
- **No failover in v1.** If the owner is off, its runs are missed under the rules of §4.5; another node does not take over (the project lives on the owner's disk).
- **Heartbeats** run on the machine that hosts the target thread. With remote sessions (0.9.10) that is the node the session is placed on (REMOTE_SESSIONS §7.2). Until 0.9.10, a heartbeat can target only a thread on the same machine.

### 4.4 A run

1. **Admission** (§4.5). Then a run record is written in state `starting`.
2. **Working copy.**
   - `local`: the project directory.
   - `worktree`: `git worktree add $CODEX_HOME/session-worktrees/<run> -b automation/<id>/<run> HEAD`, Ragon's worktree mode.
   - A non-Git project always runs locally (upstream's rule).
   - A project missing from disk: the run is skipped with the reason "project not found".
3. **Process limits.**
   - Linux: the runner starts in `systemd-run --user --scope -p MemoryMax=8G -p MemorySwapMax=0 -p CPUQuota=400% -p TasksMax=4096 -p RuntimeMaxSec=<max_minutes>`, Night Shift's values (`night_shift_task_run.py:919-923`) plus node jobs' task limit (`node_job.py:65, 405-413`).
   - Mac and Windows: the node specs' job limits once built.
4. **The protocol** (stdio to `ragon app-server`):
   - `initialize`.
   - Standalone: `thread/start` with `cwd`, `model`, `effort`, `approvalPolicy`, `sandbox`, `threadSource` (automation) and `dynamicTools` (`automation_update`, so a run may update its own automation, as upstream's skills do). Then `turn/start` with the prompt, `turnTrigger: "automation_cron_scheduled"` and `additionalContext.automation_info`.
   - Heartbeat: `thread/resume` of `target_thread_id`, then `turn/start` with one text input holding the envelope and `turnTrigger: "automation_heartbeat_scheduled"`. Core marks the input `user.heartbeat` itself (`core/src/session/mod.rs:4925-4935`).
   - The runner waits for `turn/completed`, then reads the final message.
5. **Sandbox and approvals** (Q1). The runner computes the policy for each run. Its levels:

   | Case | Sandbox | Approvals |
   |---|---|---|
   | The user's default is `read-only` | read-only | `never` |
   | The user's default is `workspace-write` or full access | `workspace-write`; network as the user's `sandbox_workspace_write.network_access`, or off when the sidecar says `network = "off"` | `never` |
   | Sidecar `allow_full_access = true` (set in the form behind a warning; refused at `/airgapped on`, AIRGAPPED:139) | full access | `never` |
   | `requirements.toml` forbids `never` | as above | `on-request` (upstream's fallback, D-sb's preset), with `approvals_reviewer` as configured. A request nobody answers is declined after 10 minutes and the run is marked "needs approval" in the inbox |
   | `requirements.toml` constrains sandbox modes | the strictest allowed | as above |

   - With `never`, an escalation is refused and the command reruns sandboxed, exactly as in phone threads (CHAT:41).
   - The extra writable roots are the automation's own folder (`memory.md`) and nothing else.
   - Rules (the `rules/` allow-list) apply as upstream says: they are the way to let a named command out.
6. **Environment.** `CODEX_HOME` is exported (row 43), as are `DREAMFERENCE_VLLM_HOST` (AGENTS.md: unattended callers set it) and `RAGON_AUTOMATION_ID`.
7. **The record.** The run record gets the final message, the thread id, the diff stat of the worktree, the state (`done`, `failed`, `timed_out`, `skipped`) and `findings`. Nothing is committed, pushed or merged (Night Shift's rule, NIGHT_SHIFT:229). Changes stay in the worktree until the user applies them.
8. **Findings.** A run has findings if its worktree or project has changes, or if its final message is not empty after trimming and does not end with the line `NO_FINDINGS`. `automation_info` tells the model this convention ("if there is nothing to report, end with NO_FINDINGS"). Upstream's rule is unverified (row 11); this one is stated and tested.

### 4.5 Off, busy, benchmark, night round, model down

Every firing passes admission on the owning machine at the moment it would start. There is one rule per case, stated:

| Case | Standalone automation | Heartbeat |
|---|---|---|
| Machine off or asleep at the due time | On wake-up: **once**, if less than `catch_up_hours` late (12 h by default, or one period if that is shorter); several missed times give a single run; older ones are recorded as `missed` | Skipped and recorded; the next one fires normally |
| `ragon web` restarting (update) | Same as off (usually seconds late, so caught up) | Same |
| Job cap reached (NODE: admission; the default is one automation run at a time, plus Night Shift's own slot) | **Waits** up to 30 minutes in a FIFO queue, then is skipped with "node busy" | Skipped if the last one is still running (never two at once on one thread) |
| Benchmark or GPU window: the runner lock held by a benchmark, a `ragon-swe-*` unit, or `~/.ragon-gpu.lock` held (the checks of `night_shift_autofill.py:115-147`; the lock file as in `node_role_provisioner.py:72`) | Skipped with "benchmark running", then caught up after the benchmark under the off rule | Skipped |
| A Night Shift round running | Runs if the job cap has a slot (Night Shift holds its own); its 8 GiB scope counts toward the node's memory reserve | Same |
| Model server down or not answering its health check | Retried every 5 minutes for up to 30 minutes, then skipped with "model down" | Skipped |
| Project missing, invalid file, model not served | Skipped with the reason (model not served: runs on the default model, row 7) | Same |
| Automation PAUSED | Nothing is scheduled; resuming does not catch up | Same |
| Clock jumps back, or the DST fall-back hour | A scheduled time already in the run history never fires twice (key: automation id + scheduled UTC time) | Same |

Every skip is a run record with its reason. Skips are archived, not in the inbox, except when the same automation skips three times in a row: then one inbox item says so.

### 4.6 The inbox

- **The Scheduled view** lists automations (active, paused, invalid, foreign) and runs.
  - The inbox is the runs with findings and not archived; an unread dot shows on the sidebar entry.
  - Night Shift's results appear in the same inbox, marked "Night Shift" (§4.8).
- **Opening a run** opens its thread in Work, which fixes DESKTOP_PARITY row 18, and shows the final message, the diff and the branch.
- **Actions:**
  - **Apply:** writes the worktree's changes into the project as uncommitted changes, as `ragon night apply` does.
  - **Discard:** removes the worktree and the branch, after the snapshot of row 13.
  - **Archive** and **pin**.
  - **Mark read** and **archive all read**.
- **Runs without findings** are archived at once (row 11). The "All runs" tab shows them.
- **Retention.** Run records are kept 90 days, pinned ones for ever. Worktrees follow row 13 (15 kept).

### 4.7 Notifications

- **How they travel.** Notifications use the existing pull delivery: the bridges already poll `GET /api/night/reports?after=` every five minutes (SIGNAL:911; `ragon-rs/web/src/server.rs:442`). They will also poll `GET /api/automations/reports?after=`.
- **What is sent.** A report is the automation's name, the project, one line of result and a link (`ragon://automations/<run>`), cut at 3,500 characters like the morning report.
- **Who gets them.**
  - `[chat] automation_report_to` defaults to the value of `night_report_to` (CHAT:267: Signal and Matrix; Telegram only when named).
  - Per automation, `notify` (Q7) applies.
  - The desktop app also shows an OS notification when it is open.
- **Airgapped.** At `/airgapped on` nothing is sent, as for Night Shift.

### 4.8 Night Shift

- **Recommendation (Q2):** Night Shift stays a separate feature beside automations, with one shared Scheduled inbox. Only its repeating templates become automations.
- **What stays.**
  - The one-off queue: `/night add`, `night/tasks/<id>.json`, best-of-N, the window, the morning report and the runner lock (NIGHT_SHIFT §§3-6).
  - It is a queue, not a schedule. Folding it into automations would rewrite a working feature and its file format for no gain to the user.
- **What moves.** The repeating templates (`night/recurring/r<12 hex>.json`, `night_shift_recurring.py`) are schedules in all but name. Each becomes an automation:
  - `kind = "cron"`, the template's task as `prompt`, `cwds = [repo]`, the RRULE of the table below;
  - a sidecar with `runner = "night"` and the template's `test` and `best_of`.
  - When such an automation fires, it does not run immediately: it queues a Night Shift task through `ragon night add`, as the template did. The run then follows the window, best-of-N and the morning report.
  - The cadence in Night Shift's terms becomes an RRULE timed at the window's start:

    | Template cadence | RRULE |
    |---|---|
    | `daily` | `FREQ=DAILY;BYHOUR=<start>;BYMINUTE=<start>` |
    | `weekly mon` | `FREQ=WEEKLY;BYDAY=MO;…` |
    | `days mon,wed,fri` | `FREQ=WEEKLY;BYDAY=MO,WE,FR;…` |
    | `every N` | `FREQ=DAILY;INTERVAL=N;…` with the first night as anchor (sidecar `anchor`, since DTSTART in the file is unverified) |
    | `monthly D` with D ≤ 28 | `FREQ=MONTHLY;BYMONTHDAY=D;…` |
    | `monthly D` with D > 28 | `FREQ=MONTHLY;BYMONTHDAY=28,…,D;BYSETPOS=-1;…` |

    The last row keeps Night Shift's rule that a day the month lacks is its last day (`night_shift_recurring.py:261-266`); a plain `BYMONTHDAY=31` would skip short months.
- **The migration loses nothing.** `ragon night migrate-recurring`, also run once by the update:
  1. It never touches `night/tasks/`, so every queued task stays queued.
  2. For each template it writes the automation and its sidecar, then moves the template to `night/recurring/migrated/`. It never deletes the template.
  3. It records the template's "last queued" date in the sidecar, so the night of the migration does not queue twice. The runner's existing once-a-day rule is a second guard.
  4. It is idempotent: a template already migrated is skipped.
  5. `--undo` moves the templates back and pauses the automations.
  6. `ragon night every add` keeps working: it now creates the automation.
- **One inbox.** The Scheduled view (which already shows Night Shift, DESKTOP_PARITY §2.12) lists Night Shift results beside automation runs, with the same actions. The morning report is unchanged.

### 4.9 Limits

| Limit | Default | Setting |
|---|---|---|
| Active automations per machine | 50 standalone + 20 heartbeats | `[automations] max_active` |
| Runs at once per machine | 1 (GB10 node; Night Shift holds its own slot) | `[automations] max_parallel` (1-2) |
| Run time | 30 min | sidecar `max_minutes`, 1-90 (Night Shift's `task_timeout`) |
| Memory, CPU, tasks | `MemoryMax=8G`, `MemorySwapMax=0`, `CPUQuota=400%`, `TasksMax=4096` | `[automations] task_memory` |
| Heartbeat interval | at least 5 min (Q5) | `[automations] heartbeat_floor_minutes` |
| Projects per automation | 10 | — |
| Prompt | standalone: 32,000 characters; heartbeat: as fits the envelope (row 36) | — |
| `memory.md` | 64 KiB; beyond that the run gets a warning in `automation_info` to shorten it | — |
| Managed worktrees | 15 most recent (row 13) | `[automations] keep_worktrees` |
| Run records | 90 days, pinned for ever | — |

### 4.10 Airgapped

- **The scheduler connects to nothing.** It uses local files, the local app-server, and the LAN only for the primary's list and forwarded edits, the same exception as `/node` (AIRGAPPED:131).
- **Runs inherit the machine's level.** At `on` the sandbox has no network, and full access is refused, even with `allow_full_access` (AIRGAPPED:139).
- **Automations that need a connection** (an app or plugin, web search) still run. Their tool calls fail as for any thread at `on`, and the run notes it.
- **No notifications** are sent (§4.7).
- **Egress.** The egress audit's process list (EGRESS §3.3 point 7) gains the scheduler and the runner as processes that must show no outside connection.
- **Unattended runs.** At `/airgapped env` (one run) the level applies to the run that set it, not to unattended runs.

## 5. Security

- **The tool is a privilege.** `automation_update` lets a model schedule future unattended work. Safeguards:
  - In phone threads, every create, update or delete needs the user's YES.
  - In desktop threads (and the TUI once it has the tool, §7), a create shows the user a card (the name, schedule and prompt) before the file is written.
  - A run can update only its own automation, never create another.
- **What a run can reach.** A run never gets more than the cap of §4.4. A prompt in the file cannot raise its sandbox: only the sidecar can, and the sidecar is written only by the user's own edit through the form or `ragon automations edit --allow-full-access`.
- **Imported upstream automations** arrive PAUSED (Q3). The user resumes them after reading them.
- **No secrets** are stored in either file. The API uses the existing device credentials.
- **Phone approvals.** Unattended runs never wait on an approval from a phone in v1 (§7).
- **Hardening beyond this** (signed automations, per-automation network allow-lists) is for release 3.0 (owner rule).

## 6. Tests

### 6.1 Schedule vectors (shared with upstream's format)

- **The file.** `tests/data/automations/rrule_vectors.json` holds the vectors, one per rule, each with `rrule`, the time zone, `after`, and the next times expected in UTC. The Rust crate and the desktop's form preview read the same file.
- **Formats covered.**
  - Both forms, with and without the `RRULE:` prefix.
  - `FREQ=DAILY;BYHOUR=9;BYMINUTE=0` read as daily, not weekly (#21342).
  - `FREQ=WEEKLY;BYDAY=MO,WE,FR`.
  - The docs' example `FREQ=MONTHLY;BYMONTHDAY=1;BYHOUR=9;BYMINUTE=0`.
  - `BYMONTHDAY=-1`, and `BYSETPOS=-1` for the Night Shift migration (D = 29, 30, 31 across February in leap and other years).
  - `FREQ=HOURLY;INTERVAL=2`, and `FREQ=MINUTELY;INTERVAL=15` for heartbeats.
  - `INTERVAL=1` minutely, raised to the 5-minute floor.
  - `BYSECOND=0` accepted; another `BYSECOND` rounded to the minute and flagged.
  - `COUNT` and `UNTIL`.
- **Invalid rules,** each refused with a message and no crash: `BYDAY=MON` (#36225), `FREQ=` missing, an unknown part, `INTERVAL=0`.
- **Local time and DST.**
  - Spring forward: a 02:30 rule on the change day fires at 03:30 (RFC 5545 §3.3.5).
  - Fall back: a 01:30 rule fires once.
  - A machine time zone change between two runs.
- **Missing DTSTART:** anchored at `created_at` (upstream's rule unverified; the vector documents Ragon's).
- **Expected values** come from RFC 5545's own examples where they exist, and otherwise are computed by hand in the test file's comments. No other implementation's code is copied.

### 6.2 Missed runs (§4.5)

The tests use a fake clock and a fake machine state. Cases:
- off across one due time: one catch-up;
- off across five due times: one run;
- off past `catch_up_hours`: `missed`, no run;
- a missed heartbeat: no catch-up;
- the clock jumps back an hour: no duplicate;
- the DST fall-back hour: no duplicate;
- the job cap full for 10 minutes: the run waits, then runs;
- the cap full for 40 minutes: skipped with "node busy";
- a benchmark lock: skipped, then caught up after release;
- model down for 10 minutes: retried and run;
- model down for 40 minutes: skipped;
- two machines with the same id: only the owner fires;
- a restart in the middle of a run: the run is marked `failed` ("interrupted") and not rerun;
- three skips in a row: one inbox item.

### 6.3 Files and protocol

- **Round trip.** An upstream-shaped `automation.toml` (keys of row 24, a comment and two unknown keys) is edited (name, then status) and every other byte stays equal.
- **Reported bugs, each a test** (#36256, #39899, #24007, #23798):
  - PAUSED stays PAUSED;
  - an edit is used by the next run;
  - a custom model id survives an update;
  - an update without `model` keeps it.
- **Import.** `~/.codex/automations/<id>` is copied as PAUSED, the original is untouched, and a second import does nothing.
- **Protocol** (recorded transcript against a stub app-server):
  - `thread/start` carries `threadSource` automation and the dynamic tool;
  - `turn/start` carries the exact trigger strings;
  - a heartbeat input equals the bytes of upstream's envelope vectors (`history/src/heartbeat_tests.rs`);
  - `automation_info` is present.
- **Upstream tests kept green.** These must pass in Ragon's codex-tests run:
  - `heartbeat_tests`;
  - `additional_context`;
  - `guardian_heartbeat_authorization`;
  - `scenarios_guardian_heartbeat`;
  - `mcp_server_tool_call_declines_full_access_elicitation_for_automation_thread`;
  - `in_app_local_automation_feature_requirements_are_valid`, plus a Ragon test that the scheduler stays off under that requirement.
- **Night Shift migration.**
  - Every cadence maps to the table of §4.8, and the vectors agree with `NightShiftRecurring.due` for 400 consecutive days.
  - Queued tasks are untouched.
  - No double queue on the migration night.
  - `--undo` restores the templates.

### 6.4 Unattended sandbox

These run on the node, with a real app-server and a tiny model or a scripted one:
- a write outside the workspace is refused;
- a network call is refused under `workspace-write` with network off;
- an escalation is refused under `never` and rerun sandboxed;
- a user default of full access is capped to `workspace-write` (Q1);
- `allow_full_access = true` gives full access, but not at `/airgapped on`;
- `requirements.toml` forbidding `never` gives `on-request`, and an unanswered request is declined after the timeout and marked "needs approval";
- `memory.md` is writable while its sibling `automation.toml` is not;
- `CODEX_HOME` and `DREAMFERENCE_VLLM_HOST` are set in the run's shell;
- the memory cap stops a run that allocates 9 GiB;
- `RuntimeMaxSec` stops a run at `max_minutes`;
- at `/airgapped on`, no notification leaves and the egress audit shows nothing.

## 7. Not in v1

- event triggers and team tasks (row 20);
- automations scheduled on Mac and Windows machines (they wait for the 0.9.10 nodes and Phase 12);
- heartbeats on remote-session threads (they wait for 0.9.10 attach);
- the `automation_update` tool in the TUI: it needs a Codex patch to inject a dynamic tool; until then the TUI has `ragon automations`;
- failover of an automation to another node;
- approving an unattended run's request from the phone;
- per-automation profiles beyond the sidecar's caps (#16695 asks for that upstream);
- writing upstream's SQLite (row 29);
- syncing run records between devices (the primary's list shows them; they stay on the owner);
- hardening for release 3.0 (§5).

## 8. Phases

Agent-days are for one lane. Rows for site, docs and tests go to Ragon first, then the cheapest model.

| Phase | Work | Days | Depends on | 0.9.11 |
|---|---|---|---|---|
| 0 | Settle the unverified keys (owner step 2 or issue reports); write the RRULE vectors file and the upstream-shaped fixture | 0.5 | — | yes |
| 1 | `ragon-rs/automations`: file model with round trip, RRULE engine, due times, run records, sidecar | 2.5 | 0 | yes |
| 2 | Scheduler in `ragon web` and `ragon automations serve` (Linux unit): admission, catch-up, the API | 2 | 1 | yes |
| 3 | Runner over the app-server: standalone kind, worktree or local, sandbox policy, limits, environment, findings | 2.5 | 1 | yes |
| 4 | Scheduled view: form, inbox, apply, discard, pin, archive, unread, `ragon://automations` | 3 | 2, 3 | yes |
| 5 | `automation_update` in the desktop, web and phone clients; `policy.json`; the confirmation cards | 1.5 | 2 | yes |
| 6 | `ragon automations` CLI, `import`, `doctor` | 1 | 2 | yes |
| 7 | Notifications: the bridges' `/auto` commands and the reports endpoint | 1 | 2 | yes |
| 8 | Night Shift template migration, one inbox | 1.5 | 2, 4 | yes |
| 9 | Heartbeats inside a chat (same machine) | 2 | 3, 5 | stretch |
| 10 | Docs and site (Ragon rows) | 1 | 4-8 | yes |
| 11 | Tests of §6 end to end on the node, the airgapped and egress checks | 2 | all above | yes |
| 12 | Mac launchd and Windows Task Scheduler / service hosting, the job limits there | 2 | 0.9.10 Mac and Windows nodes | no |
| 13 | Heartbeats on remote-session threads | 1 | 0.9.10 attach, 9 | no |
| 14 | TUI tool (one Codex patch, within the patch cap) | 1 | 5 | no |

**Totals.**
- Phases 0-8, 10 and 11, the 0.9.11 cut: **18.5 agent-days**.
- With the heartbeat stretch: 20.5.
- Everything: 24.5.

**Does 12-18 hold?** Only on a condition.
- **The window.** From Mon 12-01 to the freeze before Fri 12-18 there are 13 working days. 0.9.10 (12-10) takes lanes until then, and desk work, the IDE extension and browser use are specced for the same release.
- **The chain on two lanes** is about 10.5 days:
  - lane A: 1 → 2 → 4 → 8 (9 days);
  - lane B: 0, then 3 (after 1) → 5, 6, 7 (after 2);
  - then 10 and 11 shared (about 1.5 days on two lanes).
- **It holds if two lanes start by about 12-02**, overlapping the last week of 0.9.10. Nothing in the cut touches the 0.9.10 work (remote sessions, Mac and Windows nodes), so the overlap costs lanes, not conflicts.
- **If the lanes are free only after 12-10,** about 5 working days remain (about 10 agent-days on two lanes). The full cut does not fit. What can ship then is phases 0-3, 6 and 11 (10.5 days, at the edge): automations from the terminal on Linux nodes, with runs, catch-up and records. The Scheduled view, the tool, the phone and the Night Shift migration move to the release after.
- **One lane only:** phases 0-3, 5, 6, 10 and 11 add up to 13 days, the whole window with no slack: automations from the terminal and the chat tool, the inbox as a minimal list.
- **The heartbeat stretch** lands only if both lanes finish the cut by 12-12.
- **The 0.9.10 dependency.** Nothing in the cut depends on remote sessions or on the Mac and Windows nodes. Phases 12 and 13 do, and go to the release after 0.9.11.

## 9. Risks

- **Unverified upstream keys** (rows 24-27). Mitigations: round trip, unknown keys kept, Phase 0 and owner step 2. The risk is an imported automation running locally instead of in a worktree until the worktree key is known. Default to worktree for imports.
- **GPU contention.** Mitigations: one automation run at a time, the heartbeat floor, admission against benchmarks and the GPU lock.
- **A model that schedules work.** Mitigations: confirmations and the self-only rule for runs (§5).
- **Two apps on one machine.** Mitigations: separate homes; import as PAUSED; no automatic copy (row 30).

## 10. Work list

1. Phase 0: the vectors file and fixture, the key check.
2. Crate `ragon-rs/automations` with its tests.
3. Scheduler in `ragon web`; `ragon automations serve` and its user unit.
4. Runner `ragon automations run-one`.
5. Scheduled view: form and inbox.
6. `automation_update` in the clients; `policy.json`.
7. CLI.
8. The bridges' `/auto` and the reports endpoint.
9. `ragon night migrate-recurring`; `ragon night every add` writing automations.
10. Docs: `docs/dev/automations.md`, the user guide page, CHANGELOG; DESKTOP §6.8 and DESKTOP_PARITY §2.12 pointing here.
11. Tests of §6.
12. After 0.9.11: phases 12-14.

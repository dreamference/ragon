# Codex's log store, `logs_2.sqlite`

An audit (LOGGING §13 P7) of the SQLite log store that Codex keeps in `CODEX_HOME`, which for `ragon` is `~/.ragon/logs_2.sqlite`: what it stores, for how long, and whether a secret reaches it. It covers `ragon` 0.9.1, built on the pinned `rust-v0.158.0`. Re-run it after a Codex bump (see "Method" below).

## What writes it

`codex-rs/state/src/log_db.rs` installs a tracing layer that sends events into the store. The layer has its own fixed filter, `default_filter()`, which is TRACE by default and is independent of `RUST_LOG`: setting `RUST_LOG=trace` changes stderr but not the store.

- **The TUI** (`ragon`) and **the app-server** (`ragon app-server`, behind `ragon web` and the desktop app) install the layer.
- **`ragon exec`** does not install it (`log_db: None`). The file is created but gets no rows. Night Shift, SWE-bench and the egress audit therefore write nothing to the store.

The filter in `default_filter()`:

- caps `h2`, `hyper_util`, `tonic::transport` and `tower::buffer` at WARN;
- turns off `log`, `sqlx`, the OpenTelemetry targets and two token-estimate targets;
- caps the MCP client targets at INFO;
- caps `codex_http_client::transport`, `codex_api::sse` and two TUI streaming targets at DEBUG.

As a result, full HTTP request and response bodies and per-chunk SSE events stay out of the store.

## Schema and retention

`logs(id, ts, ts_nanos, level, target, feedback_log_body, module_path, file, line, thread_id, process_uuid, estimated_bytes)`. Writes are batched: a queue of 2,048 events, flushed in batches of 512 or every 10 s.

- **Per partition.** Each thread has a partition, and so does each process for its threadless rows. A partition is capped at 10 MiB and 1,000 rows (`LOG_PARTITION_SIZE_LIMIT_BYTES`, `LOG_PARTITION_ROW_LIMIT`); the insert transaction enforces both.
- **By age.** Rows older than 10 days (`LOG_RETENTION_DAYS`) are deleted only by the startup maintenance, `run_logs_startup_maintenance`. A long-lived app-server never prunes by age, so a store held open by the desktop app or `ragon web` keeps rows older than 10 days.
- **Deleted text lingers.** The database uses `auto_vacuum=INCREMENTAL`, but nothing calls `incremental_vacuum` and nothing sets `secure_delete`. Deleted rows stay in free pages until SQLite reuses them; in a real store, pruned message text was still readable in the file.

The file is created with the umask (0644 here); `~/.ragon` itself is 0700.

## What reaches it

**Two leaks, both fixed by patch `0037`:**

1. **Every submission's payload.** `codex_core::session::handlers` logged `debug!(?sub, "Submission")` for every operation except elicitation answers. That Debug-prints the whole operation into the store:
   - the user's prompt text and image references;
   - `!` shell commands and dynamic tool responses;
   - answers to user-input requests;
   - review requests and inter-agent messages;
   - realtime audio and text.

   A secret pasted into a prompt therefore landed in the store.
2. **The model's reply, in the TUI.** `codex_tui::markdown_stream` traces every streamed delta (`push_delta: "…"`) and the finished message at TRACE. The reply is split across rows, so a scan of single rows misses a secret the reply repeats. A model that echoes a file's content (a token read with `cat`) puts it in the store this way.

**What does not reach it:**

- **Tool calls** are logged by name, call id and duration only, not by arguments or output. A file's content read by a tool reached the store only through the reply (leak 2). The app-server runs, scanned with the reply split-aware as below, stored nothing of it.
- **The environment** never appeared. A variable set for the session did not appear in the store under the default filter or with `RUST_LOG=trace`.
- **Auth** is logged as presence flags (`feedback_tags`), not as values. A real store had no line naming an account or an email address.
- **HTTP** logs URLs and response headers at DEBUG; the bodies are capped as above.
- **Cwd and paths** are stored: the working directory, home-relative paths and the sandbox policy appear in the feedback tags and span fields. That is expected for a diagnostic store; it is private and stays in `~/.ragon`.

**The other stores in `~/.ragon` hold the conversation by design and are not logs:** the rollout files under `sessions/`, `state_5.sqlite`, `thread_history_1.sqlite` and the TUI's `history.jsonl`. This audit leaves them alone.

## The patch

`codex-patches/0037-submission-log-without-payload.patch` makes two changes:

- **handlers.rs:** the Submission line logs only the submission id and the operation kind, for every operation, as upstream already did for elicitations.
- **log_db.rs:** `default_filter()` caps `codex_tui::markdown_stream` at DEBUG, as upstream already caps the other streaming targets. The DEBUG line that gives the reply's lengths stays; the text goes.

**Checked with the patched build:** the TUI and app-server canaries, run again with the binary that `ragon-admin codex test` builds from the patched tree, left no canary in the store. The Submission row held only the id and `operation="turn_input"`, and no `codex_tui::markdown_stream` row was written at TRACE. This held even though the TUI's reply quoted the file's token on screen.

Explicit TRACE subscribers on stderr (`RUST_LOG=trace`) still see reply text, as documented in LOGGING §2.3.

**The patch does not scrub existing stores.** Rows already written go when the startup maintenance next runs, 10 days after they were written, and their text can survive in free pages after that. To clear an old store, delete `~/.ragon/logs_2.sqlite*` while no `ragon` process runs; Codex recreates it.

## Method

These canary runs, each in a temporary `CODEX_HOME` against the local model server, are the way to repeat the audit:

- **The test.** Use random tokens: one in the prompt, one in a file the agent is asked to `cat`, one in the environment. After the run, look for each token in three places: in each row of `logs` (`level`, `target`, `feedback_log_body`); in each target's rows joined in id order, since a streamed reply is split across rows; and as raw bytes in every file under `CODEX_HOME`, including the WAL and the free pages.
- **The app-server** was driven over JSON-RPC (`initialize`, `thread/start`, `turn/start`), once with the default environment and once with `RUST_LOG=trace`.
- **The TUI** ran under a pty, with the folder trusted and a window size set, and the prompt was typed into it.
- **`ragon exec`** ran once.
- **A real store** was scanned by counts only: levels, targets, row age, free pages, and secret patterns (bearer tokens, `sk-`, GitHub, AWS, JWT, PEM, `hf_`, `key=value`, Slack). None of its content belongs in the repository. The secret-pattern scan found nothing, but the store held conversation text from the Submission rows.

**Not exercised:** the desktop app's renderer (it uses the same app-server) and MCP servers that log their own arguments. MCP targets are capped at INFO.

## Notes for the spec

Two lines in `specs/DREAMFERENCE_LOGGING.md` predate this audit and should follow it the next time the spec is edited:

- §2.3 (Codex's TRACE store);
- the line that calls the store's tool payloads "unaudited".

O8 kept upstream's filter and retention. The patch adds one target cap to that filter, in upstream's own style, and changes no retention; the owner was asked whether to keep the cap.

# Ragon Egress — audit and airlock

**Status:** Phase 1 (the audit) implemented: `exec` sessions, the full-screen interface (`--tui`) and the audit after `ragon-admin codex build`: `dreamference/audit/`, `ragon-admin audit egress`; §10 records what was built and where it differs. Phase 2 (the airlock) is not built; its mechanism in §4.2 was checked on this host (details in §4.2). The desktop app (`--app`), the web server (`--web`) and the file index (`--docs`) are traced (§10.7, §10.8); the preview tab, the image search turn and the declared exceptions for the messenger bridges (§10.9); Claude Code compatibility (`--compat claude`), sync (`--sync`), and declared exceptions for remote access and the node's forge (§10.9).
**Superseded in part:** the airlock's switch is now the `on` level of `/airgapped` ([RAGON_AIRGAPPED §5.4](./DREAMFERENCE_RAGON_AIRGAPPED.md)), not `ragon --airlock`. The mechanism (§4.1, §4.2), the ledger (§4.4) and the audit (§3) stand; the surface (§2), the allowlist (§4.3) and §5 are read through that spec, which allows only the model server at `on`.
**Target:** the `ragon` terminal agent. `ragon-admin` runs the audit.
**Builds on:**
- the network-channel work of patches `0013` and `0015` ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md));
- the launcher's forced `chatgpt_base_url`;
- the trace scripts used to find those channels (`netproof.sh`, `tuitrace.py`);
- `bwrap`, which Codex's own sandbox already uses.

---

## 1. Goal

Make "your code stays on your machine" something a user can check, not a promise.

- **Phase 1, `ragon-admin audit egress`:** run one real `ragon` session under a tracer and print every network destination and every process it started, with a verdict.
- **Phase 2, `ragon --airlock`:** run `ragon` where only an allowlist of local services is reachable. Every other attempt fails and is written to a ledger.

**Why it matters.**
- **Developers care:** 81% of developers report security and privacy concerns about AI coding agents.
- **Toggles are not enough:** at least one commercial agent was found uploading whole repositories over a channel that ignored its privacy toggle.
- **Our own record:** tracing ragon found five channels to the upstream vendor or GitHub. One needed the vendor's sign-in; four needed none: Statsig metrics, featured plugins, a startup `git ls-remote` and the TUI announcement tip. All five are closed (`0013`, `0015`), and traced `exec` and TUI sessions now reach only `127.0.0.1:8000` (vLLM) and `127.0.0.1:8767` (the Gmail service). One more is closed without a trace: `/import`, whose plugin step can add a marketplace hosted on GitHub, a path no audit scenario reaches; patch `0012` hides it and a typed `/import` is refused. Phase 1 makes that trace a command anyone can rerun; Phase 2 makes a regression fail locally instead of leaking.

**Non-goals:**
- **Calling it air-gapped.** Web search and `ragon-fetch` reach the internet by design (§5), and the docs must never say "air-gapped" for a mode that allows them. The command `/airgapped` keeps to this: only its `on` level, which allows neither, carries the word ([RAGON_AIRGAPPED §1](./DREAMFERENCE_RAGON_AIRGAPPED.md)).
- **Inbound exposure.** The model server listening on `0.0.0.0:8000` is accepted by design: Ragon assumes the local network is trusted ([README](./README.md), "Accepted by design").
- **Containers.** The containers' own egress (SearXNG, the sidecars) is out of scope.

---

## 2. Surface

This spec adds no slash command of its own. (When it was written the patch budget was reserved for `/night`, which is now built; the airlock's switch has since moved to `/airgapped`, see the note at the top.)

| Command | Phase | Effect |
|---|---|---|
| `ragon-admin audit egress [--tui \| --app \| --web \| --compat claude \| --docs \| --sync] [--prompt "…"] [--json]` | 1 | Traces one session (the flags are mutually exclusive, §10.5, §10.7, §10.8, §10.9; `--docs` and `--sync` refuse `--prompt`; `--sync` is [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md) §17.5); prints destinations, processes, verdict; exit 0 on pass, 1 on an unexpected destination, 2 when the trace itself failed. |
| `ragon --airlock …` or `airlock = true` in `$CODEX_HOME/config.toml` | 2 | Runs `ragon` in the airlock. The launcher handles the flag before Codex parses its arguments, like `ragon app`, so it needs no patch. |
| `ragon airlock log [--since 1d]` | 2 | Prints the ledger (§4.4). |

**`codex build` runs the audit.** After a build with a new Codex release, `ragon-admin codex build` runs `audit egress` and prints the verdict. A failing verdict does not undo the build, but it is shown in red with the offending destinations. (As built, §10.5: after every build that installs a new `ragon`, both kinds of session, `--no-audit` to skip; the verdict is marked ❌, the CLI's mark for a failure, not coloured.) [RAGON_CODEX §6](./DREAMFERENCE_RAGON_CODEX.md) says to re-run the trace after every Codex bump; this makes that automatic.

---

## 3. Phase 1 — the audit

### 3.1. Method

This productises the earlier procedure.
1. **Setup.** A throwaway git repository with one committed file, and a throwaway `CODEX_HOME`, so that no login, history or config of the user's influences the result, and none is touched.
2. **Exec session.** `strace -f -qq -e trace=connect,sendto,sendmsg,sendmmsg,execve -s 256` (as built: `sendmmsg` is how glibc sends a lookup's queries, and without it a DNS query leaves a connect to the resolver and no name, §10) around `ragon exec --skip-git-repo-check "<prompt>"`, with the default prompt `Reply with exactly: pong`.
3. **TUI session** (`--tui`). The same trace around the interactive TUI, driven on a pseudo-terminal: accept the trust prompt, send the prompt, wait for the reply, quit. The TUI-only announcement fetch was found this way and is invisible to `exec`.
4. **Destinations.** Every `sin_addr`/`sin6_addr` and port from `connect`, `sendto` and `sendmsg`, counted.
5. **DNS.** Every name in UDP payloads to port 53.
6. **Unix sockets.** Every `sun_path`.
7. **Processes.** Every `execve`, with git subcommands that touch a network (`ls-remote`, `fetch`, `clone`, `pull`, `remote-https`) listed separately.

### 3.2. Verdict

- **Pass:** every IP destination is loopback and on the allowlist (§4.3), no DNS query was sent, and no networked git subcommand ran.
- **Fail:** anything else, listed first.
- **Trace failed:** the session did not produce a reply, or `strace` could not attach. This is not a pass.

`--json` writes the full result to `~/.ragon/audit/<timestamp>-<exec|tui|app|web|claude|docs|sync>.json` as well (the session kind is in the name, so a build's records cannot collide): destinations, DNS names, processes, verdict, the `ragon --version` output, and the Codex tag and patch hashes from the build stamp. Two audits can then be compared across builds.

### 3.3. Requirements

- **Tracer:** `strace`, installed here. Tracing one's own child needs no privilege under the default Yama `ptrace_scope` of 1.
- **Model server:** the session needs a model server; without one the audit reports "trace failed".

---

## 4. Phase 2 — the airlock

### 4.1. Model

`ragon` runs in its own network namespace, where the only interface is `lo`. Every outbound connection fails with `ENETUNREACH` at the kernel. On the namespace's `lo`, the launcher listens on each allowlisted port and relays each connection to the same port on the host's loopback. The allowed services therefore see ordinary loopback clients, and nothing else is routable.

This is the stronger form of what the patches do. The patches close channels one at a time at their call sites. The airlock needs no knowledge of the channels: a new one in a future Codex release fails closed.

### 4.2. Mechanism

**Starting the airlock.** `ragon --airlock` makes the launcher do the following before Codex parses its arguments:
1. **Relay directory.** It creates `$XDG_RUNTIME_DIR/ragon-airlock/<pid>/`, mode `0700`.
   - The path must stay short: unix socket paths are limited to 108 bytes, and a path under the scratch directory used for the checks failed with `AF_UNIX path too long`.
2. **Host-side relay.** It starts a relay, in-process on a thread of the launcher that stays outside the namespace. For each allowlisted port, the relay:
   - listens on a unix socket `<port>.sock` in that directory;
   - forwards each connection to `127.0.0.1:<port>` on the host.
3. **Re-exec.** It re-executes itself under `bwrap --unshare-net --dev-bind / / --bind <dir> <dir> --die-with-parent`, with an environment marker so the child knows it is inside.
4. **Inside the namespace,** the child launcher:
   - listens on `127.0.0.1:<port>` for each allowlisted port;
   - relays to the matching unix socket;
   - then continues into Codex exactly as today.

**Verified on this host,** as an unprivileged user:
- `bwrap --unshare-net` works despite `kernel.apparmor_restrict_unprivileged_userns = 1`, because bwrap's AppArmor profile permits it;
- inside, `lo` is the only interface;
- an outbound connect fails with `[Errno 101] Network is unreachable`;
- a unix socket bind-mounted from the host is reachable;
- a listener on `127.0.0.1:8000` can be opened;
- a nested `bwrap --unshare-net` inside succeeds, which matters because Codex's own command sandbox runs bwrap inside ours.

**Not yet verified:**
- **Codex's sandbox inside the airlock.** It also uses Landlock and seccomp. Running its full command sandbox inside the airlock must be checked end to end, not only a bare nested bwrap.
- **`pasta`,** the alternative that would forward chosen ports without a relay, is not installed here. It would add a system dependency, so the relay stays the design unless it proves unworkable.

### 4.3. Allowlist

The allowlist is `airlock.allow` in `$CODEX_HOME/config.toml`, written by the launcher, with these defaults:

| Port | Service | Why |
|---|---|---|
| vLLM port (from the resolved `vllm_host`) | model server | The agent's model. |
| `8767` | Gmail search service | `ragon-admin gmail`. |
| `8888` | SearXNG | `ragon-search` (§5). |
| `8768` | image search service | The `image_search` tool. On the audit's allowlist for every session (`IMAGE_SEARCH_PORT`, §10.9). |

**The diffusion endpoint (`8001`) is not allowed:** the agent does not use it. Unix sockets on the host are unaffected by a network namespace, which is intended: the Docker socket, the D-Bus session and the like stay reachable. If one of them should be closed too, that belongs to Codex's command sandbox, not the airlock.

### 4.4. Ledger

A connection that fails at the kernel leaves no trace by itself. The ledger is built from two sources:
- **DNS.** Inside the namespace, `/etc/resolv.conf` is bind-mounted to point at `127.0.0.1`. The launcher runs a stub resolver there that answers every query `REFUSED` and records the name, the time and the asking process (from `/proc/net/udp` inode to pid). Almost every real egress starts with a lookup, so this catches nearly all attempts, with the name the program wanted.
- **Direct-IP attempts** skip DNS. With `--airlock=trace`, the airlocked process tree also runs under `strace -f -e trace=connect -e status=failed`, and failed non-loopback connects are added to the ledger. This is off by default for its overhead.

The ledger is appended to `~/.ragon/airlock/ledger.jsonl`:

```json
{"at": "…", "session": "…", "kind": "dns", "name": "ab.chatgpt.com", "pid": 4242, "exe": "/home/…/ragon"}
```

`ragon airlock log` prints it grouped by name and executable. An empty ledger over a week of use is the record the goal asks for.

---

## 5. Search and fetch, honestly

- **Search goes out by design:** `ragon-search` reaches SearXNG on `127.0.0.1:8888`, and SearXNG queries the upstream engines from outside the airlock. The ledger cannot see those queries. The airlock's own docs and the agent's prompt must say plainly that searches leave the machine through SearXNG.
- **`ragon-fetch`** connects directly to the URL (or through `https_proxy` when set), so it fails inside the airlock. It fails with its existing error path, and the ledger records the lookup.

**The prompt must match the mode.** The launcher writes the model catalog, including `WEB_ACCESS_INSTRUCTIONS`, at every start. In airlock mode it writes a variant:
- search is available, and its queries leave the machine;
- `fetch` is not available.

Without that, the model would be told it can fetch pages, and would keep trying.

**Planned:** fetch through a local, logged proxy that asks for confirmation. That is out of scope here.

---

## 6. Where it is used

- **Opt-in.** Interactive use is opt-in (`--airlock` or the config key) until a release has run a week of the maintainer's own sessions with an empty ledger and nothing broken.
- **Night Shift** runs use the airlock by default once it exists ([NIGHT_SHIFT §6](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md)): no one is watching those sessions.
- **`ragon-admin drafter tune`** runs its training process in the airlock ([SELF_SPEEDING §7](./DREAMFERENCE_SELF_SPEEDING.md)).

---

## 7. Tests

- **Phase 1:**
  - unit tests of the trace parser on recorded `strace` output: IPv4, IPv6, DNS payloads, unix paths, `execve` lines, including the earlier traces with the five channels, which must fail the verdict;
  - a live test (skips without a server): the audit passes on the current build.
- **Phase 2:**
  - launcher unit tests for the allowlist, the socket-path length check and the resolver stub;
  - a live test in which an airlocked `ragon exec` session answers through vLLM;
  - inside the same session, a `curl https://example.com` run by the agent fails, and `example.com` appears in the ledger;
  - `ragon-admin gmail status` and `ragon-search` still work inside it;
  - Codex's command sandbox still starts inside the airlock (§4.2, not yet verified).

---

## 8. Acceptance criteria

- **Audit:**
  - `audit egress` on the current build prints only allowlisted loopback destinations, no DNS and no networked git, and exits 0;
  - on a build without `0015` it fails and names all four channels.
- **Airlock:**
  - an airlocked TUI session works as usual: model, slash commands, Gmail, search;
  - removing `0015` from the build and repeating the session leaves the Statsig, plugin and announcement lookups in the ledger, and sends nothing out.
- **Docs:** nothing describes a mode that allows search or fetch as air-gapped.

---

## 9. Open questions

- **`ragon app`.** The desktop app launches its own process tree. Should the airlock also cover it, or is it only for the terminal agent?
- **MCP servers.** A user-configured MCP server that needs the network fails inside the airlock. Should the allowlist accept per-server exceptions (by port), or should such servers be run outside and reached over a unix socket?

---

## 10. As built: Phase 1, exec sessions

### 10.1 What exists

`ragon-admin audit egress [--prompt "…"] [--json]`, in `dreamference/audit/`:

| Piece | Path |
|---|---|
| The trace's content (destinations, resolvers, names, unix sockets, processes, networked git) | `egress_trace.py` |
| Reading strace's output | `strace_parser.py` |
| The verdict and its exit code (0 pass, 1 unexpected destination, 2 trace failed) | `egress_verdict.py` |
| The session, the judgement, the report, the JSON result | `egress_audit.py` |

**Run on this machine, twice, both a pass.** At 21:34 on the 16-patch build of `rust-v0.158.0` installed at 19:10, and at 21:44 on the 17-patch build that the `/airgapped` work installed at 21:41 (the kept record has `build_matches_checkout: true`). Each time one `ragon exec "Reply with exactly: pong"` in a throwaway repository with a throwaway `CODEX_HOME` connected to `127.0.0.1:8000` (twice) and `127.0.0.1:8767` (once), sent no DNS query, opened no unix socket but glibc's absent `nscd` one, and ran no networked git command; 2.7 s. A second run whose prompt made the agent run `curl https://example.com` also passed, correctly: Codex's read-only sandbox refuses the `socket` call (`socket(AF_INET6, SOCK_DGRAM, …) = -1 EPERM` in a trace taken with `socket` added), so `curl` never reaches a `connect` or a lookup and exits 6.

### 10.2 Where it differs from §2 and §3

- **`sendmmsg` is traced** (§3.1 listed `connect`, `sendto`, `sendmsg`). On this machine a lookup is a `connect` to `127.0.0.53:53` followed by one `sendmmsg` carrying the A and the AAAA query.
- **Port 53 is a resolver, not a destination.** A connect to `127.0.0.53:53` is loopback, and a query sent there still leaves the machine through systemd-resolved. So resolvers are listed apart, every name asked is a failure, and a query whose name could not be read (strace cuts payloads at 256 bytes) fails too, naming the resolver.
- **A lookup through a local resolver's unix socket is a failure, the same as a DNS packet**. nss-mdns asks avahi (`/run/avahi-daemon/socket`) and nss-resolve asks systemd-resolved (varlink `/run/systemd/resolve/io.systemd.Resolve`, or `org.freedesktop.resolve1` on the system D-Bus), and they resolve the name on the network, so no DNS packet is ever traced. A connect or send to either socket, or a D-Bus message addressed to `org.freedesktop.resolve1` or `org.freedesktop.Avahi`, fails every scenario with `asked a local resolver through <socket> (<n>x)` (`LOCAL_RESOLVER_SOCKETS` in `strace_parser.py`). A plain connect to the system bus and nscd's socket are not lookups.
- **A connect counts whatever it returned**: `EINPROGRESS` is the normal result of a non-blocking connect, and a refused or unreachable one was still an attempt.
- **`127.0.0.1:9` is a failure with its own wording.** It is where the launcher points `chatgpt_base_url`; a connect there is a call to the vendor's backend that no patch closes and that failed only because of the redirect.
- **`git ls-remote --get-url` is not networked** (it prints a URL after applying `insteadOf`; `ragon-code` uses it for the submodule policy). git's subcommand is the first word that is neither an option nor an option's value, so a directory named `fetch` or a `--grep pull` is not a finding. `push` and `remote-http` are counted as networked beside §3.1's list (`NETWORKED_GIT` in `strace_parser.py`).
- **A failed `execve` is not a process**: it is the shell walking `PATH`. An `execve` interrupted in the trace (`<unfinished ...>`) is counted when its `resumed` line reports success.
- **The code index and the local file index are off for the traced session** (`code_index_enabled = false` and `ragon_docs = false` in a throwaway config named by `DREAMFERENCE_CONFIG_PATH`, plus `DREAMFERENCE_RAGON_DOCS=0`, `DREAMFERENCE_RAGON_ADAPTER=""` and `DREAMFERENCE_RAGON_WORKTREE=off` in the environment). Their indexers run detached, in their own network-less sandbox, and outlive the session; they are not what this trace can show (`--docs` traces `ragon-docs` separately, §10.8).
- **The JSON result also says whether the audited binary was built from the checkout as it is** (`build_matches_checkout`): the patch hashes are the checkout's.
- **The session is stopped with its whole process group** after 300 s, so a session that never answers (no model server) ends as "trace failed" and leaves nothing running.

### 10.3 Tests (`tests/test_egress_audit.py`, 14)

- The recorded trace of the passing session (`tests/fixtures/egress/exec_pass.strace`, 347 lines, 22 of them unfinished or resumed).
- The same trace with the channels of `0013` and `0015` written back in (`exec_leaks.strace`): the verdict fails and names `ab.chatgpt.com`, `chatgpt.com`, `raw.githubusercontent.com`, the `git ls-remote` of `openai/plugins`, each address, and the `127.0.0.1:9` redirect. **These lines are written by hand in strace's format**: the traces were in a scratch folder that has since been deleted, so §7's "the earlier traces" could not be used.
- A real recording of `curl https://example.com` (`curl_example.strace`): the name, the resolver and the four addresses are read from what glibc and the kernel actually printed.
- A session with no reply, and an empty trace, are "trace failed"; a local port off the allowlist fails; the escapes and the DNS decoder; git commands that reach nothing; the report; the audit run end to end with a stand-in for strace (throwaway repository and home, both removed afterwards; the JSON written to the real `CODEX_HOME`); missing strace or `ragon`.

### 10.4 Not built

- ~~**§8's acceptance on a build without `0015`**~~ Done. A scratch build of the same tree with every patch except `0015` (built in `~/.cache/dreamference/ragon-codex/no0015`, the installed build untouched) **fails `audit egress --tui` with exit 1**, naming all four channels: DNS lookups of `ab.chatgpt.com` (the metrics exporter), `git ls-remote` and a fetch of `github.com/openai/plugins`, `raw.githubusercontent.com` (the announcement tip only the interface fetches), and a connect to `127.0.0.1:9`, reported as a call to the vendor's backend no patch closes. The installed build passed the same audit (model server and Gmail only, no DNS). That run's file listed `0015` among the patches, because the result recorded the checkout's patches rather than the traced binary's; since then the result names the traced binary and its SHA-256, and records the patch list only when the binary is the installed build and that build matches the checkout.
- **Phase 2**, the airlock and its ledger (§4): its switch is now the `on` level of `/airgapped`.

### 10.5 The interface, and the audit after a build

**`ragon-admin audit egress --tui`** traces the full-screen interface instead of `ragon exec` (`dreamference/audit/tui_session.py`, `TuiSession`). The same strace is put around `ragon` with no subcommand, on a pseudo-terminal of 160×50 with `TERM=xterm-256color`; the audit waits for the composer, types the prompt, waits for the reply, lets the session settle for 3 s, types `/quit`, and reads the trace.

Where it differs from §3.1 step 3:

- **The trust prompt is not answered; it is not shown.** The throwaway `CODEX_HOME` is given `[projects."<repo>"] trust_level = "trusted"` before the session starts (the launcher edits `config.toml` in place and keeps the table), as the live slash-command tests do. So the trust screen itself, and any other first-run screen, is not part of the trace.
- **The reply is read from the session file, not from the screen.** The screen also shows the typed prompt, and the default prompt contains the word the reply consists of. A session has replied when a `rollout-*.jsonl` under the throwaway home holds a `task_complete` event with a non-empty `last_agent_message`.
- **The terminal is driven with `pexpect` and rendered with `pyte`**, which the live tests already use and which are not dependencies of the package. Without them `--tui` is "trace failed" (exit 2) and prints the `pip install` line, as a missing `strace` does.
- **A session that does not end is stopped with its process group**, after the same 300 s; "trace failed" then also says whether the interface had opened and taken the prompt.
- **The interface's commands can reach the network; `exec`'s cannot** (at the time of this measurement; patch `0031` gives the read-only sandbox network access at `/airgapped off` too unless `sandbox_workspace_write.network_access = false`, [RAGON_AIRGAPPED §14.11](./DREAMFERENCE_RAGON_AIRGAPPED.md)). The launcher gives the interface's workspace-write sandbox `network_access = true`, while `ragon exec` runs read-only with none. A command the model chooses to run in the traced interface session is therefore traced with network access. The default prompt asks for one word so that no command runs; `--prompt` is how to look at what a task does.

**Run on this machine, both a pass**, on the `ragon` installed (build key `064c6b8c737f-4b9f72c9a5ce`, 17 patches; `build_matches_checkout: false`, because the launcher source had moved on).

| | `exec` | interface |
|---|---|---|
| `127.0.0.1:8000` (model server) | 2 connects | 3 |
| `127.0.0.1:8767` (Gmail service) | 1 | 1 |
| DNS queries, networked git | none | none |
| Unix sockets | glibc's absent `nscd` | the same, and `/run/user/1000/bus` (twice, with one message to `/org/a11y/bus`) |
| Wall time | about 3 s | 17 s |

The session bus is the one thing the interface opens that `exec` does not. It is a unix socket on this machine and is listed, not judged; which part of the interface asks the accessibility service was not looked into.

**After a build.** `ragon-admin codex build` now ends with the audit when, and only when, it installed a new `ragon`: the build was not current beforehand, or `--force` was given, and it succeeded. `EgressAudit.after_build()` traces an `exec` session and then the interface, and writes both records (and also `--compat claude` via `CompatPrintSession.after_build()` when the user's new sessions start in `claude` mode, §10.9).

- **Not every build:** §10.4's old objection was a model request in every build. A `codex build` that finds the binary current runs nothing. Whether the binary was current is asked *before* the build, because afterwards a build that compiled and one that found nothing to do both report success.
- **It never waits for a model server.** The server is asked once with a 3 s timeout; if it does not answer, one line says the audit was skipped and names the two commands to run later. Without that check a build on a machine whose server is down would end with the launcher's five-minute wait.
- **It never changes the build's exit code.** The binary is installed either way. An unexpected destination ends with a ❌ line saying the build reaches something it should not; a trace that failed says the audit could not show what the build does; an audit that itself breaks is reported in one line.
- **`--no-audit`** skips it. Without `pexpect` and `pyte` only the `exec` session is traced, and a line says so.
- **Where the hook is:** in the CLI's `codex build` branch, not in `CodexBrandedBuilder`, so nothing that calls the builder from a test can start a session.
- **Not exercised by a real build.** Once several tasks were rebuilding `ragon` and the model server was shared with a benchmark, so no `codex build` was started for this. The branch is covered by tests with the builder and the audit replaced; the two sessions it runs are the ones measured above.

### 10.6 Tests added (`tests/test_egress_audit.py`, 27 in all)

- A recorded trace of a real interface session (`tests/fixtures/egress/tui_pass.strace`, 385 lines): the same destinations as `exec` plus one more connect to the model server, and the session bus among the unix sockets.
- The session file: a prompt alone is not a reply; a `task_complete` with a message is.
- The interface played by a stand-in on a real pseudo-terminal, under a stand-in `strace`: `ragon` is started with no subcommand on a terminal, the prompt and then `/quit` are typed, the throwaway home trusts the throwaway repository, both are removed afterwards, and the record is `…-tui.json` with `"session": "tui"`.
- A stand-in that never answers: "trace failed", the report says the interface had taken the prompt, and the process is gone.
- `--tui` without `pexpect` and `pyte`.
- After a build: both sessions traced and recorded; no model server, no session and no wait; a failed trace does not hide a failing verdict; a broken audit is reported, not raised.
- `codex build`: audited when a new binary was installed or `--force` was given; not when the build was current, failed, or `--no-audit` was given; its exit code is the build's whatever the audit returned.

### 10.7 The desktop app, and route lookups

`--app` traces the Electron desktop app (`ragon-app`) in a hidden session of its own (DESKTOP_ELECTRON §6): Chat on the web UI, Work's `ragon app-server`, then quit; scratch `HOME` and `CODEX_HOME`; the web UI's port 3000 joins the allowlist for that session alone. Now (ASK §18.6) the app has one window and one app-server: the session opens it on Ask with no argument, as `ragon app` does, starts its app-server and quits, and its allowlist is the `exec` session's, so a connect to `ragon web`'s 3100 or the web UI's 3000 is a finding.

Its first trace failed on one destination, `[2001:4860:4860::8888]:443` (Google Public DNS), 9 times. A trace with socket labels (`strace -yy`) showed what it was: `connect()` on a **UDP** socket, failing with `ENETUNREACH` here, and nothing ever sent on it. Chromium's network log named it `HOST_RESOLVER_MANAGER_IPV6_REACHABILITY_CHECK`, run for every host resolution (207 for `localhost:3000`, 12 for the page's own requests to `localhost:8767`), IP literals included (`HostResolverManager::RequestImpl` starts in `STATE_IPV6_REACHABILITY` unconditionally). In Chromium's source the check is `StartGloballyReachableCheck`: `ConnectAsync` on a UDP socket to that address, port 443, then a look at the local address the kernel chose; the result is cached for one second. A UDP `connect()` puts no packet on any network: it asks the kernel for a route and fixes the socket's peer. No switch turns the check off: `EnableIPv6ReachabilityOverride` only changes how the result is used (measured: the probes stay), and mapping `localhost` to `127.0.0.1` (`--host-resolver-rules`) removed the `::1` attempts but not the check.

So the audit now tells a route lookup from a connection, and still judges anything sent:

- strace runs with **`-yy`**, which labels each descriptor with its socket kind and inode (`23<UDPv6:[10868489]>`), and traces **`write` and `writev`** besides the `send*` calls, because a connected UDP socket accepts a payload through either.
- A `connect()` on a UDP socket still labelled by its inode (not connected before) to anything but port 53 is recorded under **`route_lookups`**, not `destinations`, and the report lists it on a line of its own ("Route lookups (UDP connect, nothing sent)").
- The **first payload on that socket** (`send*`, `write`, `writev`; matched by inode, or by the peer in the label once strace prints one) puts its destination into `destinations`, where the verdict judges it like any other.
- A TCP connect is never a route lookup (it sends a SYN), a UDP connect to port 53 stays a resolver connect (its payloads are read as queries), and a trace without labels keeps the old reading: every connect a destination.
- With `-yy` a descriptor's label changes between connect and send (inode, then the peer), so resolver sockets are keyed by the descriptor's number alone.

Cost, measured on the app session: 2.4 MB of trace and 10,522 lines for 25 seconds, finishing normally; the `exec` session's verdict and destinations are unchanged. Results on this machine: `--app` **pass** (loopback ports 3000, 8000 and 8767 only; 11 route lookups to the probe address, nothing sent), `exec` **pass**.

Tests added (36 in all): the recorded probe line is a route lookup and the app session passes; a `write` or a `sendto` on that socket (labelled by inode, or by its peer) makes it a destination and fails; without labels it is a destination as before; a TCP connect is never a route lookup; a labelled resolver socket still yields its query's name; the app session runs hidden with a scratch `HOME` and needs a display and a build.

### 10.8 Three more scenarios

`ragon-admin audit egress` now traces one of five things, chosen by a flag ([CLI §4.24](./DREAMFERENCE_CLI.md)); the verdict and the exit codes are the same for all.

- **`--app`:** the desktop app's first window is Ask on `ragon web` (DESKTOP_ELECTRON §11), so the allowlist for that session names `ragon web`'s port 3100 (ASK §18.6): the app starts no `ragon web`, so that session's allowlist is the `exec` session's, with neither port on it.
- **`--web`:** `ragon web serve` on a free loopback port, traced, while an untraced `ragon web ask` asks it one question through the bridge (RAGON_ASK §13). Everything the server starts is traced with it: the app server it launches on its own socket, and `ragon prompt show ask --composed`. `HOME` and `XDG_RUNTIME_DIR` are scratch too, so the server finds no advertised node, no user unit and no app server of the user's to join. RAGON_ASK makes a pass a condition of shipping Phase A.
- **`--docs`** (`DocsEgressAudit`, `audit/docs_egress_audit.py`): `ragon-docs index` over a fixture folder and a `ragon-docs search`, both under `strace -f` in a throwaway home. It passes only with **no network destination at all**, loopback included, and no DNS query, because the document index never talks to anything; it needs no model server. The extractor and the embedder run in `systemd-run --scope` inside bwrap with an empty network namespace, and the trace follows them ([RAGON_LOCAL_INDEX](./DREAMFERENCE_RAGON_LOCAL_INDEX.md) §10.2).

`codex build` still runs only the `exec` and `--tui` scenarios after a new build (§10.5; plus `--compat claude` when the user's new sessions start in `claude` mode, §10.9).

### 10.9 The preview tab, the image search turn, and declared exceptions

- **`--app` opens a preview tab** (DESKTOP_PARITY §5, Phase 3 item 4). `PreviewPage` (`audit/preview_page.py`) serves one loopback page for the session's length, and the app opens it in a preview tab (`RAGON_APP_AUDIT_PREVIEW`). The page reaches for `example.com`, `.org` and `.net` by picture, script, stylesheet, `fetch`, `dns-prefetch` and `preconnect` hints (in the page and added by script) and a link followed by script; the preview's rules must turn each away before a connection or a lookup. Only the page's own port joins the allowlist. A pass whose page never loaded is reported as "trace failed", because the rules were not exercised. The same commit turned Blink's DNS prefetching off in the app, which had resolved hinted names past the request rules.
- **`--web` asks an image question** where image search is set up on this machine (its secret file exists). The scratch home is laid out as a node with a copy of that secret and a `ragon-admin` that runs the code under audit (`write_admin`, not a link to `~/.local/bin/ragon-admin`); the audit starts the tool's MCP server as Codex will and asks for its tools first. A second `ragon web ask` then calls `image_search`; the tool's server is traced with the app-server, and its one connection, to `8768`, is allowed (§4.3). The report prints `Image search turn: …` (`skipped`, `not asked: …`, `no answer`, `answered, without an image` or `answered with an image`, the last only when the answer names an image the service's store holds) and the JSON result carries `image_search_turn`.
- **Declared exceptions** (`EgressAudit.declared_exceptions`). Every report ends with one `Declared exception:` line per messenger bridge or node service that is on and talks to an outside service outside any traced session: the system unit `ragon-signal.service` and each further `ragon-signal@<name>.service` (SIGNAL §18), the user unit `ragon-chat.service` when a Telegram token is set, the user socket `ragon-matrix-proxy.socket` (with whether push is on), the system unit `ragon-remote-tunnel.service` for remote access (`REMOTE_TUNNEL_UNIT`), and the system unit `ragon-forge.service` for the node's forge (`forge_lines()`, with its role and outbound GitHub mirror destinations). With every bridge and service off nothing is added, and they never change the verdict.
- **The allowlist** is the same for every session (model server, `8767`, `8888`, `8768`), and a connect to port `3000` or `ragon web`'s `3100` from an `--app` session is a finding.
- **`--compat claude` and `--sync`.** `CompatPrintSession` (`audit/compat_print_session.py`, [RAGON_CLAUDE_CODE_COMPAT](./DREAMFERENCE_RAGON_CLAUDE_CODE_COMPAT.md) §13) traces `ragon --compat claude -p "<prompt>" --output-format json` in a throwaway repository populated with `CLAUDE.md`, `.claude/settings.json`, `.claude/commands/hello.md`, `.claude/agents/reviewer.md` and `.mcp.json` (approving the stdio `fixture` server with `ragon compat mcp approve fixture` while leaving the HTTP `remote` server unapproved), and `EgressAudit.after_build()` also runs it when `DREAMFERENCE_RAGON_COMPAT=claude` or `ragon_compat = "claude"` in `~/.config/dreamference/config.toml`. `SyncEgressAudit` (`audit/sync_egress_audit.py`, [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md) §17.5) traces `ragon sync agent --once` between a throwaway device and a throwaway hub on loopback, requiring the interactive thread to reach the hub's port and the `ragon exec` session to stay off it.

Tests: 46 in `tests/test_egress_audit.py` (52 earlier).

## 11. Local traffic and a leftover proxy

The audit proves where a session connects; it does not cover a proxy variable left in the user's shell. With `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` set, upstream Codex routes a request through it unless `NO_PROXY` names the host (`http-client/src/outbound_proxy.rs`: the environment route reads `HTTP_PROXY` or `ALL_PROXY` for an `http://` URL, and `NO_PROXY`, then `no_proxy`, for the exemptions), and so do `requests` and `urllib`. The model server, SearXNG, the Gmail service and the code index would then be reached through the proxy: at best a detour, at worst a refusal that looks like a dead server, and the prompt leaving the machine. Other local agents have hit exactly this (an `HTTP_PROXY` intercepting a loopback health check). Upstream Codex has no per-provider proxy setting, so the variable is the channel:

- **`ragon`** (`ragon-rs/src/proxy.rs`): first thing in `main()` (`home::use_ragon_home`, before any other thread exists), `NO_PROXY` and `no_proxy` are both set to the union of what either held, then `localhost`, `127.0.0.1`, `::1`, the host of `DREAMFERENCE_VLLM_HOST` or `vllm_host`, and the remembered node's (including its remote-access overlay host when `overlay_name_is_its_own()` holds). Both spellings, because the readers disagree on which wins (Codex and reqwest the upper case, `requests`, `urllib` and `ragon-search` the lower). Everything the session starts inherits it. The launcher's own requests to the model server and the Gmail service (`wait_for_model`, the KV-pool read, the node checks) use a client with no proxy at all, which also covers a node found by a browse for the first time; that node is exempt for Codex from the next start on, once it is remembered.
- **`ragon-admin`** (`ProxyBypass`, `dreamference/config/proxy_bypass.py`): `run_cli` extends its own environment the same way before anything else (loopback and `DREAMFERENCE_VLLM_HOST`), and again with the configured `vllm_host` once the configuration is read. Every `requests` and `urllib` call in the process and every child that inherits the environment is covered. `http.client` (used by nothing that reaches the model server) does not read proxies at all.
- **Night Shift**: each `ragon exec` and test run gets the exemption for its lane's host, which may be another node's; a task from another machine gets it inside the job sandbox.
- **SWE-bench**: the container's environment carries both variables with loopback and the model server's address on the network's gateway, plus the host's own entries. Docker can also inject a proxy into every container from the client's configuration; the explicit exemption covers that too.

Existing entries are kept and nothing is removed, so a user's corporate exemptions survive. Tests: `proxy::tests` (the launcher), `tests/test_proxy_bypass.py` (including that `requests` and `urllib` then bypass a set proxy), and the Night Shift and SWE-bench environment tests. Not done: a line in `ragon-admin host check` reporting a proxy variable.

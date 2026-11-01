# Ragon Node — splitting Ragon into a client and `ragon-node`

**Status:** implemented in part, on one GB10 (a second one was paired, §18.10), and Parts 2 and 3 completed except for sending from a client (§18.8). Part 1: the node side (`ragon-admin node enable|disable|status|id`), the launcher's resolution tiers and `ragon node`, and `ragon-search` on the node's SearXNG; `ragon-app`'s forwarder was built too, and removed (§7). Part 2: the SSH pairing, `node list|add|remove|status|set|start|stop|sync-model`, and replicas as extra model servers for Night Shift and SWE-bench. Part 3: script jobs, `node run|jobs|logs|cancel|fetch` with `--setup`, `--out` and `--bind`, pruning, and night tasks worked by another node (`/night add --on`). Since: `/node` in the TUI (§18.9), `node provision|prepare|askpass` ([RAGON_FLEET](./DREAMFERENCE_RAGON_FLEET.md), merged), and the advertised web UI, `ragon web` on 3100. **§18 records what was built, what was measured and what was not;** where it differs from the sections above, §18 and §15.1 win. Two machines have been paired and the second was installed with `node enable --yes` (§18.10); jobs and `/night add --on` between them have still not been run, and a Night Shift lane on the second machine ran once, on one night, and was lost to a DHCP move (§13.2). Clients for x86-64 Linux and macOS have been built (§8.4) and for Windows as a preview.
**Target:** the GB10 (DGX Spark) as the server, and Ubuntu, macOS and Windows machines on the same local network as clients.
**Builds on:**
- the launcher in `ragon-rs/` and its `vllm_host()` tiers ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md));
- the web commands in `ragon-web-rs/` and the code index in `ragon-code-rs/` ([RAGON_CODE_INDEX](./DREAMFERENCE_RAGON_CODE_INDEX.md));
- the web UI and its desktop window (`ragon web`, [RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md));
- the model server and its host-safety layer ([INFERENCE](./DREAMFERENCE_INFERENCE.md));
- the trusted-LAN decision ([README](./README.md), "Accepted by design");
- `/airgapped` and the egress audit ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md), [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md)).

**Decisions made here, stated first because each could be read the other way:**

1. **`ragon-node` names a role, not a command.** It is the server half of the product: the model server and its sidecars on a GB10, the service advertised on the network (`_ragon-node._tcp`), and the install profile. The node's command line stays `ragon-admin`, which gains a `node` group. A separate console script would break the prompt's `ragon-admin gmail` line. If a `ragon-node` executable is wanted as well, it is an alias (§15, question 1).
2. **The client is Rust binaries only: no Python, no Docker.** `ragon`, `codex-code-mode-host`, `ragon-code`, `ragon-search`, `ragon-fetch` and `ragon-app`. Everything Python stays on the node.
3. **No new always-on daemon for the split.** The node's existing Avahi daemon advertises it, and clients talk to the services' own ports. A front-door reverse proxy was considered and rejected (§5.4). (A control agent for managing a node from another machine was designed in §12.4 and dropped: control goes over SSH, §15.1.)
4. **No authentication and no TLS**, as the trusted-LAN decision says. Using a node (inference, search, the web UI) is open as soon as the node is advertised. Controlling one (loading or stopping its model from another machine) is a different class of act and needs the SSH pairing (§13.2, §15.1).
5. **A GB10 gets both halves; every other machine gets the client only** (§9).
6. **Three parts, in order.** Part 1 is the split with one node (§3–§11). Part 2 is several nodes (§12). Part 3 is running jobs on another node (§13). Neither later part delays the first.
7. **Nodes have no roles.** Every node installs identically; there is no "primary" or "secondary" setting and no question at install time. The machine a person runs `ragon-admin node …` on is the one doing the managing (§12.4).

---

## 1. Goals and non-goals

**Goals**
- A person installs Ragon on a laptop, types `ragon`, and is talking to the model on the GB10 in the next room, with no address typed and no file edited.
- The same for `ragon-app` (the desktop window on the web UI) and for web search.
- The client runs on Ubuntu, macOS and Windows.
- On the GB10 itself nothing changes for the user: client and node are both installed and the client uses the local node.

**Non-goals**
- Access from outside the local network. Discovery is link-local by construction and nothing here crosses a router.
- Accounts, API keys, per-user quotas or TLS.
- A node on anything but a GB10. A Linux machine with another GPU gets the client only in this version.
- Night Shift, SWE-bench or the egress audit run from a remote client (§10).

---

## 2. What was checked, and what was not

Checked on this GB10 (`gx10-1a2b`, Wi-Fi address `192.0.2.10`):

| Question | Result |
|---|---|
| Is an mDNS responder already running on the node? | Yes: `avahi-daemon` 0.8 is active and enabled; `/etc/avahi/services/` holds only `ssh.service`. |
| Does the service type work? | `avahi-publish -s ragon-spec-probe _ragon-node._tcp 8000 proto=1 version=0.9.0 node=… web=3000 search=8888`, run as an ordinary user, was browsed back with `avahi-browse -rtp`: it resolved to `gx10-1a2b.local`, `192.0.2.10`, port 8000, with all five TXT records. |
| Which interfaces does Avahi advertise on? | All of them: the Wi-Fi interface, loopback, both Docker bridges and every container veth. mDNS is per link, so LAN clients see only the Wi-Fi record. |
| Is the model server already reachable from the LAN? | Yes: `http://192.0.2.10:8000/v1/models` answers 200 (it binds `0.0.0.0:8000`, accepted by design). |
| What else listens beyond loopback? | Nothing of Ragon's. The web UI (3000, 80), SearXNG (8888), Gmail (8767), image search (8768), speech-to-text (8100) and the diffusion sidecar (8001; switched off) are all on `127.0.0.1`. |
| Does the web UI accept a non-localhost `Host`? | Yes: `/app` answers 200 for `Host: 192.0.2.10:3000` and `Host: gx10-1a2b.local:3000` as for `localhost:3000`. |
| Do the model containers come back after a reboot with nobody logged in? | Their restart policy is `unless-stopped` (model server, SearXNG, nginx). The user's systemd lingering is **off**, so a user unit would not run before login. (`install.sh` turns lingering on for a node, SETUP §3.2.1.) |
| Can Codex be built for the client platforms? | Upstream's release workflows build `aarch64`/`x86_64` for `apple-darwin` and `unknown-linux-musl`, and `x86_64`/`aarch64-pc-windows-msvc` in a separate Windows workflow. The pinned V8 manifests (`third_party/v8/rusty_v8_150_4_0_release_manifests.sha256`) list pointer-compression-and-sandbox builds for all of them plus `linux-gnu`. Read from the submodule; **never built here**. |
| Do the index tools exist for the client platforms? | codebase-memory-mcp v0.11.0 ships `darwin-amd64`, `darwin-arm64`, `linux-amd64` and `linux-arm64`; a Windows asset was seen only for its UI build. The scip CLI v0.10.0 ships `darwin` and `linux`, both architectures, and **no Windows build**. |

Read on the web, not tested here:
- **A page served from `http://<LAN address>` is not a secure context**, so `navigator.mediaDevices` is undefined and the microphone button cannot work; `http://localhost` is exempt (MDN).
- **macOS 15 and later gate local-network access per application.** A bundled app must declare `NSLocalNetworkUsageDescription` and `NSBonjourServices` or its multicast and unicast LAN traffic fails silently with no prompt (Apple TN3179, as reported by several projects).
- **`mdns-sd`** is a pure-Rust mDNS/DNS-SD implementation for Linux, macOS and Windows with no async runtime.

Not checked, and so the first work of Part 1 (Phase 0 in §14):
- any build of `ragon`, `ragon-code` or `ragon-app` on macOS or Windows;
- whether Avahi publishes a file in `/etc/avahi/services/` that the node's user, not root, owns (§5.2);
- whether a command-line `ragon` started from Terminal on macOS inherits Terminal's local-network grant;
- `mdns-sd` browsing beside Avahi, Bonjour and Windows' own resolver;
- SearXNG answering JSON to a non-loopback client (its bot limiter may treat LAN addresses differently);
- the web UI's streaming and voice traffic through a loopback proxy.

---

## 3. The two halves

| Component | Today | Client | Node |
|---|---|---|---|
| `ragon` (terminal agent) and `codex-code-mode-host` | Rust; released for Linux (arm64, x86-64), macOS and, as a preview, Windows | ✔ | ✔ (a GB10 has both halves) |
| `ragon-search`, `ragon-fetch` | Rust | ✔ | ✔ |
| `ragon-code` (code index) and its pinned tools | Rust, tools installed by `ragon-admin code setup` | ✔, tools installed by `ragon-code setup` (§8.3) | ✔ |
| `ragon-app` (desktop app) | Electron (Tauri before): a Linux `.deb`, a Mac `.dmg` as an unsigned preview | ✔ | ✔ |
| `ragon web` (the Ragon UI, Ask and Work) | Rust, in `ragon` | ✔ | ✔ |
| Model server (SGLang or vLLM), diffusion sidecar (switched off) | Docker | | ✔ |
| SearXNG, speech-to-text, image search, Gmail service | Docker sidecars | | ✔ |
| `ragon-admin` (Python): `server`, `model`, `ragon`, `night`, `swe-bench`, `audit`, `mcp`, … | Python package | | ✔ |
| Host safety, PSI watchdog | Python | | ✔ |

**Where the user's code lives.** On the client. The agent's shell commands, its sandbox, the git repository and the code index all run on the client machine; only prompts and completions cross the network. That is why indexing is a client component and why Night Shift is not (§10).

**Web access is split.** `ragon-search` asks the node's SearXNG, so search queries leave for the internet from the node. `ragon-fetch` fetches pages itself, from the client. `/airgapped` levels are resolved on the client, as now.

---

## 4. What leaves loopback on the node

A node that is not advertised keeps today's binds. `ragon-admin node enable` (§5.2) is what changes them, and `node disable` puts them back; `--no-web` keeps the web UI on loopback. The advertisement's `web` key names `ragon web`'s port 3100.

| Service | Bind today | After `node enable` | Who needs it |
|---|---|---|---|
| Model server, 8000 | `0.0.0.0` | unchanged | `ragon` on every client |
| `ragon web`, 3100 | `127.0.0.1` | every interface (`ragon web start --lan`, a user unit `ragon-web.service`); a device is served only once paired (`ragon web pair`) | a phone or another computer's browser; the messengers' bridges |
| SearXNG, 8888 | `127.0.0.1` | `0.0.0.0` | `ragon-search` on clients |
| Google service (Gmail, Drive, Calendar), 8767 | `127.0.0.1` | unchanged | node only (§10) |
| Diffusion sidecar, 8001 (switched off) | `127.0.0.1` | unchanged | node only |
| Speech-to-text 8100, image search 8768 | `127.0.0.1` | unchanged | `ragon web`, the app and the `image_search` tool on the node (on `dreamference-sidecars`) |

**This reverses two fixes**, deliberately:
- nginx had been published on every interface, which put the web UI's default admin account on the LAN; `configure()` bound it to loopback.
- SearXNG was loopback-only.

Under the trusted-LAN decision both become "accepted by design" on an advertised node. When this is implemented the two rows move to that table in [README](./README.md) with the date and this reason; they are not silently reopened. Mechanically: `HOST_PORT=0.0.0.0:3000` in the web UI's `.env` (the variable `bind_to_loopback()` already writes) and the SearXNG sidecar's publish address, both keyed on one config value, `node_advertise`. *As built, the value is `~/.config/dreamference/node-advertise.json` (§18.2). Now there is no `.env` to write: `NodeAdvertiser.apply_binds()` starts `ragon web start --lan` as the user unit `ragon-web.service` (`serve_web`) and restarts SearXNG on its publish address (`dreamference/node/node_advertiser.py`).*

**What sharing the web UI exposes, stated plainly** (§11 has the rest; `ragon web` serves a device only once paired, RAGON_ASK §4.3):
- **The web UI's Gmail tool reads the node owner's mail.** Anyone on the LAN using the web UI can ask it to search that mail. `ragon-admin node enable --no-web` leaves the web UI on loopback for an owner who wants remote `ragon` but not a shared web UI (question 2).

---

## 5. Discovery

### 5.1 The service

- **Type:** `_ragon-node._tcp` (the name is 11 characters; DNS-SD allows 15).
- **Instance name:** the host name (`gx10-1a2b`).
- **Port (SRV):** the model server's, 8000.
- **TXT records:**

| Key | Example | Meaning |
|---|---|---|
| `proto` | `1` | Version of this contract. A client refuses a node whose `proto` is higher than it knows. |
| `node` | `7c1e…` | The node's stable id, a UUID written once to `~/.config/dreamference/node-id`. It, not the address or the name, is what a client remembers. |
| `version` | `0.9.0` | Ragon's version on the node, for the skew notice (§6.5). |
| `web` | `3100` | Port of the web UI (`ragon web`, `WEB_PORT` in `node_advertiser.py`); absent when it is not shared or `ragon` is not installed. |
| `search` | `8888` | Port of SearXNG; absent when it is not shared. |
| `state` | `ready` | `stopped`, `loading` or `ready`. Written by `server start` when it launches the model (`loading`) and when `/v1/models` first answers (`ready`), and by `server stop`. It changes once per launch, not per request. |
| `main` | `1` | Present when the model assigned to the node is a chat model a coding client can use: any matrix entry that is not a diffusion, speech or embedding model. It is a property of the matrix entry, so nobody sets it. Used only to choose between several nodes (§12.3). |
| ~~`control`~~ | | Not advertised. The open control agent was dropped (§15.1): control between nodes goes over the SSH pairing, which needs no advertised port. |

Nothing that changes from minute to minute is advertised. Which model is loaded and its context length are asked of the model server itself (`/v1/models`), which the launcher already does. `state` is advertised because the server cannot say it: SGLang opens its port only once the model is loaded, so a refused connection looks the same for a stopped node and a loading one.

### 5.2 The node side: a static Avahi service file

`ragon-admin node enable [--no-web] [--no-forge] [--yes]` writes `/etc/avahi/services/ragon-node.service` (Avahi watches that directory and publishes the file without a restart), sets `node_advertise`, re-publishes the web UI and SearXNG on every interface, and sets up the node's Forgejo forge unless `--no-forge` ([RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md)). `node disable` removes the file and restores the loopback binds. `node status` prints the file's records, the binds, and what a browse of the local network returns.

- **Why a file and not a process.** A file needs no running publisher, so the node is advertised after a reboot with nobody logged in, like the model containers. A user unit holding the registration would need lingering, which is off here (§2).
- **Why Avahi and not a responder of our own.** The node already runs one. A second responder on the same host competes for port 5353 and for the host name.
- **It needs root once**, to create the file under `/etc/avahi` and hand its ownership to the node's user. The command is printed before it runs and `sudo` prompts on the terminal, the rule `ragon-admin desktop install` set. Where `sudo` cannot prompt, the file's content and destination are printed instead.
- **Later changes need no root.** The file holds the model port, the version and the `state` and `main` records. `server start`, `server stop` and an update rewrite it, as the user who owns it. That Avahi publishes a service file not owned by root is assumed, and is a Phase 0 check; if it does not, those commands say that `node enable` must be run again instead.
- **Interfaces.** Avahi advertises on the Docker bridges too (§2). That is harmless and is left alone: changing `allow-interfaces` would edit a system file other software reads.

### 5.3 The client side: `mdns-sd`

One code path on all three systems: the pure-Rust `mdns-sd` crate browses `_ragon-node._tcp.local.` for up to 2 s (up to 4.5 s when it looks for a remembered or named node: `WANTED_TIMEOUT` in `ragon-rs/src/node.rs`) and stops at the first answer when it is looking for one remembered node.

- **Connect to the resolved address, never the `.local` name.** The browse answer carries the addresses. Resolving `gx10-1a2b.local` through the operating system depends on `nss-mdns` on Linux and is unreliable on Windows.
- **IPv4 first**, then IPv6 with its scope id; link-local IPv6 without a scope is dropped.
- **Only the launcher and `ragon-app` browse.** `ragon-search` and `ragon-code` read the file the launcher wrote (§6.2), so they need no multicast and work inside Codex's sandbox.
- **The alternative, the system's own DNS-SD** (Bonjour on macOS, the Win32 API, Avahi over D-Bus), is three code paths. It is kept in reserve for the case Phase 1 finds `mdns-sd` blocked where the native API is not, which is plausible on macOS (§8.2).

### 5.4 Rejected: a front-door proxy

A `ragon-node` daemon that advertises itself, serves a descriptor and reverse-proxies every service on one port would keep the sidecars on loopback and give one place for version and state. It was not chosen because it puts a new always-on process in the path of every token, needs supervising and restarting like the model server, and buys nothing the TXT records and the services' own ports do not already give. If per-client accounting or access rules are ever wanted, that is the place they would go.

---

## 6. Connecting

### 6.1 Order of resolution

`vllm_host()` in the launcher gains tiers. The first that yields a host wins:

1. `DREAMFERENCE_VLLM_HOST`.
2. `vllm_host` in `DREAMFERENCE_CONFIG_PATH`, `./dreamference.toml` or `~/.config/dreamference/config.toml`. A repository's `./dreamference.toml` may name no server, not even one on this machine; `vllm_host` there is ignored, and the user's file decides ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md)).

   *As built, `RAGON_NODE=<name>` comes between tiers 2 and 3 (one node, for one command; §18.2), and the launcher's own list (`ragon-rs/src/node.rs`) numbers tiers 1 and 2 as one.*
3. **This machine is a node** (`~/.config/dreamference/node-id` exists): `http://localhost:8000`, with no discovery at all. A GB10 never browses for itself.
4. **The remembered node** (`$CODEX_HOME/node.json`): a browse looks for its `node` id and stops at the first answer, typically well under a second, and the address in that answer is used. This is what survives a DHCP address change. Only when the browse returns **nothing at all** is the remembered address tried, with one line saying so. The order matters: the model server's own answers carry no node id, so trying the old address first would connect a laptop that has moved to another network to whatever answers there on port 8000.
5. **A browse.** One node found: it is used and remembered, with one line saying so. Several: §6.3. None: §6.4.

Tiers 1 and 2 are today's behaviour and still mean "I know where the server is". On the GB10 the checked-in `dreamference.toml` already names `localhost:8000`, so tier 2 answers there and nothing changes.

### 6.2 `node.json`

`$CODEX_HOME/node.json` (`~/.ragon/node.json`), written whole and atomically by the launcher:

```json
{ "node": "7c1e…", "name": "gx10-1a2b", "address": "192.0.2.10",
  "model_port": 8000, "web_port": 3100, "search_port": 8888,
  "version": "0.9.0", "last_seen": "2020-10-01T22:10:04+01:00" }
```

(`web_port` is the advert's `web` record: 3100, `ragon web`.)

- A small std-only crate, `ragon-rs/node-locator/`, reads it and returns the endpoints. The launcher, `ragon-search`, `ragon-code` and `ragon-app` share it, the way `/airgapped`'s resolver is shared, with the same byte-identical-copy test for the web crate. *(The app's copies, Tauri's `node_locator.rs` and then Electron's `node_locator.ts`, were removed: the app's bundled `ragon` resolves the node itself. Two copies remain, compared by `test_the_web_crates_locator_is_a_byte_identical_copy`.)*
- `ragon-search` uses `http://<address>:<search_port>` when `DREAMFERENCE_SEARXNG_URL` is unset, the machine is not a node and the file exists; otherwise today's `127.0.0.1:8888`.
- **The remembered address is the fallback for blocked multicast.** Where a browse silently returns nothing (a guest Wi-Fi with client isolation, macOS without the local-network grant), a node used once, or set by hand, keeps working by address (tier 4).
- **The file is the agent's to write when a session's working directory is the home folder**, as `/airgapped`'s level files are: the workspace-write sandbox allows writes under the working directory. The "never adopted silently" rule of §6.3 is launcher logic that such a session could get round by rewriting the file. The check that `$CODEX_HOME` does not lie under a writable root, which the `/airgapped` spec lists as not yet built, covers both.

### 6.3 `ragon node`

A launcher subcommand, intercepted in `prepare_args` the way `night` is, so it costs no patch:

| Command | Does |
|---|---|
| `ragon node` or `ragon node list` | Browses and lists every node: name, address, served model and context (asked of each), version, and which one is in use |
| `ragon node list --json` | The same browse as one line of JSON, for the desktop app: each node's name, id, the name to choose it by, served model and context, state, version and whether it is in use; whether this machine is a client, a node or configured; the remembered node when it did not answer. No address, URL or configured host is in it |
| `ragon node use <name\|address>` | Remembers that node. An address works with no mDNS at all |
| `ragon node forget` | Deletes `node.json`; the next start browses again |

~~There is no `/node` slash command: the patch series stands at 31,175 of 31,500 bytes.~~ *Built: patch `0025-node-slash-command` adds `/node`, which runs `ragon-admin node …` (`provision`, `add`, `list`, `status`, `remove`) in the terminal, outside the sandbox (§18.9). It is the node-management command, not this one: `use` and `forget` stay `ragon node`'s.*

**Settings › Node in the desktop app** (`desktop/electron/src/nodes.ts`) lists `ragon node list --json` when the page opens or on Refresh, never at start-up, marks the node in use, and Use runs `ragon node use <name>` (the id for a node advertising no name) in the main process, which accepts only a choice the last list offered and nothing that reads as an address. The remembered node keeps its last address as the fallback after a failed browse (§6.2), unchanged. The app's own `ragon app-server` resolved its node when it started, so the page offers Restart Ragon. On a node or with a configured host the page shows the list and offers no choice.

**Several nodes and none remembered:** an interactive `ragon` lists them and asks which; `ragon exec` and other non-interactive commands refuse with the list and the `ragon node use` line. The client does not guess.

**The remembered node is gone and another is present:** the same rule. Prompts and source code go to whichever node is chosen, so a new one is never adopted silently.

### 6.4 Messages, not a ten-minute wait

Today the launcher prints dots for up to 600 s when the server does not answer. With discovery there are three distinct cases, each with its own text:

| Case | What the client says |
|---|---|
| No node found, none remembered | `No Ragon node found on this network.` then: start one on a GB10 (`ragon-admin server start`, `ragon-admin node enable`), or `ragon node use <address>` |
| Node found, `state=stopped` | `Node gx10-1a2b found, but its model server is not running. On the node: ragon-admin server start` and no wait |
| Node found, `state=loading` | `Node gx10-1a2b: model loading` and today's dotted wait |
| Node found, `state=ready`, port refuses | The advert is stale (the server died without `server stop`): `Node <name> is advertised as ready, but its model server at <url> does not answer. On the node: ragon-admin status (it may still be loading after a restart)` |

Added: when a browse misses (no node found and none remembered; the remembered node did not answer and its last address is tried; `RAGON_NODE` names a node that did not answer) and the client is Linux with a connected Wi-Fi connection that saves power, one more line follows: `Hint: Wi-Fi power saving is on for <interface> (<connection>), and it loses most of the multicast a node is found by. Turn it off: sudo nmcli connection modify <connection> 802-11-wireless.powersave 2 && sudo iw dev <interface> set power_save off`. A hint, never a failure; `ragon` runs none of it. A remembered node that did not answer while other nodes did (the question of §6.2) gets no hint.

### 6.5 Version skew

- `proto` higher than the client knows: refuse, and name `ragon update`.
- `version` differs: one line, at most once a day, naming both versions. Nothing is blocked; the model API is the contract and it does not move with Ragon's version.

### 6.6 Every hard-coded address, and what it becomes

From the sources' outlines and a search; Phase 1 starts by repeating the search over the whole tree, since a constant missed here is a feature that silently talks to nothing on a client.

| Where | Today | Becomes |
|---|---|---|
| `ragon-rs/src/lib.rs` `DEFAULT_VLLM_HOST` | `http://localhost:8000` | the last tier only; the locator's address before it (§6.1) |
| `ragon-rs/src/lib.rs` `GMAIL_SERVICE_URL` | `http://127.0.0.1:8767` | unchanged, and asked only when this machine is a node (§10) |
| `ragon-rs/src/lib.rs` `OFFLINE_CHATGPT_BASE_URL` | `http://127.0.0.1:9/…` | unchanged: it is a deliberate dead end |
| `desktop/src-tauri/tauri.conf.json` window `url` | `http://localhost:3000/app` | unchanged, for the same reason. *Replaced by the Electron app in `desktop/electron/`, which loads no node address* |
| `ragon-web-rs/src/search.rs` `DEFAULT_SEARXNG_URL` | `http://127.0.0.1:8888` | the locator's address when not a node (§6.2) |
| `ragon-web-rs/src/search.rs` `SEARXNG_START_HINT` | `ragon-admin searxng start` | on a client: "on the node: …" |
| `ragon-code-rs/src/index/probe.rs` | asks the served model's `/metrics` whether it is busy, to freeze indexers | skipped when this machine is not a node: a remote model's load is no reason to pause a laptop's indexer (§8.3) |
| `ragon-rs/src/usage.rs` (`/usage`) | to be read in Phase 1: whether it asks the server or only local session files | the locator's address if it asks the server |

### 6.7 What the launcher writes

Unchanged in kind: the catalog and `config.toml` in `$CODEX_HOME`, with the provider's base URL now the node's address. `check_for_update_on_startup = false` and the `chatgpt_base_url` blackhole stay. The Gmail prompt block is added only when this machine is a node (§10).

---

## 7. `ragon-app` on a client

> **Superseded.** This section describes the Tauri `ragon-app`, which the Electron app replaced ([RAGON_DESKTOP_ELECTRON](./DREAMFERENCE_RAGON_DESKTOP_ELECTRON.md)); `desktop/src-tauri/`, its forwarder and its copy of the node locator no longer exist, and neither do the Electron ports of them (`forwarder.ts`, `discover.ts`, `node_locator.ts`, `sign-in.ts`, removed). The Electron app reaches no web UI on the node and signs in to nothing: Ask and Work are two views of one window on the app's own `ragon app-server`, started from the bundled `ragon`, whose launcher resolves the node's model server (§6.1; `ragon-rs/src/app.rs`, RAGON_ASK §18.6). The macOS bundle's local-network keys are in `desktop/electron/forge.config.js` (`extendInfo`), for that `ragon`'s browse. The node's advertisement names `ragon web`'s port 3100. Kept as the record of the Tauri design.


**Chosen design: a loopback forwarder inside the app.** On a machine that is not a node, `ragon-app` resolves the node (§6.1, with its own browse), binds `127.0.0.1:3000` and forwards every request, streamed response and WebSocket upgrade to `<address>:<web_port>`. The window keeps loading `http://localhost:3000/app`.

- **The origin stays `localhost`**, which is a secure context, so the microphone works; a window on `http://192.0.2.10:3000` would have no `navigator.mediaDevices` (§2).
- **Nothing about the web UI changes**: cookies, the Google sign-in redirect and every patch `configure` applies see the address they see today.
- **The `Host` header is passed through unchanged** (`localhost:3000`), so redirects and any absolute address the UI emits come back pointing at the forwarder. The web UI answers the same for any `Host` (§2), so nothing on the node has to know.
- **If port 3000 is taken** on the client, the app uses 33000 and says so in its title bar once; the saved session is per origin, so the port must not change from run to run.
- **On a node** the app does what it does now and starts no forwarder.
- **No node:** the app shows §6.4's first message in its own window, in place of a connection error.

**Signing in.** The web UI asks for its account. With no session, the app posts the default credentials to `/api/auth/login` once; if the node's owner changed the password, the normal login page appears. This keeps "no authentication required" true for the default install without removing the owner's ability to set a password.

**Rejected:**
- **HTTPS with a self-signed certificate on the node:** a trust prompt, or a certificate to distribute, on three operating systems.
- **Marking the origin as secure by a browser flag:** exists only in Chromium, and the app's engines are WebKitGTK, WKWebView and WebView2.

**A plain browser** on a client can open `http://<node>:3000` and use everything except the microphone.

---

## 8. The client on three systems

### 8.1 Builds

*As built: the x86-64 Linux and both macOS targets (`.github/workflows/build-clients.yml`, §8.4); `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` as an unsigned preview (`.github/workflows/windows.yml`, `install.ps1`, RAGON_WINDOWS_ARM). `ragon-app` is Electron (Forge's `.deb` and `.zip` makers, and an unsigned `.dmg` preview from `.github/workflows/mac-preview.yml`), not the AppImage and `.msi` below.*

| Target | Agent and web commands | `ragon-code` | `ragon-app` |
|---|---|---|---|
| `aarch64-unknown-linux-gnu` | today | today | `.deb`, AppImage (today) |
| `x86_64-unknown-linux-gnu` | new | new | `.deb`, AppImage |
| `aarch64-apple-darwin` | new | new | `.dmg` |
| `x86_64-apple-darwin` | new, if Intel Macs are wanted (question 5) | new | `.dmg` |
| `x86_64-pc-windows-msvc` | new | universal layer only, if at all (§8.3) | `.msi` |

- **The builder runs on each platform's CI runner.** `CodexBrandedBuilder` is Python and uses `git archive`, `git apply` and Cargo, none of them Linux-only; `fetch_rusty_v8()` already names the asset by target. Python is needed to *build* the client, never to run it.
- **Windows ships more than one executable.** Upstream's Windows workflow builds `codex-windows-sandbox-setup`, `codex-windows-sandbox-service` and `codex-command-runner` beside `codex`; the Windows package carries them next to `ragon.exe`.
- **Known Linux assumptions to remove:** `ragon update` returns early unless `target_os = "linux"`; `ragon app` finds the window through a `.desktop` entry. (Vendored OpenSSL is already scoped to glibc Linux in the launcher's manifest, so macOS and Windows build against what upstream uses there.)
- **`$CODEX_HOME`** is `~/.ragon` everywhere (`%USERPROFILE%\.ragon` on Windows).

### 8.2 What each system changes

| | Ubuntu | macOS | Windows |
|---|---|---|---|
| Command sandbox | bubblewrap, as on the node | Seatbelt (Codex's own) | Codex's Windows sandbox |
| `/airgapped on` enforced by the kernel | yes (patch `0019` hooks the Linux sandbox helper) | **yes, for a level set before `ragon` starts** (Seatbelt; §8.4); a mid-session switch is refused | **no: cooperative only** (but see RAGON_WINDOWS_ARM §7) |
| Discovery | `mdns-sd` | `mdns-sd`; local-network permission applies (below) | `mdns-sd`; Windows Firewall may ask once for UDP 5353 |
| Installer | `.deb` / script | script, `.dmg` for the app | PowerShell script, `.msi` for the app |

- **`/airgapped` off Linux.** The `on` level's guarantee comes from three lines in `linux-sandbox`. On macOS and Windows the level still switches off `ragon-search` and `ragon-fetch` and still tells the model, but a command can reach the network. `/airgapped` must print that on those systems, as it already prints its other holes. Extending the hook to Seatbelt and the Windows sandbox is that spec's work, and costs patch bytes.
- **macOS local-network permission.** `ragon-app`'s bundle declares `NSLocalNetworkUsageDescription` and `NSBonjourServices` (`_ragon-node._tcp`; in `desktop/electron/forge.config.js`, `extendInfo`), or the browse and the forwarder both fail silently (the forwarder is gone; the browse is the bundled `ragon`'s). For `ragon` in a terminal the grant belongs to the terminal application; whether that covers multicast from a child process is unverified. If it does not, macOS falls back to the system's own DNS-SD (§5.3) or to `ragon node use <address>`.
- **Unsigned binaries.** A script that downloads with `curl` sets no quarantine flag, so the command-line tools run unsigned. (Apple silicon still needs a signature to run anything; the Rust linker gives every `aarch64-apple-darwin` binary an ad-hoc one, which gzip preserves.) A `.dmg` or `.msi` opened from a browser meets Gatekeeper or SmartScreen; signing needs an Apple Developer ID and a Windows code-signing certificate (question 4).

### 8.3 Indexing on a client

`ragon-code` answers queries the same way everywhere: it reads SQLite files. What differs is running the indexers.

- **Installing the tools.** `ragon-admin code setup` is Python. `ragon-code setup` does the same in Rust: the pinned codebase-memory-mcp and scip CLI for the platform, checked against their published checksums, plus the npm indexers when Node is present. `ragon-admin code setup` on a node calls it.
- **Linux clients:** as on the node, every indexer in a `systemd-run` scope inside bubblewrap. Without a systemd user bus (a container, WSL) a request is queued and nothing starts, which is today's behaviour.
- **macOS and Windows have neither `systemd-run` nor bubblewrap.** In this version:
  - indexers that only read source run unsandboxed: codebase-memory, scip-python, scip-typescript, scip-go;
  - indexers that execute the project's build (rust-analyzer, scip-java, scip-dotnet) **do not run**, and `ragon-code status` names them as skipped and why. The reason for their sandbox, a hostile build script, does not go away on a laptop.
- **Windows:** the scip CLI has no Windows build, so there is no exact layer; the universal layer depends on a Windows build of codebase-memory-mcp, which was not confirmed (§2). Until it is, `ragon-code` on Windows answers by text search only and says so.
- **The memory budget changes meaning.** On a node it protects the model server. On a client with no model server it is a plain cap (a quarter of RAM), and the freeze-while-the-model-is-busy rule does not apply.
- **`ragon-code session`** is started by the launcher and uses the systemd user bus; off Linux it runs the static indexers as ordinary child processes at low priority.

### 8.4 As built for x86-64 Linux and macOS (branch `clients/x86-macos`)

- **Builds.** `.github/workflows/build-clients.yml` builds `ragon`, `codex-code-mode-host`, `ragon-search`, `ragon-fetch` and `ragon-code` for `x86_64-unknown-linux-gnu` (`ubuntu-24.04`), `aarch64-apple-darwin` (`macos-15`) and `x86_64-apple-darwin` (`macos-15-intel`), each natively on its own runner with the same `CodexBrandedBuilder.build()` the node uses; only the builder's own imports are installed, not the node's Python dependencies. `host_target()` now names the Mac targets (`uname -m` says `arm64` where Rust and the V8 assets say `aarch64`), and the submodule's V8 manifest already pins a prebuilt V8 for all three. It runs on pushes to `clients/**`, by hand, and from `release.yml` behind `build_clients` (default off), whose artifacts are `ragon-client-<target>`. `scripts/package_ragon.sh` names the assets as `install.sh` and `ragon update` read them. `ragon-app` is not built for these targets yet (§7, the `.dmg`).
- **`ragon update`** names its target by OS (`update::target_for`) and runs on Linux and macOS; elsewhere it says which builds exist.
- **`install.sh`** installs the client on all four targets and refuses `--role node` on macOS before downloading anything. It needs nothing newer than macOS's bash 3.2.
- **`/airgapped on` on macOS is enforced, with the level fixed at launch.** Codex's Seatbelt profile takes its network rule from `sandbox_workspace_write.network_access`, read once when the session starts. At a configured `on` (environment, either configuration file) the launcher adds `-c sandbox_workspace_write.network_access=false` (`airgapped::with_launch_policy`), so every workspace-write command is denied the network by Seatbelt; the read-only sandbox is given the network only below `on` (patch `0031`, AIRGAPPED §14.11), and Full Access stays refused at `on`. A user `-c` of the same key at `on` is refused rather than silently overridden. Inside a running session `/airgapped on|off` changes nothing on macOS and says so, with the command that does (`ragon airgapped default <level>`, then restart); the status names Seatbelt instead of bwrap. No patch was needed. Linux is unchanged.
- **Verified in CI:** see the run recorded with this branch's report (each target built, the web commands' tests, `--version` and `--help` of every binary, and on both Macs a sandboxed `curl` that reaches the network with `network_access=true` and is blocked with `false`).
- **Verified only on real machines, still to do:** the client against a GB10 node over a real network (discovery through `mdns-sd` under macOS's local-network permission, which belongs to the terminal app, §8.2); a whole `ragon` session at a configured `on` on a Mac (the CI check exercises Seatbelt's rule directly through `ragon sandbox`, not a session); `ragon update` replacing itself on a Mac; `ragon-code` indexing on a Mac (§8.3).

---

## 9. Installing

One entry point per system, and the machine decides the role:

| Machine | Installs | How it is decided |
|---|---|---|
| GB10 (DGX Spark) | node **and** client, then `ragon-admin node enable` | a shell check in the installer, which runs before any Python exists on a fresh machine: `uname -m` is `aarch64` and the GPU name from `nvidia-smi` (or the board model under `/proc/device-tree`) names the GB10. The Python detection in `dreamference/hardware/` confirms it during `node enable` |
| Any other Linux, macOS, Windows | client only | everything else |

- **`install.sh`** (Linux and macOS) and **`install.ps1`** (Windows) download the release assets for the machine's target, verify them against the release's checksum file and place them in `~/.local/share/dreamference/ragon/bin` (or the platform's equivalent) with links on `PATH`. This is the path `ragon update` already implements for linux-arm64; `asset_names()` grows `ragon-code`, and the release workflow grows the matrix of §8.1.
- **On a GB10** the script then does today's node install (the Python package, `ragon-admin`, the model) and runs `node enable`, which is where `sudo` is asked for, once and visibly.
- **`--role client|node|both`** overrides the detection, for a GB10 that should only be a client of another, and for tests. `--role both` is accepted for older instructions and treated as `node`.
- **A node without the client is not offered.** The node's own tools (`night run`, `swe-bench`, `audit egress`) run `ragon`.
- **Updating.** `ragon update` on a client updates the client. On a node it updates the client binaries as now; the Python half is updated as it is today.

---

## 10. What works from a remote client

| Feature | Remote client | Why, when not |
|---|---|---|
| `ragon`, `ragon exec`, resume, `/cavemode` *(removed)*, `/usage` | yes | |
| `ragon-search` | yes, through the node's SearXNG | |
| `ragon-fetch` | yes, from the client | |
| `ragon-code` | yes, with the per-system limits of §8.3 | |
| `ragon-app`, including voice | yes; voice on a node only | forwarder (§7), removed: Ask and Work run on the client's own `ragon app-server` against the node's model server. The microphone sends to the speech-to-text service on the same machine's loopback (`SPEECH_URL` in `desktop/electron/src/voice.ts`), and a client runs no Docker sidecars |
| `/airgapped` | yes; `on` is kernel-enforced on Linux only | §8.2 |
| Gmail in `ragon` (`ragon-admin gmail`) | **no** | The command is Python, and the service's shared secret is a file on the node. The launcher adds the Gmail block to the prompt only on a node; a remote client never sees a command it cannot run |
| `/night` and Night Shift | **no** | The queue is on the machine where `/night add` ran, and the runner is `ragon-admin` on the node. On a client `/night` answers that Night Shift runs on the node, in place of queueing tasks nothing will run. Part 3's follow-up (§13.3) is what would change this |
| `ragon-admin` anything (`server`, `model`, `swe-bench`, `audit egress`, `mcp`) | **no** | Node only. Managing a node is done from a node (§12.4) |
| Cline, Continue, OpenHands | by hand | `ragon node list` prints the model URL to paste into them |

These are scoped out of Part 1, not designed around. The one that will be asked for first is Night Shift for a repository on a laptop; sending the task to the node as a job (§13) is the route to it.

**Several clients share one node's cache.** The model server's KV pool holds about 157K tokens for all sessions together, while each session is told the context is 262K ([RAGON_COMPACTION](./DREAMFERENCE_RAGON_COMPACTION.md)). With one user that gap was theoretical. Lowering the compaction limit to follow the pool, which that spec proposes, should land before more than one person uses a node.

---

## 11. Threat model

Stated so the trade is visible, not to reopen it:

- **Anyone on the local network can use the node**: send prompts to the model, search through its SearXNG, and (unless `--no-web`) use the web UI with its one account, its chat history and its Gmail tool.
- **Nothing is encrypted.** Prompts, source code in them and completions cross the LAN in clear text.
- **mDNS can be spoofed.** A device on the LAN that advertises `_ragon-node._tcp` would be offered as a node, and a client that chose it would send it prompts and code. The guard is the rule of §6.3: a node is remembered by id, and a different one is never adopted without the user choosing it. That is protection against accidents, not against an attacker on the LAN, who by the trusted-LAN assumption is not there.
- **Nothing new reaches the internet.** Discovery is multicast on the local link, and a client's traffic goes to the node's LAN address. The egress audit runs only on a node and names the model server to `ragon` itself (`DREAMFERENCE_VLLM_HOST`, tier 1), so the launcher never browses during it and its allow-list does not change. That matters: a browse is a multicast DNS query, which the audit would count as a failure. The check that matters is that it still passes with `node enable` on (§16). If the audit is ever run from a client, the node's address and its three ports join the allow-list.
- **A node that should not be shared is not advertised.** Without `node enable` every bind stays as it is today.
- **Paired clients are not isolated from one another.** Every paired key reaches the same queue: any paired client can list, read, cancel and fetch the output of another client's night tasks (`night_shift_remote.py`, which records `sender` and never compares it) and jobs (`job-list`, `job-out`, `job-cancel` and `job-logs` check no key; a job's `sender` is what the client declares, `node_job.py`). Pairing is for the owner's own machines, so this is the trust model, not a slip; isolation is planned for 1.0 (§15.3). Found by the node security read (F6).
- **Any paired client can switch a configured power plug off.** The forced command's `power-plug <plug name> off` (`node_power_cycle.py`, what `node power-cycle` sends to the node that reaches the plug) has no precondition: no check that the model server is stopped or that no job runs, and no owner-only key. It reaches only a plug the owner configured on that node, and switching it off is the point of the command, so it is kept as it is (owner decision, finding F7).

---

## 12. Part 2: several nodes

A second DGX Spark beside the first, managed from either. Nothing in §3–§9 depends on this section.

### 12.1 What NVIDIA documents

- **The hardware link.** Each Spark has two QSFP ports on a ConnectX-7 NIC; NVIDIA's "Connect Two Sparks" playbook links two units with one QSFP cable for a direct 200GbE connection, configures the interfaces with netplan and sets up passwordless SSH between the nodes (read from the playbook's summary page; its step-by-step text was not retrieved).
- **Splitting a model** across the pair is done, in NVIDIA's and the community's guides, with vLLM on a Ray backend, the weights partitioned by tensor or pipeline parallelism and activations exchanged over the QSFP link on every forward pass.
- **Measured by others** (StorageReview, two Sparks, GPT-OSS-120B, batch 128): 555 tok/s with pipeline parallelism against 252 with tensor parallelism, and the order reversed on an 8B model. Those are batched-throughput figures; they say nothing about one interactive stream.

### 12.2 Two topologies, and which comes first

| | (a) Independent nodes | (b) One model split across both |
|---|---|---|
| What each Spark runs | a whole model, by its own recipe | half of one model |
| Good for | throughput (a replica: more Night Shift and SWE-bench tasks at once); a second model (the 122B fallback, a fast-tools model, embeddings, speech) | a model that does not fit in 128 GB |
| Engine | whatever the recipe names, including SGLang with DFlash2 | vLLM with Ray; none of Ragon's recipes is written for it |
| If one Spark fails | the other keeps serving | the model is down |
| Host safety | each node's own checks and watchdog, unchanged | a load pins memory on both hosts at once, and a watchdog trip on either must stop both halves |
| Network needed | the ordinary LAN | the QSFP link |

**(a) is supported first, and is the only one this spec designs.** Every model in the matrix fits on one GB10, the default model runs on an engine that has no split mode here, and (a) reuses everything that exists. **(b) is recorded as not planned:** it becomes worth a spec of its own only for a model that needs more than 128 GB, and it would need a Ray-based recipe, a joint pre-flight and a joint kill path that do not exist.

The QSFP link is still useful under (a): copying a model's files from one Spark's cache to the other at 200GbE in place of a second download. `ragon-admin node sync-model <name> <model>` over that link is an optional extra, not a dependency.

### 12.3 Discovery and choice with more than one node

- **Every node is installed the same way and advertises itself** exactly as in §5. A browse returns all of them. No node knows about the others, and none is told it is first or second.
- **A client still uses one node per session.** `ragon node list` shows each node's model, context and current load (`/v1/models` and `/metrics`, which are already open). `ragon node use` pins one; `RAGON_NODE=<name>` picks for one command.
- **Unprompted choice**, with nothing remembered and several nodes: the one advertising `main=1` is used and remembered. Failing that, of the nodes marked, the only one not advertising `state=stopped` is used and remembered (a second node kept paired with its model server usually stopped made the desktop app refuse to start). Only when none, or more than one, qualifies does §6.3's "list and ask" apply, once, and the answer is remembered. A node advertising `state=stopped` that is used because it is the only one is not remembered. A remembered node that does not answer while the only other answers come from stopped nodes is tried at its last address, as when nothing answers: a stopped node is no choice. Two Sparks with a coding model on one and speech, embeddings or a diffusion model on the other therefore need no decision from anyone; two Sparks that both serve a coding model need one answer, once per client.
- **A session stays on its node.** The server's prefix cache is per node (a new session reuses about 11K cached prompt tokens), so moving a session between replicas throws that away. There is no load balancer; spreading is done by whatever starts many sessions.
- **Night Shift and SWE-bench** are where a replica pays: the runner computes parallelism per node from that node's KV pool and gives each task a node. That is a change to those runners, listed here and specified there.

### 12.4 Managing one node from another

**There is no primary.** "The primary" is whichever machine a person is sitting at when they type `ragon-admin node …`. It discovers the nodes by the same browse a client uses and sends commands to the one named. A role stored at install time was rejected: the question means nothing with one Spark, can be answered wrongly, has to be redone when machines are swapped, and creates states (two primaries, none) that then need handling.

`ragon-admin node`, run on any node:

| Command | Does |
|---|---|
| `node list` | every node on the network: name, assigned and loaded model, load, memory, whether it accepts control |
| `node status <name>` | that node's `ragon-admin status` |
| `node set <name> --model <key>` | assigns a model to that node and starts it there |
| `node stop <name>` / `node start <name>` | stops, or starts again, the model server there |
| `node control on\|off` | on **this** node: whether it accepts the three commands above from other machines |

- **Status needs no control channel.** Model, load and KV pool come from each node's open model port.
- **Each node keeps its own assignment.** `node set` stores the model key in that node's own config (the existing `model` key), so the node comes back with it after a reboot without the other machine being up. Nothing on the commanding machine has to be remembered, so there is no shared state to fall out of step.
- **Every load is executed by the node that carries it.** The command carries a model key and nothing else; the node runs its own `ragon-admin server start`, with its own `check_host_safety()`, its own memory pre-flight and its own PSI watchdog. Nothing sent from outside can skip them. A refusal comes back as that node's own message.
- **Only keys of the node's own model matrix are accepted**, never a repository name.

**What "trusted LAN, no authentication" means for control.** Inference open to the LAN means anyone on it can *use* the node. Control open to the LAN means anyone on it can stop the model under someone's session, start a load (the one operation that can freeze a unified-memory host, which is why host safety exists), or make a node download tens of gigabytes. That is a different class from sending a prompt, so it gets its own switch.

**Superseded (§15.1): control uses the SSH pairing of §13.2, and the switch and agent below are not built.** The text is kept for the reasoning. *Originally chosen: a per-node switch, off until the owner turns it on, and then open to the LAN.*
- `ragon-admin node control on`, typed once on the node being offered, starts a small control agent there (a user service, port 8002, advertised in the `control` TXT record) with exactly three operations: status, set-model-and-start, stop. It is the only new process in this spec, it runs only where switched on, and it is never in the path of a token.
- With it on, no key, password or pairing is needed: any machine on the LAN can run `node set` against it. That is the trusted-LAN decision applied to control, by a deliberate act at the node's own keyboard.
- With it off, which is the state of a single-Spark install, the node can be changed only from its own shell, as today.
- The agent needs lingering to run before anyone logs in; `node control on` says so, as `night enable` does.

**The stricter alternative is SSH**, the commanding machine running `ssh <node> ragon-admin …`. NVIDIA's two-Spark setup already establishes passwordless SSH between the units, it adds no listening service, and it authenticates control by key. It costs a key exchange the user has to do. It stays available without any code of Ragon's, and the spec does not build on it because the stated aim is the least setup; question 3 asks whether that trade is right.

**If topology (b) is ever built**, the same no-roles rule applies: the Ray head is the node the launch is typed on, chosen at each launch, not stored.

### 12.5 Alternatives considered

- **A container orchestrator (Docker Swarm, k3s, Nomad).** They decide where containers run and move them when a machine fails. A model server that pins most of a host's memory, takes minutes to load and must pass that host's safety checks is not something to reschedule automatically, and with two machines there is nothing to schedule. They also bring a control plane, join tokens and their own networking: more to set up than the whole of this spec.
- **exo.** Worth citing for what it gets right: devices find each other on the local network with no configuration and no designated leader, which is the experience this spec copies for discovery. Its purpose is topology (b), partitioning one model across unlike devices with its own inference engines, which is not what Ragon's recipes run on.
- **GPUStack.** The closest existing product: a server that workers register with, which then places models on them behind one standard /v1 endpoint, with vLLM among its backends. It has what this spec deliberately leaves out: a stored server/worker role, registration tokens, a scheduler and a gateway in the request path. It would replace Ragon's launch recipes and host-safety layer, not sit beside them.
- **What is used instead:** mDNS for finding nodes, each node's existing ports for using it, and a three-operation agent for managing it.

Neither exo nor GPUStack was installed or measured here; the descriptions are from their documentation.

---

## 13. Part 3: running jobs on another node

Sending work to a node: a script, or an agent task of Night Shift's kind. It comes after Parts 1 and 2 and delays neither. With one Spark here, nothing in this section could be run between two machines; §13.8 says what was checked.

### 13.1 Decisions

1. **The transport is SSH, not the open node API.** An unauthenticated "run this" endpoint would be remote code execution for every device on the network. Sending prompts to a model, and even stopping one (§12.4), are things the trusted-LAN decision can carry; running arbitrary programs as the node's user is not. SSH is already listening on the node (§13.8), and NVIDIA's two-Spark setup establishes passwordless SSH between the units anyway.
2. **The user never types an SSH command.** `ragon-admin node add <node>` sets the key up once (§13.2).
3. **The job model is Night Shift's**, pointed at another machine: a git worktree per job, a memory-capped scope, a time limit, tests, and a branch plus a report as the only output ([RAGON_NIGHT_SHIFT](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md), including its §11).
4. **Code travels by git.** The commit is pushed to the node, the job runs in a worktree there, and the resulting branch is fetched back. Nothing lands in the user's checkout until they merge.
5. **Admission and host safety are the working node's.** The sender asks; the node that would carry the load decides, with its own checks.
6. **Remote jobs do not inherit Night Shift's sandbox gap.** They run inside a sandbox on the node (§13.5), test command and plain scripts included.

### 13.2 Pairing: `ragon-admin node add`

- `ragon-admin node add <node>` finds the node by the browse of §5, creates a key used for nothing else (`~/.ssh/ragon-node_ed25519`), and installs its public half on the node. That one step needs the node's password, typed once at `ssh-copy-id`'s own prompt; there is no way to authorise a key without authenticating once. *(As built, not `ssh-copy-id`: one ordinary SSH login runs `ragon-admin node authorize` on the node, which writes the forced-command line, §18.6.)*
- **The key is restricted on the node** to one forced command, `ragon-admin node serve-job`, with no terminal, no port forwarding and no agent forwarding. It can push to the job repositories and ask for jobs; it cannot open a shell. That is what makes the caps of §13.4 mandatory: a raw `ssh <node> python train.py` with this key is refused.
- **How git gets through a forced command.** `sshd` ignores the command the client asked for and runs `serve-job`, passing the request in `SSH_ORIGINAL_COMMAND`. `serve-job` runs `git-receive-pack` or `git-upload-pack` itself when the request is one of those two for a path under the job repositories, handles its own job operations, and refuses everything else. This is the pattern gitolite uses.
- **The node's host key is pinned to its `node` id** at `node add`, so a different machine answering at the same address later is refused, not trusted.
- **The node's address is resolved at every connection, as the client resolves its node (§6.1, tier 4).** A paired record remembers the address seen last, but no connection uses it directly: a browse looks for the node's id, the address in that answer is used and the record updated. The remembered address is never connected to: a browse that does not show the node (nothing answering, or other nodes answering and this one not) is repeated for up to a minute, then the node is reported as off the network and the old address, which may be another machine's by now, is left alone. (Added after a night's lane was lost to a DHCP move: the lanes had used the recorded address. The owner's rule: never use an address, always resolve; when the network does not answer, retry for a minute, then refuse.) `node sync-model --address` is the one deliberate exception, for a direct link. (`NodePairing.resolve`. This is stricter than the launcher's tier 4, which still tries the last address when a browse returns nothing at all: a paired node never is.)
- `ragon-admin node remove <node>` deletes the key on both sides.
- **Once this exists, control (§12.4) can ride it.** A paired node could accept `node set` and `node stop` through the same restricted key, which would make the open control agent unnecessary between paired Sparks. Whether to keep the agent at all is decided when Part 3 is built (question 3).

### 13.3 What the user types

```bash
# A script, in the current repository at HEAD, on another node; output streams back.
ragon-admin node run spark-2 -- python train.py --epochs 3
ragon-admin node run spark-2 --memory 16G --time 2h --test "pytest -q" -- python train.py

ragon-admin node jobs [<node>]          # running and finished jobs
ragon-admin node logs <job>             # the output again, or from where it stopped
ragon-admin node cancel <job>
ragon-admin node fetch <job>            # bring the result branch into this repository
```

```text
/night add --on spark-2 Fix the flaky test in tests/test_sync.py
```

- **`node run`** pushes `HEAD`, runs the command in a fresh worktree of it on the node, streams the output, and at the end commits what `git status` shows on `job/<id>` and fetches that branch. With no change there is no branch, only the log and the exit code.
- **Large outputs are not committed.** What is committed is what git would track, Night Shift's rule, with the same caveat for a repository whose `.gitignore` misses something. A script that writes checkpoints or other artifacts names `--out <path>`; that folder is kept under the job's directory on the node and brought back by `node fetch` as files, never as a commit.
- **`/night add --on <node>`** records the node with the task. At night the local runner hands that task to the named node, whose own runner works it with its own `ragon exec` against **its own** model server over loopback, so two Sparks work one queue at once without sharing a model. The result branch `night/<id>` is fetched back and the morning report lists the task with the node it ran on. The flag is parsed in `ragon-rs/src/night.rs`; it needs no patch.
- **Uncommitted changes are not sent**, and the command says so, as `/night add` does.
- **Files that are not in git do not travel**: datasets, model weights, a local `.env`. A job that needs them names a path that exists on the node (§13.6).
- **The sender is a node in this version.** `ragon-admin` is Python, so a laptop cannot yet send jobs. The pieces a client would need are small (git, ssh, and the two commands above in the launcher), and adding them is what would close §10's Night Shift gap for repositories on a laptop. It is listed as the follow-up, not designed here.

### 13.4 Limits, admission and host safety

- **Every job has a memory cap and a time limit, with no way to send one without them.** Defaults are Night Shift's (`task_memory` 8 GiB, `task_timeout` 90 min); `--memory` and `--time` change them up to ceilings the *node's* config sets.
- **Admission runs on the working node**, at the moment the job would start: Night Shift's checks (model server answering if the job is an agent task, host-safety checks, free memory above the reserve plus the job's cap, no build, index run or night run in the way). A refusal comes back as that node's message. The sender's own state is irrelevant.
- **A memory cap does not bound GPU memory on a GB10.** Measured here: the model server's container reports 3.84 GiB to its cgroup while its process holds 53.8 GB of GPU memory. A script that allocates through CUDA is therefore outside its cap, on a machine where GPU and host memory are the same pool. For jobs:
  - a job is CPU-only unless sent with `--gpu`, and a CPU-only job is started with the GPU hidden (`CUDA_VISIBLE_DEVICES=` and no device nodes in its sandbox);
  - a `--gpu` job is admitted against free memory with the model server's needs counted, runs under the node's PSI watchdog as a model load does, and by default requires the node's model server to be stopped. Running a training script beside a resident model is the case host safety exists to prevent.
- **Interactive use still wins** on the working node, by Night Shift's rule.

### 13.5 Sandbox

Night Shift's known gap is that the runner's own test run executes agent-written code with the user's full rights ([RAGON_NIGHT_SHIFT §6](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md)). On one's own machine that equals running the tests oneself in the morning. On another node it would hand code written by a model, or sent from another machine, the node owner's home folder: model caches, the Gmail service's secret, SSH keys.

- **Every remote job runs inside bubblewrap on the node**, the profile `ragon-code` already uses for indexers: the system read-only, the home folder an empty tmpfs, the job's worktree and its environment directory (§13.6) bound in, and a scratch `/tmp`. This covers plain scripts, the agent's session (in addition to Codex's own sandbox) and the runner's test command.
- **Each job has its own `CODEX_HOME`**, `~/.ragon/jobs/<id>/home`, bound read-write into every `ragon exec` of that job and nothing else of the home folder. Night Shift's nudges and resumed tasks call `ragon exec resume <session>`, and the session files have to be there on the second call; a fresh tmpfs each time would lose them, and the node owner's real `~/.ragon` (their sessions, their `node.json`) must not be what the job's agent sees. The launcher rewrites the catalog and config on every start, so nothing is missing from a new one.
- **The `/airgapped` level travels with the job.** The job record carries the sender's level, the node applies the stricter of that and its own, and the result is written as the level file in the job's `CODEX_HOME`. What it means differs by kind of job:
  - **A plain script, or the runner's test command:** at `on` the sandbox has no network at all.
  - **An agent task keeps the host's network namespace**, because `ragon exec` has to reach the node's model server on loopback, which a sandbox with its own network namespace cannot. The agent's commands are confined as on any node: Codex's own sandbox runs each of them, and patch `0019` removes the network there at `on`.
- **`--gpu` adds the GPU's device nodes** to the sandbox. Whether CUDA works inside that profile is unverified.
- **The same wrapper would close the gap for local Night Shift**, which is that spec's change to make, not this one's.

### 13.6 Dependencies

A worktree on another machine has no virtualenv, and Night Shift's fallback (the main checkout's `.venv`) does not exist there.

- **Nothing is installed implicitly.** A job that needs an environment says how to build it: `--setup "<command>"`, or `[night] setup` in the repository's `dreamference.toml`.
- **The setup command runs once per repository and per content of its lock files**, inside the sandbox, with the network unless the level is `on`, and its result is kept under `~/.ragon/jobs/envs/` on the node and bound into later jobs read-only.
- **With no setup command** the job runs with the node's system interpreter, and the report says so; a test run that fails on imports is reported as an environment failure, not as a failing test.
- **Data a job needs** is named by a path on the node and bound read-only with `--bind <path>`; the node's config lists which paths may be bound.

### 13.7 When the sender disconnects

- **A job is a unit on the node, not a child of the SSH connection.** `node run` streaming its output is a view; closing the laptop lid, or losing the network, does not stop the job.
- **That holds only with lingering on.** Jobs are user units, and with lingering off the user's service manager stops when their last session ends, taking a job started over SSH with it. Lingering is off on this machine (§2). `node add` checks `loginctl show-user <user> -p Linger` on the node and, if it is off, says to run `loginctl enable-linger` there: the same dependency `night enable` and the control agent (§12.4) already name.
- **Output is kept on the node** (`~/.ragon/jobs/<id>/`), and `node logs` reads it again or continues from where the stream stopped.
- **The result waits there** as a branch in the node's copy of the repository until `node fetch`, or for a night task until the sender's runner next looks. If the sender is off in the morning, the report line appears when it next starts.
- **Only `node cancel`, the time limit, the memory cap or the node's watchdog stop a job.**
- **Finished jobs are pruned** after they are fetched, and unfetched ones after 14 days, with `node jobs` showing what is about to go.

### 13.8 Checked here, and assumed

| | |
|---|---|
| SSH is listening on the node | Yes, on every interface, port 22 |
| Passwordless SSH to it exists already | No: `ssh -o BatchMode=yes localhost` is refused (`publickey,password`), so pairing has to create it |
| The tools are present | `git`, `bwrap` and `ssh-copy-id` are installed |
| A cgroup memory cap bounds GPU memory | **No** (3.84 GiB charged against 53.8 GB held), which is why §13.4 treats GPU jobs separately |
| Everything between two machines | **Not run.** There is one Spark here. Push, remote worktree, fetch, the restricted key, a job surviving a dropped connection and two nodes working one queue are designed from Night Shift's single-machine behaviour and are the first things to test when a second node exists |
| Lingering, which jobs need to outlive the sender's connection | **Off** on this machine (`Linger=no`) |
| CUDA inside the bubblewrap profile; a restricted key carrying `git push`; Codex's own bubblewrap sandbox nested inside the job's | Assumed |

---

## 14. Phases

*As: Phase 1 built (its "done when" met on one machine only, §18.3); Phase 2's forwarder and sign-in built and removed (§7), the installers built (`install.sh`, `install.ps1`); Phase 3 built for the client builds (§8.4) but not `ragon-code setup` in Rust; Phase 4 built with SSH in place of `node control` (§15.1); Phase 5 built, not yet run between two machines (§18.10).*

| Part | Phase | Work | Done when |
|---|---|---|---|
| 1 | 0 | The unverified items of §2: build the client on macOS and Windows; `mdns-sd` beside each system's resolver; the macOS local-network grant for a terminal tool; SearXNG from a LAN address; the web UI through a forwarder | each has a measured answer recorded here |
| 1 | 1 | `node enable/disable/status`, the service file, the binds; the resolution tiers, `node.json`, `ragon node`, the messages; `ragon-search` on the locator; **Linux clients** (x86_64 and arm64) | a second Linux machine runs `ragon` and `ragon-search` against this GB10 with nothing configured |
| 1 | 2 | `ragon-app`'s forwarder and auto sign-in; the installers and role detection; the release matrix | `ragon-app` on a second machine shows the web UI, microphone included |
| 1 | 3 | macOS client, then Windows client; `ragon-code setup` and the per-system indexing rules | each system passes §16's client checks |
| 2 | 4 | Several nodes (§12): the `main` record and the choice rule, `node list/status/set/start/stop`, `node control`, runner changes in Night Shift and SWE-bench | a second Spark serves a model that was assigned from the first |
| 3 | 5 | Remote jobs (§13): `node add`, the restricted key, `node run/jobs/logs/cancel/fetch`, the job sandbox, `/night add --on` | a script and a night task sent from one Spark run on the other and come back as branches |

---

## 15. Decided, and still open

### 15.1 Decided by the user

These settle the corresponding questions below; where an earlier section says otherwise, this list wins and the section is to be brought into line when the part is built.

- **The web UI is shared with the whole LAN by default** (question 2). `ragon-admin node enable` publishes it; `--no-web` remains for a node whose owner does not want that. The spec's warning stands and is printed at `node enable`: the UI has one account and can search the node owner's mail.
- **Control between nodes uses the SSH pairing, not an open switch** (question 3). §12.4's per-node switch that lets any LAN machine set or stop a node's model is dropped. Managing a node from another (`ragon-admin node set|stop|status <node>`) goes over the pairing of §13.2, so pairing (`ragon-admin node add`) moves forward from Part 3 into Part 2. Inference stays open on the trusted LAN; only control needs the key.
- **Installers are not signed** (question 4). The first-open warning on macOS and Windows is documented with the steps to get past it; no certificates are bought.
- **Windows is supported natively** (question 6), not through WSL (first answered "WSL" and corrected the same day). The native client of §8 is built: the Windows targets, Codex's Windows sandbox executables, and mDNS discovery working as on the other systems. Its known cost stays as §8 states it: no exact code index on Windows, because the scip CLI has no Windows build, and the universal layer needs a Windows build of codebase-memory-mcp that is not confirmed; until it is, `ragon-code` on Windows answers by text search only (§8.3). `/airgapped on` is cooperative there, not kernel-enforced (§8.2). WSL is not a supported install path; nothing prevents it, and discovery there would need `ragon node use <address>`.
- **GPU jobs beside a resident model are not allowed, for now** (question 8). No override is built; a GPU job needs the node's model server stopped.

### 15.2 Still open

1. **Should a `ragon-node` executable exist**, as an alias for `ragon-admin` on a node, or is the name only the role and the advertised service?
2. *(Decided, §15.1.)* **Is the web UI meant to be shared with the whole LAN by default?** It has one account and can search the node owner's mail. The alternative default is `node enable --no-web`, with `ragon-app` on other machines switched on deliberately.
3. *(Decided, §15.1.)* **Control between nodes.** Part 2 chooses a per-node switch that, once on, lets any LAN machine set or stop that node's model with no key (§12.4). Part 3 introduces an SSH pairing for jobs (§13.2) that could carry control as well. Keep the open switch for its simplicity, or drop it once pairing exists?
4. *(Decided, §15.1.)* **Signing.** Without an Apple Developer ID and a Windows code-signing certificate, `ragon-app` installers show a warning on first open. Buy them, or document the warning?
5. **Which client targets matter?** Intel Macs and Windows on ARM each add a build and a test machine. *(Answered in part by building both: Intel Macs, §8.4; Windows on Arm as a preview, RAGON_WINDOWS_ARM.)*
6. *(Decided, §15.1.)* **Windows natively, or WSL?** Native costs the items of §8 (sandbox executables, no exact code index). Under WSL the Linux client runs unchanged, but mDNS does not cross WSL's default NAT, so discovery would be `ragon node use <address>`.
7. **Gmail from a remote client.** Out of scope here. Wanted at all, given that it would let every LAN client read one person's mail?
8. *(Decided, §15.1.)* **GPU jobs beside a resident model** (§13.4). The default refuses them unless the node's model server is stopped. Is there a case, such as a small fine-tune beside the 27B, worth an override?
9. **Sending jobs from a laptop** (§13.3). Wanted soon enough to put the sender in the Rust launcher in Part 3, or after it?
10. **Should a non-GB10 Linux machine with a capable GPU be allowed as a node** behind `--role node`? Host safety and every recipe are written for the GB10.

### 15.3 Planned for 1.0: isolation between paired clients

Decided by the owner: planned for 1.0, not built yet. Today every paired key sees every night task and job (§11). The design options from the node security read:

1. **Own the record by key.** The forced command already knows which key connected (`--key <tag>` in the `authorized_keys` line). Pass it into the night and job handlers, store it on every record as the owner, and let `list`, `out`, `logs` and `cancel` act only on the caller's own records. `sender` stays as a label and stops being trusted. This is the smallest change and the one the read recommends.
2. **An owner key besides client keys.** As 1, plus a pairing flag (`node add --owner`) for a key that sees and cancels everything, for the owner's main machine. Needs the flag recorded in the key line, not in a file the node user can write.
3. **One Unix account per client.** Each pairing gets its own account on the node and its jobs run as it, so the kernel separates them. Strongest, but it multiplies accounts, home directories and sandbox scopes, and the shared model server stays shared.

Records made before the change have no owner; they would be visible only to an owner key (option 2) or expire with the queue.

---

## 16. Tests

Offline, with no network and no real Avahi:
- **Launcher:** each resolution tier in order, with a stand-in browser; a node machine never browses; the remembered node is found again by id at a new address; several nodes and none remembered refuses in `exec`; a different node is never adopted silently; each message of §6.4; `proto` too new is refused; `ragon node list/use/forget`; `node.json` written whole.
- **Locator crate:** the web crate's copy is byte-identical; `ragon-search` picks the node's SearXNG only when the machine is not a node and no override is set.
- **`ragon-admin node`:** the service file's exact text for a given port, id, version, `main` and `control`; `enable` and `disable` change the two binds and nothing else, with `sudo` and `docker` mocked (the suite must not write `/etc` or touch a running container); staleness is reported by `server start --port` and by an update.
- **`ragon-app`:** the forwarder passes a streamed response and a WebSocket upgrade from a stand-in server; no forwarder on a node. *(Replaced by `test_ask_in_the_app_reaches_no_server_and_reads_no_password` in `tests/test_node.py`: the retired forwarder, discovery, locator and sign-in sources stay absent from `desktop/electron/src`.)*
- **Night Shift on a client:** `/night add` refuses with the node message.
- **Remote jobs (Part 3), with `ssh` and `git` replaced by stand-ins:** a job cannot be composed without a memory cap and a time limit; the `authorized_keys` line written by `node add` carries the forced command and the no-terminal, no-forwarding options; `serve-job` refuses anything that is not one of its operations; the sandbox argument list has no read-write bind outside the worktree, the job's own `CODEX_HOME` and scratch; an agent job keeps the network namespace and a script job at `on` does not; `node add` reports lingering that is off; the stricter of two `/airgapped` levels is applied; a CPU-only job's environment hides the GPU; the setup result is reused for unchanged lock files; a job record survives its stream being closed.

Live, on two machines:
- a second Linux machine with nothing configured runs `ragon exec`, `ragon-search` and `ragon-app` against this GB10; the node's address is changed and the client finds it again;
- the node's model server stopped, then loading: the client prints the matching message of §6.4;
- `ragon-admin audit egress` on the node still passes with `node enable` on;
- per client system: the build runs, discovery works, the microphone works in `ragon-app`, and `/airgapped on` prints the right statement for that system.

---

## 17. Changes to other specs when this is built

- [README](./README.md): the header line "Deployment Model: single-node, air-gapped"; the two rows of §4 move to "Accepted by design".
- [ARCHITECTURE](./DREAMFERENCE_ARCHITECTURE.md): the two halves of §3.
- [SETUP](./DREAMFERENCE_SETUP.md): the installers and roles of §9.
- [RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md): the resolution tiers, `ragon node`, the build targets.
- [RAGON_CODE_INDEX](./DREAMFERENCE_RAGON_CODE_INDEX.md): `ragon-code setup` and the per-system rules of §8.3.
- [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md): the statement for macOS and Windows; [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md): the allow-list entry for the node.
- [RAGON_NIGHT_SHIFT](./DREAMFERENCE_RAGON_NIGHT_SHIFT.md) and [RAGON_SWE_BENCH](./DREAMFERENCE_RAGON_SWE_BENCH.md): the client refusal; per-node parallelism in Part 2; `--on <node>`, the job sandbox and the setup command in Part 3.

---

## Sources

- [mdns-sd on crates.io](https://crates.io/crates/mdns-sd)
- [MDN: `MediaDevices.getUserMedia()` and secure contexts](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia)
- [Apple Developer Forums: local network access on macOS 15](https://developer.apple.com/forums/thread/769037)
- [NVIDIA: Connect Two Sparks](https://build.nvidia.com/spark/connect-two-sparks)
- [StorageReview: NVIDIA DGX Spark cluster review](https://www.storagereview.com/review/nvidia-dgx-spark-cluster-review-distributed-inference-on-dell-gigabyte-and-hp)
- [Two-node DGX Spark vLLM and Ray deployment (community)](https://github.com/makiisthenes/dgx-spark-multinode-vllm-ray)
- [exo](https://github.com/exo-explore/exo)
- [GPUStack](https://github.com/gpustack/gpustack)

---

## 18. As built

Built on one GB10 with no second machine, no root and no Mac or Windows machine. Everything below says which of "tested offline", "run live here" or "not run" applies.

### 18.1 What exists

*As built. The `desktop/src-tauri` rows are history: the Electron app replaced the Tauri one, whose TypeScript ports of the forwarder, discovery, locator and sign-in were removed (§7).*

| Piece | Where | State |
|---|---|---|
| `ragon-admin node enable [--no-web]`, `disable`, `status`, `id` | `dreamference/node/` (`NodeAdvertiser`, `NodeServiceFile`, `NodeSettings`, `NodeIdentity`, `NodeBrowser`) | Tested offline (20 tests). `status` run live. **`enable` was not run on this machine**: it needs root once and it opens the web UI to the LAN, which is the owner's act |
| The Avahi service file, rewritten by `server start|stop|remove` | `NodeServiceFile`; hooks in the CLI controller | Exact text tested; the three state changes tested |
| The two binds (§4) | `SearxngSidecar.run_command()` and `start()` | Tested offline; no container was recreated. *Replaced: the web UI's bind is `NodeAdvertiser.serve_web()` (`ragon web start --lan`, unit `ragon-web.service`)* |
| The locator crate and its copies | `ragon-rs/node-locator/`, `ragon-web-rs/src/node_locator.rs`, `desktop/src-tauri/src/node_locator.rs` | 8 tests; a Python test holds the three byte-identical. *Two copies: the Tauri copy went with Tauri, its Electron port* |
| The resolution tiers, the messages, `node.json` | `ragon-rs/src/node.rs`, three hunks in `lib.rs` | 25 launcher tests with a stand-in browser. The real browse run live (§18.3) |
| `ragon node list|use|forget`, `RAGON_NODE=<name>` | `node.rs` | Parsing and decisions tested; run live as commands (§18.3) |
| `ragon-search` on the node's SearXNG | `ragon-web-rs/src/search.rs` | 3 new tests in the web crate's 43 |
| `ragon-app`'s forwarder and one-time sign-in | `desktop/src-tauri/src/forwarder.rs`, `discover.rs`, `auto_sign_in.js`, `main.rs` | 7 Rust tests and 4 Python tests (the sign-in script runs under node); the crate builds; the forwarder run live in front of the real web UI (§18.3). The window itself was not opened. *Replaced: ported to Electron, removed; nothing forwards or signs in since (§7)* |
| `/night add` refused on a client; Gmail prompt block only on a node | `ragon-rs/src/night.rs`, `lib.rs` | Tested |

### 18.2 Where the build departs from the design

- **Whether a node is advertised is not a `dreamference.toml` value.** §4 names one config value, `node_advertise`. It is kept in `~/.config/dreamference/node-advertise.json` instead: `dreamference.toml` is resolved from the working directory first, and the address a container publishes on must not depend on the folder `ragon-admin chat configure` was run from.
- **A machine becomes a node when it first loads a model, builds `ragon` or builds the window**, not only at `node enable`: `server start`, `codex build` and `desktop build|install` write `~/.config/dreamference/node-id` if it is missing. Otherwise a GB10 that never ran `node enable` would browse for a node on every start (tier 3 is keyed on that file).
- **A last tier keeps an old install working.** With no node id, nothing remembered and nothing found, the launcher uses `localhost:8000` if a model server answers there, and `ragon-app` uses a web UI answering on `localhost:3000` (until recently; the app's bundled `ragon` follows the launcher's rule). This is the GB10 that was installed before the split and has not run any of the three commands above yet. It costs such a machine one 2 s browse per start until then.
- **`RAGON_NODE` ranks above "this machine is a node"**, so a node can use another node for one command.
- **Loopback and link-local IPv6 addresses are never used from a browse.** §5.3 says IPv6 "with its scope id"; the URL type the launcher uses cannot carry a scope, so a link-local IPv6 answer is dropped and a global or unique-local one is used. Loopback is dropped because a machine that browses is by definition not the node it finds.
- **A stale advert and a node still loading after a reboot look the same.** The file says `ready` from before the reboot while Docker restarts the model server. The message for `state=ready` with a refusing port therefore says both: the server may have stopped, or may still be loading.
- **`disable` without root empties the file** instead of removing it (the folder is root's, the file is the user's), and prints the `sudo rm` line. That Avahi publishes nothing from an empty file is assumed.
- **No `control` record** (§15.1).
- **`ragon-app` reads `node.json` and never writes it.** It browses for itself, with the launcher's rules, but only the launcher remembers a node. With several nodes and none remembered the window shows a page naming `ragon node use <name>`.
- **The forwarder is a TCP forwarder, not an HTTP proxy.** It copies bytes, so a streamed response, a WebSocket upgrade and the unchanged `Host` header need no code of their own. With no node, or a node that does not answer, it answers each request with a page that says so and reloads itself every 10 s.
- **Night Shift names its model server to every task** (`DREAMFERENCE_VLLM_HOST`), as the egress audit, SWE-bench and the Codex test runner already did, so no unattended caller ever reaches the browse.
- **`ragon-code` needed no change.** Its busy probe asks `vllm_host`, default `localhost:8000`; on a client nothing answers there, which it already treats as "no model server" and does not freeze for.

### 18.3 Measured on this machine

- **`mdns-sd` browses beside Avahi.** With a test advert published through Avahi (`avahi-publish -s … _ragon-node._tcp 8000 proto=1 node=… state=ready main=1`), the launcher's own browse found it in 0.48 s with all records, and in 1.08 s when told which id to stop at. Phase 0's item for Linux is answered.
- **A browse run on the node itself is answered once per interface**: three browses gave `172.18.0.1`, `172.19.0.1` and `127.0.0.1`, never the Wi-Fi address, and a client run on the same machine announced that the node had moved on every start. The browse now collects every usable address a node answers on (`172.18.0.1`, `172.19.0.1`, `172.20.0.1`, `192.0.2.10` here), keeps the remembered address for as long as the node still answers there, and stops as soon as it does. §2's assumption that a LAN client receives only the LAN record **was not verified from a second machine**, and no longer has to hold.
- **The forwarder in front of the real web UI**: `/app` (44,756 bytes, the same as direct), `/api/health` and a login `POST` answer through it as they do directly, with `Host: localhost:3000` passed through.
- **The web UI is not reachable on the LAN address today** (`192.0.2.10:3000` refuses), as it should be on a node that has not been enabled.
- **A client with nothing configured, on one machine.** The installed `ragon` (rebuilt at 11:59 with the launcher above) was run with a scratch home folder, so with no node id, no `node.json` and no configuration, against a test advert of this GB10 published through Avahi:
  - `ragon node list` showed `gx10-e2e  http://172.18.0.1:8000/v1  RadixArk/Qwen3.8-27B-NVFP4 (262144 tokens)  Ragon 0.9.0` and said none was remembered;
  - `ragon exec` printed `Using Ragon node gx10-e2e at 172.18.0.1 (remembered; …)`, wrote `node.json` and a `config.toml` whose `base_url` is the node's address, and the model answered; the second start found the node by its id and answered in 4.0 s;
  - with the advert saying `state=stopped`: `Node gx10-e2e found, but its model server is not running. On the node: ragon-admin server start`, in 0.07 s and with no wait;
  - with `state=loading`: `Node gx10-e2e: model loading`, then the dotted wait;
  - with the remembered node gone and another advertised: `The remembered node, gx10-e2e, is not on this network, and another is:` with the list and the `ragon node use` line, and nothing adopted;
  - `ragon node use other-node` and `ragon node forget` wrote and removed the file;
  - `RAGON_NODE=other-node ragon exec` on the node itself used that node for the one command and left no `node.json`.
  This is the Phase 1 "done when" on one machine: the model server was reached through an address a browse returned, but the client and the node were the same host.
- **`ragon-search` as that client** read the launcher's `node.json` and asked `http://172.19.0.1:8888`; it was refused, correctly (this node is not enabled, so its SearXNG is on loopback), with `Web search runs on the Ragon node (gx10-e2e). On the node: ragon-admin searxng start`. With the node's address written as `127.0.0.1` it returned real results through the same path.
- **On this GB10 itself** `ragon node list` says `This machine is a node: ragon uses its own model server, http://localhost:8000.` The node id was written at 11:39 by a `ragon-admin codex build`, as designed.

### 18.4 Not built, not run

- **No client on a second machine.** The Phase 1 "done when" needs one; §18.3 has the one-machine stand-in. The "No Ragon node found" message was not seen live either: on this machine a model server always answers on loopback, which is the last tier.
- **`node enable` with root**, the file under `/etc/avahi/services`, Avahi publishing a file the user owns, and the two containers recreated on `0.0.0.0`. *Run: the second GB10 was installed and advertised, and this machine's browse found it (§18.10).*
- **SearXNG answering a LAN client.** It does answer a client that is not on loopback: asked for JSON from the web UI's API container (`172.18.0.7`, over the Docker network), it returned 200 with 20 results. A client on the LAN address was not tried, because the port is not published there until `node enable`.
- `install.ps1` and a macOS run of `install.sh` (§9). `install.sh` itself exists (written on the same day: release assets with checksums, the role decided by the machine, `--role`); its node role now writes the node id (`ragon-admin node id`) and runs `ragon-admin node enable --yes` with no question asked (with lingering and the model too: SETUP §3.2.1), with `--no-advertise` to opt out. The node role was not run end to end. Also not built: the release matrix of §8.1, `ragon-code setup` in Rust (§8.3), and every macOS and Windows build. The macOS bundle's two local-network keys are in `desktop/src-tauri/Info.plist`, untested. *Since built: `install.ps1` and the Windows preview, the x86-64 Linux and macOS clients (§8.4); the node role ran unattended on the second GB10 (§18.10). The local-network keys are in `desktop/electron/forge.config.js`. `ragon-code setup` in Rust is still not built.*
- **The window itself**: `ragon-app` was built and tested as a crate, but not opened as a client, so the one-time sign-in (tested under node, and the default account answers 204 here), voice through the forwarder and the port-33000 fallback were not watched. *Moot: the forwarder and the sign-in are removed (§7).*
- **In Part 3:** sending jobs from a client (the sender is Python; open question 9 is undecided). Everything else this bullet listed was built (§18.8).

### 18.5 Tests

- `tests/test_node.py` (Python): the service file's exact text; an update changing one record; a node never enabled never advertised by a side effect; the state through `server start|stop`; a file the user cannot write reported; the node id written once; settings independent of the working directory; the web UI's and SearXNG's publish addresses; SearXNG recreated when its address no longer matches; `enable` without root (binds applied, file printed), with root once (then no root), `disable` with and without root; `status`; the browse output; the three locator copies; the commands; every unattended caller naming its model server.
- `ragon-rs/src/node.rs` (launcher, in the export): each tier in order; a node never browses; the remembered node found again by id at a new address; the last address only when nothing, or only a stopped node, answers; a different node never adopted silently; several nodes with and without exactly one `main`, and with only one not stopped; the refusal where no question can be asked; `proto` too new; each message of §6.4; names, addresses and id prefixes; `ragon node` parsing, `use`, `forget`, the list; the advert parsed; address choice; one row per node; `node.json` written whole; the version notice once a day. One ignored test runs the real browse.
- `ragon-rs/node-locator` (8), `ragon-web-rs` (3 new), `desktop/src-tauri` (forwarder 4, discovery 3). *The `desktop/src-tauri` tests went with Tauri; their Electron ports went, replaced by `test_ask_in_the_app_reaches_no_server_and_reads_no_password`.*

### 18.6 Part 2 as built: pairing, and managing a node from another

| Piece | Where |
|---|---|
| `ragon-admin node add <node> [--user] [--ssh-port]`, `node remove` | `dreamference/node/node_pairing.py` |
| `ragon-admin node list`, `node status <node>`, `node set <node> --model <key>`, `node start|stop <node>` | `node_remote.py` |
| `ragon-admin node authorize` and `node serve-job` (run on the other node, not typed) | `node_serve.py` |

- **Control is SSH, as decided (§15.1).** No control agent, no port, no `control` record.
- **The key is authorised by the node, not by `ssh-copy-id`.** `ssh-copy-id` installs a bare key; the pairing needs a line with a forced command. `node add` therefore runs `~/.local/bin/ragon-admin node authorize` on the node over an ordinary SSH login (the one place the node's password is typed) and sends the public key on standard input; the node writes `command="<its ragon-admin> node serve-job --key <tag>",no-pty,no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-user-rc <key> ragon-node`.
- **The tag** is a digest of the key. sshd does not tell a forced command which key connected, so `unpair` removes the line carrying the tag it was started with, and no other.
- **The host key is pinned to the node id** with `HostKeyAlias=ragon-node-<id>` in a `known_hosts` of its own (`~/.config/dreamference/nodes/known_hosts`): `accept-new` at pairing, `yes` afterwards. A paired node is followed to a new address by a browse for its id.
- **`node add` also checks the answer.** After authorising, it asks the node for `info` through the new key and refuses the pairing if the node id that comes back is not the advertised one.
- **`set-model` is the node's own three commands**: `ragon-admin main-model set <key>`, `server stop`, `server start`, run from the node's home folder, so the assignment lands in the node's user-level config. Only a key of the node's own matrix is accepted, and not a diffusion model. A node whose owner keeps a `dreamference.toml` in a repository can therefore have two assignments; the pairing changes the user-level one.
- **A load started over SSH is a child of the connection.** `server start` leaves the container to Docker and only watches it; if the connection drops, the load continues and the advert stays at `loading` until the next `server start` or `stop` on that node.
- **`node list` has no memory column.** Model, context, running requests and KV pool come from each node's open model port; free memory is not served there. *(Superseded, §18.8: memory is asked over the pairing.)*

**Run live**, against an `sshd` started for the test on `127.0.0.1:2222` with its own host key and `authorized_keys` file, so the user's `~/.ssh` was not touched and this GB10 played both sides:
- `info` returned the node's id, name, version and `linger: false`; `status` returned the node's `ragon-admin status` (2,670 bytes).
- A shell (`bash -c 'id'`), `cat /etc/passwd`, a `git-upload-pack` for `/tmp`, a path as a model key and an empty request were each refused with exit code 2.
- A request for a terminal failed (`PTY allocation request failed`), and a local port forward was refused (`administratively prohibited`).
- With the stored host key replaced, the connection was refused (`Host key verification failed`).
- **Not verified live:** `unpair` (the test server reads a different `authorized_keys` than the one the command edits), `node add`'s password step, `set`, `start` and `stop` (they would have stopped the model server other tasks were using), and everything between two machines.

### 18.7 Part 3 as built: script jobs on another node

| Piece | Where |
|---|---|
| `ragon-admin node run <node> [--memory] [--time] [--test] -- <command>`, `node jobs|logs|cancel|fetch` | `node_job_sender.py` |
| The job on the node: record, admission, unit, sandbox, commit | `node_job.py`; `ragon-admin node job-exec <id>` inside the unit |
| git and job requests through the forced command | `NodeServe.git_service`, `NodeServe.job_operation` |

- **Options go after the node's name**, as §13.3 writes them, and what follows `--` is the command verbatim: `node run spark-2 --memory 16G --test "pytest -q" -- python train.py --epochs 3`.
- **What travels.** `node run` pushes `HEAD` to `ssh://<node>/jobs/<name>-<digest>.git` as `refs/jobs/<id>`; `serve-job` runs `git-receive-pack` for that path and refuses any other, creating the bare repository on first push. The request (`job-submit`, base64 JSON) names the commit, the command, the caps, the sender's `/airgapped` level and its git identity.
- **The node validates everything**: the id and repository name by pattern, the commit as a full hash it already holds, the command as a list, the caps present and under the node's ceilings (32 GiB, 8 h; constants for now, not config), and `--gpu` refused outright (§15.1).
- **Admission is Night Shift's without the model checks**: no night run or benchmark run holding the lock, no build or index run, and memory available for the reserve (8 GiB) plus the job's cap. An open `ragon` session does not block a script job.
- **The job is a systemd user service**, `ragon-job-<id>`, started with `MemoryMax`, `MemorySwapMax=0`, `RuntimeMaxSec` and `CPUQuota=800%`; `serve-job` then prints its output file until the job finishes. A job the unit manager stopped (time limit, memory cap) has no chance to write its own result, so reading a job's record first settles one whose unit is gone.
- **The sandbox** is bubblewrap with the indexers' profile: `/` read-only, tmpfs over `/tmp`, `/run`, `/var/tmp`, `/dev/shm` and the home folder, the worktree the only read-write bind, a cleared environment with `CUDA_VISIBLE_DEVICES` empty, and no network at the level `on`. The runner's git commands (worktree, commit) run outside it, because the worktree's git metadata lives in the job repository under the hidden home folder: a job cannot run `git` on its own tree.
- **With the network on, a job shares the node's network namespace**, loopback included: it can reach the node's model server and its other loopback services, as any process on the node can.
- **The result** is committed as the sender's git identity on `job/<id>` in the job repository and fetched into the sender's repository when `node run` ends, or later with `node fetch`. A job that changes nothing leaves only its log and exit code. The test command, if any, runs in the same sandbox after the command succeeds, and its exit code becomes the job's.
- **Cancel** marks the record and stops the unit; the worktree of a cancelled or killed job is left for a later prune (built, §18.8).
- **A node that cannot sandbox a job refuses it.** Admission first runs bubblewrap in a unit like the job's own; if that fails the job is refused with the reason and the fix, and is never run without a sandbox.

**Run live**, with this GB10 as sender and node and the test `sshd` of §18.6:
- `node run` pushed a commit through the forced command, which created `jobs/job-repo-<digest>.git` and ran `git-receive-pack` for it.
- **Through the real unit the job was refused, and that is a finding about this machine, not about jobs** (fixed: `ragon-admin host setup` installs an AppArmor profile for `/usr/bin/bwrap`, loaded and verified here, SETUP §3.3; the job has not been re-run through the unit). `kernel.apparmor_restrict_unprivileged_userns` is 1 and `/etc/apparmor.d` has no profile for bubblewrap, so a process that AppArmor counts as unconfined cannot create the user namespace bubblewrap needs: `bwrap: setting up uid map: Permission denied`. A unit of the user's service manager is unconfined, which is what was measured; an SSH login and an ordinary terminal should be too, and were not tried. Everything in this repository that uses bubblewrap has so far been run from the PyCharm terminal, whose processes carry the profile `snap.pycharm.pycharm (complain)`, where it is allowed. **Codex's own sandbox fails the same way**: `ragon sandbox -- sh -c 'echo x'` prints `x` from that terminal and `bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted` from a user service. So a Night Shift run started by its timer would have every sandboxed command fail on this machine, and so, if an ordinary terminal is unconfined as expected, would `ragon` typed in one. The fix needs root (an AppArmor profile with `userns,` for the installed `ragon` and for `/usr/bin/bwrap`, or the sysctl), belongs with the host checks of `server start`, and was not applied.
- **With the node's code run in the terminal's context**, the same job ran end to end: worktree `~/.ragon/jobs/<id>/tree`, home folder holding only `.ragon`, three lines of output a second apart, the test command passing, a commit `Job Test <job-test@localhost> job: python3 work.py` on `job/<id>` changing `data.txt` and adding `result.txt`, 3 s wall. `node fetch` brought the branch into the sender's repository, whose checkout stayed on `master` with `data.txt` unchanged, and `node jobs` listed both jobs through the pairing.
- The test jobs and their repository were removed from `~/.ragon/jobs` afterwards.

**The sandbox profile, outside a unit:** Inside it the home folder held only the path to the worktree, `~/.ssh`, the Hugging Face cache, `~/.config/dreamference` and the Docker socket were absent, `/usr` and `/etc` were read-only, there was no `/dev/nvidia0`, five processes were visible, and the model server answered on loopback with the network on and was unreachable with it off.

### 18.8 Added: the rest of Parts 2 and 3

| Piece | Where |
|---|---|
| Replica lanes for a run (§12.3) | `dreamference/node/node_lanes.py` (`NodeLanes`); `NightShiftRunner.lanes/schedule/start_blocker`; `SweBenchRunner.lanes/open_relays/schedule` |
| The relay from the benchmark network to a replica | `dreamference/swe_bench/swe_bench_relay.py` (`SweBenchRelay`) |
| Memory in `node list` | `NodeRemote.memory`; `info` now carries `mem_available`, `mem_total`, `model_port` and `runner` |
| `node sync-model <node> <model> [--address]` | `dreamference/node/node_model_sync.py` (`NodeModelSync`); `serve-job` operation `model-receive <key> <bytes>` |
| `node run --setup/--out/--bind`, pruning | `NodeJob` (`_environment`, `_keep_out`, `send_out`, `allowed_binds`, `prune`, `mark_fetched`); `NodeJobSender._fetch_out`; operations `job-out`, `job-fetched` |
| `/night add --on <node>` | `ragon-rs/src/night.rs` (the flag, `[node]` in `list`, `Node:` in `show`, a drop of a `sent` task); `dreamference/night_shift/night_shift_remote.py` (`NightShiftRemote`: hand-off, collection, and the node's `night-submit|status|cancel|fetched`); `NightShiftTaskRun.remote_sandbox` |

**Replicas (§12.3), as built.**
- **A lane is a model server, not a machine.** The tasks of a night run, and a benchmark's containers, still run on the machine that runs the run; a paired node serving the same model only answers some of their model requests. That is what "a replica pays" means when every model is the same: the KV pool doubles, and nothing about the tasks' worktrees, scopes or sandboxes changes. A task stays on the lane it started on.
- **Only a node serving the same model id is a lane.** A run measures one model; a node serving another, one whose own night or benchmark run holds its runner lock, one that does not answer over the pairing, and one whose model server does not answer are each left out with a line in the report saying which and why.
- **`[night] nodes` and `[swe_bench] nodes`**: `"paired"` (the default: every paired node), `"none"`, or a list of names. With no paired node nothing is asked and the run is exactly as before.
- **Each lane is sized from its own KV pool** with the same rule as this machine's (`task_budget` for Night Shift, `parallelism` for SWE-bench), so `max_parallel` is per server.
- **A `ragon` session open here holds up this machine's lane only.** A replica's own users show up as requests on its server that are not the run's, which block that lane as they block this one. Memory admission stays this machine's, since that is where the tasks run.
- **SWE-bench reaches a replica through a relay.** A container's only reachable address is the internal network's gateway; the run listens on the gateway at a port of its own and forwards each connection to that one replica's model port, for as long as the run lasts. The network is not opened. **Run live here:** with the relay on `172.20.0.1` (the `ragon-swe-bench` gateway) forwarding to this machine's model server, a `curlimages/curl` container on that network got `/v1/models` through it, and the same container could not reach `1.1.1.1` (curl exit 7).
- **Not built:** handing tasks without `--on` to another node's *runner* to balance a queue; the report names which lane each task used, and that is the extent of it.

**`node sync-model` (§12.2).** The sender streams the hub cache folders of the model and of its recipe's drafter (`models--<owner>--<name>`, unfinished downloads excluded) as one tar over the pairing; `--address` sends it over another address of the same node, such as the QSFP link's (the host key is pinned to the node, so any address of it is checked). The node accepts only the folders of a key of **its own** matrix, checks free disk space against the announced size plus 10 GiB, extracts into a staging folder with tarfile's `data` filter (no absolute names, no `..`, no links out), checks every content-addressed blob against its SHA-256 name, and only then moves files into the cache, keeping any it already has. Tested offline end to end (a real tar into a real receiving process) and for each refusal; not run between two machines.

**Script jobs (§13.3, §13.6, §13.7), as built.**
- **`--setup "<cmd>"`, or `[night] setup` in the repository's `dreamference.toml`**, builds the job's environment in `~/.ragon/jobs/envs/<repo>-<digest>`, where the digest covers the command and the content of the lock files at the worktree's top (`requirements*.txt`, `pyproject.toml`, `setup.py`, `setup.cfg`, `poetry.lock`, `uv.lock`, `Pipfile.lock`, `package.json`, its locks, `Cargo.toml`, `Cargo.lock`, `go.mod`, `go.sum`). It runs once, in the job sandbox with the folder writable and the network unless the level is `on`, under a lock so two jobs do not build it at once; a failure discards the folder and fails the job. Later jobs bind it **read-only at the same path** (a virtualenv's paths are absolute) with `RAGON_ENV`, `VIRTUAL_ENV` and its `bin` first on `PATH`. With no setup command the record says the job ran with the node's system interpreter, and a failure whose output shows a missing Python module is reported as an environment failure.
- **`--out <folder>`** names a folder inside the repository. After the job (and its test) it is moved out of the worktree before the commit, so it is never committed, and kept as `~/.ragon/jobs/<id>/out/`. `node fetch` (and `node run` at its end) brings it back as files to `$CODEX_HOME/jobs/received/<id>/` on the sender, extracting with the `data` filter: a node is paired, not trusted with the sender's files.
- **`--bind <path>`** (repeatable, at most 16) binds a path on the node read-only, at the same path, only if it resolves under one of the folders the node owner listed in `[node] bindable` in the node's user-level `config.toml`. Nothing is bindable by default.
- **Pruning.** A finished job is removed (its folder, any worktree left by a cancel or a kill, `job/<id>` and the pushed ref) **a day after the sender fetched it**, so `node logs` still reads it that day, or **14 days after it finished** when nobody fetched it. The sender marks it fetched when `node run` ends with the job finished, and on `node fetch`; a run whose stream was lost does not. Pruning happens when a job is submitted and when jobs are listed, and `node jobs` shows each job's date. *Departure from §13.7, which said "after they are fetched": the day's grace is for `node logs`.*

**Night tasks on another node (`/night add --on <node>`, §13.3, §13.5), as built.**
- **The hand-off is at night, by this machine's runner**, whatever this machine's own state: no admission here, since the node admits the task by its own checks. The task's base commit is pushed to the node's job repository (`refs/night/<id>`), and `night-submit` queues the task in the node's own Night Shift queue, under the same id, with the stricter of the two `/airgapped` levels and this repository's git identity. Here the task becomes `sent`.
- **The node refuses a task it cannot sandbox** at submission, with the reason (the AppArmor finding of §18.7), rather than failing it at night. A refused task is `failed` here with the node's message; an unreachable node leaves it queued.
- **If the node's timer is installed and its window is open, the node's run is started for the task**; otherwise the reply says when it will be worked (its next window, or not until Night Shift is enabled there) and the report carries that line.
- **On the node the task runs like any night task, with four differences**: every process (`ragon exec` and the test run) runs inside the job sandbox, with the home folder an empty tmpfs, the worktree and the task's own `CODEX_HOME` (`~/.ragon/jobs/<id>/home`, kept across nudges and nights) writable, `ragon`'s own folder read-only, and the host's network namespace, because `ragon` reaches the node's model server on loopback; Gmail is switched off for it (`DREAMFERENCE_RAGON_GMAIL=false`; its secret is in the hidden home anyway); the node owner's `[night] test` is not applied; and its repository is not indexed. The result is committed as the sender's identity.
- **Collection does not hold the runner lock.** After its own tasks this machine asks each node about the tasks it holds every two minutes until they finish or the window closes, outside the lock, so `server start`, `codex build` and `index` are not refused meanwhile. A finished task's `night/<id>` is fetched into the repository it was queued from, its result is copied with `node: <name>`, and the node is told, which starts the same one-day keep as a job's (14 days unfetched). A task still running at the window's end stays `sent` and is collected by a later run. `/night drop` of a `sent` task is forwarded to the node at the next collection.
- **The report** says "Ran on: <node>" or "Sent to: <node>", and for a task that ran here against a replica, "Model server: <node>'s".
- **Unverified, as §13.8 assumed:** Codex's own sandbox nested inside the job sandbox. It needs the node to allow unprivileged user namespaces in the night timer's context, which this GB10 does not (§18.7), so on this machine such a task is refused at submission, as designed.

**Tests (offline).** `tests/test_node.py`: lanes (a same-model replica is a lane; another model, a held runner lock and a silent node are each named; `none` asks nobody), memory in the list and in `info`, the environment built once and reused, a changed lock file or command is a new one, a failed setup and a missing module, `--out` kept and not committed and its tar stream, a hostile archive that cannot write outside, binds only under `[node] bindable`, pruning by both rules, `sync-model` end to end and each refusal, the CLI reaching every new option. `tests/test_night_shift.py`: a session here holding this lane and not a replica's, a replica counting only its own tasks, no SSH without a paired node, a task handed to a node (simulated in-process, with git pushing and fetching through real repositories) worked there and its branch brought back, a drop forwarded, the node's refusals, the job sandbox argument list for a task from another machine. `tests/test_swe_bench.py`: instances on a replica through the relay URL, the relay forwarding and closing. `ragon-rs/src/night.rs`: `--on` in both orders and forms, refused names, the field, `list`, `show` and the forwarded drop (14 tests, run in a stub crate with the launcher's own sources).

**Not run:** anything between two machines, a real `node sync-model`, and a real hand-off over SSH. *(Since: a `node sync-model` was tried and did not complete, and a lane ran; §18.10.)* The launcher change: `ragon night --help` lists `/night add [--test "<cmd>"] [--on <node>] <task>`; no task was queued with `--on`, since there is no second node.


### 18.9 Added: `/node` in the TUI

`/node provision [host…]`, `/node add <name>`, `/node list`, `/node status [<name>]` and `/node remove <name>` do what `ragon-admin node …` does, without leaving `ragon`; `/node` alone prints a short help. The command is the TUI's and never reaches the model, and no turn starts. Patch `0025-node-slash-command` (3,382 bytes; `0024` is left to Windows Phase 2) adds the variant, its description, two capability lists, two dispatch arms that send an app event, and that event's arm, which suspends the TUI with `Tui::with_restored` (the external editor's mechanism) and calls `ragon_launcher::node_command`. That module (`ragon-rs/src/node_command.rs`) finds `ragon-admin` as the launcher's links do (`~/.local/bin/ragon-admin`, then `PATH`), puts the terminal in cooked mode (`with_restored` keeps raw mode, in which `getpass` and SSH's prompt never see the end of a line), runs `ragon-admin node …` with the terminal's own stdin, stdout and stderr, outside the agent's sandbox, waits for Enter, and restores the terminal. The conversation keeps one line, `node <args>: done (exit 0)` or `failed (exit N)`; nothing the command prints or is typed at its prompts, a password included, passes through the TUI, the session, its history or its rollout.

- **At `/airgapped on`** it runs, after printing `airgapped is on: /node talks to machines on your local network only` (kept in the history too); the user trusts the local network. AIRGAPPED §5.2 records the exception.
- **Without a terminal** (the app server, `ragon exec`) it refuses and says to run `ragon-admin node …` in a shell. Without `ragon-admin` it says so. `add` and `remove` with no name are refused before anything runs; every other argument is `ragon-admin`'s to judge.
- **Ctrl-C stops the command, not `ragon`.** Cooked mode makes Ctrl-C a SIGINT for the terminal's whole foreground process group, and the first tmux check showed it ending the session along with the stand-in. `ragon` now ignores SIGINT and SIGQUIT from just before the command starts until Enter, and the child gets the default back before it runs (`pre_exec`), so provisioning stays interruptible: in tmux, Ctrl-C at the password prompt left `node list: stopped by a signal`, and a second Ctrl-C at "Press Enter" was ignored. Ctrl-Z would still stop both; not handled.
- **Not available during a turn** (it takes the terminal), and queued behind one like `/model`. No subcommand completion: the popup lists `/node` with its description only.
- `node provision` came from the fleet/provision branch, merged since, so `/node provision` now runs it.

### 18.10 Added: the first two machines

A second GB10 (an ASUS GX10, named `node-b`, on Wi-Fi like this one) was installed over SSH, with no one at its terminal, and paired with this machine. It is the development machine of `ragon-engine` and serves no lanes. What two real machines showed, and what changed in the code for it:

- **`node add` picked an address ssh could not use.** The browse returned a link-local IPv6 address without its interface, and ssh answered "Invalid argument". The pairing went through by IPv4 (`node add <address> --user <user>`). Since then a browsed node is kept once, IPv4 first, then IPv6 that needs no scope, then link-local IPv6 with `%<interface>` added (`NodeBrowser`, mirroring `usable_addresses` in `ragon-rs/src/node.rs`, which drops link-local addresses altogether), and rsync brackets IPv6 hosts.
- **`host setup` and `node enable` needed a terminal**, because each asks before running `sudo`. Both now take `--yes`, which runs `sudo -n` and needs none; `install.sh` uses it on an unattended node install (SETUP §3.2.1).
- **`host setup` piped into `head` died mid-swap** of the closed pipe, after `swapoff` and before `swapon`, leaving the machine without swap. It now survives a closed pipe and finishes an interrupted swap resize on the next run (SETUP §3.3).
- **`node sync-model` did not complete** between the two installs; the weights were copied with rsync, and the transfer was not retried.

Not yet run between the two: jobs (`node run`), lanes for Night Shift or SWE-bench (the second machine is reserved), `/night add --on`, and `node provision`. *(Except once: a Night Shift lane on the second machine ran on one night and was lost when its DHCP lease moved, which led to resolving every connection by id, §13.2.)*

### 18.11 Added: a proxy left in the environment

A node's address is exempt from any `HTTP_PROXY` the user's shell carries (EGRESS §11): `ragon` adds the configured host and the remembered node's address to `NO_PROXY` before Codex starts, and Night Shift adds a lane's host to each task's environment. A node found by a browse for the first time is not in it for that first session, because the variable can only be set before any thread exists and the browse comes later; the launcher's own requests to it skip proxies regardless, and from the next start it is the remembered node and exempt.

### 18.12 Added

The `node` group as the code has it (`dreamference/cli/dreamference_cli_controller.py`): `enable [--no-web] [--no-forge] [--yes]`, `disable`, `status [<name>]`, `id`, `list`, `add <name> [--user] [--ssh-port 22]`, `remove`, `set <name> --model <key>`, `start`, `stop`, `run <name> [--memory] [--time] [--test] [--gpu] [--setup] [--out] [--bind …] -- <command>`, `jobs [<name>]`, `logs`, `cancel`, `fetch`, `sync-model <name> <model> [--address]`, `provision [hosts…] [--all] [--web] [--role night|engine] [--dry-run] …`, `prepare`; and, typed by no person, `askpass`, `job-exec`, `authorize` and `serve-job [--key]`. `--gpu` is parsed and refused by the node (§18.7).

| Piece | Where |
|---|---|
| `node provision [hosts…] [--all] [--web] …` (set up a new GB10 from this one, or update paired ones) and `node prepare` (its root steps, run with sudo) | `dreamference/node/node_provisioner.py`, `node_prepare.py`, `fleet_*.py`; spec [RAGON_FLEET](./DREAMFERENCE_RAGON_FLEET.md) |
| `node askpass`: ssh's password prompt answered from the provisioning run's socket | `dreamference/node/fleet_askpass.py` (FLEET §8.2) |
| `provision --web` shares `ragon web` through `node enable` | `node_provisioner.py` |
| Wi-Fi power saving: `node enable` warns, `node prepare` turns it off (its one NetworkManager change), a client's missed browse adds a hint (§6.4) | `dreamference/vllm_server/wifi_power_save.py`, `ragon_node_locator::wifi_power_save_hint` (both copies) |
| `node enable` sets up the node's Forgejo forge by default (`--no-forge` skips it) | `dreamference/forge/forge_service.py`, `dreamference/cli/dreamference_cli_controller.py`; spec [RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md) |
| `node sync-model` also copies installed Dreamference-signed LoRA adapters (`adapter-receive` in `serve-job`) | `dreamference/node/node_adapter_sync.py` (`NodeAdapterSync.send`), `node_serve.py`; spec [RAGON_ADAPTERS](./DREAMFERENCE_RAGON_ADAPTERS.md) §6.4 |
| `node provision --role night\|engine [--dry-run]`: configures a paired node for its role (model server, night timer, paired lanes) | `dreamference/node/node_role_provisioner.py` (`NodeRoleProvisioner`) |
| `ragon node join`: enrols a client with a node over the NetBird overlay | `ragon-rs/src/remote_join.rs`, `dreamference/remote/` |

`node provision` has still not been run against a machine (FLEET's status line).

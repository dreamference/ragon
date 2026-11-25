# One endpoint for the whole group: the primary node as the front door

**Status:** decided, build starts 2026-11-24. It answers the owner's words of 2026-11-24 (the primary node becomes the load balancer and proxy for everything, so the group looks like a single endpoint); the owner's answers of the same day are folded in (§14 lists them, §15 is the record). Where it changes an earlier decision, §14 quotes the old line.
**Target:** slices 0 and 1, and the rows pulled forward in §12, in 0.9.10 with the endpoint **switched on for every group** and every member refusing clients that go around the primary (owner, 2026-11-25); slices 2 to 5 in version 1.0 (the standby in version 1.1); nothing in 0.9.9 (§12). A single node, alone, is unchanged.
**Builds on:**
- the model gate in front of every engine, port 8000 (`dreamference/vllm_server/model_gate_service.py`, [INFERENCE](./DREAMFERENCE_INFERENCE.md), [docs/dev/swe-bench.md](../docs/dev/swe-bench.md#the-model-gate-the-benchmark-first));
- accounts, one primary per network, TLS, the 60 s check cache ([AUTH](./DREAMFERENCE_AUTH.md) §5-§7, §10, §13);
- nodes, pairing, discovery and replica lanes ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md), [docs/dev/node.md](../docs/dev/node.md)), the fleet ([RAGON_FLEET](./DREAMFERENCE_RAGON_FLEET.md));
- remote access ([RAGON_REMOTE_ACCESS](./DREAMFERENCE_RAGON_REMOTE_ACCESS.md)), the egress audit ([RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md));
- the platform quality rule ([PLATFORM_QUALITY](./DREAMFERENCE_PLATFORM_QUALITY.md)), adapters ([RAGON_ADAPTERS](./DREAMFERENCE_RAGON_ADAPTERS.md)), the prefix cache ([PREFIX_CACHE](./DREAMFERENCE_PREFIX_CACHE.md)).

## 1. The goal

A group of nodes presents itself as **one endpoint, as hosted model APIs do**. Every client talks to one address, the primary: the launcher, the desktop app, `ragon web`, the phone bridges, any tool that speaks the hosted-API style with a key, benchmarks, Night Shift and the coordinator's route rows. The primary authenticates the request, picks a node, forwards it, and streams the answer back. In a group **the primary is the address**: clients talk only to it, and members refuse a client that goes around it (§5). The client never learns which node served it, unless it asks (an admin or a service account, §3.3). A single node, alone, is its own address and nothing changes for it (§7.1).

The case that prompted this: a benchmark run in which four tasks shared one model server and 28 of 89 tasks timed out, while seven nodes stood idle on the LAN. A client that sends everything to one node cannot use the others, and a client that picks a node itself (today's least-loaded pick, [node.md](../docs/dev/node.md)) chooses once, at start, from latency alone.

What the endpoint is not: it does not make the nodes one machine. A session's cached prefix lives on one node's engine (the engine logs "session N, X prompt tokens (Y reused)"), so placement is sticky (§2.1); a request no node can take is refused, not split; and a model only some nodes serve is served only there (§3).

### 1.1 What "everything" includes

| Traffic | Treatment | Why |
|---|---|---|
| `/v1/responses` (also its WebSocket), `/v1/chat/completions`, `/v1/embeddings` | **Proxied**, sticky (§2) | the load, and the reason for this spec |
| `/v1/models` | **Answered by the primary** from the union of the nodes (§3.1) | one catalogue |
| `/oauth/*`, `/.well-known/*`, `/ragon/v1/users*`, `/ragon/v1/bindings*`, `/backend-api/*` | **Served by the primary itself**; a secondary keeps forwarding them to it as today (AUTH §7) | the primary already holds the accounts; clients no longer need the secondary route |
| `/ragon/v1/info`, `/ragon/v1/ca.pem` | **Answered by the primary** for the group (`endpoint` object, §9); each node still answers for itself on its own port | one CA to pin: the primary pins every member's CA, the client pins the primary's (AUTH §6) |
| Embeddings, search, image search, speech and voice sidecars behind `ragon web` | **Proxied** by the primary to a node with the sidecar (stateless: placed by load; voice WebSocket sticky by connection) | the browser and bridges see one origin; sidecars are per node today |
| `ragon web` itself (port 3100) and the account pages | **Stays on the primary**; its model calls go to the endpoint (the primary) | the UI and its accounts are already there (AUTH §14.4) |
| Node jobs (`node run`, `serve-job`), pairing, `node sync-model`, `/night add --on <node>` | **Stay direct, over SSH**, addressed by node name | admin plane; the owner's rule: "managing and using a second node goes over SSH, never an open port" |
| Forgejo (the forge of record and its mirrors) | **Stays direct** at the forge's node; the primary's `info` names its URL; its OAuth login is already the primary's gate | git pushes are large and not balanceable; one forge of record |
| Model and adapter sync | **Stays direct** (SSH pairing) | bulk copy between named nodes |
| Remote access overlay | **Reaches the primary only** (§8) | one door |
| mDNS advert | The primary advertises the endpoint; a member advertises itself as a member (§9) | clients discover one address |

## 2. Routing

### 2.1 Sticky sessions

A **session key** says which requests share a cached prefix. The primary derives it, in this order, from the first field present:

1. the `session_id` header Codex sends (AUTH §10 lists it among the accepted session headers);
2. `prompt_cache_key` in a Responses body (the field the vendor's API defines for cache routing);
3. `previous_response_id` in a Responses body: the key of the response it names (the primary remembers response id to key for 24 h);
4. an `X-Ragon-Session` header (new; for clients that want to say so);
5. otherwise a hash of the first 2,048 tokens' worth of the leading messages (system prompt and first user message, byte-hashed after the gate's own rewrite), which is what a plain chat-completions client repeats on every turn.

The key is scoped by user id, so two users never share an entry. **Which of these Codex actually sends is verified in row S0.2 on the real client before the order above is fixed**; this spec states the order as the design, not as measured.

**Placement.**
- A key with no entry is **new**: it goes to the node with the best score (§2.2) among those that serve the requested model and fit its prompt (§3.2).
- A key with an entry goes **back to that node**. The entry is soft state: key hash, node id, last-used time, kept in the primary's memory for 2 hours idle, 100,000 entries at most.
- On a table miss for a request that is plainly not a first turn (a `previous_response_id`, or more than one user turn), the primary asks the members `GET /ragon/v1/sessions?key=<hash>` in parallel (each gate keeps the hashes of the keys it served recently); one that says yes gets the request. This rebuilds the table after a primary restart (§7) at one LAN round trip. A first turn skips the question.

**When the node that holds the cache cannot take the request:**

| State of the holder | What happens | Cost, stated to the client nowhere |
|---|---|---|
| Busy (all slots taken) | wait up to 5 s for a slot on the holder; after that move the session to the best other node | a full re-prefill of the prompt on the new node: at the engine's prefill rate this is seconds for a long agent context and is the price of not waiting behind a long turn |
| Draining or held (§8) | the session finishes its running turn there; the next turn is placed elsewhere | one re-prefill |
| Down | the request goes to another node at once; the entry is replaced | one re-prefill; a turn that was streaming fails, §4 |

A moved session's reuse figure (`reused / prompt tokens`, from the node's usage line) is recorded as `place=moved` in the audit line (§8.3), so the cost shows in the numbers.

### 2.2 The load signal

Each node's gate answers `GET /ragon/v1/load` (new, behind the node's service credential), polled every second, and puts a one-line `X-Ragon-Load` on every response it sends to the primary so the primary's view is never older than the last answer. The fields:

| Field | Meaning |
|---|---|
| `state` | `ok`, `loading`, `unhealthy` (the engine's health route) |
| `sessions_total`, `sessions_busy`, `queue` | the engine's session pool (`--sessions`, [models-and-engines.md](../docs/dev/models-and-engines.md)) and requests the gate holds |
| `lru_age_s` | age of the least recently used cached session: placing a new key evicts it, so the node whose oldest cache is oldest is cheapest |
| `mem_available_mib` | against the host-safety floor ([host-safety.md](../docs/dev/host-safety.md)) |
| `hold` | `null` or `{kind: bench\|night\|gpu-lock\|operator, until}`: today's run hold (`run.json`, heartbeat 15 s, ignored after 180 s), the night window, the GPU lock (`~/.ragon-gpu.lock`), an operator's drain |
| `models`, `adapters`, `engine`, `platform`, `max_context`, `sidecars` | what it can serve (§3) |
| `ttft_ms_p50` | recent time to first token |

The score of a node for a **new** key is `free_sessions` first, then `lru_age_s`, then `ttft_ms_p50` as the tie. The primary also counts its own in-flight requests per node, authoritative for what it sent; the polled `sessions_busy` covers work that bypasses the primary (a direct local user, an older client).

### 2.3 Fairness

- **Between users.** One queue at the primary for requests no node can take now, served round-robin by user (not by request), so a user with 20 waiting requests does not delay a user with one.
- **Interactive and batch.** A request is batch when its credential is a service account or it carries the job header (`JOB_HEADER`, gate); everything else is interactive. Interactive requests are dequeued first. On a node with three or more sessions one session is **reserved** for typing (one interactive seat, decided by the owner 2026-11-24): batch cannot take the last free one. This is the intent of [ADVISOR_NODE](./DREAMFERENCE_RAGON_ADVISOR_NODE.md) ("interactive sessions high; Night Shift and the benchmark low"), done at the front door so it does not depend on the engine's priority scheduling.
- **Starvation.** A batch request that has waited 60 s is promoted to interactive for placement.

### 2.4 Limits

**There are no per-user or per-key limits in 1.0**: fair turns (§2.3) and the one reserved typing seat are all (owner, 2026-11-24). Limits stay on the 3.0 list (AUTH §19, quoted in §14); no setting for them is shipped, not even one that is off.

### 2.5 Back-pressure and the errors a client sees

The errors take the shape of the API imitated (AUTH §10: `{"error": {"message", "type", "param", "code"}}`), so Codex and the SDKs retry as they already do.

| Case | Status and body | Header |
|---|---|---|
| Waiting for a slot | no bytes yet; the connection is held up to `[node] queue_wait_s` (default 60) | none |
| Queue wait over, no capacity | `503`, `type: server_error`, `code: server_overloaded`, the message names how long it waited | `Retry-After: <s>` |
| No node serves the model | `404`, `code: model_not_found` (as today's gate) | none |
| Prompt fits no node | `400`, `code: context_length_exceeded`, the largest context in the group in the message | none |
| All nodes held (a benchmark on every node) | `503`, the hold's own message as today's gate writes it | `Retry-After: 0` as today |
| Primary cannot reach the chosen node before the first byte | retried once on the next-best node (§4.3), else `502`/`503` as above | |

The agent retries 5xx about 30 times and honours `Retry-After` without a cap (swe-bench.md): the table is built for that.

## 3. Different nodes

The fleet is mixed: 128 GB full nodes, 64 GB entry-level nodes, a Windows HIP node, Mac Metal nodes, different engines (`ragon-serve`, the LoRA fallback engine), an adapter installed on one node only, and different `--sessions` and `max_context` per node.

### 3.1 One catalogue

`/v1/models` lists the **union** of the members' served ids and aliases (`ragon`, `ragon-default`, the Codex default name; AUTH round 5), each once, with the engine in `owned_by` as today. A model is listed while at least one member in state `ok` serves it. `context_window` is the largest among its members. Adapters appear as they do on a single node ([ADAPTERS](./DREAMFERENCE_RAGON_ADAPTERS.md)), listed when at least one member has one installed and verified.

### 3.2 A request only some nodes can serve

Eligible nodes for a request are those whose `models` (and `adapters`, when the request names one) include it and whose `max_context` is at least the prompt plus the requested output. Placement (§2) runs on the eligible set only. A sticky entry to a node that is no longer eligible is dropped. An empty set gives `model_not_found` or `context_length_exceeded` (§2.5), never a silent downgrade to another model. A request for an adapter only one node holds therefore queues for that node while the other six idle; that is the correct behaviour, and `node balancer status` (§8.2) shows it.

### 3.3 "Same quality, not same results"

The owner's ruling, 2026-11-18: "The goal is for windows to have the same quality not the same results … This should also be true for other platforms like mac". The endpoint promises:

- **Quality parity by admission.** A node joins the pool for a model id only (owner, 2026-11-24: yes) when its platform has passed the quality table for that model ([PLATFORM_QUALITY](./DREAMFERENCE_PLATFORM_QUALITY.md)). A member that has not passed is listed as `member (not serving <model>)` and receives nothing for it.
- **A conversation stays on one node** while it can (§2.1), so its own history is not answered by two numerically different engines in turn.
- **No promise of identical output.** The same prompt sent twice may be answered by two nodes and differ token by token, with a fixed seed and temperature 0 too: different hardware kernels round differently. The endpoint says so in its documentation and never claims otherwise.
- **For debugging and measurement** a request may name a node with `X-Ragon-Backend: <node id>`, honoured only for admin users and service accounts (owner, 2026-11-24: yes), and the answer carries `X-Ragon-Served-By: <node id>` for them; the answer then comes from that node or fails.

## 4. Streaming and long turns

### 4.1 Pass-through

The proxy core is the gate's existing streaming code (`pipe`, `pipe_response`, `copy_body`, one request per connection, `Connection: close` towards the engine) pointed at a peer's gate instead of the local engine. Server-sent events are copied chunk by chunk as they arrive; the proxy never waits to fill a buffer and never rewrites the body of a streamed answer (the usage tap only reads). The budget for what the hop costs is §7.2.

### 4.2 Timeouts and cancellation

- Connect to a node: 2 s. A node that does not accept in 2 s is marked `unhealthy` for 10 s.
- First byte from the node: no limit beyond the engine's own prefill time (a 200,000-token prefill takes minutes); the client's own timeout decides.
- Idle stream: none by the proxy while the node's connection is alive; the proxy sends nothing of its own into a stream.
- **Cancellation reaches the node.** The existing `watch_client` notices the client's close; the proxy closes its connection to the node in the same step, and the node's gate closes the engine's, which frees the session. Checked by a test (§11).

### 4.3 A node dying mid-stream; retries

- **Before the first byte has gone to the client**, a failure to connect or a reset by the node is retried once on the next-best eligible node. Generation is stateless on the node's side and the tools of a Ragon turn are run by the client, so a retried request cannot run a tool twice.
- **After the first byte** the proxy never retries. It ends the stream the way the API ends a failed stream (`response.failed` for the Responses stream, an error `data:` line without `[DONE]` for chat completions, with `code: server_error`) and closes. The client sees the failure and decides; Codex discards the partial turn and re-sends, which places the session elsewhere (§2.1, one re-prefill).
- **Never a silent double run of a tool-calling turn:** the proxy keeps no copy of a streamed answer to replay, and starts a second generation only for a request that has produced no bytes. If a server-side tool is ever added to the endpoint, a request carrying it is excluded from retry.
- **`Idempotency-Key`.** When a client sends one, the primary answers a second request with the same key and user, while the first is in flight, with `409`. This is for clients that retry blindly; Codex does not need it.

## 5. Security and trust

The primary becomes the one door, so it is also the one place to get right.

- **TLS.** The gate serves HTTPS today (AUTH §6, owner 2026-11-19: "TLS NOW"; plain HTTP is refused except on loopback at 1.0). The primary-to-member hop is HTTPS too, against the CA the primary pinned for that member at pairing. The README's "Accepted by design" says the local network is trusted, the same premise as RAGON_EGRESS ("Ragon assumes the local network is trusted") and SECURITY_REVIEW item 10 ("trusted-LAN premise"). Those say the LAN needs no more than the node's own TLS; they do not say a hop may be plain, and AUTH's TLS decision is the later word. **The hop stays encrypted** (owner, 2026-11-24): HTTPS on every wire, a wired LAN included. The cost, a little CPU per connection, is measured in §7.2.
- **Node-to-node authentication.** The primary calls a member with its own service account's key (AUTH §13: `ragon-node-<short id>`, created on the primary, key in a 0600 file). A new account property, `forwarder`, is set only on the primary's own service account. A member accepts `X-Ragon-User: <user id>` and `X-Ragon-Role` only from a caller authenticated as that account **and** whose source address is the primary's resolved address; from anyone else the headers are ignored. The member then skips the live check against the primary (AUTH §7) for forwarded requests: the user's token never leaves the primary, and the request costs one check, not two.
- **No bypass of the primary by talking to a member.** Today any client may talk to a secondary, which checks against the primary. In a group (its endpoint is on from 0.9.10), **a member accepts requests only from the primary and from loopback**; any other client address is refused with `403`, `code: use_group_endpoint`, whose message names the group's endpoint. This holds **from 0.9.10, and the endpoint is on for every group from 0.9.10** (owner, 2026-11-24 and 2026-11-25: on right away, not at 1.0, and no group is left off), not from 1.0. The switch is `[node] endpoint_only`, which `node balancer add` sets on the member and `remove` clears; nobody sets it by hand. **Unaffected:** a single node (its own primary) and a primary with no members: neither has the setting. In 0.9.10 the primary calls a member with its own service account's key and the member checks that key and the source address (S1.7); the forwarded user header and the skipped second check (S2.1, S2.2) follow in 1.0. Cost for debugging: a developer cannot point a client at a member; the replacements are `X-Ragon-Backend` through the primary (§3.3), `ragon-admin node debug-direct <node> --for 10m` (opens the member to one named client address for a stated time, logged; S1.8) and loopback on the member itself.
- **Audit lines.** The gate's request line (AUTH §10) gains `endpoint=1 backend=<node id> place=new|sticky|moved|retried queue_ms=<n> session=<8 hex of the key hash> reused=<n>/<n>`; the member logs the same request once with `forwarded=1` and no user name beyond the id. Nothing about the prompt or answer is logged. Usage per user stays on the primary and is never exported (AUTH round no. 24).
- **Air gap and the egress audit.** Nothing here sends anything off the LAN, and load polling carries no content. The audit (AUTH round no. 25: "allowlists the gate of the resolved node and of its pinned primary … and still fails on any other address or DNS query") traces the agent's own connections, which now go to the primary only. The primary's connections to members are LAN traffic by the gate process, outside that trace, so they are declared the way the tunnel is (REMOTE_ACCESS §6): a verdict line "Group endpoint: the gate forwards to <member names>, by node id, on port 8000" and an `audit egress --group` check that every connection of the primary's gate is to a paired member's resolved address. Any other destination fails. `/airgapped on` changes nothing, as for login.
- **Remote access** reaches only the primary: `remote setup` is run on the primary, and a member's gate listens on the LAN interface only when `endpoint_only` is on, never on the overlay.

## 6. Failure of the primary

Today the primary is a single point of failure for sign-in. With the endpoint it is one for **work**: nothing reaches a model while it is down, although every engine is healthy. Running streams die with it (the proxy process holds the connection); sessions' caches survive on the members, and the sticky table is rebuilt by asking them (§2.1).

| Option | What it costs | What it gives |
|---|---|---|
| A. Accept the outage | nothing built | the group is down until the primary is back; the same exposure the primary has for sign-in today |
| B. Clients fall back to a known member | a member cannot authenticate (no accounts, 60 s cache, then "authentication server unreachable", AUTH §7) | works for about a minute for tokens already seen, and in 0.9.x mode for unauthenticated requests; useless after that, and it reopens direct access (§5) |
| C. Standby primary | a second node holds a replica of the accounts store and the signing key (hourly export/import, AUTH round 17), advertises `auth=standby`; promotion is one command, `ragon-admin node promote <node>`, which imports the last export on that node, makes it the primary and points every member at it over SSH; automatic promotion needs a quorum the group does not have, so it is manual | back in about five minutes by hand; users keep their sign-in only if the signing key is replicated, which is a stored secret on a second machine |

**In 0.9.10, before the promotion command exists** (this follows from the rulings of 2026-11-24 and 2026-11-25): members refuse direct clients (§5), so option B is not available, and the group is down for work until the primary is back or an administrator re-points the group by the documented manual steps (import the last accounts export on another node, make it the primary, set each member's primary over SSH; S1.12 writes the steps). Nothing promotes itself. A single node is not affected.

**Decided (owner, 2026-11-24):** at 1.0, A with **one tested command** for manual promotion (`node promote`, C without its automation, using the export/import AUTH already specifies); at 1.1 a **synced standby** (S5.7); **never automatic for now**. A standby that promotes itself is not built: two primaries issuing tokens is worse than an outage.

## 7. Single-node users, and the added-time budget

### 7.1 Nothing changes for one node

A lone node, or a primary with no members, takes the gate's existing path: no placement, no table, no polling, no second hop. The code that runs is the code that runs today (and a member refuses nothing, §5), and a test (§11) compares the bytes and timing of a request with the feature compiled in against one without.

### 7.2 The budget

Added by the proxy when a request is forwarded to a member, on the LAN, measured against the same request sent to the member directly:

| Quantity | Budget | How it is measured |
|---|---|---|
| Time to first byte, 99th percentile | under 5 ms added | a fake backend answering with an SSE head and the first chunk immediately; 10,000 requests, direct versus through the proxy, client-side timestamps, three runs |
| Per streamed chunk, 99th percentile | under 1 ms added (median under 0.2 ms) | the same fake backend sending a chunk every 5 ms; arrival time of each chunk, direct versus through; 10,000 chunks at 1, 8 and 32 concurrent streams |
| Placement decision | under 0.5 ms | timed inside the process, 100,000 decisions |
| CPU of the primary at 32 streams | under one core | measured while the above runs |

If the gate's Python code cannot meet the chunk figure at 32 streams, the proxy core moves into a Rust crate in `ragon-rs/` (one task per stream, the gate's routes unchanged); S0.9 is the decision gate.

## 8. Operations

### 8.1 Adding and removing a node

- `ragon-admin node balancer add <node>`: the node must be paired and serve a model; the primary pins its CA, creates nothing else (the member's service account exists from AUTH §13), and starts polling. `remove <node>` drains it first (§8.2), then forgets it.
- Setting a node up with `node provision` pins the primary and, when the primary's `[node] backends = "paired"` (the default once the feature is on), adds it.

### 8.2 States, drain, and today's rules

| State | New sessions | Running turns | Set by |
|---|---|---|---|
| `active` | yes | yes | default |
| `draining` | no | finish | `ragon-admin node drain <node>`; automatic ahead of a night window |
| `held` | no | finish (a benchmark: the run owns the node) | the node itself: the run hold (`run.json`), the GPU lock, a night |
| `down` | no | | health failing for 5 s |
| `out` | no | | `[node] group = "out"` |

- **A benchmark on a node** is a `held:bench` node: the gate's refusal (503 to anyone outside the run's subnet) stays for direct callers, and the primary treats it as unavailable instead of forwarding into a 503 answer. **A benchmark on the primary's own engine must not make the endpoint refuse everyone:** today's hold refuses any caller outside the run's subnet, and the primary would be one. The hold covers only the primary's own model (its local backend); the endpoint keeps serving through the other nodes (owner, 2026-11-24, §14).
- **The GPU lock** (`~/.ragon-gpu.lock`, held by an engine lane) maps to `held:gpu-lock`; the lane that releases it before 20:00 UTC returns the node to `active` by itself.
- **Night windows** (20:00-08:00 UTC on free boxes) become `held:night` while a night unit holds the node; "boxes 6 and 7 are night-first, boxes 2-5 lane-first" is the order in which a hold is taken, unchanged; the endpoint only reads the result.
- **"This node is never a benchmark box"** and its relatives are the node's `group` setting: `serve` (the default), `hold-only` (takes a hold, never public traffic), `out`.
- **`ragon-admin node balancer status`** prints the table: node, state and why, `sessions busy/total`, queue, models, adapters, cache reuse over the last hour, health age. `node balancer sessions [--user <name>]` lists sticky entries (hash, node, age). `node drain <node> [--wait] [--reason night|bench|maintenance]` and `node undrain <node>` set and clear the operator hold.
- **Desktop app and `ragon web`:** the status bar shows one line, "Group of <n> nodes (<m> serving)", and `/status` expands it to the table. They never show a node's address.

### 8.3 Audit

See §5. The status table's counters come from the same lines.

## 9. What it replaces or changes

- **Client discovery (mDNS tiers).** The launcher's tiers are unchanged up to resolving *a* node. After resolving, it reads `/ragon/v1/info`; when the answer has an `endpoint` object (`{id, name, url}`), it switches to that and remembers the endpoint's id in `node.json`. The least-loaded pick among several serving nodes (`pick_least_loaded`, owner Q108) applies only when no endpoint is advertised (and for a single node alone it is unchanged). Members advertise `role=member` and are skipped by a browse that wants a node; the primary adds `endpoint=1` to its record (it already carries `auth=primary`).
- **The launcher's resolve step** no longer asks "who is your primary" separately for the issuer: the endpoint is the primary. The issuer and the backend base are the same URL, one allowlist entry (AUTH round no. 12).
- **The coordinator's routing of rows** is unchanged for jobs over SSH. Its model requests (reviewers, the local-model route) go to the endpoint; where a row must run on a named node it sends `X-Ragon-Backend`.
- **The benchmark harness's endpoint list** (`[swe_bench] nodes`, lanes reached through a relay on the run's network gateway) becomes one URL, the primary's, plus the run's `--sessions-per-node` target (§11); the relay remains for runs on an unconfigured group.
- **Night Shift lanes** (`[night] nodes`, `paired`) likewise become the endpoint; `/night add --on <node>` stays.
- **Windows and Mac members** serve as members, never primaries (AUTH §7), with the auth check in front of their engine at version 1.0.

## 10. Config summary

```toml
[node]
group = "serve"          # serve | hold-only | out        (every node)
endpoint_only = false    # members accept requests only from the primary; set by node balancer add, cleared by remove
backends = "paired"      # primary only: "paired" (the default) = every paired node serving a model, or a list of node ids; no setting turns the endpoint off for a group (owner, 2026-11-25)
queue_wait_s = 60
```

## 11. Measurements that gate each slice

| Slice | Gate (the slice is not released until it holds) |
|---|---|
| 0 | per-task speed with 1 versus 4 sessions per node on the seven-node SWE slice shape: a task's steps per minute through the proxy at 4 sessions per node is at least 70% of its direct, one-session figure, and the total throughput is at least 3.5x one node's; added time within §7.2; cache reuse through the proxy: `reused / prompt tokens` over a 20-turn agent session matches direct use within 2 points; cancellation frees the node's session within 1 s |
| 1 | the catalogue equals the union on a mixed group (full, entry-level, one with an adapter); a request for the adapter goes only to its node; an over-long prompt goes only to a node that fits; a member with `endpoint_only` refuses a direct client with `use_group_endpoint` and admits the primary and loopback; a single node and a primary with no members refuse nobody (byte-for-byte test); the same checks pass on a real pair of nodes, with the endpoint on, before 0.9.10 ships |
| 2 | a revoked key is refused through the proxy within the 60 s of AUTH; a forwarded-user header from a non-primary caller is ignored (test); a vendor SDK client against the primary passes the AUTH suite test |
| 3 | `ragon web` search, images and speech work against the primary alone, with the members' sidecar ports closed to the client |
| 4 | a drain finishes running turns and places the next ones elsewhere; a benchmark hold on the primary does not refuse the endpoint; a night start moves traffic off a node with no failed request |
| 5 | **kill-a-node**: stop a member's engine mid-stream under 8 streams; the streams on it fail with the shaped error, every other stream is unharmed, the next turn of each killed session succeeds on another node, and no tool call runs twice (a counting fake tool); `ragon-admin node promote` restores the endpoint within five minutes on a test group, as one command |

## 12. Delivery in slices

Main freezes on Wednesday 2026-11-25 at 12:55 UTC for 0.9.9, which ships on Sunday 2026-11-29 with the accounts work (AUTH). **Nothing in this spec is in 0.9.9** (owner, 2026-11-24 and 2026-11-25): the accounts rows are the freeze's whole budget, and the gate is the file they change most. **The build starts today (2026-11-24)**, on branches. Rows that only add new files can land before the freeze, because nothing calls them while the gate's code path is unchanged; a row that edits a file the freeze covers (`dreamference/vllm_server/model_gate_service.py`, `dreamference/node/node_settings.py`, `dreamference/swe_bench/swe_bench_runner.py`, the launcher's resolve step, the accounts modules) waits for Sunday's release. The backlog `backlog/plan-primary-endpoint.md` marks each row. This spec's own commit is documentation only. 1.0 is planned before the end of the year. **0.9.10 carries the whole single endpoint, switched on** (owner, 2026-11-25), so its date moves as late as the rows listed in `backlog/plan-primary-endpoint.md` need; the plan gives the count, the sizes and the estimate.

Each row has a mechanical check; **L** is for the local model (small, testable, one named test), **C** is for a cloud model with review.

### Slice 0: a sticky reverse proxy for `/v1/*` (release: 0.9.10, on for every group; build starts 2026-11-24)

Enough for a benchmark run with one task per node.

| Row | What | Check | Who |
|---|---|---|---|
| S0.1 | `[node] backends` and `group` settings, parsed and validated (`dreamference/node/node_settings.py`) | unit test: defaults, bad ids refused | L |
| S0.2 | `session_key(headers, body, user)`: the order of §2.1, pure function; and a probe that records what the real Codex client sends (`session_id`, `prompt_cache_key`, `previous_response_id`) | table test of 12 cases; the probe's recorded headers added to the spec as a table | C (the probe), L (the function) |
| S0.3 | sticky table: in-memory LRU with idle expiry and a size cap | unit tests: expiry, cap, per-user scoping | L |
| S0.4 | `place(key, nodes, table)`: eligibility, score, tie-break, the busy-holder 5 s rule | table test of 20 states; property test that a sticky key never moves while its holder is `active` with a free session | L |
| S0.5 | `GET /ragon/v1/load` on the gate and `X-Ragon-Load` on responses | gate test with a stub engine: fields present, `hold` follows `run.json` | L |
| S0.6 | the primary's poller and per-node state (health, hold, in-flight counts) | test with three fake gates: one slow, one down | C |
| S0.7 | forwarding to a member: HTTPS to the pinned CA, streamed both ways, usage tap kept (`pipe_response`), one retry before the first byte | gate test with a fake member: SSE copied chunk by chunk, retry only before the first byte, none after | C |
| S0.8 | cancellation: client close closes the member connection | test: the fake member sees the close within 1 s | C |
| S0.9 | the latency harness of §7.2 and its report; the Rust-or-Python decision | the report states the six figures and the decision; budget met or the row opens a Rust-core row | C |
| S0.10 | shaped errors of §2.5 and the wait queue with per-user round-robin | tests: each table row's status, body and header; fairness test with two users | L |
| S0.11 | the benchmark harness takes one endpoint and a `--sessions-per-node` figure | harness test with fake nodes; one real 14-task run, one task per node, no 5xx | C |
| S0.12 | `docs/dev/balancer.md`, the AGENTS.md line, spec status | the docs test (links resolve, row in `specs/README.md`) | L |

### Slice 1: the catalogue and the closed door (0.9.10, on for every group)

| Row | What | Check | Who |
|---|---|---|---|
| S1.1 | `/v1/models` union with aliases and largest context | test: three fake nodes, one model each, one shared | L |
| S1.2 | eligibility by model, adapter and context (§3.2) | table test; `model_not_found` and `context_length_exceeded` bodies | L |
| S1.3 | a member not yet quality-admitted for a model is listed but receives none of it | test with a platform-quality fixture | L |
| S1.4 | `X-Ragon-Backend` for admins and service accounts | test: honoured, ignored for a user, failing when the node is down | L |
| S1.5 | the quality-admission table read from PLATFORM_QUALITY's result file | test against a fixture; real file read on the QA run | C |
| S1.6 | the mixed-group acceptance (full + entry-level + adapter node) | the gate row of §11 on real nodes | C |
| S1.7 | `endpoint_only`: a member accepts only the primary (its service key and source address) and loopback, refuses others with `use_group_endpoint`; set by `node balancer add`, cleared by `remove` | tests: direct client refused, primary and loopback admitted, setting absent on a single node and on a primary with no members | C |
| S1.8 | `ragon-admin node debug-direct <node> --for <t>` | test: one named address admitted, window expires, logged | C |
| S1.9 | the default: a primary with paired members runs the endpoint with `backends = "paired"`; no setting turns it off for a group | test: a primary with members forwards with no setting written; a primary with none takes the existing path | L |
| S1.10 | the upgrade path for an existing group: on its first start in 0.9.10 the primary runs `node balancer add` for every paired member over SSH (pins the CA, sets `endpoint_only`); a member it cannot reach is listed as "not closed" in `balancer status` and is retried at each start | test with fake SSH: members set, an unreachable one listed and retried | C |
| S1.11 | the real-pair test: two real nodes, endpoint on; a direct client refused by the member, the primary admitted, a streamed turn through the primary, a member stopped mid-stream gives the shaped error | the script's pass line on two real nodes, run before 0.9.10 ships | C |
| S1.12 | docs: what to do while the primary is down in 0.9.10 (the manual re-pointing steps of §6) | docs test | L |

**Rows pulled forward from later slices into 0.9.10**, because the endpoint is on and the door is single: S2.7 (pin the member CA at `balancer add`), S2.8 (the egress audit of the group's gate), S3.6 (the launcher stops picking a member; the `info` object and the member advert, since a client that picks a member is refused), S4.1 (states), S4.3 (`node balancer add`, `remove` and `status`; `sessions` stays 1.0), S4.4 and S4.5 (the holds; without S4.5 a benchmark hold on the primary refuses the whole endpoint).

### Open for the owner

- **Upgrade order.** A member that runs 0.9.10 before its primary does must not close its door early. The plan assumes `endpoint_only` is set only by the primary's `balancer add` (S1.10), so an older or not-yet-upgraded member stays open until the primary reaches it. Say if the primary should instead refuse to switch on until every member runs 0.9.10.
- **A kill switch.** The rulings leave no setting that turns the endpoint off for a group, and the spec ships none. If the owner wants an operator escape for a bad release (a member's `endpoint_only` cleared by hand, or `backends` set empty), it is one more row; say so.
- **Clients while the primary is down.** S5.6 (the 503 with `Retry-After` that stops a tight retry loop) stays in 1.0. In 0.9.10 a client that talks to a down primary gets a connection error, and one that talks to a member gets `use_group_endpoint`. Say if S5.6 should ship in 0.9.10.

### Slice 2: auth through the proxy (version 1.0)

| Row | What | Check | Who |
|---|---|---|---|
| S2.1 | the `forwarder` account property and the member-side acceptance of `X-Ragon-User` (§5) | test: accepted only with the primary's key and address, ignored otherwise | C |
| S2.2 | forwarded requests skip the live primary check on the member | test: no call to the accounts service for a forwarded request | C |
| S2.3 | (moved to S1.7 and S1.8: the door closes with the endpoint, from 0.9.10) | | |
| S2.4 | audit line fields of §5 | test on the log line's exact format | L |
| S2.5 | (dropped: no per-user limits in 1.0, §2.4) | | |
| S2.6 | the vendor SDK suite test run against the primary with two members | the existing AUTH suite test, pointed at the group | C |
| S2.7 | TLS member hop: pin the member CA at `balancer add` | test: a changed CA refuses with the AUTH message | C |
| S2.8 | `audit egress --group` | test with a fake trace: a member passes, a stranger fails | C |

### Slice 3: sidecars (version 1.0)

| Row | What | Check | Who |
|---|---|---|---|
| S3.1 | member sidecar advertisement (`sidecars` in `/ragon/v1/load`) | test | L |
| S3.2 | proxy search and image routes to a member with the sidecar | test with two fake members | L |
| S3.3 | speech and voice: WebSocket proxy sticky by connection, the Bearer rule of AUTH round 7 kept | test: upgrade passes, a closed client closes the member | C |
| S3.4 | `ragon web` points its model and sidecar calls at the endpoint | test: no member address in any page or header | L |
| S3.5 | bridges (Signal, chat) use the endpoint | the existing link tests, endpoint config | L |
| S3.6 | the `info` object of §9 and the member advert `role=member` | launcher test: a browse skips members; `pick_least_loaded` used only without an endpoint | C |

### Slice 4: drain and status (version 1.0)

| Row | What | Check | Who |
|---|---|---|---|
| S4.1 | the states of §8.2 and their transitions | state-machine test, every edge | L |
| S4.2 | `node drain|undrain` | CLI test against the primary's API | L |
| S4.3 | `node balancer add|remove|status|sessions` | CLI tests, table output snapshot | L |
| S4.4 | the run hold, the GPU lock and the night window mapped to holds | tests with the three files | C |
| S4.5 | the primary's own benchmark hold applies to its local backend only | test: a held primary still forwards | C |
| S4.6 | desktop status line and `/status` | app test, a snapshot | C |
| S4.7 | `ragon web` status page | web test | L |
| S4.8 | the night chain's drain-ahead (a night start drains 10 minutes before) | scheduler test | C |

### Slice 5: failure handling (version 1.0; the standby in version 1.1)

| Row | What | Check | Who |
|---|---|---|---|
| S5.1 | mid-stream failure endings per API (§4.3) | test: both shapes parse in a vendor SDK client | C |
| S5.2 | table rebuild by `GET /ragon/v1/sessions` after a restart | test: restart the primary, the next turns reuse their nodes | C |
| S5.3 | kill-a-node test (§11, slice 5) as a script | the script's pass line on a three-node test group | C |
| S5.4 | `Idempotency-Key` | test | L |
| S5.5 | `ragon-admin node promote <node>`: import the last export, make it primary, repoint every member over SSH; runbook and test | the §11 five-minute run, timed, one command | C |
| S5.6 | primary-down message and the clients' retry behaviour | test: Codex gets a 503 with `Retry-After`, no tight loop | L |
| S5.7 | synced standby primary (hourly export to the standby, signing key included); never promotes itself | **1.1**: test a round trip | C |
| S5.8 | docs: the outage and what to do | docs test | L |

### Release plan

| Slice | Release | Reason |
|---|---|---|
| spec and docs | 0.9.9 (documentation only) | nothing runs |
| 0 and 1, and the rows pulled forward below | 0.9.10, endpoint on for every group | the benchmark needs them; a group's primary becomes its one address at the upgrade and its members' doors close at once; a single node is unaffected |
| 2, 3, 4 | 1.0 (release candidate window) | the forwarded user and the sidecars, drain and status |
| 5 | 1.0 (S5.1-S5.6), 1.1 (S5.7) | the promotion command, the stream endings and the kill-a-node test follow in 1.0; the door is single from 0.9.10 (§6 says what happens meanwhile) |

## 13. Tests and keeping it honest

- A group in a box: three fake gates and a fake engine in the test suite, no real node, no port 8000 (conftest forbids real connections to the model server's ports).
- Every number in §7.2 and §11 comes from a script kept in `scripts/`, not from a one-off.
- The gate keeps its rule that read-only `GET`s always pass and that its log never contains the loading monitor's readiness words.

## 14. Earlier decisions this changes

Each is quoted from the record. Rows 1 and 3 are **changed by the owner on 2026-11-24**; the others are kept or narrowed as stated.

1. AUTH §7: "Every other node is a secondary and keeps no accounts … A client never needs the primary's address; the screen names the primary." **Changed by the owner, 2026-11-24:** in a group the primary is the address and clients talk only to it; the secondary becomes a member that clients do not call. The accounts spec's sentence is updated in the same change that ships the behaviour (see the last row), not before.
2. AUTH round no. 12 / §15: "the launcher … asking the resolved node who its primary is". Replaced by "the endpoint is the primary".
3. Owner Q108 (node.md): "With none remembered and two or more serving a coding model, the session uses the least loaded." **Changed by the owner, 2026-11-24:** kept only for groups with no endpoint; a single node alone is unchanged.
4. AUTH §19 and decision 9: "Hardening goes to version 3.0. Key rotation, refresh-token rotation, rate limits, lockout …". Kept: no per-user limits in version 1.0 (§2.4).
5. AUTH round no. 25: "the egress audit allowlists the gate of the resolved node and of its pinned primary". Now one address; the primary's connections to members are a declared group exception (§5).
6. SWE_BENCH §18: "a request from outside the run's network subnet is answered 503". On the primary's own engine this would refuse the whole endpoint, so the hold covers only the primary's own model (S4.5).
7. AUTH §6: "plain HTTP is refused except on loopback", and README "Accepted by design": the local network is trusted. Kept: the hop stays encrypted (§5).
8. Remote access §6: "The overlay is one more interface the model server already listens on". With `endpoint_only` a member does not listen on it.
9. Node `[swe_bench] nodes` and `[night] nodes` (lanes through a relay) become the endpoint for groups that configure it.

**Rows to change in the same change that ships the behaviour** (the accounts spec is not edited now):

| Where | Now says | Becomes | Ships with |
|---|---|---|---|
| AUTH §7 (the accounts spec) | "A client never needs the primary's address" | "In a group the primary is the address; clients talk only to it; members refuse direct clients while the group's endpoint is on" | S3.6 and S1.7 (the launcher switch and the closed door) |
| node.md, Q108 least-loaded pick | the session picks the least loaded of several serving nodes | the pick runs only when no endpoint is advertised | S3.6 |

## 15. The owner's answers of 2026-11-24 (the record)

| # | Question | Answer | Where |
|---|---|---|---|
| 1 | Per-user limits | none in 1.0; fair turns and one reserved typing seat; limits stay 3.0 | §2.3, §2.4 |
| 2 | TLS on the member hop | stays encrypted | §5 |
| 3 | `endpoint_only` | refuse direct clients **from 0.9.10, with the endpoint on for every group**; single nodes unaffected (2026-11-25: no group is left with the endpoint off) | §5, S1.7 |
| 4 | Primary failure | manual promotion with one tested command at 1.0; synced standby at 1.1; never automatic for now | §6, S5.5, S5.7 |
| 5 | Benchmark hold on the primary | covers only the primary's own model; the endpoint keeps serving through other nodes | §8.2, S4.5 |
| 6 | Interactive reserve | yes, one reserved typing seat | §2.3 |
| 7 | Release plan | build starts today; **2026-11-25 ruling: the endpoint ships switched on for every group in 0.9.10 and every member refuses clients that go around the primary; this replaces "slices 0 and 1 in 0.9.10 switched off"**; the rest 1.0; nothing in 0.9.9; the date of 0.9.10 moves as late as this needs | §12 |
| 8 | Quality admission per model | yes, before a member gets traffic | §3.3, S1.3 |
| 9 | Which-box header | yes, for admins and service accounts | §3.3, S1.4 |
| 10 | The primary is the address | yes: clients talk only to it | §1, §9, §14 |

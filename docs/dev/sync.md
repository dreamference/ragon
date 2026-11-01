# Sync: threads from every device on the node

Developer notes behind `ragon sync`. Spec: `specs/DREAMFERENCE_RAGON_SYNC.md`; what Phase 0 measured is its §16, Phase 1 as built its §17. Phases 0 and 1 built, Phase 2 (live turns) (spec §18), all **off by default** until `ragon sync enable`.

## The pieces

| Where | What |
|---|---|
| `ragon-rs/web/src/sync_store.rs` | The hub's store under `~/.ragon/sync/hub/` (0700): one file per rollout plus its record, tombstones, the sequence number; the prefix hash (`Chain`, 1 MiB chunks chained); the hybrid clock (`Hlc`, `Clock`); the transcript a device shows (`items_of`) |
| `ragon-rs/web/src/sync_hub.rs` | The routes under `/api/sync/`, each a 404 until `hub.json` says the hub is on |
| `ragon-rs/web/src/auth.rs` | `DeviceKind` (`browser`, `sync`) and `Scope` (`control`, `watch`) on every paired device |
| `ragon-rs/sync/` (`ragon-sync`) | The agent (`agent.rs`), its client of the hub (`client.rs`), the scan of Codex's folders (`scan.rs`), `device.toml` and `[sync]` (`config.rs`), `ragon sync …` (`cli.rs`) |
| `ragon-rs/src/sync.rs` | The launcher's routing, and the node found by the node tiers without asking its model server |
| `desktop/ui/src/sync.ts`, `SyncedView.tsx` | Other devices in `ragon web`'s sidebar: the list, the transcript, Archive and Delete |
| `dreamference/audit/sync_egress_audit.py` | `ragon-admin audit egress --sync` |
| `ragon-rs/web/src/sync_live.rs` | Phase 2's hub half: each thread's last 2,000 live events, whether a turn runs, the approvals waiting, commands to the owner's host and their answers, the offline queue (`queue.json`), the bridges' notices |
| `ragon-rs/sync/src/live.rs` | The agent as a host: attached to the app-servers listening on the control socket and `ragon web`'s socket |
| `desktop/electron/src/sync-host.ts` | The desktop app's main process as a host: its own app-server, followed and answered |

`ragon-sync` depends on `ragon-web-server` (the store's hash, clock and ids, `auth::write_private`), as `ragon-chat` does, so its tests run a real hub on loopback.

## Rules that are easy to break

- **Off means nothing.** No route answers, no folder is created, no unit is written until `ragon sync enable`. `Server::sync_store()` asks for `hub.json` on every request and opens the store only then; `Agent::load` returns `None` without `device.toml`. Each surface has a test.
- **An agent's secret is not a cookie, and a cookie is not an agent.** `auth::device_for_token` finds browsers only and `sync_device_for_token` agents only; the guard lets a bearer secret through on `/api/sync/` alone.
- **Codex's files are changed only by Codex.** A deletion or an archive state decided elsewhere is applied with `ragon delete --force`, `ragon archive` and `ragon unarchive`. Moving a rollout by hand breaks Codex's reads of it (Phase 0); a downloaded rollout's turns are never projected, so nothing of another device's is written into `~/.ragon` (`[sync] replicate` is not built, for that reason).
- **The node is found, never stored**, and only an address on the user's own network is a hub (`cli::hub_base`). The resolver must not ask the model server (`node::resolve`, not `node::resolve_host`): the audit allows the hub's port alone.
- **A rollout, not a thread, is the unit.** A revert writes a new rollout that starts from a byte offset of the one before; the hub keeps both and the transcript follows `history_base`.
- **A command from another device is the owner's act, through the owner's policy.** Both hosts pass every command through their own `policy.json` enforcer (`Policy::vet_outgoing` in Rust, `vetOutgoing` in the app) with the server requests still waiting, so an approval can answer only what waits, and only the two approvals in `sync_live::ANSWERABLE` are answered from elsewhere. The hub checks the scope (`control`) and that the approval waits, the turn runs, the folder was offered; the host checks again and does not take the hub's word. Both hosts (`sync_live`'s `carry_out` in Rust, `execute` in `desktop/electron/src/sync-host.ts`) behave the same: an approval that is not waiting is refused, and one that waits is answered in the thread it was asked in, whatever thread the hub names; a send, an approval or a stop about a thread kept on this device (in a `[sync] exclude` folder, or a throwaway: ephemeral or a subagent's) is refused with "this thread is kept on this device", a thread not seen yet being described first with `thread/read`, which loads nothing; a new thread starts only in a folder the host offers. A change to one host's checks goes to the other in the same commit, with its test.
- **Following needs the store.** `/api/sync/live/<thread>` answers only for a thread the store holds, so a thread kept on its device (`[sync] exclude`, `ragon exec`) is followed by nobody, whatever a host posts; its events make no notice.
- **The bridge policy is untouched.** No method was added: the hosts use requests the policy already allows.

## Tests

- `cargo test` in a copy of `ragon-rs/web` (beside `../airgapped`) and of `ragon-rs/sync` (beside `../web` and `../airgapped`): the store, the routes' guard, and the agent end to end against a real hub on loopback with a stand-in `ragon` (`ragon-rs/sync/tests/sync.rs`). The web crate's `night_shift_is_queued_and_read_for_a_phone_bridge` reads the real user's `[chat] night_repos`: run the suite with a scratch `HOME` (and `CARGO_HOME`, `RUSTUP_HOME` set).
- In the export: `cargo test --release -p ragon-launcher -p ragon-web-server -p ragon-sync`.
- `desktop/ui`: `src/sync.test.ts`. Python: `tests/test_sync_egress_audit.py`.

## Trying it

1. On the node: `ragon web start --lan` (an advertised node), then `ragon sync enable`.
2. On the node: `ragon web pair`; on the laptop: `ragon sync enable` and type the code (or `--code`).
3. Start a thread in the laptop's app; within a few seconds it is under **Other devices** in the node's `ragon web`, in the node's browser and on the phone's.
4. `ragon sync status` on either; `ragon-admin audit egress --sync` on the node.
5. `ragon sync disable` stops it and keeps everything; `ragon sync forget` deletes the hub's store (node) or the pairing (device).

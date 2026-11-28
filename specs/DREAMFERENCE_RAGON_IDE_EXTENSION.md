# Ragon IDE extension

**Status:** *Proposed for 0.9.11 (Fri 2026-12-18); nothing built.* This is a plan for a panel inside the editor. In it the user talks to Ragon, follows its plan, reviews diffs, answers approvals and applies changes. Owner request 2026-11-28: "Lets plan ide extension for 0.9.11". The extension is built to be as compatible as the protocol allows with the upstream Codex extension for VS Code. The owner's rules apply: copy ideas and interfaces, never code; user benefit over less work; no telemetry; unsigned previews before 1.0.

**Names:**
- The extension: `dreamference.ragon`, display name **Ragon**. The namespace `dreamference` is already held on Open VSX and on the VS Code Marketplace (DECISIONS 2026-11-25).
- The editor family: **VS Code family**, meaning VS Code, its Insiders build and the open-source builds that install from Open VSX.
- The panel: **the Ragon panel**.

**Pinned Codex:** `rust-v0.158.0`, commit date 2026-09-27. Every `codex-rs/…` path below is at that tag.

**Builds on:**
- [RAGON_DESKTOP_PARITY](./DREAMFERENCE_RAGON_DESKTOP_PARITY.md): `desktop/ui`, the policy file `ragon-rs/web/policy.json`.
- [AUTH](./DREAMFERENCE_AUTH.md): the 0.9.9 accounts, `account/ragon/signIn`.
- [AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md) and [EGRESS](./DREAMFERENCE_RAGON_EGRESS.md).
- The remote-sessions spec (`specs/DREAMFERENCE_RAGON_REMOTE_SESSIONS.md` on branch `spec/remote-sessions`, not yet on main): the session WebSocket of 0.9.10.

---

## 0. Decisions

### 0.1 The route question, settled first

**Can the upstream Codex extension be pointed at `ragon`?**

**Technically yes, but only through a setting its makers mark for development.**
- The extension's manifest has the setting `chatgpt.cliExecutable`. Its description: "DEVELOPMENT ONLY: Path to the Codex CLI executable… You do NOT need to set this unless you are actively developing the Codex CLI". *Verified* in the Open VSX package manifest, version 26.51120.50957, fetched 2026-11-28.
- The vendor's settings page adds a warning: "manually overriding the bundled executable can prevent parts of the extension from working". *Verified*, the vendor's IDE settings documentation, fetched 2026-11-28.
- The extension ships eight platform packages: linux, alpine, darwin and win32, each for x64 and arm64. This implies it bundles its own agent binary. *Verified*: the Open VSX listing has eight targets. The binary inside was not inspected.
- No environment variable for the binary is documented. *Unverified*: nothing was found, which proves nothing.
- Ragon's side keeps it possible:
  - AUTH §5.8 (owner 2026-11-19, auth round 6) keeps Codex's `account/*` requests identical "so IDE extensions built for Codex keep working".
  - A stdio app-server marks its sessions as coming from VS Code (`codex-rs/app-server/src/lib.rs:445`, `SessionSource::VSCode`). *Verified* in the source.

**Would its licence and its calls home allow us to recommend it?**

**No.**

| Point | Evidence | Status |
|---|---|---|
| Licence | The manifest says "SEE LICENSE IN LICENSE.md". That file says only "See https://openai.com/policies/row-terms-of-use." The terms forbid users to "modify, copy, lease, sell or distribute any of our Services" and to "reverse engineer, decompile or discover the source code". | Licence text *verified* (Open VSX package and the terms page, 2026-11-28). The legal reading, whether pointing the extension at another binary counts as modifying it, is *unverified* and not ours to make. |
| Source | "The VS Code extension is not open sourced" (a maintainer on the openai/codex issue tracker, issue 2938). Issue 5822 asks to open it. | *Reported* by the research pass from the issue pages, not re-fetched |
| Calls home | Not measurable within our rules: running it may contact the vendor. Issue 5882 reports that the extension did not honour the user's telemetry export setting. | Its own telemetry and any remotely loaded webview assets are *unverified*. Issue 5882 is *reported*, not re-fetched. |
| Release pace | Several builds a week (26.51117…, 26.51120…). Each bundles a newer agent than our pin of 2026-09-27, so the protocol it expects drifts from `rust-v0.158.0`. | The build numbers are *verified* on Open VSX. The size of the drift is inference from dates. |
| Our product rules | It does not know `account/ragon/signIn`. Its sign-in UI leads to the vendor's sign-in. It does not enforce `policy.json`. It cannot be covered by the egress audit. | Inference from the protocol and the policy file |

The no-telemetry rule and the egress rule ("the extension opens no connection except to the node") cannot be met by a closed binary that we cannot audit. User docs therefore do not recommend it. We **keep it working where that costs nothing**: the app-server stays Codex's protocol, and test T9 runs a protocol client that sends the requests a Codex IDE client sends. But we do not test the vendor's binary, ship it, or support it.

### 0.2 Three routes compared

| | (a) Support the upstream extension against `ragon app-server` | (b) **Our own extension: same app-server protocol, `desktop/ui` in a webview, same policy file** | (c) Extend today's tool server (`ragon-admin mcp`) only |
|---|---|---|---|
| Compatibility with upstream | Highest on paper: it *is* the upstream client. In practice it drifts with each vendor build and the setting is development-only. | Same protocol (`codex-rs/app-server-protocol`, v2) and same context format (§4). A thread started in either one replays in the other. | None: it is a tool server for other agents, not an agent panel |
| Work | About 3 agent-days: docs, T9, a `ragon` that answers its start-up | About 23 agent-days (§8) | About 6 agent-days, and it still needs an editor companion: `IDEState` is empty without one and `ide_apply_diff` always answers `not_applied` (CLI §4.10) |
| Risk | Legal (proprietary terms), egress unknown, breaks without notice, users sent to the vendor's sign-in | The schedule (two lanes, §8.3) and coupling to the UI's host messages (§3.3) | Gives no panel: the user gets nothing they can see |
| What the user gets | A panel we do not control. It cannot show Ragon's sign-in, air-gapped levels or nights. Support questions we cannot answer. | One Ragon panel in the editor that looks and behaves like the desktop app's thread view. Editor context, native diffs, approvals, Ragon accounts, works air-gapped. | Other agents can read the editor. Nothing for Ragon's own users. |

**Recommendation: (b).**
- It is the only route that meets the no-telemetry and egress rules, uses Ragon's accounts, and enforces the same policy as the desktop app and `ragon web`.
- It keeps the user's threads interchangeable with every other Codex-protocol surface.
- Route (a) stays a *passive* compatibility promise, covered by T9.
- Route (c)'s tool server stays as it is. The extension can later fill its `IDEState` (§9, not in v1).

### 0.3 Decisions taken here (each can be reversed by an owner answer in §10)

| # | Decision |
|---|---|
| D1 | The VS Code family only in v1. JetBrains is the next phase, sized in §7. |
| D2 | The extension is a second **host** of `desktop/ui`. It plays the role the desktop app's main process plays: it spawns `ragon … app-server` over stdio and enforces `policy.json` on every frame. |
| D3 | The extension opens **no network connection of its own**. Everything goes through the `ragon` child it spawns. That child opens connections only to the node (§6). |
| D4 | Editor context uses Codex's exact text format (§4.1). Diagnostics are an added section: named difference N1. |
| D5 | File changes are applied **by the agent** after an approval, exactly as in the desktop app. The extension never writes files from a diff (`fs/writeFile` is not in `policy.json`). |
| D6 | Distribution: an unsigned `.vsix` on the GitHub release, plus Open VSX. The VS Code Marketplace waits for the project account (§10 Q5). |

### 0.4 Observed versus only read

- *Observed* (on this machine, 2026-11-28):
  - the strings in the installed `ragon` binary: `# Context from my IDE setup:`, `## My request for Codex:`, `ipc.sock`, `codex-ipc`, `ide-context`, `no-handler-for-request`. The branded build keeps the delimiter, so the extension sends it unchanged.
  - The upstream manifest fields above.
- *Read only*: every `codex-rs/…` and `desktop/…` line reference.
- *Not run*: the upstream extension (our rules forbid running it, §0.1), and a VS Code instance.

---

## 1. Scope

**In v1 (0.9.11):**
- The Ragon panel in the side bar and as an editor tab: new thread, thread list, conversation, plan, diffs, approvals, composer, model and reasoning pickers, the air-gapped level, stop and steer.
- Editor context: active file, selections, open tabs, diagnostics; "add file" and "add selection" commands.
- Diffs in the editor's own diff view, with Accept and Decline. Command approvals. Review of a turn's changes. Undo of a turn.
- Finding the node: the local binary first; then, as a gated row, attaching to a session on the paired node.
- Sign-in with Ragon accounts. Air-gapped behaviour.
- Packaging: `.vsix` on the release and on Open VSX; `ragon ide install`.
- Tests: policy conformance and a headless editor run in CI.

**Not in v1:** see §9.

## 2. Terms

- **Host:** the program that loads `desktop/ui` and answers its host messages. Today: `electron` (the desktop app) and `web` (`ragon web` in a browser), `desktop/ui/src/bridge.ts:20-24`. v1 adds `ide`.
- **Extension host:** the editor's Node.js process that runs the extension's code. In a remote window (SSH, WSL, container) it runs next to the files.
- **Webview:** the editor's sandboxed iframe that shows the panel.
- **Child:** the `ragon` process the extension spawns.

---

## 3. Design

### 3.1 Processes

```
editor UI ── webview (desktop/ui bundle, host "ide")
                │  postMessage (request id, host message)
extension host (TypeScript): policy filter · context collector · diff provider · child manager
                │  stdio JSONL (app-server protocol v2)
ragon -c features.code_mode_host=true app-server     ← same command as desktop/electron/src/server.ts
                │  HTTPS to the node's gate (model), as every local ragon does today
node
```

- The manifest sets `"extensionKind": ["workspace"]`, so in a remote window the child runs where the files are. *Interface copied*: the upstream manifest also activates on `onStartupFinished` and `onUri`.
- **One child per window**, started on the first panel open and stopped when the window closes. The initialize request sends `clientInfo { name: "ragon_vscode", title: "Ragon for VS Code", version }`.
- Reuse, *verified* by reading the imports:
  - `desktop/electron/src/policy.ts` imports only `policy.json` (`:19`). It moves unchanged to a shared folder `desktop/shared/` that both the Electron main process and the extension build from.
  - `desktop/electron/src/login.ts` imports only the type `AppServer` (`:11`), so it moves the same way.
  - `server.ts` is split: the spawn and JSONL framing are shared; the Electron-specific paths stay behind.

### 3.2 Finding the node

Tried in order; the first that answers wins. The panel shows which one it used.

1. **The setting `ragon.executable`**, when set. *Interface copied* from the upstream `chatgpt.cliExecutable`, but here it is a normal setting.
2. **`ragon` on `PATH`** of the extension host. In a remote window that is the remote machine's `PATH`.
3. **The install locations** the installer uses: `~/.local/bin/ragon`; on macOS also the app bundle's `Contents/Resources/ragon`; on Windows `%LOCALAPPDATA%\Programs\Ragon\ragon.exe`. *The Windows path is unverified* against the Windows installer: row I2 checks it.
4. **Nothing found:** the panel shows "Ragon is not installed on this machine", the install command, and a link to the release page. The link opens in the user's browser and is the only link the panel offers.

**A local child already reaches the node.** On a paired client, `ragon` finds its node through the usual tiers, ending in mDNS (AGENTS.md "Nodes"), and talks to the node's gate for the model. The user's files stay on the machine where the editor has them.

**Attaching to a session on the paired node** (row I9, gated on 0.9.10). This is for continuing a thread that runs on the node, for example one started from a phone or a night run.
- It needs remote sessions A1 (the session WebSocket at `wss://<primary>.local:8000/`), A3 (the shared app-server always on), A5 (`ragon --remote` defaults to the primary and fills the token) and A6 (the trust-root patch).
- The extension does **not** open the WebSocket itself. It spawns `ragon remote bridge --stdio`, a new subcommand (named difference N3). The subcommand:
  - copies JSONL between its stdio and the session WebSocket;
  - pins the CA at `~/.ragon/nodes/<primary id>/ca.pem`;
  - sends the saved account token as `Authorization: Bearer`.
  So the token and the TLS code stay in Rust, and the extension keeps D3.
- The front door has **no frame filter** (remote-sessions §6.4). The extension's own `policy.json` filter therefore runs on this path too.
- If 0.9.10 slips, I9 moves to 1.0 and v1 ships local-only. Nothing else in v1 depends on remote sessions.

### 3.3 The UI host `ide`

- `Host` becomes `"web" | "electron" | "ide"` (`bridge.ts:20`). The webview's preload script defines `window.electronBridge.sendMessageFromView` over the editor's `postMessage`, with request ids, and sets `ragonWindowType = "ide"`. This is the transport `bridge.ts` was written to allow: "this page could also run in a browser against another transport" (`bridge.ts:2-4`).
- **Shown in the `ide` host:** the thread view (Work), meaning conversation, plan, diffs, approvals and composer, plus the thread list, the model picker, the air-gapped chip and the account gate.
- **Hidden:** nights, docs, pets, previews, terminals, the git bar, plugins, settings pages beyond the extension's own, and the window commands.
- **Host messages.** `bridge.ts` defines 117 message types. The `ide` host answers the `work/*` family (`work/send`, `work/start`, `work/stop`, `work/target`, `work/airgapped`), the `account/*` family, and `editor/open` / `open/editor` (they open the file in the editor). That is about 15 messages, so for this host the UI gets a list of the messages that exist. *The count is from reading `bridge.ts`, not measured*: row I4's first step is the inventory, written to `desktop/ui/src/host-inventory.json` and checked by a test.
- The 37 places that test `host() === "electron"` (the `host()` sites) are read in I4. Each one either keeps the Electron-only branch or gains an `ide` branch.
- **The CSP** is `default-src 'none'`, with scripts, styles, fonts and images only from the extension's `media/` folder through the editor's webview resource scheme. There is no `connect-src`: the webview can talk only to the extension host.

### 3.4 Policy

The extension host applies `ragon-rs/web/policy.json` to every frame both ways, using the shared `policy.ts`, exactly as the desktop main process does:
- 74 allowed requests. They exclude `fs/writeFile` and `account/login/start`.
- Requests refused when air-gapped: `plugin/install`, `marketplace/add`, `marketplace/upgrade`, `thread/shellCommand`.
- Answers go only to pending server requests. Dropped thread fields are dropped.
- The two vector files `ragon-rs/web/vectors/outgoing.json` and `account.json` run against the extension's build in T1. A frame the policy refuses is answered with the same JSON-RPC error the desktop app returns.

---

## 4. Editor context

### 4.1 The exact items sent

**There is no context field in the app-server protocol.** Codex sends editor context as text at the head of the user's message, and Ragon does the same, so that history replays the same everywhere. The comment at `codex-rs/tui/src/ide_context/prompt.rs:12-16` says the desktop app and the IDE extension use the same delimiter.

`turn/start` carries `input: [UserInput…]` (`app-server-protocol/src/protocol/v2/turn.rs:425`):
- **`UserInput::Text`**. Its text is, in this order (`prompt.rs:101-182`):

```
# Context from my IDE setup:

## Active file: <path relative to the workspace root>

## Active selection range:            (or "ranges:" when there are several)
- <path>: line L, column C to line L2, column C2      (1-based)

## Active selection of the file:
<selected text>

## Open tabs:
- <label>: <path>

## Diagnostics:                        ← N1, Ragon only, see §4.2
- <path>:L:C <severity> <source>: <message>

## My request for Codex:
<what the user typed>
```

  The caps are Codex's: 40,000 selection characters, 100 tabs, 20,000 characters of tab lines (`prompt.rs:9-11`). The headings and the request delimiter are byte for byte Codex's. The installed `ragon` binary contains `# Context from my IDE setup:` and `## My request for Codex:` (*observed*).
- **`UserInput::Mention { name, path }`** (`turn.rs:455`) for "Add file to thread" and for files dropped on the composer. *Interface copied*: the upstream commands `addToThread` and `addFileToThread` (manifest, *verified*).
- **`UserInput::LocalImage`** for an image pasted or dropped into the composer.
- `thread/start` sets `cwd` to the workspace folder of the active file. If there is none, it uses the first workspace folder.

The context collector reads `vscode.window.activeTextEditor` (path, `selections`, the selected text), `vscode.window.tabGroups` (open tabs) and `vscode.languages.getDiagnostics(uri)`. It builds the data model of `codex-rs/tui/src/ide_context.rs:17`: `activeFile { label, path, selection, activeSelectionContent, selections[] }` and `openTabs[]`, with positions 0-based and printed 1-based. Then it renders the text above.

**The composer shows a context chip** listing the active file, the selection and the tab count, with a toggle. Off means plain `Text`. *Interface copied* from the desktop composer's IDE-context toggle (DESKTOP_PARITY, the row "IDE context toggle", today "No").

### 4.2 Named differences

| # | Difference | Why |
|---|---|---|
| N1 | A `## Diagnostics:` section, placed after `## Open tabs:` and before the delimiter. It holds errors and warnings for the active file and for files with a selection: at most 50 lines and 8,000 characters, errors first. | Users fix errors more often than anything else, and the brief asks for diagnostics. A Codex client replaying the thread shows it as plain text, so nothing breaks. |
| N2 | `desktop/ui` folds the context block into a chip when it shows a user message, in every host. Today it would show the raw block. | Threads started in the editor show up in the desktop app and in `ragon web`. *Checked*: `desktop/ui/src` has no handling for the delimiter yet. |
| N3 | `ragon remote bridge --stdio` (§3.2) | Keeps all network code and the token out of the extension (D3) |

### 4.3 The context socket for the terminal (stretch row I6)

The Ragon TUI's `/ide` command asks an editor for context over IPC (`codex-rs/tui/src/ide_context/ipc.rs`). The extension can answer, so a `ragon` started in the editor's terminal sees the editor too.
- **Socket:** `$CODEX_HOME/ipc/ipc.sock`, which for Ragon is `~/.ragon/ipc/ipc.sock` (`ipc.rs:179`). The legacy socket is under the temp directory (`:184`), and Windows uses a named pipe (`:194`).
- **Frames:** a u32 little-endian length followed by JSON (`:698`).
- **Request:** `{type:"request", requestId, sourceClientId:"codex-tui", version:0, method:"ide-context", params:{workspaceRoot}}` (`:685-692`).
- **Answer:** `{type:"response", requestId, resultType:"success", result:{ideContext}}`. Any other method gets `no-handler-for-request`. The client checks that the peer is the same user and that the directory is not writable by other users, and gives up after 5 s.

This row is cut first if the schedule slips.

---

## 5. Diffs, approvals and applying changes

- **The proposed change.** The agent's `fileChange` item carries `changes: [{ path, kind, diff }]` (`app-server-protocol/src/protocol/v2/item.rs:330-333`, `:1140-1143`). `item/fileChange/patchUpdated` (`common.rs:1975`) streams updates to it.
  - The extension builds the proposed text of each file in memory, from the file on disk plus the diff, and serves it under a read-only virtual document scheme `ragon-proposed:`.
  - The editor's own diff view compares the file with its `ragon-proposed:` twin. The panel's diff card has "Open in editor".
- **Approval.** `item/fileChange/requestApproval` (params `threadId, turnId, itemId, startedAtMs, reason?, grantRoot?`, `item.rs:1632-1646`) is answered with `{ decision }`. The decision is one of `accept`, `acceptForSession`, `decline` or `cancel` (`item.rs:115-123`, `:1650-1652`).
  - The diff view's title bar has **Accept** and **Decline** buttons, which send the same answer as the panel's buttons. Whichever answers first wins. The other closes.
  - `item/commandExecution/requestApproval` is shown in the panel only, with the command and its working folder. Both requests are the two the UI can answer today (`desktop/ui/src/sync.ts:112`).
- **The agent applies the change.** The extension never writes the files (D5).
  - If one of the files is open with **unsaved edits**, Accept is disabled and the card says "Save or revert <file> first". Applying over a dirty buffer would make the editor's copy and the disk differ.
  - After the item completes, the editor reloads the changed files the usual way, from file-system events.
- **Review and undo.**
  - "Review changes" runs `review/start` on the turn's changes, as the desktop app's review pane does.
  - "Undo turn" runs `thread/revert` (`common.rs:755`, allowed by the policy).
  - The turn's diff card opens all changed files as one multi-file diff.

---

## 6. Accounts, air-gapped mode and egress

- **Sign-in.** The account gate of `desktop/ui` and the shared `login.ts`:
  - the child's `ragon login status --json` (`ragon-rs/src/auth/commands.rs:41`, `:61`) decides whether the gate shows;
  - the form calls `account/ragon/signIn`. The errors are `-32001` (wrong credentials), `-32002` (name taken) and `-32003` (accounts unreachable), with the screen's own text (AUTH §5.8).
  - `account/login/start` stays refused, as in the desktop app: no vendor sign-in from the panel.
  - The extension stores no secret. The token lives where `ragon` keeps it.
- **Air-gapped.** The panel shows the thread's level and changes it as the desktop app does (`work/airgapped`). At `on`, the policy refuses the four requests in §3.4. The extension has no other network path, so it behaves the same with the machine offline. The panel works with no internet as long as the node is reachable, on the LAN or on the same machine.
- **Egress rule: the extension opens no connection except to the node.** Enforced three ways:
  1. **By construction.** The extension depends on no HTTP library and no telemetry library. Its `package.json` is checked by T4 against a short allowlist of dependencies. It has no `connect-src` (§3.3).
  2. **By the bundle test** (T4). The packed `.vsix` contains no `http://` or `https://` string except the release-page link of §3.2 and the schema URLs in a fixed list.
  3. **By the audit.** `ragon-admin audit egress --ide` (row I10) runs the headless editor (T7) under the audit's namespace. It passes only if every connection goes to the node's gate, `:8000`, or to loopback. *Interface copied* from `--app`.
- The editor's own telemetry is the editor's business. The extension's README says how to switch it off and does not touch it.

---

## 7. Editors

- **v1: the VS Code family.** `engines.vscode` is `^1.96.0`, the same floor the upstream extension declares (*verified*, manifest). One platform-neutral `.vsix`: the extension carries no binary and uses the user's `ragon` (§10 Q9).
- **Remote windows** (SSH, WSL, containers) work through `extensionKind: workspace`, provided `ragon` is installed on the remote machine. If it is not, the panel shows the install command for that machine.
- **Next phase: the JetBrains family**, sized at **10–12 agent-days**, after 1.0 by default (§10 Q8). The vendor name `dreamference` is held on the JetBrains Marketplace (DECISIONS 2026-11-25).
  - A Kotlin plug-in: a tool window hosting the same `desktop/ui` bundle in the IDE's embedded browser, as host `ide`. A diff provider that uses the IDE's diff viewer. A context collector over the editor, selection and problems APIs, using the same text format.
  - To avoid a third copy of the policy, the plug-in spawns `ragon ide-bridge --stdio`, a small Rust command that runs the web crate's `policy.rs` (`POLICY_JSON`, `ragon-rs/web/src/policy.rs:29`) between the plug-in and the app-server. The plug-in itself then carries no policy code.
  - The vendor documents that "Xcode and JetBrains IDEs provide their own integrations" for Codex (*reported* from the vendor's IDE page, not re-fetched). Which protocol those integrations speak is *unverified*. A first half-day of the phase checks whether an open agent protocol in the JetBrains IDEs would let Ragon plug in without a plug-in. If so, the phase shrinks.

  | Row | Days |
  |---|---|
  | Plug-in skeleton, tool window, embedded browser host | 2 |
  | `ragon ide-bridge --stdio` (Rust, reuses `policy.rs`) | 1.5 |
  | Context collector and format vectors | 1.5 |
  | Diffs and approvals in the IDE's diff viewer | 2 |
  | Accounts and node discovery (shared with VS Code rules) | 1 |
  | Packaging (zip on the release; Marketplace upload by owner account) | 1 |
  | Headless test with the IDE's test runner in CI | 1.5 |
  | Docs | 0.5 |
  | **Total** | **11** |

---

## 8. Phases

### 8.1 v1 rows (0.9.11)

"Ragon first" means the row goes to a Ragon row first, by the owner's rule for site, docs and test rows. The rest go to the cheapest model that can do them.

| id | files | test command | one line | days | who |
|---|---|---|---|---|---|
| I1 | `ide/vscode/` (new: `package.json`, `src/extension.ts`, `tsconfig.json`), `.github/apps/CODEOWNERS` | `cd ide/vscode && npm test` | Extension skeleton: activation, the panel view and the editor tab, the webview with CSP, an empty bundle | 1.5 | lane |
| I2 | `ide/vscode/src/node.ts`, `desktop/shared/server.ts` (split from `desktop/electron/src/server.ts`) | `cd ide/vscode && npx vitest run node` | Find the binary (§3.2 steps 1–4), spawn the child, JSONL framing, restart on exit | 1.5 | lane |
| I3 | `desktop/shared/policy.ts` (moved), `ide/vscode/src/policy.ts` | `cd ide/vscode && npx vitest run policy` | The policy filter on both paths; the two vector files pass | 1 | lane |
| I4 | `desktop/ui/src/bridge.ts`, `desktop/ui/src/host-inventory.json`, `ide/vscode/media/preload.js`, `tests/test_desktop_parity.py` | `cd desktop/ui && npx vitest run bridge` and `.venv/bin/python -m pytest tests/test_desktop_parity.py -q` | Host `ide`: the transport, the host inventory, the hidden pages, the 37 `host()` sites | 3 | lane |
| I5 | `ide/vscode/src/context.ts`, `ide/vscode/test/vectors/context.json`, `desktop/ui/src/ideContext.ts` | `cd ide/vscode && npx vitest run context` | The context collector, the exact renderer (§4.1), N1 diagnostics, the chip, and N2 folding in the UI | 2 | lane |
| I6 | `ide/vscode/src/ipc.ts` | `cd ide/vscode && npx vitest run ipc` | *Stretch, cut first:* the context socket for the TUI's `/ide` (§4.3) | 1.5 | lane |
| I7 | `ide/vscode/src/diff.ts`, `ide/vscode/src/approvals.ts` | `cd ide/vscode && npx vitest run diff approvals` | `ragon-proposed:` documents, the native diff with Accept and Decline, the dirty-buffer guard, review and undo | 2.5 | lane |
| I8 | `desktop/shared/login.ts` (moved), `ide/vscode/src/account.ts` | `cd ide/vscode && npx vitest run account` | Account gate in the panel; `account.json` vectors | 1 | lane |
| I9 | `ragon-rs/src/remote_bridge.rs`, `ide/vscode/src/node.ts` | `cargo test --release -p ragon-launcher remote_bridge` (in the export) and `cd ide/vscode && npx vitest run node` | *Gated on 0.9.10 A1, A3, A5, A6:* `ragon remote bridge --stdio` and "Attach to a thread on the node" | 2.5 | lane |
| I10 | `dreamference/audit/egress_audit.py`, `ide/vscode/test/bundle.test.ts`, `tests/test_egress_audit.py` | `.venv/bin/python -m pytest tests/test_egress_audit.py -q` | Bundle URL test, dependency allowlist, `audit egress --ide` | 1 | Ragon first |
| I11 | `.github/workflows/ide-extension.yml`, `ide/vscode/scripts/package.sh`, `ragon-rs/src/ide_install.rs` | `cd ide/vscode && npm run package` and `cargo test --release -p ragon-launcher ide_install` (in the export) | `.vsix` build in CI, attached to the release; Open VSX upload step; `ragon ide install` | 1.5 | lane |
| I12 | `ide/vscode/test/e2e/`, `desktop/electron/e2e/stand-in.ts` | `cd ide/vscode && xvfb-run -a npm run test:e2e` | The headless editor run (T7) against the scripted app-server | 2 | Ragon first |
| I13 | `docs/ide.md`, site page, `specs/README.md` | `.venv/bin/python -m pytest tests/test_anonymity.py -q` | User docs (install, offline install, remote windows, privacy) and the site row | 1 | Ragon first |
| | | | **Total v1** (I6 and I9 included) | **22.5** | |
| | | | **Without the stretch I6** | **21** | |

### 8.2 Calendar

- There are 15 weekdays from Mon 11-30 to Fri 12-18. Release hardening takes 12-16 and 12-17, which leaves about **12 working days for build**.
- **Two dedicated lanes**, A and B (§10 Q2), give 24 lane-days for 21–22.5 agent-days of work, about 1½ days of slack.

| Week | Lane A | Lane B |
|---|---|---|
| 11-30…12-04 | I1, I2, I3 | I4 (the host inventory first), I13 skeleton |
| 12-07…12-11 | I7, I8, I11 | I5, I10, I12 |
| 12-14…12-15 | I9 (once 0.9.10 is out Mon 12-14) | I6 (stretch), fixes |
| 12-16…12-17 | Release hardening, docs final | the same |

- **Does 12-18 hold?**
  - **Yes with two dedicated lanes from 11-30**, for a v1 of I1–I5, I7, I8, I10–I13.
  - **I9 holds only if 0.9.10 ships on 12-14** with A1, A3, A5 and A6.
  - **I6 holds only if nothing slips.**
- **With one lane, 12-18 does not hold.** That would be about 21 days of work for one lane, so it lands around 12-29, after 1.0 on 12-27.
- **The lanes cannot come from the 1.0 work.** The four 1.0 lanes are fully booked from 12-01 to 12-24 (remote-sessions §11, about 64½ of 72 lane-days), and desk work also has full lanes for 0.9.11.

### 8.3 Risks

| Risk | Mitigation |
|---|---|
| `desktop/ui` is more tied to Electron than the reading suggests (117 host messages, 37 `host()` sites) | I4 starts with the inventory. If more than 25 messages need a real `ide` answer, I7 drops "review changes" and the slack goes to I4. |
| The webview CSP breaks a UI library that loads code at run time | Found on day 1 of I4 with the real bundle. The fix lives in the build, not in a looser CSP. |
| 0.9.10 slips | I9 moves to 1.0 and v1 ships local-only, which loses nothing for files on the user's machine |
| Editor API change in a new VS Code release | `engines` floor 1.96. T7 runs against both the floor and the latest stable. |
| Users point the upstream extension at `ragon` and file issues | Docs say it is not supported. T9 keeps the protocol honest. |

---

## 9. Tests

| # | Test | Where | What it proves |
|---|---|---|---|
| T1 | The policy vectors `ragon-rs/web/vectors/outgoing.json` and `account.json` run against the extension's filter | vitest, I3 | The editor enforces exactly what the desktop app and `ragon web` enforce |
| T2 | Context renderer vectors: active file, one and several selections, caps at 40,000 / 100 / 20,000, N1 at 50 lines / 8,000 characters, no editor open | vitest, I5 | Byte-for-byte Codex text, plus N1 |
| T3 | Binary finder: setting, `PATH`, install paths, none; a remote window | vitest, I2 | §3.2 |
| T4 | Bundle: no URL outside the allowlist in the packed `.vsix`; `package.json` dependencies in the allowlist; CSP has no `connect-src` | vitest, I10 | The egress rule |
| T5 | Approvals: `accept`, `acceptForSession`, `decline`, `cancel` from the panel and from the diff view; the first answer wins; the dirty-buffer guard | vitest, I7 | §5 |
| T6 | IPC socket: frame round trip, `no-handler-for-request`, foreign-uid refusal | vitest, I6 | §4.3 (stretch) |
| T7 | **Headless editor run in CI:** `@vscode/test-electron` downloads VS Code at CI time (floor and stable), under `xvfb-run`, with the extension loaded and `ragon.executable` pointing at `desktop/electron/e2e/stand-in.ts` (the scripted app-server) | GitHub Actions, I12 | Open the panel, send a message with a selection, see the context text arrive at the stand-in, open a diff, Accept, see the answer arrive |
| T8 | `audit egress --ide`: the headless run inside the audit; only loopback and the node | I10, the nightly egress job | The egress rule, measured |
| T9 | Codex-client conformance: a protocol client sends the start-up a Codex IDE client sends (`initialize` with `clientInfo`, `account/read`, `thread/start`, `turn/start` with a context-prefixed `Text`) to a real `ragon app-server` | `tests/test_desktop_parity.py` family, I4 | Route (a)'s passive promise; Ragon's app-server answers a Codex client |
| T10 | Desktop and `ragon web` show a context-prefixed message as a chip (N2) | vitest in `desktop/ui`, I5 | Threads from the editor replay cleanly |

---

## 10. Owner questions (each with the default taken if there is no answer)

| # | Question | Default |
|---|---|---|
| Q1 | Route (b), our own extension, with route (a) kept as a passive promise and not mentioned in user docs? | **Yes** |
| Q2 | Two dedicated lanes from Mon 11-30 for the extension, besides the 1.0 and desk-work lanes? | **Yes.** Without them the extension moves after 1.0 (§8.2). |
| Q3 | Extension id `dreamference.ragon`, display name "Ragon"? | **Yes** |
| Q4 | Publish to Open VSX in 0.9.11, where the namespace is already held? | **Yes**, plus the `.vsix` on the GitHub release. It needs owner step 1. |
| Q5 | Publish to the VS Code Marketplace in 0.9.11? | **No.** Wait until the project account replaces the personal one on the publisher (DECISIONS 2026-11-25), then publish at 1.0. |
| Q6 | Diagnostics in the context (N1)? | **Yes**, as a section before the delimiter |
| Q7 | The context socket for the TUI's `/ide` (I6) in v1? | **Stretch**, cut first |
| Q8 | JetBrains timing? | **After 1.0** (11 agent-days, §7) |
| Q9 | Bundle a `ragon` binary in the extension, as the upstream extension does? | **No.** Use the installed `ragon`: one binary, one update path, smaller package. |
| Q10 | Attach to a thread on the node (I9) in v1? | **Yes if 0.9.10 ships on 12-14** with A1, A3, A5 and A6; otherwise 1.0, without holding 0.9.11 |

## 11. Owner steps

1. **Open VSX token.** Sign in to Open VSX with the project account and create an access token. Store it as the repository secret `OVSX_PAT`; the coordinator never sees it. The namespace `dreamference` is held. The token made on 11-25 was dropped and may be deleted. Needed by 12-15 for the release upload.
2. **(For 1.0, not 0.9.11) VS Code Marketplace.** Add the project account as owner of the publisher `dreamference`, remove the personal account, and create a publishing token as the secret `VSCE_PAT`.
3. **(After 1.0) JetBrains.** The vendor `dreamference` is held. Its verification by the marketplace is pending on their side.
4. Answer Q1–Q10 or let the defaults stand.

## 12. Not in v1

- JetBrains and every other editor family (§7).
- Cloud tasks and the vendor's cloud sign-in.
- The relay, and any connection that does not go to the node.
- Inline completion and next-edit suggestions.
- "Implement this TODO" code lenses (*interface noted*: the upstream `implementTodo` and `commentCodeLensEnabled`).
- Notebooks.
- Chat-provider integration through the editor's proposed APIs. The upstream manifest enables the proposed APIs `chatSessionsProvider` and `languageModelProxy`, *verified*. The editor's documentation says extensions that use proposed APIs cannot be published to its marketplace (*unverified here*).
- Filling `ragon-admin mcp`'s `IDEState` from the extension.
- Nights, docs, pets, previews and terminals in the panel.
- Signing of the `.vsix` and auto-update outside the registry.
- VS Code Marketplace publishing (Q5).

## 13. Sources

Verified means fetched or read on 2026-11-28 by this spec's author. Reported means taken from a research pass and not re-fetched.

- Open VSX API, extension `openai/chatgpt`, version 26.51120.50957: manifest (`chatgpt.cliExecutable`, views, commands, `engines`, `enabledApiProposals`, activation events), `LICENSE.md`, eight platform targets. *Verified.*
- The vendor's IDE settings page (the `cliExecutable` warning) and its config basics page ("The CLI and IDE extension share the same configuration layers"). *Verified.*
- The vendor's terms of use, the clauses quoted in §0.1. *Verified.* The legal reading is *unverified*.
- The vendor's app-server page ("the interface Codex uses to power rich clients (for example, the Codex VS Code extension)"; stdio JSONL is the default). *Verified.*
- The vendor's IDE page ("VS Code and compatible editors use the Codex extension; Xcode and JetBrains IDEs provide their own integrations"). *Reported.*
- openai/codex issues 2938, 5822 and 5882. *Reported.*
- `codex-rs` at `rust-v0.158.0`:
  - `tui/src/ide_context/prompt.rs:9-16`, `:101-182`; `tui/src/ide_context.rs:17`; `tui/src/ide_context/ipc.rs:179`, `:184`, `:194`, `:685-698`;
  - `app-server/src/lib.rs:445`;
  - `app-server-protocol/src/protocol/v2/turn.rs:425`, `:455`; `v2/item.rs:115-123`, `:330-333`, `:1140-1143`, `:1632-1652`; `common.rs:755`, `:1097`, `:1468`, `:1975`.
  - *Read.*
- This repository:
  - `desktop/ui/src/bridge.ts:2-4`, `:20-24`, `:167-187`; `desktop/ui/src/sync.ts:112`;
  - `desktop/electron/src/policy.ts:19`, `login.ts:11`;
  - `ragon-rs/web/policy.json`, `ragon-rs/web/src/policy.rs:29`; `ragon-rs/src/auth/commands.rs:41`, `:61`;
  - specs AUTH §5.8, CLI §4.10, DESKTOP_PARITY;
  - remote-sessions §6.4, §7.1, §11 on branch `spec/remote-sessions`.
  - *Read.*
- The installed `ragon` binary's strings (§0.4). *Observed.*

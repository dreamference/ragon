# Terminal agent

`ragon` is a coding agent that works in your repository. It reads files, runs commands, edits code
and explains what it did. Its prompts go to the model your GB10 serves, never to a cloud model.

```bash
cd ~/my-project
ragon                              # interactive session
ragon "why do the tests fail?"     # start with a prompt
ragon exec "add a --verbose flag"  # run once, non-interactively (from a script: < /dev/null)
ragon resume --last                # pick up the last session
```

## What happens when you run it

Before the agent starts, `ragon` connects itself to the local model:

1. It finds the model server. On the GB10 that is the server on the same machine. On any other
   computer it looks for a Ragon node on the local network and uses the one it finds, with no
   address to type (see [Using a GB10 from another computer](#using-a-gb10-from-another-computer)).
   `DREAMFERENCE_VLLM_HOST`, or `vllm_host` in `dreamference.toml`, overrides both.
2. If the server is still loading, it waits and shows progress.
3. It asks the server which model it serves and how long a context that model takes.
4. It writes the settings the agent needs to use that model, and starts the session.

`ragon --version`, `ragon --help` and commands that never need the model (such as `completion`
and `apply`) skip all of this and answer at once.

While a SWE-bench run is using the model, the model server turns every other request away, from
`ragon`, the web UI, the desktop app and the messengers alike, and says so: "The model is running a
benchmark (about 9 h left). Try later or run `ragon-admin night pause`."
`ragon-admin night pause` (one hour; `--for 2h` for longer) lets you work meanwhile, and
`ragon-admin night resume` hands the model back to the benchmark.

## Commands and flags

The everyday commands and flags:

| Command or flag | What it does |
|---|---|
| `ragon exec`, `ragon review` | Run non-interactively, or review changes. `ragon exec` reads more input from standard input when it is not a terminal, so a script, a cron job or another program that starts it must close stdin (`< /dev/null`) or the run waits for input that never comes |
| `ragon resume`, `ragon fork` | Continue or branch a saved session |
| `ragon apply` | Apply the agent's last diff with `git apply` |
| `-c key=value`, `-p profile` | Override configuration, pick a profile |
| `-a on-request`, `-s workspace-write` | Approval policy and sandbox policy |
| `ragon mcp`, `ragon sandbox`, `ragon completion` | MCP servers, sandbox, shell completion |

Configuration and saved sessions live in `~/.ragon` (or `$CODEX_HOME`). If another tool left
sessions in `~/.codex`, the first run copies them and your settings across, never a cloud sign-in.

## What Ragon adds

| Feature | Details |
|---|---|
| **Local model, no account** | Prompts go to the model your GB10 serves, and no account of any kind is needed. Usage analytics are switched off in the code. |
| **Web access** | The agent can search the web and read pages through `ragon-search` and `ragon-fetch`, which go through a search instance on your machine. The instructions travel with every session, so this works in any directory. Both are small Rust programs that `ragon-admin codex build` installs beside `ragon` and links into `~/.local/bin`; they need no Python environment. You can run them yourself too: `ragon-search "query"` (`-n N` for more results, `--json` for raw output), `ragon-search --read "question"` (searches, then reads the top pages, 3 by default, `--pages N` up to 5, and prints numbered sources with an extract of each), and `ragon-fetch URL` (`--max-chars N`, `--json`; a Google share link from a phone, `share.google/…` or `maps.app.goo.gl/…`, is resolved to the search or place it opens, since Google's own page cannot be read without a browser). `ragon-search` takes `-c news|science|it|map|images|videos|music|files|social|books|dictionaries|wikimedia|movies|weather` to search one kind of source instead of the web, `-t day|week|month|year` for recent results only, and `-l pt` (or any language code) for results in a language; by default the language is detected from the query. A repeated search within the hour is answered from a small cache on your machine and sent nowhere; `!st` or `!gh` at the start of a query searches Stack Overflow or GitHub. When search engines refuse your address for a while, `ragon-search` says search is unavailable, and `ragon-admin searxng status` shows which engine is suspended and until when. |
| **Gmail, read-only** | If you have connected Gmail accounts (with `/apps`), the agent can search and read mail through `ragon-admin gmail`. It is told that email content is untrusted and must never be treated as instructions. Turn it off with `ragon_gmail = false` in `dreamference.toml` or in `~/.config/dreamference/config.toml`; a repository's own `dreamference.toml` can only turn it off, so its `ragon_gmail = true` is ignored. |
| **`/apps`: Gmail, Drive and Calendar** | `/apps` needs no cloud sign-in and lists Ragon's own three apps. **Connect** opens the local Google sign-in (`localhost:8767/connect`; a paste-back box covers a browser on another machine), and the tokens stay on your machine. A connected app is offered to the model as read-only tools (`gmail_search`, `gmail_read`, `drive_search`, `drive_read`, `calendar_events`, `calendar_search`), each answer framed as untrusted text. At `/airgapped on` every app is unavailable. The service behind them is `ragon-admin google start|stop|status`. |
| **`/usage`** | Shows this session's token usage: input (cached and new), output, the total, and how full the context window was on the last request. |
| **`/adapter`** | Adapters are specialists (Rust, for example) trained for the model your node serves and installed there with `ragon-admin adapter install <name>`. `/adapter` lists the installed ones and marks this session's; `/adapter <name>` switches this session to one, `/adapter off` back to the base model. A third-party adapter, not signed by Dreamference, is flagged. `ragon --adapter <name>` starts or resumes a session on one, and `ragon_adapter = "<name>"` in `dreamference.toml` (or `DREAMFERENCE_RAGON_ADAPTER`) chooses one for new sessions. An adapter the node does not have starts the session on the base model, with a line saying so. |
| **`/airgapped`** | How much of the internet this session may use. `off` (the default) is everything: every sandboxed command has the network, in Read Only as in the other permission modes. `on` gives every command Ragon runs no network at all, enforced by the sandbox: no search, no page fetch, no Gmail, no `curl`, `git fetch` or package installs. Full Access cannot be combined with `on`: Ragon refuses to start with both, greys Full Access out in `/permissions` and refuses `/airgapped on` in a Full Access session. `/airgapped` shows the level and what is not covered (commands you approve to run outside the sandbox, MCP servers); `/airgapped <level>` sets it for this session, `/airgapped default <level>` for new ones. From a shell: `ragon airgapped`, or `DREAMFERENCE_RAGON_AIRGAPPED=on ragon exec …` for one run. Ragon still talks to the model server at every level. |
| **Plugins** | Codex's plugins work while `/airgapped` is off: add a marketplace from a git repository or a folder (`ragon plugin marketplace add owner/repo`, or the desktop app's Skills and tools › Plugins), then install from it with `/plugins` or `ragon plugin add <plugin>@<marketplace>`. A plugin can bring skills, MCP servers, hooks and apps; its MCP servers and hooks run as you, outside the sandbox, so add only what you trust. The vendor's own catalogue stays closed. At `/airgapped on` a session starts without plugins, and installing a plugin or adding or upgrading a marketplace is refused; a session switched to `on` keeps the plugins it loaded until you restart it. |
| **Skills from other agents** | Skills you installed for Claude Code, Gemini CLI, OpenClaw or Hermes are offered to the model too, and `ragon skill add` installs one from the public skill catalogues or any GitHub folder. See [Skills](#skills). |
| **`ragon app`** | Opens the [desktop app](desktop.md). |
| **`ragon update`** | Installs the latest published Ragon release. |
| **`ragon uninstall`** | Removes a client installed by `install.sh` or `install.ps1`: its programs, the links in `~/.local/bin` (Linux, macOS), and on Windows its `Path` entry and its firewall rules (one Administrator prompt; over SSH it prints the command for an administrator). It asks first (`--yes` does not). `~/.ragon`, with your sessions and settings, stays unless you add `--purge`. A node, and a `ragon` built from a checkout, are not removed this way. |
| **Code index** | `ragon-code` answers where a name is defined, who calls it and what a change would touch, from an index of the repository kept current as you work. The model is offered it as tools (`code_search`, `code_show`, `code_refs`, …) and from the shell. |
| **`/night`** | Queue a task for tonight: `/night add <task>`. `ragon-admin night run` (on a timer once `ragon-admin night enable` is set) works through the queue while the model is otherwise idle, each task on its own git branch; nothing is merged or pushed. `ragon night apply <id>` (or `/night apply <id>`) in the repository brings a finished task's changes into your checkout as uncommitted changes to review and commit; when they conflict with your own changes it refuses and changes nothing. Each task is attempted three times, one after another, and only the best attempt is committed: an attempt whose tests fail is dropped, and among those left the model compares the diffs in pairs and picks one (with no test command to run, the model's comparison alone decides). The morning report lists every attempt, which one was chosen and why, and where each one's diff and logs are. `/night add --best-of 1 <task>` runs a task once (or `[night] best_of = 1` in `dreamference.toml`, for every task); `--best-of N` asks for N. A task sent to another node with `--on <node>` is worked once there. Three attempts take about three times as long as one, so a short window may fit fewer: the report says when it did. Night Shift can also fill its own queue (off by default): with `[night] autofill = true` in `dreamference.toml`, each night begins by queueing up to five small tasks (`[night] autofill_max`, 1 to 5) from the failing tests of the latest failed CI run, the desktop QA report and unchecked items (`- [ ]` and `TODO:` lines) in the repository's `specs/` documents. They are ordinary tasks on their own branches, marked `[autofill:...]` on their first line, with the test that must pass written into them; a finding is queued at most once in 14 days, anything that mentions the sandbox, credentials, signing or similar is skipped, and nothing is merged. `ragon-admin night autofill --dry-run` shows what it would queue. Review each branch in the morning like any other Night Shift result. `ragon-admin night archive` moves finished runs older than four weeks (`--weeks N` for another age), with their transcripts and the old reports, to `~/.ragon/night/archive/`; their branches stay. |
| **`/worktree`: in place or a worktree** | By default a session edits your checkout in place, as Codex does, and your editor sees every change at once. `/worktree on` (or `ragon worktree on`) makes new sessions in a git repository start in a fresh git worktree on a branch of their own (`ragon/<id>`, under `~/.ragon/session-worktrees/`), so your checkout is untouched while the agent works; `/worktree off` goes back. The footer always shows the folder and branch the session edits. In a worktree session, `/worktree status` shows what is not yet applied, `/worktree apply` brings the changes into your checkout as uncommitted changes (all or nothing: it refuses, changing nothing, if they conflict with yours), and `/worktree discard` drops them, keeping a snapshot under `refs/ragon/snapshots/`. `ragon worktree list` names a repository's worktree sessions. The worktree starts from your last commit: uncommitted changes, ignored files and submodules are not in it. `/worktree` alone opens Codex's own worktree picker. The desktop app's Local/Worktree toggle starts on the same choice. |
| **Refine mode (off by default)** | Each new task is first studied by a separate session that changes nothing and writes a refined description (intent, requirements, every code path, edge cases, what must not change, acceptance checks); a fresh session then does the task with that description. It runs by itself with no stop: in `ragon exec`, on the first prompt of each interactive session, and for each new Night Shift task. Switch it on with `ragon --refine`, `DREAMFERENCE_RAGON_REFINE=on` or `ragon_refine = true` in `dreamference.toml` (`[night] refine` for Night Shift alone); `ragon refine` shows the setting. A second version of its texts, refine-v2, is built for a comparison night and chosen with `ragon --refine-version v2`, `DREAMFERENCE_RAGON_REFINE_VERSION=v2`, `ragon_refine_version = "v2"` or `[night] refine_version = "v2"`; v1 stays the default. |
| **Guards (off by default)** | Two checks on the agent's own work, built from what went wrong in benchmark runs. The test guard points out a change that weakens an existing test (an assertion rewritten or removed, a test deleted or skipped) and holds the first stop once while it stands, so the agent decides from your task whether that behaviour was meant to change. The finish check holds the first stop once when source files changed and no test has run on them since, when the agent calls failures pre-existing without having run the tests on the unchanged code, or when it stops announcing a next step. Each hold happens at most once per session and never refuses anything. Switch them on with `ragon_test_guard = true` and `ragon_finish_check = true` in `dreamference.toml`, or `DREAMFERENCE_RAGON_TEST_GUARD=on` and `DREAMFERENCE_RAGON_FINISH_CHECK=on`; Night Shift's sessions follow the same settings. They work in a git repository only. |
| **Named system prompts** | `ragon prompt list`, `show [<name>]` and `use <name>` choose the system prompt for new sessions: `default`; `offline`, the same without the web and email instructions, for a machine with no network; or `high-swe`, a method for repository tasks; your own go in `~/.ragon/system-prompts/<name>.md`. A resumed session keeps the prompt it started with. |

## What Ragon removes

Anything that would need a vendor's cloud or an account is hidden or refused:

| Feature | In Ragon |
|---|---|
| `login`, `logout`, `/logout` | Refused: there is no account to sign in to. |
| `cloud` (`cloud-tasks`) | Switched off: it runs tasks on a vendor's servers. It may return for a private cloud. |
| `remote-control` | Hidden. It relays sessions through a vendor's servers. |
| `/feedback` | Removed. It uploaded session logs. |
| `/voice` | Hidden. It needs a cloud voice service. |
| `/approve` (auto-review) | Hidden. It needs a cloud review model. |
| `/import` | Hidden. Its plugin step fetches marketplaces inside `ragon`'s own process, where `/airgapped on` cannot stop it; the desktop app's Import follows the level. |
| Update check | Ragon's own: `ragon update`. |

## Skills

A skill is a folder with a `SKILL.md`: instructions the model reads when a task matches the skill's
description, sometimes with scripts beside them. Ragon, Claude Code, Gemini CLI, OpenClaw and Hermes
all use the same file, so Ragon offers the model the skills it finds in any of their folders:

| Where | What |
|---|---|
| `~/.ragon/skills/<name>` | Skills installed for Ragon |
| `~/.agents/skills` | The folder several agents, Gemini CLI and OpenClaw among them, share |
| `~/.claude/skills`, `~/.gemini/skills`, `~/.openclaw/skills`, `~/.hermes/skills` | Other agents' own folders. Ragon links to each skill there at every start; nothing is copied, and a session cannot change them through the link |
| `.agents/skills` in the repository | Skills that travel with the project |
| A trusted project's `.codex/config.toml` | Once you trust a folder, Codex reads its `.codex/config.toml` above your own settings. Five keys in it can replace the system prompt the model is given: `model_catalog_json`, `model_instructions_file`, `instructions`, `developer_instructions` and `experimental_compact_prompt_file`. That changes what the model is told, not what is enforced: the air gap, the sandbox and the guards still apply. Only a folder you trusted can do this; trust only repositories whose authors you trust. |
| `.claude/skills`, `.gemini/skills` in the repository | Linked like the other agents' folders, but only once you have trusted the repository in Ragon (its trust prompt), since whoever wrote the repository wrote them |

| Command | What it does |
|---|---|
| `ragon skill list` | What the model is offered, by source, and what it is not offered and why. `--all` adds skills another one shadows |
| `ragon skill show <name>` | One skill: its description, origin, licence, files and scripts |
| `ragon skill search <words>` | Search the public skill catalogues, Hermes's and ClawHub |
| `ragon skill add <source>` | Install into `~/.ragon/skills`. `<source>` is `openai/<name>`, `anthropic/<name>`, `hermes/<category>/<name>`, `clawhub/<owner>/<slug>`, `<owner>/<repo>/<path>`, a github.com URL or a folder. It shows the description, licence and scripts first and asks; `--yes` skips the question |
| `ragon skill remove <name>` | Delete a skill Ragon installed |
| `ragon skill enable <name>` / `disable <name>` | Offer a skill anyway, or never offer it |
| `ragon skill source <agent> on\|off` | Link, or stop linking, the skills of `claude`, `gemini`, `openclaw` or `hermes`, or with `repo` a trusted repository's `.claude/skills` and `.gemini/skills` |
| `ragon skill adopt <name>` | Record a skill you wrote by hand as known, so later changes to it are reported |

Things to know:

- **Not every skill is offered.** One that names another operating system or a program that is not
  installed is left out, as is one its author marked as not to be started by a model. When two
  skills have the same name, one is offered: the repository's first, then Ragon's own, then
  `~/.agents/skills`, then the repository's `.claude` and `.gemini` skills, then Claude's,
  Gemini's, OpenClaw's and Hermes's, then the bundled ones.
- **Skills cost context.** The model is shown each skill's name and description in every session,
  within a budget of 2% of the context window. Ragon tells you at start when that is nearly full,
  and leaves linked skills out before the budget would drop every description.
- **Nothing in a skill runs by being installed or loaded.** Hooks, pre-approvals and
  run-before-reading lines some agents support are never acted on. A skill's scripts run only as
  commands the model issues, under the session's sandbox and approvals.
- **Ragon tells you about skills it did not install.** A skill that appears or changes in
  `~/.ragon/skills` outside `ragon skill add` is reported once at the next start, and anything
  found in the `from-*` link folders that Ragon did not put there is moved to
  `~/.ragon/skills/.quarantine/`.
- **ClawHub checks every skill it lists, and Ragon shows what it found.** A skill its scan calls
  clean installs like any other; one it calls suspicious, or has no result for, is installed only
  when you confirm it at a terminal (`--yes` is refused); one it marks malicious is never installed.
- **`/airgapped on`** refuses `add` and `search` before any request is made; installing from a
  folder still works.
- Skills written for another agent name its tools ("use the Read tool"). The local model followed
  such skills correctly in testing without help; `glossary = true` in `~/.ragon/ragon-skills.toml`
  adds a short translation table to the prompt if one of yours does not.

## Using a GB10 from another computer

`ragon` on a laptop can use the model on a GB10 in the same network. The GB10 is the *node*; the
laptop is a *client*, and only the agent's prompts and answers cross the network: your code, the
commands the agent runs and the code index stay on the laptop.

On the GB10, once:

```bash
ragon-admin node enable        # asks for your password once; `--no-web` keeps the web UI private
```

That announces the node on the local network and lets other computers reach its web search and
its web UI. **Anyone on your network can then use the model and search through the node**, and
nothing is encrypted on the local network, so do this only on a network you trust; the web UI
(`ragon web`, port 3100) serves another computer or phone only once paired on the node
(`ragon web pair`). `ragon-admin node disable` undoes it, and `ragon-admin node status` shows what is
announced.

On the other computer, type `ragon`. It finds the node and remembers it:

| Command | What it does |
|---|---|
| `ragon node list` | Every node on the network, the model each serves, and the address to paste into other tools |
| `ragon node use <name or address>` | Use that node from now on. An address works on networks that block discovery |
| `ragon node forget` | Forget the node; the next start looks again |
| `RAGON_NODE=<name> ragon …` | Another node, for one command |

If there are several nodes, `ragon` asks once which to use and never switches by itself. If the
node's model server is stopped or still loading, it says so instead of waiting.

What does not work from a client yet: Gmail in `ragon`, Night Shift (`/night add` is refused: tasks
run on the node), and anything `ragon-admin` does. The desktop app, `ragon-app`, runs its own
bundled `ragon` against the node's model. See
`specs/DREAMFERENCE_RAGON_NODE.md` §18 for what was measured.

### A second GB10

Every GB10 is installed the same way and announces itself the same way; none is "primary". The one
you are sitting at manages the others, with `ragon-admin`:

| Command | What it does |
|---|---|
| `ragon-admin node list` | Every node on the network, the model each serves and its load |
| `ragon-admin node add <node>` | Pair with a node, once. It asks for that node's password one time and sets up a key that can only ask it for Ragon operations |
| `ragon-admin node status <node>` | That node's `ragon-admin status` |
| `ragon-admin node set <node> --model <key>` | Give that node a model and start it there; the node runs its own safety checks |
| `ragon-admin node start <node>` / `stop <node>` | Start or stop its model server |
| `ragon-admin node run <node> -- <command>` | Run a command there, in the current repository at its last commit. Its changes come back as a branch, `job/<id>` |
| `ragon-admin node jobs`, `logs <job>`, `cancel <job>`, `fetch <job>` | Follow, stop and collect jobs |
| `ragon-admin node remove <node>` | Undo the pairing |

Using a node (asking its model, searching) needs no pairing. Changing one, or running something
on it, always does. A job runs with a memory cap and a time limit (8 GB and 90 minutes unless you
say otherwise), cannot use the GPU, and sees nothing of the other node's home folder. Uncommitted
changes are not sent. All of this was built and tested on a single GB10 playing both sides; it has
not yet run between two machines.

### Setting up a node from far away

A new GB10 can be set up from the one you sit at, over the network or over the
[overlay](remote.md) when it is at another site:

```bash
ragon-admin node provision <name> --user <the account on that machine>
```

It installs Ragon there, gives the node its model, pairs it and starts it; run on nodes that are
already paired, `ragon-admin node provision --all` updates them all. With no name it lists the
unprovisioned GB10s it finds on the network. `--dry-run` connects and reads only, then says what
would change. It asks for the new machine's password once (`--per-host-password` asks for each).

By default the model's files (about 22 GiB for the default model) are copied from the GB10 you run
it on. For a node on a slow connection to you, let the node download them itself:

```bash
ragon-admin node provision <name> --user <account> --model-source hf
```

The node fetches the public weights from Hugging Face, at the exact revision your own GB10 holds,
and resumes a download that is cut off. Every file is then checked against your copy by its
SHA-256 checksum; only a file that Hugging Face does not carry, or whose checksum differs, is sent
from your GB10. If two machines are provisioned in one command and the download keeps failing on
the second, it takes the files from the first over their own network. The default, `--model-source
copy`, stays as it was.

### Restarting a hung node

If a node stops answering, `ragon-admin node power-cycle <node>` switches its smart plug off and on
again. See [Away from home](remote.md#how-do-i-restart-a-hung-gb10-from-a-distance) for the setup.

## Debugging

Set `RUST_LOG` (for example `RUST_LOG=codex_mcp=trace ragon`). The
interactive interface writes its log to `~/.ragon/logs_2.sqlite`, which
`ragon-admin logs mcp` reads.

## How it is built

`ragon` is compiled from source pinned to a release, with Ragon's own launcher and a short series
of patches added. See
[Architecture](architecture.md#how-ragon-is-built).

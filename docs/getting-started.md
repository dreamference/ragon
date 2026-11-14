# Get started

!!! tip "The quickest way: install a release"
    Every [release](https://github.com/dreamference/ragon/releases) carries `install.sh`, which
    installs the prebuilt binaries and, on a GB10, everything else, unattended: `ragon-admin`, the
    host settings, advertising on your network, and the model, downloaded and started. It asks for
    your sudo password once, at the start, and whether to install pending system updates:

    ```bash
    curl -fsSLO https://github.com/dreamference/ragon/releases/latest/download/install.sh
    bash install.sh
    ```

    When its summary says every step is done, the model server is running. The steps below install
    from a checkout, which is what you want for changing Ragon itself.

## What you need

- **Any NVIDIA GB10 machine**: the DGX Spark, or the Acer Veriton GN100, ASUS Ascent GX10, Dell
  Pro Max with GB10, Gigabyte AI TOP ATOM, HP ZGX Nano, Lenovo ThinkStation PGX or MSI EdgeXpert.
  All have the same chip and 128 GB of unified memory, and ship DGX OS 7 (Ubuntu 24.04 underneath);
  Ragon was developed on the ASUS. A 1 TB drive is enough. The RTX Spark laptops run Windows and
  cannot be a Ragon node.
- **Docker with the NVIDIA Container Toolkit.** The model server runs in a container. DGX OS
  ships both; on a machine reinstalled with plain Ubuntu, `ragon-admin host check` lists them,
  with bubblewrap and Avahi, among what to install.
- **Python 3** (Ragon is developed on 3.12) and **Git**.
- **Disk space for model weights.** The default model's weights are tens of gigabytes, downloaded
  from Hugging Face by the first run (`ragon-admin init`) or the first `server start`.
- **Swap and an out-of-memory guard.** `ragon-admin server start` checks the host before loading a
  model and explains any setting it wants changed (swap, `sysctl` values, `earlyoom` or
  `systemd-oomd`). See [Architecture](architecture.md#keeping-the-host-alive).

## 1. Install

```bash
git clone --recurse-submodules https://github.com/dreamference/ragon.git ragon
cd ragon
python3 -m venv .venv
.venv/bin/pip install -e .
```

This installs `ragon-admin`. Put the environment on your `PATH`, for example in `~/.bashrc`:

```bash
export PATH="$HOME/ragon/.venv/bin:$PATH"
```

!!! warning "`ragon-admin`, `ragon-search` and `ragon-fetch` have to be on your PATH"
    The agent searches the web and reads pages by running `ragon-search` and `ragon-fetch`, and
    reads Gmail by running `ragon-admin gmail`, as shell commands. If they are not on the `PATH`
    the agent inherits, those commands fail and the agent concludes it has no web access.
    `ragon-admin codex build` links all three into `~/.local/bin`.

## 2. Build the terminal agent

```bash
ragon-admin codex build
```

This compiles `ragon` from the source in the repository, installs it
under `~/.local/share/dreamference/ragon/`, and links `~/.local/bin/ragon` to it. It installs a
Rust toolchain if you have none. The first build compiles several hundred dependencies and takes a
while; rebuilds after small changes take a few minutes.

## 3. First run: one command

```bash
ragon-admin init
```

It writes the config and indexes the folder you run it in, then runs the four first-run steps in
order, one progress line each (ending with how long the step took), and stops at the first that
fails, naming the command that runs that step alone:

```text
[1/4] install checks … ✅ docker, ragon and the host settings are in place (…)
[2/4] model download … ✅ the weights are in the Hugging Face cache (…)
[3/4] server start … ✅ serving qwen3.8-27b-nvfp4-dflash2 (…)
[4/4] first ragon run … ✅ ragon answered: ready (…)
```

| Step | Its own command |
|---|---|
| Install checks: docker, `ragon` and the host settings | `ragon-admin host check` (`ragon-admin host setup` applies the settings) |
| Model download: the model and its drafter | `ragon-admin model download` |
| Server start (skipped when the server already serves the model) | `ragon-admin server start` |
| A first `ragon` turn against the server | `ragon exec "Reply with the single word: ready"` |

Every step's output goes to `~/.local/state/dreamference/first-run.log`. The download is the long
part (the default model's weights are tens of gigabytes); running `init` again skips what is
already done. Check that the server is answering at any time:

```bash
ragon-admin endpoints
```

## 4. Run it

```bash
cd ~/my-project
ragon
```

See [Terminal agent](ragon.md) for what it can do.

## 5. Optional: the web UI and desktop app

```bash
ragon web start               # Ask and Work in a browser, on this machine only
ragon web open                # sign this machine's browser in (a link that works once)
ragon-admin images start      # optional: image search in Ask
ragon-admin voice start       # optional: the microphone button (speech-to-text on the CPU)
```

See [Web UI](web.md).

For a window of its own:

```bash
ragon-admin desktop install     # build tools for the desktop app (asks for sudo once)
ragon-admin desktop run
```

After that, `ragon app` also opens it. See [Desktop app](desktop.md).

## 6. Optional: your laptop as a client

Your other computers can use the GB10's model. On the GB10, let them find it:

```bash
ragon-admin node enable          # publishes this machine to your local network (asks for sudo once)
```

!!! warning "A GB10 on Wi-Fi: turn power saving off"
    Clients find the node by multicast DNS, and Wi-Fi power saving, which NetworkManager turns on
    by default, loses most of it: with it on, up to ~80% of the queries to a GB10 went unanswered
    and laptops could not find it. `node enable` and `ragon-admin host check` warn when it is on,
    with the commands that fix it; `ragon-admin host setup` (or `sudo ragon-admin node prepare`) turns
    it off for good, with no reconnect. A Linux laptop on Wi-Fi loses the answers the same way: when
    its `ragon` finds no node and its own power saving is on, it says so, with the commands.
    On Ethernet there is nothing to do.

Then on a Linux laptop (Intel/AMD or Arm) or a Mac (Apple silicon or Intel):

```bash
curl -fsSL https://github.com/dreamference/ragon/releases/latest/download/install.sh | bash
cd ~/my-project && ragon
```

The script installs the client: `ragon`, `ragon-search`, `ragon-fetch`, `ragon-code` (and on Linux
`ragon-docs`), and on a Mac the [desktop app](desktop.md) as well (`--no-app` leaves it
out). On Windows 11, the same in PowerShell (a preview; Smart App Control must be off):

```powershell
irm https://github.com/dreamference/ragon/releases/latest/download/install.ps1 | iex
```

A Mac is never installed as a node. `ragon` finds the GB10 on the network by itself and asks before
it uses a node it has not used before; `ragon node list` shows what it found. To name one yourself:
`RAGON_NODE=<host> ragon`.

On Windows, `install.ps1` installs the client into `%LOCALAPPDATA%\Programs\Ragon\bin` and
the desktop app (`-NoApp` leaves it out), puts that folder on your `Path`, and asks once for
Administrator rights: for the sandbox the agent's commands run in, the firewall rules that let
`ragon` and the app hear nodes announce themselves, and an egress audit. Over SSH there is no
desktop for that prompt; the script says so and prints the command an administrator runs instead.

A client has no `ragon-admin` (that is the node's tool). To see which node it uses, run `ragon node list`;
to pick one, `ragon node use <name or address>`; to check that the model answers, `ragon exec "say hi"`.

**ripgrep is recommended.** The agent searches your code with `rg`, and `ragon doctor` warns "search
command could not be verified" without it. Install it with `sudo apt install ripgrep` (Debian, Ubuntu),
`brew install ripgrep` (macOS) or `winget install BurntSushi.ripgrep.MSVC` (Windows).

**Checking a release by hand.** The installers verify every download; there is no separate key file
among the release assets. The release key is embedded in `install.sh` and `install.ps1` and kept in
the repository as `ragon-rs/release-signing.pub` (the line below is its key). To verify `SHA256SUMS.sig` yourself, download
`SHA256SUMS` and `SHA256SUMS.sig` from the release, then:

```bash
echo 'release-key namespaces="ragon-release" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII9boVZe01adX9jSk8Hh/GAJURmqzCznictI3meZX0/i' > allowed_signers
ssh-keygen -Y verify -f allowed_signers -I release-key -n ragon-release -s SHA256SUMS.sig < SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

To remove the client, on any of them: `ragon uninstall` (it asks first; `--purge` also removes
`~/.ragon`, your sessions and settings). The desktop app stays: remove it as any other app.

!!! note "`/airgapped` on a Mac"
    macOS's own sandbox enforces `/airgapped on`, but it is set once, when `ragon` starts: use
    `ragon airgapped default on` (or `DREAMFERENCE_RAGON_AIRGAPPED=on ragon`) and restart. Typed
    inside a running session, `/airgapped on` and `off` say so and change nothing.

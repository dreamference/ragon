# Self-hosted CI runners

Two jobs of the release pipeline can run on our own machines instead of GitHub's hosted runners,
each switched by a repository variable holding a JSON `runs-on` value, with a workflow input that
forces GitHub's runner for one run:

| Job | Variable | Input (`github` = hosted) | Hosted fallback |
| --- | --- | --- | --- |
| windows.yml, x86-64 `ragon` and tools | `WINDOWS_X64_RUNNER` | `windows_x64_runner` | `windows-latest` |
| build-clients.yml, `aarch64-apple-darwin` | `MACOS_RUNNER` | `macos_runner` | `macos-15` |

Unset the variable when the machine is offline: a job waiting for a self-hosted runner queues for a
day. The x86-64 macOS client and the dmg jobs (mac-preview.yml, which opens a window) stay on
GitHub's runners.

## Never from a pull request

The repository is public, and a fork's pull request runs the workflow as the fork wrote it. None
of the workflows that reach a self-hosted runner has a `pull_request` or `pull_request_target`
trigger, and each runner's job-started hook refuses every event but push, workflow_dispatch and
schedule, any other repository, any ref but a branch, and any workflow but its own few
(`tests/test_macos_ci_runner.py` runs the macOS hook's cases). Both runners are registered to
dreamference/ragon only, run as a non-administrator account, and take a lock that a hand build
on the same machine shares.

## The macOS runner

`scripts/macos/ci-runner/setup.sh` sets a Mac up in two steps, idempotent, so moving the runner to
another Mac is the same two commands (its header has them): `--stage` as the administrator
registers the runner while the token is fresh, then `sudo bash /Users/Shared/mac-runner/ci-runner/setup.sh`
creates the hidden `ghrunner` account, installs the root-owned hooks and `.env`, puts Python 3.12
in the tool cache and rustup in the account's home, and runs the runner as a LaunchDaemon.
`CARGO_BUILD_JOBS` is one per 3 GB of memory. Labels `self-hosted, macOS, ARM64, mac-mini`: a
replacement with the same labels takes over without changing the variable.

### Setting up a new Mac

After the Mac's first boot (administrator account, network, Remote Login on) the rest is one paste,
run from the node. `scripts/setup_macos_runner.sh` wraps the runner script above and adds what a
fresh Mac lacks, each step skipped when already done: Homebrew with `gh`, `jq`, `git` and
`python@3.12`; power (never sleep, wake on network, restart after a power cut); Remote Login and the
coordinator's public key; the overlay client (`--overlay netbird|tailscale`, joined when
`NETBIRD_SETUP_KEY` or `TAILSCALE_AUTHKEY` is in the environment); and the runner itself. The daily
QA (`scripts/qa/macos_qa.sh`) needs only tools macOS ships. The registration token is fetched at run
time and passed as an argument; it is never printed and nothing secret is in the file.

```bash
scp -qr scripts/setup_macos_runner.sh scripts/macos/ci-runner <mac>:/tmp/ \
  && ssh -t <mac> bash /tmp/setup_macos_runner.sh --name <runner name> \
       --ssh-pubkey "$(cat ~/.ssh/id_ed25519.pub)" \
       --token "$(gh api -X POST repos/dreamference/ragon/actions/runners/registration-token --jq .token)"
```

The script also runs from a pipe on the Mac itself (`curl -fsSL <raw URL> | NETBIRD_SETUP_KEY=… bash -s -- --overlay netbird --overlay-url <management URL> --name <name> --token …`): with no `ci-runner/` beside it, it fetches the three runner scripts from the public repository. This is how a Mac away from the node joins; the one-time key and the token live only on that line.

`ssh -t` lets `sudo` ask the administrator's password once. Add `--dry-run` to list what it would
change without a password or any change.

The 8 GB Mac mini (M1, macOS 12.3) was tried and is too small: the `ragon` build
thrashed (swap grew to 2.7 GB, `codex_tui` alone took two hours) and ran for over four hours,
against 74 to 81 minutes on `macos-15`. `MACOS_RUNNER` stays unset until the 64 GB M5 Pro mini
arrives. The release's longest job is the x86-64 macOS client on `macos-15-intel` (83 to 130
minutes), so moving only the Apple silicon build does not shorten a release: that needs the
x86-64 build cross-compiled on the Mac, which the builder cannot do yet (it takes its target and
its prebuilt V8 from the host).

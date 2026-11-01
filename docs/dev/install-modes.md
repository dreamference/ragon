# The two install modes

Developer notes behind the "two install modes" rule in `AGENTS.md`. Spec: `specs/DREAMFERENCE_SETUP.md` §3.2–§3.3, which lists what was verified (a scratch home here) and what was not (a second machine).

## A checkout and a release install

There are two ways Ragon gets onto a machine, and the package must work in both.

- A **checkout** (`pip install -e .`, `ragon-admin codex build`) has `codex/`, `codex-patches/` and the crates beside the package.
- A **release install** (`install.sh` at the repository root, attached to each release) has the wheel in a virtualenv of its own and the release's prebuilt binaries in `~/.local/share/dreamference/ragon/bin`, with nothing beside the package.

`CodexBrandedBuilder.has_source()` and `DesktopRunner.has_source()` tell them apart: without source, installed binaries count as current and `codex build`/`desktop build` say what to do instead. Earlier, nothing had ever been installed from a release, and `ragon-admin run` on one installed rustup and then died on the missing `ragon-web-rs/`. Anything new that reaches for a path under `REPO_ROOT` needs the same guard.

A file a command needs on both kinds of install lives inside the package and is named in `setup.py`'s `package_data`; a wheel carries nothing else that is not Python. The code index's pins (`dreamference/cli/code_index_pins/`: `code-index.sha256` and the npm lockfile) live in the package, not in `ragon-code-rs/`, so `ragon-admin code setup` works on a release install; `test_a_release_wheel_carries_the_pins_and_code_setup_reads_them_from_it` builds the wheel as the release does and reads them from it.

## `install.sh`

`install.sh` downloads exactly what `ragon update` does (`ragon-rs/src/update.rs`: same asset names, same checksum file), so the two stay one mechanism. On a GB10 it also installs `ragon-admin` and, unattended (one sudo password at most, at the start; SETUP §3.2.1), applies `host setup --yes`, the docker group, lingering, `node enable --yes`, the model download and `server start`, offering system updates as its one question.

On a Mac it also installs the desktop app from the release's dmg, and `install.ps1` on Windows runs the release's NSIS installer silently (`--no-app` / `-NoApp` leaves it out; SETUP §3.2, the owner's decision). Both are fetched by the script, so they carry no quarantine mark or Mark of the Web and the unsigned builds open without Gatekeeper's or SmartScreen's dialog. Each records the app it installed (`desktop-app` beside the install folder's `bin`: `path=` and `version=`), and `ragon update` updates a recorded app from the same release under the same checks (signed `SHA256SUMS`, downloaded by `ragon` itself), unless it is running; an app installed by hand has no record and is left alone, and a record whose app is gone is dropped.

A GB10 is recognised by arm64 and `nvidia-smi` naming it, or the GPU's PCI id `10de:2e12` where no driver answers yet; never the vendor, since each of the eight GB10 machines names itself differently in DMI and this one says `GX10` (SETUP §3.5).

## `HostSafetySetup`

`HostSafetySetup` (`vllm_server/host_safety_setup.py`) applies what `check_host_safety()` only printed, taking every reading from the check's own helpers; it resizes swap only when it is the single `/swap.img`, and its sudo path is covered by tests alone, since this machine already passes. See [host-safety.md](host-safety.md).

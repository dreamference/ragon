#!/usr/bin/env bash
# Installs Ragon from a release, with no checkout of the repository and nothing compiled.
#
#   ./install.sh [--role client|node] [--version X.Y.Z] [--no-advertise]
#                [--from <dir>] [--no-host-setup] [--no-model] [--no-app]
#
# What it installs depends on the machine:
#
#   client   the `ragon` terminal agent and its commands (`ragon-search`, `ragon-fetch`,
#            `ragon-code` and, on Linux, `ragon-docs` when the release carries them): prebuilt
#            binaries, downloaded from the release, checked against its checksum file, placed in
#            ~/.local/share/dreamference/ragon/bin and linked into ~/.local/bin. Releases carry it
#            for arm64 and x86-64 Linux and for macOS (Apple silicon and Intel). On a Mac it also
#            installs the desktop app, Ragon.app, from the release's dmg for that Mac (checked
#            against SHA256SUMS like everything else), into /Applications when you can write
#            there and ~/Applications otherwise; --no-app leaves it out. Downloaded with curl, it
#            carries no quarantine mark, so the unsigned preview opens with no Gatekeeper dialog.
#   node     the client, plus `ragon-admin` (the Python package, from the release's wheel, in a
#            virtualenv of its own) and everything else a node needs, with no question asked:
#            python3-venv where the system lacks it; the host settings a model load needs, and
#            Wi-Fi power saving off, which loses the mDNS clients find a node by (`ragon-admin
#            host setup --yes`); you in the `docker` group; lingering, so jobs and
#            Night Shift tasks outlive a logout; the node offered to the local network
#            (`ragon-admin node enable --yes`), so `ragon` on your other computers finds it with no
#            address typed; what the local file index loads (`ragon-admin docs setup`, about
#            320 MB); and the default model, downloaded and served (`ragon-admin model
#            download`, `ragon-admin server start`, logged to ~/.local/state/dreamference/
#            install-<time>.log). It ends with a summary of every step, done or failed. This is
#            what a GB10 (DGX Spark and its siblings) gets by default; every other machine gets
#            the client.
#
# You can run it on a fresh GB10 and leave. A node install changes the machine outside your home
# folder, so it needs root, asked for once at the very start: nothing at all when sudo needs no
# password (root, or a NOPASSWD rule), otherwise sudo's own password prompt, after which sudo is
# kept from expiring until the script ends. With no terminal to ask on (`curl … | bash` over a
# plain ssh) and a sudo that wants a password, it stops before anything is downloaded and says how
# to run it so that it can ask: download it, then `bash install.sh` in a terminal (or `ssh -t`).
#
# System updates. A node also checks, at the start, for package upgrades (`apt-get update`;
# NVIDIA's DGX, CUDA and driver packages are named) and firmware (`fwupdmgr`). At a terminal it
# asks the one question it ever asks, default no: install them now? Yes installs them after
# everything else (`apt-get full-upgrade`, firmware staged for the next boot); it never reboots,
# and the summary says when to. With no terminal they are only listed, with the commands.
#
# --no-advertise keeps the node off the local network (`ragon-admin node enable` offers it later).
#
# It downloads the same assets, by the same names and with the same checks, as `ragon update`
# (ragon-rs/src/update.rs), so a machine installed this way is updated by that command, the Mac's
# desktop app included: the script records where it put the app ($RAGON_INSTALL_DIR/
# desktop-app), and `ragon update` updates an app it finds recorded, under the same checks.
#
# The repository is public, so no token is needed. One is used when present (GH_TOKEN,
# GITHUB_TOKEN, or a logged-in `gh`): it raises GitHub's rate limit, and it is what reads a private
# fork named by RAGON_RELEASE_REPO.
# RAGON_RELEASE_REPO names another repository (a fork), RAGON_RELEASE_API another API root (the
# tests' stand-in server); RAGON_INSTALL_DIR and RAGON_VENV move the two directories, and
# RAGON_APP_DIR the folder the Mac's app goes into.
#
# Releases from 0.9.0 on are signed: SHA256SUMS, which lists every file of the release, carries an
# Ed25519 signature by Ragon's release key (SHA256SUMS.sig), checked here with OpenSSH's
# `ssh-keygen -Y verify` (OpenSSH 8.1 or newer, which Linux distributions and macOS ship) against
# the key below. Nothing is installed from a signed release whose signature does not verify, or
# from a release from 0.9.0 on that is unsigned. specs/DREAMFERENCE_RELEASE_SIGNING.md.
#
# --from <dir> installs from a directory holding the same asset names and the same checksum and
# signature files instead of a GitHub release, with no network for Ragon's own files
# (`ragon-admin node provision` copies such a bundle from another node; specs/
# DREAMFERENCE_RAGON_FLEET.md §7.2). The checks are the same. A `VERSION` file in it names the
# version, and a `wheelhouse/` folder in it, if present, holds the Python dependencies so pip needs
# no index either. --no-host-setup and --no-model are for provisioning, which does those parts
# itself: the root half with `sudo ragon-admin node prepare` (host settings, Wi-Fi power saving,
# docker group, lingering), and the model copied from the machine provisioning it. With --no-host-setup this
# script needs no root, and checks for no system updates.
#
# RAGON_LINGER_DIR, RAGON_PCI_DEVICES and the PATH are where the tests point it at
# stand-ins; nothing else reads them.
#
# For a development install from a checkout, use scripts/install_gb10.sh instead.
set -euo pipefail

# The release keys, the lines of ragon-rs/release-signing.pub (a test keeps them equal).
RELEASE_KEYS='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII9boVZe01adX9jSk8Hh/GAJURmqzCznictI3meZX0/i ragon-release-2026-11'
SIGNED_SINCE="0.9.0"

PUBLIC_REPO="dreamference/ragon"
REPO="${RAGON_RELEASE_REPO:-$PUBLIC_REPO}"
API="${RAGON_RELEASE_API:-https://api.github.com}"
INSTALL_DIR="${RAGON_INSTALL_DIR:-$HOME/.local/share/dreamference/ragon}"
VENV_DIR="${RAGON_VENV:-$HOME/.local/share/dreamference/venv}"
LINK_DIR="$HOME/.local/bin"
ROLE=""
VERSION=""
ADVERTISE=1
FROM=""
HOST_SETUP=1
MODEL=1
APP=1
APP_DIR="${RAGON_APP_DIR:-}"

# A line that cannot be written (a reader that went away) is dropped, never the end of the run.
say()  { printf '%s\n' "$*" 2>/dev/null || true; }
fail() { printf '❌ %s\n' "$*" >&2 2>/dev/null || true; exit 1; }

# The comment block at the top, without its `# `. Read from standard input (`curl … | bash -s --
# --help`) the script is no file, so there is no header to read: it says where to find one.
usage() {
    if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
        awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "${BASH_SOURCE[0]}"
    else
        say "The help is the comment at the top of install.sh: download it (curl -fsSLO <the URL you piped>), then: bash install.sh --help"
    fi
}

# What the summary lists: `step done|failed|off|available <text>`. A step is done or failed; `off`
# is only ever an opt-out given on the command line, `available` only the system updates.
STEPS=()
step() { STEPS+=("$1|$2"); }

LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/dreamference"
LOG="$LOG_DIR/install-$(date +%Y%m%d-%H%M%S).log"
LINGER_DIR="${RAGON_LINGER_DIR:-/var/lib/systemd/linger}"

while [ $# -gt 0 ]; do
    case "$1" in
        --role)      [ $# -ge 2 ] || fail "--role needs a value (see --help)"; ROLE="$2"; shift 2 ;;
        --role=*)    ROLE="${1#*=}"; shift ;;
        --version)   [ $# -ge 2 ] || fail "--version needs a value (see --help)"; VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        --no-advertise) ADVERTISE=0; shift ;;
        --from)      [ $# -ge 2 ] || fail "--from needs a value (see --help)"; FROM="$2"; shift 2 ;;
        --from=*)    FROM="${1#*=}"; shift ;;
        --no-host-setup) HOST_SETUP=0; shift ;;
        --no-model)  MODEL=0; shift ;;
        --no-app)    APP=0; shift ;;
        -h|--help)   usage; exit 0 ;;
        *)           fail "unknown argument: $1 (see --help)" ;;
    esac
done

if [ -n "$FROM" ]; then
    [ -d "$FROM" ] || fail "--from: $FROM is not a directory."
    FROM="$(cd "$FROM" && pwd)"
    NEEDED="gzip awk"
else
    NEEDED="curl gzip awk"
fi
for tool in $NEEDED; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is needed and was not found."
done
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    fail "sha256sum (or shasum) is needed to check the downloads and was not found."
fi

# -- which machine ---------------------------------------------------------------------------

case "$(uname -s)" in
    Linux)  os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *)      fail "this script installs on Linux and macOS; on Windows use install.ps1." ;;
esac
case "$(uname -m)" in
    aarch64|arm64) arch="aarch64" ;;
    x86_64|amd64)  arch="x86_64" ;;
    *)             fail "unsupported processor: $(uname -m)" ;;
esac
TARGET="$arch-$os"

# A GB10 is an arm64 Linux machine whose GPU says so. Eight machines are GB10s (NVIDIA's DGX
# Spark and the Acer, ASUS, Dell, Gigabyte, HP, Lenovo and MSI boxes) and each names itself
# differently in DMI ("GX10" on an ASUS Ascent), so the GPU is asked, never the vendor: by
# nvidia-smi, or, where no driver answers yet, by its PCI id (10de:2e12, the same chip in all).
is_gb10() {
    [ "$TARGET" = "aarch64-unknown-linux-gnu" ] || return 1
    if command -v nvidia-smi >/dev/null 2>&1 \
        && nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | grep -q "GB10"; then
        return 0
    fi
    for device in "${RAGON_PCI_DEVICES:-/sys/bus/pci/devices}"/*; do
        [ "$(cat "$device/vendor" 2>/dev/null)" = "0x10de" ] \
            && [ "$(cat "$device/device" 2>/dev/null)" = "0x2e12" ] && return 0
    done
    return 1
}

case "$ROLE" in
    "")          if is_gb10; then ROLE="node"; else ROLE="client"; fi ;;
    client)      ;;
    node|both)   ROLE="node" ;;
    *)           fail "--role is client or node, not '$ROLE'." ;;
esac
# The node is a GB10's model server, its containers and its host settings; a Mac can only use one.
if [ "$ROLE" = "node" ] && [ "$os" = "apple-darwin" ]; then
    fail "macOS cannot be a Ragon node (the node runs the model on a GB10, under Linux). Install the client here (--role client, the default) and point it at your GB10."
fi
if [ "$ROLE" = "node" ] && ! is_gb10; then
    say "⚠️  This machine is not a GB10. The node's model recipes and host-safety checks are written"
    say "   for one; installing the node anyway because --role node was given."
fi

# A read-only or foreign HOME is named before anything is downloaded: the nearest folder that
# exists above each place the files go must be writable. Nothing is created here.
for dir in "$INSTALL_DIR" "$LINK_DIR"; do
    probe="$dir"
    while [ ! -e "$probe" ] && [ "$probe" != "/" ]; do probe="$(dirname "$probe")"; done
    [ -d "$probe" ] && [ -w "$probe" ] \
        || fail "cannot write to $probe, where $dir would be made (is $HOME read-only, or not yours?). Nothing was downloaded or installed; fix that, or set RAGON_INSTALL_DIR, and run this again."
done

WORK="$(mktemp -d)"
KEEPALIVE=""
MOUNTED=""
cleanup() {
    if [ -n "$MOUNTED" ]; then hdiutil detach "$MOUNTED" -force -quiet 2>/dev/null || true; fi
    rm -rf "$WORK"
    if [ -n "$KEEPALIVE" ]; then kill "$KEEPALIVE" 2>/dev/null || true; fi
}
trap cleanup EXIT

# -- root, asked for once --------------------------------------------------------------------

# A terminal to ask on: standard input, or the one the script runs in when it was piped into bash.
have_terminal() { [ -t 0 ] || { [ -r /dev/tty ] && (exec < /dev/tty) 2>/dev/null; }; }

USER_NAME="$(id -un)"
# Every root command of the node goes through "$SUDO": nothing as root, `sudo -n` otherwise, so
# that nothing after this point can stop to ask for a password.
SUDO=""
NODE_ROOT=0
if [ "$ROLE" = "node" ] && [ "$HOST_SETUP" = 1 ]; then
    NODE_ROOT=1
    if [ "$(id -u)" != 0 ]; then
        command -v sudo >/dev/null 2>&1 || fail "a node install changes the machine as root (host \
settings, the docker group, lingering) and sudo was not found. Run it as root, or install sudo."
        if sudo -n true 2>/dev/null; then
            :
        elif have_terminal; then
            say "🔑 A node install changes the machine outside your home folder (host settings, the docker"
            say "   group, lingering, advertising, system updates), so it needs sudo. Your password is"
            say "   asked for once, now; nothing else is asked after the update question below."
            if [ -t 0 ]; then sudo -v; else sudo -v < /dev/tty; fi \
                || fail "sudo did not accept the password; nothing was changed."
        else
            fail "a node install needs root, sudo wants a password, and there is no terminal to ask \
on, so nothing was done (this is what \`curl … | bash\` over a plain ssh does). Download the script \
and run it in a terminal, where sudo can ask once: \`curl -fsSL -o install.sh \
https://github.com/$REPO/releases/latest/download/install.sh && bash install.sh\` (over ssh: \`ssh -t\`)."
        fi
        SUDO="sudo -n"
        # sudo forgets the password after 15 minutes, and the model download takes longer.
        ( while kill -0 "$$" 2>/dev/null; do sudo -n -v 2>/dev/null || true; sleep 50; done ) >/dev/null 2>&1 &
        KEEPALIVE=$!
    fi
fi

# -- system updates, asked about once ----------------------------------------------------------

# Package upgrades as `apt-get full-upgrade` would install them: its simulation leaves out what apt
# holds back on its own (Ubuntu's phased updates), which `apt list --upgradable` would count.
UPGRADES=""          # every package
NVIDIA_UPGRADES=""   # NVIDIA's (DGX, CUDA, the driver, the kernels built for it)
FIRMWARE=""          # one "device: from -> to" per line
INSTALL_UPDATES=0
if [ "$NODE_ROOT" = 1 ] && command -v apt-get >/dev/null 2>&1; then
    say "🔎 Checking for system updates (apt, firmware) ..."
    $SUDO apt-get -o DPkg::Lock::Timeout=300 -qq update > "$WORK/apt-update.log" 2>&1 \
        || say "⚠️  apt-get update failed (see below); the list of updates may be out of date. $(tail -n 1 "$WORK/apt-update.log")"
    apt-get -s -o Debug::NoLocking=1 full-upgrade 2>/dev/null < /dev/null | awk '/^Inst / {print}' > "$WORK/upgrades" || true
    UPGRADES="$(awk '{print $2}' "$WORK/upgrades" | paste -sd ' ' -)"
    NVIDIA_UPGRADES="$(awk 'tolower($0) ~ /nvidia|dgx|cuda/ {print $2}' "$WORK/upgrades" | paste -sd ' ' -)"
    if command -v fwupdmgr >/dev/null 2>&1; then
        $SUDO fwupdmgr refresh --force > /dev/null 2>&1 < /dev/null || true
        # fwupdmgr prints its JSON pretty, one key a line: a device's keys at depth 3, a release's
        # at 5. Exit 2 means nothing to update.
        fwupdmgr get-updates --json --no-unreported-check < /dev/null 2>/dev/null > "$WORK/firmware.json" || true
        FIRMWARE="$(awk '
            function value(line) { sub(/^[^:]*: *"/, "", line); sub(/".*$/, "", line); return line }
            /"Name" *:/ && depth == 3 { name = value($0); current = "" }
            /"Version" *:/ && depth == 3 { current = value($0) }
            /"Version" *:/ && depth == 5 && name != "" && !(name in seen) {
                seen[name] = 1; print name ": " (current == "" ? "?" : current) " -> " value($0)
            }
            { line = $0; depth += gsub(/[{[]/, "", line) - gsub(/[]}]/, "", line) }
        ' "$WORK/firmware.json")"
    fi
    COUNT="$(printf '%s' "$UPGRADES" | wc -w | tr -d ' ')"
    FW_COUNT="$(printf '%s' "$FIRMWARE" | grep -c . || true)"
    if [ "$COUNT" -gt 0 ] || [ "$FW_COUNT" -gt 0 ]; then
        say "📦 System updates available: $COUNT package(s), $FW_COUNT firmware update(s)."
        if [ -n "$NVIDIA_UPGRADES" ]; then say "   NVIDIA (DGX, CUDA, driver): $NVIDIA_UPGRADES"; fi
        if [ "$FW_COUNT" -gt 0 ]; then printf '%s\n' "$FIRMWARE" | sed 's/^/   firmware: /'; fi
        if have_terminal; then
            printf '❓ Install them after Ragon (a reboot is needed afterwards; this script never reboots)? [y/N] '
            answer=""
            if [ -t 0 ]; then read -r answer || true; else read -r answer < /dev/tty || true; fi
            case "$answer" in y|Y|yes|Yes|YES) INSTALL_UPDATES=1 ;; esac
        fi
    else
        say "✅ No system updates."
    fi
fi

# -- the release -----------------------------------------------------------------------------

if [ -n "$FROM" ]; then
    # A bundle: the asset names are the files in the folder, and "fetching" one copies it.
    TAG="bundle"
    [ -f "$FROM/VERSION" ] && TAG="$(head -n 1 "$FROM/VERSION")"
    for file in "$FROM"/*; do
        if [ -f "$file" ]; then printf '%s\t%s\n' "$(basename "$file")" "$file"; fi
    done > "$WORK/assets.tsv"
    asset_url() { awk -F'\t' -v name="$1" '$1 == name {print $2; exit}' "$WORK/assets.tsv"; }
    fetch() {  # fetch <asset name>: copies it into $WORK, or fails
        local path; path="$(asset_url "$1")"
        [ -n "$path" ] || return 1
        cp "$path" "$WORK/$1"
    }
else
    # A token is used when GH_TOKEN or GITHUB_TOKEN is set; a logged-in gh's token only when
    # RAGON_RELEASE_REPO names another repository, a fork (the public repository needs none, and
    # a gh login carries far more access than reading a release). It is only ever sent to the API
    # root, never to a download URL the release names.
    TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
    if [ -z "$TOKEN" ] && [ "$REPO" != "$PUBLIC_REPO" ] && command -v gh >/dev/null 2>&1; then
        TOKEN="$(gh auth token 2>/dev/null || true)"
    fi
    auth=()
    [ -n "$TOKEN" ] && auth=(-H "Authorization: Bearer $TOKEN")

    if [ -n "$VERSION" ]; then
        release_url="$API/repos/$REPO/releases/tags/v${VERSION#v}"
    else
        release_url="$API/repos/$REPO/releases/latest"
    fi
    HTTP_CODE="$(curl -sSL ${auth[@]+"${auth[@]}"} -H "Accept: application/vnd.github+json" "$release_url" -o "$WORK/release.json" -w '%{http_code}' 2>"$WORK/release.err")" || HTTP_CODE="000"
    if [ "$HTTP_CODE" != "200" ]; then
        say "❌ Could not read $release_url (HTTP $HTTP_CODE)" >&2
        [ "$HTTP_CODE" != "000" ] || say "   $(tr '\n' ' ' < "$WORK/release.err")" >&2
        if { [ "$HTTP_CODE" = "403" ] || [ "$HTTP_CODE" = "429" ]; } && grep -qi 'rate limit' "$WORK/release.json" 2>/dev/null; then
            say "   GitHub's API rate limit is used up for this network. Wait a while, or set GH_TOKEN" >&2
            say "   (or GITHUB_TOKEN) to any GitHub token (a limit per account instead of per address)," >&2
            say "   and run this again." >&2
        elif [ -z "$TOKEN" ]; then
            say "   Check the network and that $REPO has a published release. If the repository is" >&2
            say "   private, set GH_TOKEN (or GITHUB_TOKEN) to a token that can read it, or log in with" >&2
            say "   \`gh auth login\`, and run this again." >&2
        else
            say "   Check that the token can read $REPO and that the release exists (drafts are not listed)." >&2
        fi
        exit 1
    fi

    # GitHub prints the answer indented or, behind some networks' proxies, compact on one line, so
    # it is first cut into one member per line (after every comma, brace and bracket) and then
    # matched with any whitespace around the colon. No jq or python is needed.
    awk '{ gsub(/[,{}[]/, "&\n"); print }' "$WORK/release.json" | tr -d '\r' > "$WORK/release.lines"

    TAG="$(awk -F'"' '/^[[:space:]]*"tag_name"[[:space:]]*:/ {print $4; exit}' "$WORK/release.lines")"
    [ -n "$TAG" ] || fail "the release at $release_url has no tag."

    # "name<TAB>API url" for every asset. GitHub prints an asset's own "url" before its "name"; the
    # uploader's "url" in between is a user URL and is not taken.
    awk -F'"' '
        /^[[:space:]]*"url"[[:space:]]*:[[:space:]]*"[^"]*\/releases\/assets\/[0-9]+"/ { url = $4 }
        /^[[:space:]]*"name"[[:space:]]*:/ && url != "" { print $4 "\t" url; url = "" }
    ' "$WORK/release.lines" > "$WORK/assets.tsv"

    asset_url() { awk -F'\t' -v name="$1" '$1 == name {print $2; exit}' "$WORK/assets.tsv"; }

    fetch() {  # fetch <asset name>: downloads it into $WORK, or fails
        local url; url="$(asset_url "$1")"
        [ -n "$url" ] || return 1
        local header=()
        case "$url" in "$API"/*) header=(${auth[@]+"${auth[@]}"}) ;; esac
        curl -fsSL ${header[@]+"${header[@]}"} -H "Accept: application/octet-stream" "$url" -o "$WORK/$1"
    }
fi

say "🐧 Ragon $TAG for $TARGET, role: $ROLE"

SUMS="ragon-$TARGET.sha256sums"
if ! fetch "$SUMS"; then
    fail "release $TAG has no binaries for $TARGET (no $SUMS). Published targets: $(awk -F'\t' '/sha256sums/ {sub(/^ragon-/, "", $1); sub(/\.sha256sums$/, "", $1); printf "%s ", $1}' "$WORK/assets.tsv")"
fi

# -- the signature ---------------------------------------------------------------------------

# Whether a tag is SIGNED_SINCE or newer. A tag that is not a version is treated as new: a name
# nobody can read is no reason to skip the check (`ragon update` decides the same way).
signing_required() {
    awk -v tag="${1#v}" -v since="$SIGNED_SINCE" 'BEGIN {
        if (tag !~ /^[0-9]+\.[0-9]+\.[0-9]+/) exit 0
        split(tag, a, /[.+-]/); split(since, b, ".")
        for (i = 1; i <= 3; i++) { if (a[i] + 0 > b[i] + 0) exit 0; if (a[i] + 0 < b[i] + 0) exit 1 }
        exit 0
    }'
}
signers() {  # the allowed-signers lines for the keys of a public-key file on stdin
    awk 'NF && $1 !~ /^#/ { print "ragon-release " $1 " " $2 }'
}

if [ -n "$(asset_url SHA256SUMS.sig)" ]; then
    command -v ssh-keygen >/dev/null 2>&1 || fail "ssh-keygen (OpenSSH 8.1 or newer) is needed to \
check the release's signature and was not found. On Debian and Ubuntu: sudo apt install openssh-client."
    fetch SHA256SUMS.sig || fail "could not download SHA256SUMS.sig."
    fetch SHA256SUMS || fail "release $TAG has SHA256SUMS.sig but no SHA256SUMS; nothing was installed."
    printf '%s\n' "$RELEASE_KEYS" | signers > "$WORK/allowed_signers"
    # A key rotation: the release names the new key, signed by a key trusted here under a namespace
    # of its own (specs/DREAMFERENCE_RELEASE_SIGNING.md §5).
    if [ -n "$(asset_url release-key-transition.pub)" ]; then
        fetch release-key-transition.pub && fetch release-key-transition.pub.sig \
            || fail "release $TAG names a new release key but its endorsement could not be downloaded."
        ssh-keygen -Y verify -f "$WORK/allowed_signers" -I ragon-release -n ragon-release-key \
            -s "$WORK/release-key-transition.pub.sig" < "$WORK/release-key-transition.pub" >/dev/null 2>&1 \
            || fail "release $TAG names a new release key that no trusted key has signed; nothing was installed."
        signers < "$WORK/release-key-transition.pub" >> "$WORK/allowed_signers"
    fi
    if ! ssh-keygen -Y verify -f "$WORK/allowed_signers" -I ragon-release -n ragon-release \
        -s "$WORK/SHA256SUMS.sig" < "$WORK/SHA256SUMS" > "$WORK/verify.out" 2>&1; then
        fail "release $TAG failed its signature check: SHA256SUMS is not signed by Ragon's \
release key ($(tail -n 1 "$WORK/verify.out")). Nothing was installed."
    fi
    # The per-target sums file is one of the files SHA256SUMS lists, which is what ties every
    # binary checked below to the signature.
    wanted="$(awk -v file="$SUMS" '{ f = $2; sub(/^\*/, "", f); if (f == file) { print $1; exit } }' "$WORK/SHA256SUMS")"
    [ -n "$wanted" ] || fail "SHA256SUMS has no checksum for $SUMS; nothing was installed."
    [ "$(sha256 "$WORK/$SUMS")" = "$wanted" ] \
        || fail "$SUMS does not match its checksum in SHA256SUMS; nothing was installed."
    say "🔏 Release $TAG is signed by Ragon's release key."
elif [ -n "$FROM" ] && [ "${TAG#this-}" != "$TAG" ]; then
    # `ragon-admin node provision` bundles the build this node runs (VERSION `this-<digest>`): no
    # release job made it, so there is no release signature to check, only its checksums. A
    # bundle of a release (VERSION `vX.Y.Z`) carries that release's signature and is checked above.
    say "⚠️  Bundle $TAG is a build copied from another node, not a release; it is checked against its checksums only."
elif [ -z "$VERSION" ] && [ -z "$FROM" ]; then
    # Asked for the latest release, an install takes whatever is marked latest: the tag of an
    # unsigned one is only a name, and an old-looking one (v0.8.9) must not bypass the signature.
    # A release from before signing is installed only when named with --version.
    fail "the latest release ($TAG) is not signed (it has no SHA256SUMS.sig); nothing was installed. \
Pass --version to install a release from before $SIGNED_SINCE on purpose."
elif signing_required "$TAG"; then
    fail "release $TAG is not signed (it has no SHA256SUMS.sig); nothing was installed. Every release \
from $SIGNED_SINCE on is signed by Ragon's release key, so an unsigned one did not come from its release job."
else
    say "⚠️  Release $TAG predates signed releases ($SIGNED_SINCE); it is checked against its checksums only."
fi

# Required, then optional, as `ragon update` treats them. The local file index, ragon-docs,
# is built for Linux only, as is ragon-signal, Ragon over Signal's bridge. The bridge is installed and nothing more: it is off until `ragon signal setup`,
# which fetches signal-cli and its Java runtime, pinned (specs/DREAMFERENCE_RAGON_SIGNAL.md §3).
# ragon-delegate, the delegation plugin's server as a binary of its own, is Linux only too and
# linked like ragon (see the install loop below).
REQUIRED="ragon codex-code-mode-host"
OPTIONAL="ragon-search ragon-fetch ragon-code"
[ "$(uname -s)" = Linux ] && OPTIONAL="$OPTIONAL ragon-docs ragon-signal ragon-delegate"
INSTALLED=""
for name in $REQUIRED $OPTIONAL; do
    asset="$name-$TARGET.gz"
    if ! fetch "$asset"; then
        case " $REQUIRED " in *" $name "*) fail "release $TAG is missing $asset." ;; esac
        continue
    fi
    wanted="$(awk -v file="$asset" '{ f = $2; sub(/^\*/, "", f); if (f == file) { print $1; exit } }' "$WORK/$SUMS")"
    [ -n "$wanted" ] || fail "$SUMS has no checksum for $asset; nothing was installed."
    [ "$(sha256 "$WORK/$asset")" = "$wanted" ] \
        || fail "$asset does not match its checksum in $SUMS; nothing was installed."
    gzip -dc "$WORK/$asset" > "$WORK/$name"
    INSTALLED="$INSTALLED $name"
done

# The desktop app on a Mac: the release's dmg for this Mac (mac-preview.yml names it
# Ragon-<version>-<arm64|x64>[-preview].dmg), checked against the release-wide SHA256SUMS,
# which the signature above covers, before anything is placed. A release without one (a Mac build
# is optional in release.yml) installs the terminal agent alone. curl sets no quarantine mark, so
# neither the dmg nor the app copied out of it meets Gatekeeper's first-launch dialog.
APP_DMG=""
if [ "$APP" = 1 ] && [ "$os" = "apple-darwin" ]; then
    case "$arch" in aarch64) app_arch="arm64" ;; *) app_arch="x64" ;; esac
    dmg="$(awk -F'\t' -v pattern="^Ragon-.*-$app_arch(-preview)?[.]dmg\$" '$1 ~ pattern {print $1; exit}' "$WORK/assets.tsv")"
    if [ -z "$dmg" ]; then
        say "ℹ️  Release $TAG carries no desktop app for this Mac; installing the terminal agent only."
    elif ! { [ -f "$WORK/SHA256SUMS" ] || fetch SHA256SUMS; }; then
        say "⚠️  Release $TAG has no SHA256SUMS to check $dmg against; the desktop app is left out."
    else
        wanted="$(awk -v file="$dmg" '{ f = $2; sub(/^\*/, "", f); if (f == file) { print $1; exit } }' "$WORK/SHA256SUMS")"
        [ -n "$wanted" ] || fail "SHA256SUMS has no checksum for $dmg; nothing was installed."
        say "📦 Downloading the desktop app ($dmg) ..."
        fetch "$dmg" || fail "could not download $dmg; nothing was installed."
        [ "$(sha256 "$WORK/$dmg")" = "$wanted" ] \
            || fail "$dmg does not match its checksum in SHA256SUMS; nothing was installed."
        APP_DMG="$dmg"
    fi
fi

# Everything is checked before anything is placed. Each file is renamed over the old one, so a
# running session keeps its binary and a new one never sees a half-written file.
mkdir -p "$INSTALL_DIR/bin" "$LINK_DIR"
link() {  # link <target> <name>: ~/.local/bin/<name> -> target, never over a real file
    if [ -e "$LINK_DIR/$2" ] && [ ! -L "$LINK_DIR/$2" ]; then
        say "⚠️  $LINK_DIR/$2 exists and is not a link; leaving it. Run $1 directly."
        return
    fi
    ln -sfn "$1" "$LINK_DIR/$2"
}
for name in $INSTALLED; do
    install -m 0755 "$WORK/$name" "$INSTALL_DIR/bin/.$name.new"
    mv -f "$INSTALL_DIR/bin/.$name.new" "$INSTALL_DIR/bin/$name"
    # Codex looks for its Code Mode host beside its own executable, so that one needs no link,
    # and `ragon signal` finds its bridge there too: its commands are `ragon signal …`. `ragon-delegate` is linked:
    # the delegation plugin runs it, and must not depend on which `ragon` is installed.
    case "$name" in codex-code-mode-host|ragon-signal) ;; *) link "$INSTALL_DIR/bin/$name" "$name" ;; esac
done
# What was installed, for `node provision`'s state probe and drift report.
printf '%s\n' "$TAG" > "$INSTALL_DIR/VERSION"
say "✅ Installed$INSTALLED in $INSTALL_DIR/bin"
step done "Ragon $TAG installed:$INSTALLED"

# The app goes where a copy already is, else into /Applications when this user may write there
# (an administrator of the Mac may), else ~/Applications, which needs no one's permission. It is
# copied out of the mounted dmg under a hidden name and renamed over the old one, so a failure
# leaves the previous app in place.
if [ -n "$APP_DMG" ]; then
    if [ -z "$APP_DIR" ]; then
        for candidate in /Applications "$HOME/Applications"; do
            if [ -d "$candidate/Ragon.app" ] && [ -w "$candidate" ]; then APP_DIR="$candidate"; break; fi
        done
    fi
    if [ -z "$APP_DIR" ]; then
        if [ -d /Applications ] && [ -w /Applications ]; then APP_DIR="/Applications"; else APP_DIR="$HOME/Applications"; fi
    fi
    APP_PATH="$APP_DIR/Ragon.app"
    # A dmg that somehow carries the mark would pass it on to everything mounted from it.
    xattr -d com.apple.quarantine "$WORK/$APP_DMG" 2>/dev/null || true
    mkdir -p "$WORK/dmg" "$APP_DIR"
    rm -rf "$APP_DIR/.Ragon.app.new"
    if ! hdiutil attach "$WORK/$APP_DMG" -nobrowse -readonly -noverify -noautoopen -quiet -mountpoint "$WORK/dmg" >> "$WORK/app.log" 2>&1; then
        say "⚠️  Could not open $APP_DMG ($(tail -n 1 "$WORK/app.log")); the desktop app was not installed."
        step failed "desktop app: $APP_DMG did not mount; run this script again"
    else
        MOUNTED="$WORK/dmg"
        if [ -d "$WORK/dmg/Ragon.app" ] && ditto "$WORK/dmg/Ragon.app" "$APP_DIR/.Ragon.app.new" >> "$WORK/app.log" 2>&1; then
            hdiutil detach "$WORK/dmg" -quiet >> "$WORK/app.log" 2>&1 || hdiutil detach "$WORK/dmg" -force -quiet 2>/dev/null || true
            MOUNTED=""
            rm -rf "$APP_DIR/.Ragon.app.old"
            if [ -e "$APP_PATH" ]; then mv "$APP_PATH" "$APP_DIR/.Ragon.app.old"; fi
            mv "$APP_DIR/.Ragon.app.new" "$APP_PATH"
            rm -rf "$APP_DIR/.Ragon.app.old"
            xattr -dr com.apple.quarantine "$APP_PATH" 2>/dev/null || true
            # What `ragon update` reads to update the app too (update.rs, APP_RECORD).
            printf 'path=%s\nversion=%s\n' "$APP_PATH" "$TAG" > "$INSTALL_DIR/desktop-app"
            say "✅ Installed the desktop app: $APP_PATH (open it from Launchpad or Spotlight)"
            step done "desktop app installed: $APP_PATH"
        else
            rm -rf "$APP_DIR/.Ragon.app.new"
            say "⚠️  Could not copy Ragon.app out of $APP_DMG; the desktop app was not installed."
            step failed "desktop app: could not copy it out of $APP_DMG; run this script again"
        fi
    fi
elif [ "$APP" = 0 ] && [ "$os" = "apple-darwin" ]; then
    step off "desktop app: --no-app"
fi

# -- the node --------------------------------------------------------------------------------

if [ "$ROLE" = "node" ]; then
    command -v python3 >/dev/null 2>&1 || fail "the node needs python3 (3.10 or newer) and it was not found."
    command -v docker >/dev/null 2>&1 \
        || say "⚠️  docker was not found. The model server runs in Docker; install it before \`ragon-admin server start\`."

    wheel="$(awk -F'\t' '$1 ~ /^dreamference-.*\.whl$/ {print $1; exit}' "$WORK/assets.tsv")"
    [ -n "$wheel" ] || fail "release $TAG has no Python wheel, so the node cannot be installed from it."
    fetch "$wheel" || fail "could not download $wheel."
    # The wheel is checked like the binaries, against the release-wide SHA256SUMS (signed, and
    # already checked above); a release without one is said rather than skipped silently.
    if [ -f "$WORK/SHA256SUMS" ] || fetch SHA256SUMS; then
        wanted="$(awk -v file="$wheel" '{ f = $2; sub(/^\*/, "", f); if (f == file) { print $1; exit } }' "$WORK/SHA256SUMS")"
        [ -n "$wanted" ] || fail "SHA256SUMS has no checksum for $wheel; nothing was installed."
        [ "$(sha256 "$WORK/$wheel")" = "$wanted" ] \
            || fail "$wheel does not match its checksum in SHA256SUMS; nothing was installed."
    else
        say "⚠️  Release $TAG has no SHA256SUMS, so $wheel is installed without a checksum check."
    fi

    if [ ! -x "$VENV_DIR/bin/python" ]; then
        say "🐍 Creating $VENV_DIR ..."
        if ! python3 -m venv "$VENV_DIR" > "$WORK/venv.log" 2>&1; then
            # DGX OS ships python3 without venv's ensurepip (python3-venv): measured on a fresh
            # GX10. It is installed here rather than left as a next step.
            rm -rf "$VENV_DIR"
            if [ "$NODE_ROOT" = 0 ] && [ "$(id -u)" != 0 ]; then
                sudo -n true 2>/dev/null && SUDO="sudo -n" \
                    || fail "python3 could not create a virtualenv: install python3-venv (sudo apt install python3-venv), then run this again."
            fi
            say "🐍 Installing python3-venv, which this system's python3 lacks ..."
            $SUDO env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 install -y python3-venv \
                >> "$WORK/venv.log" 2>&1 < /dev/null \
                || fail "could not install python3-venv: $(tail -n 2 "$WORK/venv.log" | tr '\n' ' ')"
            python3 -m venv "$VENV_DIR" >> "$WORK/venv.log" 2>&1 \
                || fail "python3 could not create a virtualenv even with python3-venv: $(tail -n 2 "$WORK/venv.log" | tr '\n' ' ')"
            step done "python3-venv installed (this system's python3 lacked it)"
        fi
    fi
    say "📦 Installing ragon-admin and what it depends on (about 6 GB with PyTorch; a few minutes) ..."
    pip_index=()
    if [ -n "$FROM" ] && [ -d "$FROM/wheelhouse" ]; then
        # Every dependency is in the bundle: no package index is asked.
        pip_index=(--no-index --find-links "$FROM/wheelhouse")
    else
        "$VENV_DIR/bin/python" -m pip install --quiet --upgrade pip
    fi
    "$VENV_DIR/bin/python" -m pip install --quiet --upgrade ${pip_index[@]+"${pip_index[@]}"} "$WORK/$wheel"
    # pip keeps an installed version it is given again, and a bundle of a checkout (node provision)
    # carries the release's version number: the wheel itself is put in place, its dependencies not.
    "$VENV_DIR/bin/python" -m pip install --quiet --force-reinstall --no-deps ${pip_index[@]+"${pip_index[@]}"} "$WORK/$wheel"
    link "$VENV_DIR/bin/ragon-admin" ragon-admin
    say "✅ Installed ragon-admin $TAG in $VENV_DIR"
    step done "ragon-admin $TAG installed in $VENV_DIR"

    # From here on a closed reader or a dropped ssh session does not stop the install halfway: the
    # rest runs to its summary (the long part is logged to a file anyway).
    trap '' HUP PIPE
    mkdir -p "$LOG_DIR"
    say "📝 The long part (model download, server start, system updates) is logged to $LOG"

    # Every `ragon-admin` here has no terminal on its standard input, so none of them can ask a
    # question (the sandbox check of every run asks one at a terminal).
    admin() { "$VENV_DIR/bin/ragon-admin" "$@" < /dev/null; }

    if [ "$NODE_ROOT" = 1 ]; then
        # The settings a model load is refused without, applied: each command is printed first,
        # and runs with `sudo -n` on the password given above.
        say ""
        if admin host setup --yes; then
            step done "host settings applied (swap, sysctls, earlyoom, sysstat, bubblewrap, Wi-Fi power saving off)"
        else
            step failed "host settings: \`ragon-admin host check\` lists what is left, \`ragon-admin host setup\` applies it"
        fi

        # Docker without sudo, which NVIDIA's first-boot wizard does not set up.
        if ! getent group docker > /dev/null 2>&1; then
            step failed "docker group: there is none, so Docker is not installed (DGX OS ships it); install Docker and the NVIDIA Container Toolkit, then run this again"
        elif id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx docker; then
            step done "docker group: $USER_NAME is in it"
        elif $SUDO usermod -aG docker "$USER_NAME" < /dev/null; then
            # Only the model's steps reach Docker, through `sg docker` (with_docker below).
            meanwhile=""
            if [ "$MODEL" = 1 ]; then meanwhile="; this script used \`sg docker\` meanwhile"; fi
            step done "docker group: $USER_NAME added (your next login has it$meanwhile)"
        else
            step failed "docker group: \`sudo usermod -aG docker $USER_NAME\` failed"
        fi

        # Lingering: without it the user's systemd stops at logout, and with it every job sent
        # from another node and every Night Shift task.
        if [ -e "$LINGER_DIR/$USER_NAME" ]; then
            step done "lingering is on (jobs and Night Shift outlive a logout)"
        elif $SUDO loginctl enable-linger "$USER_NAME" < /dev/null; then
            step done "lingering turned on (jobs and Night Shift outlive a logout)"
        else
            step failed "lingering: \`sudo loginctl enable-linger $USER_NAME\` failed"
        fi
    else
        step off "host settings, docker group and lingering: --no-host-setup (\`sudo ragon-admin node prepare\` applies them)"
    fi

    # A machine with the node half is a node: its id is written now, so `ragon` here uses this
    # machine's own model server and never looks for another one on the network
    # (specs/DREAMFERENCE_RAGON_NODE.md §6.1, §9).
    admin node id > /dev/null || true
    # Then it is offered to the local network: the model, web search and the web UI, to every
    # machine on it (the command says so). No question is asked; --no-advertise is the opt-out.
    if [ "$ADVERTISE" = 1 ]; then
        say ""
        if admin node enable --yes; then
            step done "advertised on the local network (\`ragon-admin node disable\` stops it)"
        else
            step failed "advertising: \`ragon-admin node enable\` says why"
        fi
    else
        step off "advertising: --no-advertise (\`ragon-admin node enable\` offers this node later)"
    fi

    # The model: downloaded, then served. Docker is reached through `sg docker` when this login
    # predates the group (`node provision` does the same, FleetSession.with_docker_group).
    with_docker() {
        if id -nG | tr ' ' '\n' | grep -qx docker \
            || ! id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx docker \
            || ! command -v sg > /dev/null 2>&1; then
            admin "$@"
        else
            sg docker -c "$(printf '%q ' "$VENV_DIR/bin/ragon-admin" "$@") < /dev/null"
        fi
    }
    # The local file index's run-time files: PDFium, ONNX Runtime and its embedding model, about
    # 320 MB, each pinned and checked (`ragon-admin docs setup`; ragon-docs itself opens no socket).
    case " $INSTALLED " in
        *" ragon-docs "*)
            if [ "$MODEL" = 1 ]; then
                say ""
                say "⬇️  Downloading what the local file index loads (about 320 MB) ... log: $LOG"
                if admin docs setup >> "$LOG" 2>&1; then
                    step done "local file index: PDFium, ONNX Runtime and its embedding model installed"
                else
                    step failed "local file index: \`ragon-admin docs setup\` failed, see $LOG"
                fi
            else
                step off "local file index: --no-model (\`ragon-admin docs setup\` installs what it loads)"
            fi
            ;;
    esac

    if [ "$MODEL" = 1 ]; then
        say ""
        say "⬇️  Downloading the default model (tens of GB; this is the long part) ... log: $LOG"
        if with_docker model download >> "$LOG" 2>&1; then
            step done "default model downloaded"
            say "🚀 Starting the model server ... log: $LOG"
            if with_docker server start >> "$LOG" 2>&1; then
                step done "model server started (\`ragon-admin status\`)"
            else
                step failed "model server: \`ragon-admin server start\` failed, see $LOG"
            fi
        else
            step failed "default model: \`ragon-admin model download\` failed, see $LOG"
            step failed "model server: not started, because the model is missing; \`ragon-admin server start\` downloads and starts it"
        fi
    else
        step off "model: --no-model (\`ragon-admin server start\` downloads and serves it)"
    fi
fi

# -- system updates, installed if asked --------------------------------------------------------

UPDATE_COMMANDS="sudo apt-get update && sudo apt-get full-upgrade"
FIRMWARE_COMMANDS="sudo fwupdmgr refresh && sudo fwupdmgr update"
if [ "$INSTALL_UPDATES" = 1 ]; then
    mkdir -p "$LOG_DIR"
    say ""
    say "📦 Installing the system updates ... log: $LOG"
    if [ -n "$UPGRADES" ]; then
        if $SUDO env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 \
                -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -y full-upgrade \
                >> "$LOG" 2>&1 < /dev/null; then
            step done "system packages upgraded ($(printf '%s' "$UPGRADES" | wc -w | tr -d ' '))"
            REBOOT=1
        else
            step failed "system packages: \`$UPDATE_COMMANDS\` failed, see $LOG"
        fi
    fi
    if [ -n "$FIRMWARE" ]; then
        if $SUDO fwupdmgr update -y --no-reboot-check >> "$LOG" 2>&1 < /dev/null; then
            step done "firmware staged; it is written during the next reboot"
            REBOOT=1
        else
            step failed "firmware: \`$FIRMWARE_COMMANDS\` failed, see $LOG"
        fi
    fi
elif [ -n "$UPGRADES" ] || [ -n "$FIRMWARE" ]; then
    detail=""
    if [ -n "$UPGRADES" ]; then
        detail="$(printf '%s' "$UPGRADES" | wc -w | tr -d ' ') package(s)"
        if [ -n "$NVIDIA_UPGRADES" ]; then detail="$detail, NVIDIA's: $NVIDIA_UPGRADES"; fi
        detail="$detail; install: $UPDATE_COMMANDS"
    fi
    if [ -n "$FIRMWARE" ]; then
        detail="${detail:+$detail; }$(printf '%s' "$FIRMWARE" | grep -c .) firmware update(s); install: $FIRMWARE_COMMANDS"
    fi
    step available "system updates, available, not installed: $detail; then reboot"
fi

# -- the summary -----------------------------------------------------------------------------

case ":$PATH:" in
    *":$LINK_DIR:"*) ;;
    *) say ""; say "⚠️  $LINK_DIR is not on your PATH. Add it:  export PATH=\"$LINK_DIR:\$PATH\"" ;;
esac

FAILED=0
say ""
if [ "$ROLE" = "node" ]; then
    say "━━ Ragon $TAG on $(uname -n 2>/dev/null || echo this machine) ━━"
    for entry in "${STEPS[@]}"; do
        state="${entry%%|*}"; text="${entry#*|}"
        case "$state" in
            done)      say "  ✅ $text" ;;
            failed)    say "  ❌ $text"; FAILED=1 ;;
            off)       say "  ⚪ $text" ;;
            available) say "  📦 $text" ;;
        esac
    done
    if [ "${REBOOT:-0}" = 1 ]; then
        say "  🔁 Reboot now to finish the updates: sudo reboot. The model server comes back by itself"
        say "     (Docker restarts it); the firmware is written during that reboot."
    fi
    say ""
    if [ "$FAILED" = 1 ]; then
        say "⚠️  Not everything succeeded: the lines with ❌ say what to run. Log: $LOG"
    elif [ "$ADVERTISE" = 1 ]; then
        say "🎉 Done. \`ragon\` is the terminal agent here, and \`ragon\` on your other computers finds this node."
    else
        say "🎉 Done. \`ragon\` is the terminal agent."
    fi
else
    say "🎉 Done. \`ragon\` needs a Ragon node to talk to: start one on a GB10, then run \`ragon\`."
fi
exit "$FAILED"

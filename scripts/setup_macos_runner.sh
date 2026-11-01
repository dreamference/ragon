#!/usr/bin/env bash
# Sets up a fresh Mac (the macOS CI runner and QA box) after its first boot: one command, idempotent.
# Docs: docs/dev/ci-runners.md, "Setting up a new Mac". Run it ON the Mac as its administrator
# (not with sudo; it asks for the password once and uses sudo itself), usually from the node:
#
#   scp -qr scripts/setup_macos_runner.sh scripts/macos/ci-runner <mac>:/tmp/
#   ssh -t <mac> bash /tmp/setup_macos_runner.sh --token "$(gh api -X POST \
#       repos/dreamference/ragon/actions/runners/registration-token --jq .token)" \
#       --ssh-pubkey "$(cat ~/.ssh/id_ed25519.pub)"
#
# Add --dry-run first to see what it would change; it changes nothing and needs no password.
#
# Steps, each skipped when already done:
#   1. command line tools, Homebrew, and the packages in BREW_PACKAGES
#   2. power: never sleep, wake on network, restart after a power cut
#   3. Remote Login (ssh) on, and the coordinator's public key in authorized_keys
#   4. the overlay client (--overlay netbird|tailscale), joined if its key is in the environment
#   5. the GitHub Actions runner, through scripts/macos/ci-runner/setup.sh (only with --token)
# The QA run (scripts/qa/macos_qa.sh) needs only tools macOS ships (curl, tar, hdiutil, shasum, ssh).
#
# Secrets: none in this file. The registration token is an argument (valid an hour) and is never
# printed; an overlay key is read from NETBIRD_SETUP_KEY or TAILSCALE_AUTHKEY in the environment.
set -euo pipefail

BREW_PACKAGES=(gh jq git python@3.12)
RUNNER_DIR_SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ci-runner"

dry=0 token="" pubkey="" overlay=none overlay_url="" runner_name="" runner_labels="" jobs=""
while (($#)); do
    case "$1" in
        --dry-run) dry=1 ;;
        --token) token="$2"; shift ;;
        --ssh-pubkey) pubkey="$2"; shift ;;
        --overlay) overlay="$2"; shift ;;
        --overlay-url) overlay_url="$2"; shift ;;
        --name) runner_name="$2"; shift ;;
        --labels) runner_labels="$2"; shift ;;
        --jobs) jobs="$2"; shift ;;
        -h | --help) sed -n 2,24p "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
case "$overlay" in none | netbird | tailscale) ;; *) echo "--overlay: none, netbird or tailscale" >&2; exit 2 ;; esac

[[ "$(uname -s)" == Darwin ]] || { echo "this sets up a Mac" >&2; exit 1; }
[[ $EUID -ne 0 ]] || { echo "run as the administrator, without sudo" >&2; exit 1; }
dseditgroup -o checkmember -m "$(id -un)" admin >/dev/null 2>&1 || { echo "$(id -un) is not an administrator" >&2; exit 1; }

changes=0
say() { echo "  $*"; }
# act "what" cmd...: runs cmd, or only reports it in a dry run.
act() {
    local what="$1"; shift
    changes=$((changes + 1))
    if ((dry)); then echo "  WOULD: $what"; else echo "  DO: $what"; "$@"; fi
}
step() { echo "== $*"; }

if ((!dry)); then sudo -v; fi
as_root() { if ((dry)); then sudo -n "$@" 2>/dev/null; else sudo "$@"; fi; }

# 1. Toolchain ---------------------------------------------------------------------------------
step "command line tools and Homebrew"
if xcode-select -p >/dev/null 2>&1; then say "command line tools present"; else
    act "start the command line tools install (a dialog may appear on the Mac's screen)" xcode-select --install || true
    ((dry)) || { echo "finish the install, then run this again" >&2; exit 1; }
fi
brew_bin=""
for b in /opt/homebrew/bin/brew /usr/local/bin/brew; do [[ -x $b ]] && brew_bin=$b && break; done
if [[ -z $brew_bin ]]; then
    act "install Homebrew" /bin/bash -c 'NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
    ((dry)) || brew_bin=/opt/homebrew/bin/brew
else say "Homebrew at $brew_bin"; fi
if [[ -n $brew_bin ]]; then
    installed="$("$brew_bin" list --formula -1 2>/dev/null || true)"
    for p in "${BREW_PACKAGES[@]}"; do
        if grep -qx "$p" <<<"$installed"; then say "$p present"; else act "brew install $p" "$brew_bin" install "$p"; fi
    done
else
    for p in "${BREW_PACKAGES[@]}"; do changes=$((changes + 1)); echo "  WOULD: brew install $p"; done
fi
profile="$HOME/.zprofile"
if [[ -n $brew_bin ]] && ! grep -qs 'brew shellenv' "$profile"; then
    act "put brew on PATH in ~/.zprofile" bash -c "echo 'eval \"\$($brew_bin shellenv)\"' >> '$profile'"
elif [[ -z $brew_bin ]]; then
    changes=$((changes + 1)); echo "  WOULD: put brew on PATH in ~/.zprofile"
fi

# 2. Power ----------------------------------------------------------------------------------------
step "power: never sleep, wake on network, restart after a power cut"
want_power() { # key value
    local have
    have="$(pmset -g custom | awk -v k="$1" '$1 == k {print $2; exit}')"
    if [[ $have == "$2" ]]; then say "$1 = $2"; else act "pmset -a $1 $2 (now ${have:-unset})" sudo pmset -a "$1" "$2"; fi
}
want_power sleep 0
want_power disksleep 0
want_power displaysleep 0
want_power womp 1
want_power autorestart 1
want_power networkoversleep 0 2>/dev/null || true

# 3. ssh ----------------------------------------------------------------------------------------
step "Remote Login and the coordinator's key"
if as_root systemsetup -getremotelogin 2>/dev/null | grep -qi ": on"; then say "Remote Login on"; elif ((dry)); then
    say "Remote Login: cannot be read without a password; the script turns it on"
    changes=$((changes + 1))
else act "turn Remote Login on" sudo systemsetup -setremotelogin on; fi
if [[ -z $pubkey ]]; then say "no --ssh-pubkey given; authorized_keys untouched"; else
    auth="$HOME/.ssh/authorized_keys"
    if grep -qsxF "$pubkey" "$auth"; then say "key already in authorized_keys"; else
        add_key() { mkdir -p ~/.ssh && chmod 700 ~/.ssh && printf '%s\n' "$pubkey" >> "$auth" && chmod 600 "$auth"; }
        act "append the key to ~/.ssh/authorized_keys" add_key
    fi
fi

# 4. Overlay ------------------------------------------------------------------------------------
step "overlay client ($overlay)"
case "$overlay" in
    none) say "none requested" ;;
    netbird | tailscale)
        have=0; command -v "$overlay" >/dev/null 2>&1 && have=1
        [[ -n $brew_bin && -x /opt/homebrew/bin/$overlay ]] && have=1
        if ((have)); then say "$overlay installed"; else
            if [[ -n $brew_bin ]]; then act "brew install $overlay" "$brew_bin" install "$overlay"
            else changes=$((changes + 1)); echo "  WOULD: brew install $overlay"; fi
        fi
        if [[ $overlay == netbird && -n ${NETBIRD_SETUP_KEY:-} ]]; then
            act "netbird service install and join" bash -c "sudo netbird service install 2>/dev/null || true; sudo netbird service start 2>/dev/null || true; sudo netbird up ${overlay_url:+--management-url '$overlay_url'} --setup-key \"\$NETBIRD_SETUP_KEY\" >/dev/null"
        elif [[ $overlay == tailscale && -n ${TAILSCALE_AUTHKEY:-} ]]; then
            act "tailscale join" bash -c "sudo tailscaled install-system-daemon 2>/dev/null || true; sudo tailscale up --auth-key \"\$TAILSCALE_AUTHKEY\" >/dev/null"
        else say "not joined: set NETBIRD_SETUP_KEY or TAILSCALE_AUTHKEY in the environment to join"; fi ;;
esac

# 5. The Actions runner -----------------------------------------------------------------------------
step "GitHub Actions runner"
daemon=/Library/LaunchDaemons/actions.runner.dreamference-ragon.mac.plist
if [[ -f $daemon ]]; then say "runner LaunchDaemon installed"; elif [[ -z $token ]]; then
    say "no --token given; the runner is not set up"
else
    [[ -f $RUNNER_DIR_SRC/setup.sh ]] || { echo "ci-runner/ must sit beside this script" >&2; exit 1; }
    extra=()
    [[ -n $runner_name ]] && extra+=(--name "$runner_name")
    [[ -n $runner_labels ]] && extra+=(--labels "$runner_labels")
    [[ -n $jobs ]] && extra+=(--jobs "$jobs")
    act "stage the runner (registers it with the token, which is not printed)" \
        bash "$RUNNER_DIR_SRC/setup.sh" --stage --token "$token" ${extra[@]+"${extra[@]}"}
    act "install the runner as the hidden account's LaunchDaemon" \
        sudo bash /Users/Shared/mac-runner/ci-runner/setup.sh ${extra[@]+"${extra[@]}"}
fi

echo
if ((dry)); then echo "dry run: $changes change(s) would be made; nothing was changed"; else echo "done: $changes change(s) made"; fi

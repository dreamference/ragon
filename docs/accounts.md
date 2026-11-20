# Accounts and signing in

Every Ragon node asks you to sign in before it answers. One node of your network, the **primary**,
keeps the accounts; every other node, a **secondary**, asks the primary. You sign in once per
machine and `ragon` remembers it.

## First sign-in

The first time you start `ragon` after the 0.9.9 upgrade, a one-line note says that Ragon now has
accounts, that your repositories and settings are unchanged and that phones need to pair again.
Enter continues to the sign-in screen, which has four choices:

| Choice | Use it when |
|---|---|
| Sign in to *node* | you have an account: type the username and the password |
| Create an account on *node* | you are new: username, password, and the password again |
| Sign in with a device code | the machine has no keyboard to hand: you approve a one-time code from another device |
| Provide your own API key | you hold an `sk-ragon-` key (see [API keys](#api-keys)) |

A username is 1 to 32 letters, digits, `.`, `_` or `-`. The password may be empty; the create form
asks once, "Continue without a password?", before it accepts one. There is no email address and no
display name. The first account created on a primary is its admin and the screen says so.

If the node cannot reach its primary, the screen says so and offers `r` to retry. There is no
start without an account.

## Sign in from the command line

```bash
ragon login                                        # the same choices, as a plain prompt
printf '%s' "$PASSWORD" | ragon login --username alice --password-stdin
ragon login --device-auth                          # a one-time code, approved on another device
printf '%s' "$KEY" | ragon login --with-api-key    # an sk-ragon- key from stdin
```

The password goes through stdin, never the command line. After an admin has reset your password,
your next sign-in asks you to set a new one first.

## Checking what is signed in

```bash
ragon login status        # "Signed in as alice (user) at <node>"
ragon login status --all  # every primary this machine knows, the active one marked
ragon doctor              # adds the same line to its report
```

`ragon login status` exits 1 and says "Not signed in. Run ragon login." when there is no account.
In the terminal the status line shows `<user> @ <node>`.

If you use more than one primary, `~/.ragon/auth.json` keeps one entry per primary and `ragon`
switches to the one belonging to the node it finds.

## Sessions and logging out

```bash
ragon login sessions                 # machines signed in as you, with the last use of each
ragon login sessions --revoke <id>   # sign one machine out from anywhere
ragon logout                         # this machine only
ragon logout --all                   # every machine; API keys stay until revoked by name
```

## Slash commands

These work in the terminal, and the desktop app and `ragon web` have the same actions in their
account menu and Account page.

| Command | Does |
|---|---|
| `/whoami` | user, role, node and primary, and today's usage |
| `/password` | changes your password: the current one, then the new one twice |
| `/apikey`, `/apikey list`, `/apikey revoke <name>` | mints, lists and revokes your API keys |
| `/sessions` | lists and revokes your sessions and your phone or chat pairings |
| `/logout` | signs this machine out |
| `/users` | admins only: list, reset, remove, promote, demote, and see everyone's sessions |
| `/delete-account` | removes your account after you type your username; the last admin cannot |

In a chat (Signal, Telegram, Matrix) only `/whoami` and `/sessions` exist, and read only.

## API keys

A key looks like `sk-ragon-…` and is shown once, when you mint it with `/apikey` or an admin mints
it with `ragon-admin node user key <name>`. Ragon stores only a hash, so a lost key is replaced,
not recovered. You may hold several and revoke each by name. Use a key where a tool wants an API
key for an OpenAI-compatible server.

The mint ends by printing what each tool needs. For the OpenAI SDK, or any tool that reads the
same variables:

```bash
export OPENAI_BASE_URL=https://<node>.local:8000/v1
export OPENAI_API_KEY=sk-ragon-…
```

```python
from openai import OpenAI

client = OpenAI()   # reads the two variables above
print(client.responses.create(model="<served model>", input="Reply with: ready").output_text)
```

For an upstream Codex, the mint also prints a `config.toml` block that names the node as a
provider with `wire_api = "responses"`. Ragon's own `ragon` needs none of this: it reads the
stored sign-in.

## Trusting the node's certificate

The node serves HTTPS with a certificate from its own authority. `ragon` pins that authority the
first time it meets the node (the file is `~/.ragon/nodes/<node id>/ca.pem`) and refuses a node
that later presents a certificate from another one. The installers and the pairing step also add
the authority to your own trust store, so a browser shows no warning. For other tools, the mint's
last lines print the certificate path and the variables to set:

```bash
export SSL_CERT_FILE=<ca file>
export REQUESTS_CA_BUNDLE=<ca file>
export NODE_EXTRA_CA_CERTS=<ca file>
```

## The primary and the secondaries

`ragon-admin init` asks whether this node keeps the accounts (primary) or asks another node
(secondary). A secondary pins its primary's node id, so another node cannot pose as it. It keeps
no passwords: it holds the primary's public key and a short in-memory cache of answers.

```bash
ragon-admin node primary               # show the choice
ragon-admin node primary <node id>     # a secondary changes its primary
ragon-admin node primary --self        # this node becomes the primary
```

## Admin commands

On the primary, `ragon-admin node user` manages accounts (the full list is in
[ragon-admin](admin.md#ragon-admin-node-user)):

| Command | Does |
|---|---|
| `list [--sessions]` | users, roles and last use; the sessions and pairings with the flag |
| `add <name>` | creates a user, the password read without echo |
| `reset <name>` | prints a one-time password; the user sets a new one at the next sign-in |
| `remove <name>` | removes the user with their sessions, keys and pairings |
| `promote <name>` / `demote <name>` | changes the role; the last admin cannot be demoted |
| `key <name>` | mints an API key for the user |
| `export` / `import <file>` | moves users, roles and key hashes, never sessions |
| `log` | the audit lines |
| `usage [--days N]` | requests and tokens per user per day |

Each node has one account of its own for the unattended jobs (Night Shift, the benchmark harness,
the egress audit); its key sits in a file under `~/.config/dreamference/`, readable by you alone,
and it cannot sign in at the screen. `ragon-admin node migrate` sets the accounts up on a node
that is upgrading to 0.9.9.

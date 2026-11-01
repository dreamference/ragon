# Phone messengers

You can ask Ragon questions from your phone, through **Signal**, **Matrix**, **Telegram** or **Discord**.
A question you send becomes an Ask thread on the node, the same kind `ragon web` opens, and the
answer comes back in the same chat.

Every messenger is **off by default**. Installing or updating Ragon turns none of them on. Until
you run a messenger's setup command on the node, nothing for it runs, listens on a port, or connects
anywhere. When one is on, `ragon-admin audit egress` lists it in every report as a declared exception.

| Messenger | Privacy | Turn it on | Turn it off |
|---|---|---|---|
| **Signal** | End-to-end encrypted. Uses your own account, in Note to Self. | `ragon signal setup` | `ragon signal remove` |
| **Matrix** | Your own homeserver on the node, reached over your tailnet. | `ragon-admin matrix start`, then `ragon chat start` | `ragon chat stop`, `ragon-admin matrix stop` |
| **Telegram** | Less private: messages pass through Telegram's servers unencrypted. | `ragon chat telegram setup`, then `ragon chat start` | `ragon chat telegram off` |
| **Discord** | The least private: Discord keeps every message, unencrypted, with your account. | `ragon chat discord setup`, then `ragon chat start` | `ragon chat discord off` |

All of them need `ragon web` running on the node (`ragon web start`). The air gap applies to the
three that leave the machine. At `/airgapped on`, Signal sends nothing, not even a read receipt, and
Telegram and Discord pause. When the air gap is off again, Signal tells you what you missed. Matrix keeps
working, because its homeserver is on the node and has no route out.

**Matrix, Telegram and Discord are previews.** They are tested against stand-ins of a homeserver,
of Telegram's Bot API and of Discord's API and Gateway, not yet against the real services. Signal has been used with a real account and
is not a preview.

## How a phone chat behaves

- **Text, images and files.** A screenshot or photo is read on the node: its text goes to
  Ragon as `[Screenshot text: …]`, and the image itself too when the model being served takes
  images. If nothing can be read from it, the bridge says so ("I can't read this image; describe it
  or paste the text.") instead of asking Ragon about nothing. A file is saved in the thread's
  folder. What a bridge cannot take (a video, a sticker) is named in one line, never
  dropped silently.
- **A new thread after a pause.** A message after four hours of silence starts a new thread, and
  the bridge says so: "New thread (the last message was 16h00m ago); /use 2 to go back." Change
  the pause, or switch it off with 0, in `~/.config/dreamference/config.toml`:

  ```toml
  [chat]
  new_thread_after_hours = 4
  ```
- **Voice messages** are transcribed on the node by the local speech-to-text service
  (`ragon-admin voice start`; nothing goes to a cloud service). The bridge first says what it heard
  ("Heard: …"), then answers; if it could not transcribe the message, it says so.
- **Files back.** When Ragon makes a file for you (a table, a report) and names it in the
  answer, the bridge offers to send it: press "Send it" on Telegram, or reply YES on Signal and
  Matrix. Nothing is sent unless you say so, and nothing at all while the air gap is on.
- **Night Shift from the phone.** `/night add <task>` queues a task for tonight, with the
  conversation's last question and answer as context; the bridge asks which repository (and
  waits for your yes) before anything is queued. `/night status` shows the queue, and the morning
  report arrives in Signal and Matrix (Telegram too if you add it). Configure under `[chat]`:

  ```toml
  night_repos = ["~/code/myapp"]          # besides every repository you queued in before
  night_report_to = ["signal", "matrix"]  # [] for none; add "telegram" to get it there
  ```
- **Quick follow-ups join the answer.** A message sent while Ragon is answering is added to
  the question it is working on, not answered separately afterwards. `/stop` still stops the answer.
- **Answers are bounded.** An answer that takes over three minutes, uses over 25 tool calls or
  runs one command over a minute is asked to finish with what it has; if it still runs, it is
  stopped and asked once more, without tools, and you are told. `/stop` stops it at once. Change the
  bounds (0 switches one off) in `~/.config/dreamference/config.toml`:

  ```toml
  [chat]
  answer_seconds = 180
  tool_calls = 25
  command_seconds = 60
  ```
- **Reading text in images needs Tesseract** on the node, offline: `ragon signal setup` installs it;
  otherwise `sudo apt install tesseract-ocr`. It reads every installed language; add one with
  `sudo apt install tesseract-ocr-<code>` (for example `tesseract-ocr-por`).

- **Signal and attachments.** For images in Note to Self to reach Ragon, the bridge lets
  signal-cli download the attachments your account receives (Signal offers no way to fetch one
  later). Every attachment that is not yours in Note to Self is deleted as it arrives, and the
  folder is swept every minute, so nothing from your other conversations is kept; it does touch
  the bridge's private folder on the node for a moment.

## Signal, in Note to Self

The bridge is linked to **your own Signal account** as one more device, the way Signal Desktop is.
You write to Ragon in **Note to Self**, and its replies appear there too, each starting with 🐦.
Note to Self shows no typing indicator, so the bridge answers at once with "🐦 Working on it…",
which says what Ragon is doing and then becomes the answer as it is written (one message,
edited; the finished answer is its last version). Signal raises no notification for messages from
your own linked devices, so keep the chat open while you wait.
The bridge reads Note to Self only. It drops every other message it receives without storing or
answering it.

Run this on the node:

```bash
ragon signal setup --dry-run    # prints every change it would make; changes nothing
ragon signal setup
```

Setup lists what it will change and asks for your password once (sudo). It then does the following:

- It creates a system account, `ragon-signal`. The bridge runs as that account, so the agent can
  never read your Signal keys.
- It downloads signal-cli and the Java runtime signal-cli needs. Each download is pinned by URL and
  SHA-256 and checked before it is unpacked. Nothing is bundled with Ragon, and no Java package is
  installed system-wide.
- It also checks signal-cli against its maintainer's OpenPGP signature, with a key pinned in Ragon.
- It installs the bridge and its system unit, `ragon-signal.service`.
- It shows a QR code. On your phone, open **Signal → Settings → Linked devices → Link new device**
  and scan the code.
- If the phone offers to **transfer your message history**, choose **Don't transfer**.
- It offers to make Note to Self's messages disappear after a week. Note to Self has a single timer,
  so this applies to your own notes as well as to Ragon's replies.

When setup has finished, open Note to Self and write `/help`. The commands are `/new` (start a new
thread), `/threads`, `/use N`, `/stop` and `/status`. When Ragon asks to run something, you
answer `YES` or `NO`.

**What linking means.** A linked device can read every message your account receives from now on,
and can send messages as you. signal-cli decrypts everything that arrives. The bridge acts on Note
to Self only, but the keys it holds are keys to your whole account. They are kept in
`/var/lib/ragon-signal` (mode 0700, owned by the bridge's own account), out of the agent's reach.

**Day to day**, on the node:

```bash
ragon signal status             # running, paired, the air gap, strangers ignored, versions
ragon signal stop               # stops the bridge now; it stays off until `ragon signal start`
ragon signal start
ragon signal setup --refresh    # after `ragon update`: installs the new bridge for its account
ragon signal remove             # undoes setup and deletes the keys; your Ask threads stay
```

`ragon signal remove` cannot unlink the device from your phone. Do that on the phone too: **Settings →
Linked devices**, then remove "Ragon". Signal also removes a linked device that has been offline
for about 30 days. If the node is switched off that long, run `ragon signal setup` again.

While a benchmark run holds the model on the node, Ragon does not answer from Signal. It
replies with the run, how far it has got and roughly how long is left, and you ask again later.

If you would rather not link your own account, `ragon signal setup --number +15551234567` registers a
separate number for Ragon instead (it needs a CAPTCHA and an SMS or voice code).

**More than one Signal account.** Each Signal account gets a bridge of its own, with a name. The
first is always `default`, set up by plain `ragon signal setup` (a name given for the first one is
refused); add another with `--name`:

```bash
ragon signal setup --name work            # links a second account (or --number for its own number)
ragon signal status                       # one line per bridge
ragon signal stop --name work             # with several bridges, the commands need --name
ragon signal remove --name work           # removes that bridge only; the others keep running
```

Each bridge has its own system account and keys, so one cannot read another's. A node runs at most
four, and setup needs 2 GB of memory to spare. A Signal account can have only one bridge. For now,
every bridge on a node answers with the same Linux user's agent.

The bridge runs only on a Linux node. See `specs/DREAMFERENCE_RAGON_SIGNAL.md` in the repository
for the design and what has been verified.

## Matrix, over Tailscale

Matrix runs on a private homeserver on the node. The homeserver is a container on an internal Docker
network with no route out of the machine. Your phone reaches it through **Tailscale**, so Tailscale
is a prerequisite:

1. Install Tailscale on the node and sign in. Ragon does not install Tailscale, because installing
   it needs root and signing in uses your own Tailscale account.
2. Turn on HTTPS once for your tailnet, in the Tailscale admin console (**DNS → HTTPS Certificates**).
   Your phone then gets a real certificate.
3. Install Tailscale on your phone and sign in to the same tailnet.

Then, on the node:

```bash
ragon-admin matrix start              # the homeserver, its loopback proxy and `tailscale serve`
ragon-admin matrix add-user <name>    # your account; its password is shown once
ragon chat start                      # the bridge, as the user unit ragon-chat.service
```

In **Element X** on your phone, sign in to `https://<the node's tailnet name>` with that account,
then accept the invite from Ragon.

The homeserver's name is the node's tailnet name, and **it cannot change later**, because every
account's id ends in it. If you rename the machine in Tailscale, `start` refuses to run until you
rename it back. The alternative is `ragon-admin matrix remove --yes`, which deletes the homeserver and
its accounts so you can start again. The tailnet name also appears in public Certificate Transparency
logs. Your messages do not.

Push notifications are off: the homeserver has no route out. `ragon-admin matrix push on` lets them
out. A push notification carries an event id but no message text. Even so, it reveals that a message
arrived, and when.

`ragon-admin matrix stop` turns the homeserver off until the next `start`. Your accounts and messages
are kept. `ragon-admin matrix status` shows how it stands.

## Telegram

**Topics.** If you turn on topics for your bot in @BotFather's settings for it, each
topic is its own thread: `/new` makes one, a topic you create starts one, and it is named after
your first message in it. Two topics can answer at once. Outside topics the chat works as before.

**Rich answers** (tables, headings, lists) are off until they have been checked on real clients;
turn them on with `telegram_rich = true` under `[chat]` in `~/.config/dreamference/config.toml`.
Approvals show a green and a red button, and stay in the chat with their outcome. Stopping an
answer keeps what was written, marked "(stopped)".

**Ragon's app inside Telegram** (optional). The bot's menu button, and `/app`, can open
Ragon's own web page full-screen inside Telegram. The page is loaded by your phone straight from
your machine over your own network, so what you type in it never passes through Telegram's servers;
Telegram only learns that you opened it, and its address. It needs your phone on the same private
network as the machine with an HTTPS name, for example Tailscale:

```bash
tailscale serve --bg --https=8443 http://127.0.0.1:3100   # ragon web over HTTPS on your tailnet
```

(Port 8443, because the Matrix homeserver, if you use it, has 443.)

Then set the address under `[chat]` in `~/.config/dreamference/config.toml` and restart:

```toml
[chat]
telegram_mini_app_url = "https://<your machine>.<your tailnet>.ts.net:8443/"
```

`ragon web start` and `ragon chat start` restart both with it.
The page asks you to pair it like any other device (`ragon web pair` on the machine), and it starts
only when Telegram vouches that it was opened from your bot by a paired account. Remove the line to
take the button away. It works in Telegram's phone and desktop apps, not in web.telegram.org, which
would have to frame the page.


### Work on a repository from your phone

With the app above, a Telegram chat can do real work on a repository from your node's own Git
server (Forgejo; private repositories included), as the desktop app's Work view does. Nothing
outside that server is reachable: no other Git host and no folder on the machine.

- **Pairing.** The first time the app opens on a new phone, it shows a six-digit code and the bot
  asks you, in the chat, to Approve or Decline "a Mini App on a new device". Approve and the phone
  is paired (as a `telegram-app` device; `ragon web revoke` removes it). Typing `ragon web pair` on
  the machine still works.
- **Pick a repository** in the app's Repos view, then talk to the agent there. It works in a
  checkout of its own on a branch named `phone/<thread>`; your own working folder is never touched.
  The git bar shows the diff and offers Commit, Push, Update, Merge and a pull request; every
  write asks for a fresh confirmation, and pushes are checked first for things that must not be
  published. Working on `main` directly is off unless the repository is allowed to.
- **Approvals** appear in a "Waiting for you" strip when you open the app from a notice, and are
  answered there, once. An unanswered one is declined after ten minutes.
- **What the bot says.** In a repository's topic the bot sends only short fixed notices, such as
  "Turn finished in `<repo>`: 3 files changed." or "Approval waiting in `<repo>`.", each with an
  Open button. It never sends code, file names, commands or the model's words, so none of that
  reaches Telegram's servers; everything else stays inside the app, which your phone loads straight
  from your machine. Text you type to the bot in a repository's topic is refused: type it in the
  app.
- **Full Access is off for phone threads.** To allow it, run `ragon chat repos full-access on` at
  the machine's own terminal and type `yes`. Even then, each thread asks you to type
  `FULL ACCESS <repo>` in the app; it lasts until you go back to Workspace, ends when `ragon web`
  restarts, and is never offered by the bot. It is refused while `/airgapped` is `on`. Turn the
  setting off with `ragon chat repos full-access off`.
- **Limits.** A Work turn stops after 30 minutes, 200 tool calls or a 10-minute command, and a
  chat has a daily model budget; the bot tells you at 80%. Under `[chat_repos]` in
  `~/.config/dreamference/config.toml`, `enabled` turns repository work off (default: on when the
  node's Git server answers) and `bridges` lists the messengers allowed to use it.


Telegram is the less private choice. Everything you send the bot, and every answer, passes through
Telegram's servers unencrypted. Answers can include what the agent reads for you, such as mail,
files and code. Setup shows this warning and asks you to type `yes`.

```bash
ragon chat telegram setup     # the warning, then the bot token from @BotFather (typed, not echoed)
ragon chat telegram pair      # an 8-digit code to send the bot as /pair <code>
ragon chat start
```

**Naming the bot in @BotFather.** Telegram bots are yours: you make one, so there is no shared
name to look up. In a chat with @BotFather:

1. `/newbot`. When it asks for a name, give the display name, `Ragon`. It is shown in the chat and can
   be changed any time with `/setname`.
2. When it asks for a username, give one that ends in `bot` and is not taken, for example
   `ragon_node_bot` (or `ragon_dgx_bot`, `ragon_dreamference_bot`). Telegram tells you at once if it
   is taken. A username cannot be changed later.
3. `/setdescription` for the text shown on an empty chat (for example "Ragon, your private coding
   assistant. Send /pair with the code from your machine."), and `/setabouttext` for the short line on
   the bot's profile (for example "Ragon by Dreamference. Runs on your own machine.").
4. Keep the token it prints; `ragon chat telegram setup` asks for it.

`ragon chat telegram users` lists the paired accounts, and `ragon chat telegram remove <id>` removes
one. `ragon chat telegram off` deletes the token and the pairings; revoke the token in @BotFather
(`/revoke`) as well. `ragon chat stop` stops the bridge for every messenger, and it stays off until
`ragon chat start`.

## Discord

Discord is the least private messenger: Discord's servers keep every message you and the bot
exchange, unencrypted, tied to your Discord account. Use it only for questions you would not mind
Discord reading.

1. Create an application in Discord's Developer Portal, give it a bot, and leave the bot private
   (*Public Bot* off) with no privileged intents. Setup refuses a public bot, or one with the
   *Message Content* intent.
2. On the node, run `ragon chat discord setup`. It asks you to type `yes`, then for the bot token,
   which is not shown as you type and is kept only in the bridge's state, readable by you alone.
   It prints a link that adds the bot to your account.
3. Run `ragon chat discord pair` and send the code it prints to the bot in a direct message, as
   `/pair <code>`, within 10 minutes. The code works once. Then run `ragon chat start`.

The bot answers only you, and only in your direct messages with it. Anyone else is told once a day
that the bot is private; servers it is added to are left at once, except one you name as
`discord_home_server` under `[chat]` so that you can message the bot. A status line with a **Stop**
button shows while an answer is written, and approvals come as buttons.

With `discord_web_url` under `[chat]` set to your HTTPS front for `ragon web`, `/app` and the
repository notices carry an **Open** button that opens the thread in `ragon web`. Signing in there
is the same as from any browser: `ragon web pair`, or a code confirmed from Signal or Matrix.

`ragon chat status` lists the paired accounts, `ragon chat discord users` and
`ragon chat discord remove <id>` manage them, and `ragon chat discord off` deletes the token and the
pairings and withdraws the bot's commands. Delete the application in the Developer Portal as well.

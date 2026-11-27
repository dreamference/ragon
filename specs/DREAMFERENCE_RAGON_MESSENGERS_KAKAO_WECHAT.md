# Ragon on KakaoTalk and WeChat: two more adapters for `ragon chat`

**Status:** Proposed, every owner question answered (§0.2); spec only, nothing built. Researched on 2026-11-27 from the vendors' own developer pages first, with third-party sources marked where nothing official was found (§14). The owner asked for it ("We need kakaotalk and wechat", 2026-11-27) and answered the coordinator's first questions the same day (§0.1) and every question of §13 the same evening (§0.2). Target: the 1.0 gate (Sun 2026-12-27), with four lanes on remote sessions from 2026-12-01 (§12.3).

**Spec owner:** the owner. **Depends on:** [RAGON_CHAT](./DREAMFERENCE_RAGON_CHAT.md) (the bridge, its hub, threads, approvals, rendering, splitting, bounds), [RAGON_DISCORD](./DREAMFERENCE_RAGON_DISCORD.md) (the shape of a third-party adapter, "the vendor reads it" consent), [RAGON_SIGNAL](./DREAMFERENCE_RAGON_SIGNAL.md) §8 (approvals by typed YES/NO), [RAGON_REMOTE_ACCESS](./DREAMFERENCE_RAGON_REMOTE_ACCESS.md) §2, §15 (the user's relay box, its SNI pass-through on 443, public certificates issued on the node), [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) §10.9 (declared exceptions), [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md).

---

## 0. Decisions, stated first

- **Who it is for.** People in Korea and China who use Ragon and live in KakaoTalk or WeChat, with local accounts (the owner, §0.1 A1). The experience is the one Telegram gives: the owner talks to their own node from the phone app; the bridge is off until a command turns it on; the egress audit lists it.
- **Both are adapters of `ragon chat`, not new bridges.** Threads, approvals, commands, rendering, splitting, bounds, pairing, the air-gap rule and Night Shift come from the shared hub (`ragon-rs/chat/src/hub.rs`) exactly as for Telegram and Discord. New code is the transport only: `ragon-rs/chat/src/kakao.rs`, `wechat_ilink.rs`, `wecom.rs`, and one small inbound listener (§4).
- **KakaoTalk: possible for an individual, through a Kakao Channel chatbot, with one catch each way.**
  - **In:** Kakao POSTs every message to a *skill server* at a public HTTPS URL, so the node needs an inbound path. It comes **through the user's own relay box** (the owner, A3): a third name on the box, passed through by SNI like the control plane, TLS ending on the node (§4). Without a relay box, the KakaoTalk adapter cannot be turned on.
  - **Out:** the bot answers only after the user writes: within 5 seconds in the same HTTP response, or **once** through a callback URL valid for 5 minutes. Pushing first needs a business-verified channel and costs 15 KRW a message. So an answer longer than the callback window is fetched by a tap on an **Answer** button (§5.4).
  - **Free, no business number, foreign phone numbers accepted, no review step** for a basic chatbot (deploy takes up to 30 minutes).
- **WeChat: possible for an individual, through Tencent's own ClawBot channel (iLink), with no inbound path at all.** Since 2026-03 Tencent offers personal WeChat users a ClawBot plugin and publishes its protocol under MIT: QR-code binding on the phone, then the server **long-polls** `ilinkai.weixin.qq.com` and sends replies, outbound only, like Telegram. Mainland Weixin users first (the owner, Q4); the international WeChat app when Phase 0 shows ClawBot there. Catches: iOS WeChat 8.0.70 or later only at the time of writing (gradual roll-out; Android announced without a date), the user must write first, Tencent reserves the right to restrict it, and it was made for OpenClaw (§6).
- **WeChat for a team: a WeCom intelligent bot in long-connection mode** (outbound WebSocket, no public address, no ICP filing; members use the WeCom app; free for an unverified organisation of up to 100 members) (§7). **In 1.0 only if a lane is free, otherwise 1.0.1** (the owner, Q7); the lane plan (§12.3) has no lane free for it, so it is planned for 1.0.1. Official accounts for companies come after 1.0, on request (the owner, Q5; §8).
- **The vendor reads every message.** Kakao and Tencent servers receive the owner's messages and every answer in plain text, keep them under their own terms, and are subject to Korean and Chinese law. Setup says so in plain words and asks for a typed `yes` (A4). At `/airgapped on`, both adapters pause, as Telegram does.
- **Unofficial personal-account clients are a separate, explicit opt-in, never the default and never the first documented choice** (A2). The spec covers them (§9) and states the risk: they break the vendor's terms and the account can be banned. **For KakaoTalk there is no maintained open-source library to build on**; for WeChat the official ClawBot channel removes most of the reason for one. **Decided (the owner, Q1, 2026-11-27 evening, replacing the morning's built-in ruling): add-on page only.** For WeChat, a documented user-installed add-on page with the warning; for KakaoTalk, nothing. No unofficial client is in Ragon's binaries (§9).
- **First version: text and approvals** (A7): send a task, read the answer, approve or refuse a step. No files, images or voice. No KakaoTalk push notifications in 1.0 (the owner, Q3; §5.6).
- **WeChat identity:** the iLink adapter sends the known plugin's version string as `channel_version` (the owner, Q2, against the recommendation; §6.4).
- **Fixed lines** (consent, "working…", approvals, errors) in **English, Korean and Simplified Chinese** (the owner, Q8), chosen from the language of the user's message, with the user's config as override.
- **Live acceptance in Korea and China: the owner, with the test accounts of §10** (Q6; §11.2).
- **Order** (A6): WeChat iLink first (no inbound path, no vendor review, the Telegram adapter's shape), then KakaoTalk (needs the relay box's inbound name), then the WeCom team bot (1.0.1 unless a lane is free). What fits before 12-27 is in §12.3.

### 0.1 The owner's answers (2026-11-27)

| # | Question | Answer |
|---|---|---|
| A1 | Audience | Ragon users in Korea and China, with local accounts and local rules. The owner may not be able to test personally: the spec says how it is tested (§11). |
| A2 | Official only? | Official routes **and** unofficial clients; the unofficial mode is a separate opt-in on the user's own account, off by default, with a plain warning; never the default or the first documented choice. **Superseded the same evening by Q1 (§0.2): add-on page only, WeChat only.** |
| A3 | Public callback | Through the user's relay box (remote access over the overlay). No project-run relay. |
| A4 | Privacy | Off by default; setup says plainly the vendor can read the messages and asks for a yes; the egress audit lists the bridge as a declared exception. |
| A5 | Release | In 1.0 (Sun 2026-12-27); both bridges are part of the 1.0 gate. §12 sizes it against that date. |
| A6 | Order | Whichever is feasible first: WeChat iLink, then KakaoTalk, then WeCom. |
| A7 | First version | Text and approvals. |
| A8 | Test accounts | The owner registers, from the exact list in §10. Nothing is created by a job. |

### 0.2 The owner's answers to §13 (2026-11-27 evening)

Recorded in DECISIONS.md ("OWNER round", 22:07 and 22:11 UTC). They are the owner's words and apply as written, also where they differ from the recommendation this spec made.

| # | Question | Answer | Where it changes the spec |
|---|---|---|---|
| Q1 | Unofficial personal-account mode | **Add-on page only:** for WeChat a user-installed add-on page with the warning; for KakaoTalk nothing. Replaces the morning's built-in ruling (A2). | §0, §9 |
| Q2 | iLink client identity | **Send the known plugin's version string** (the owner's choice against the recommendation). | §0, §6.4, §6.5, §11.1 |
| Q3 | KakaoTalk "send to me" notifications | **Not in 1.0.** Answers are fetched with the Answer button. | §0, §5.6 |
| Q4 | Which WeChat users | **Mainland Weixin first.** | §0, §10.2, §11.1 |
| Q5 | Companies in China (service account) | **After 1.0, on request.** | §0, §8 |
| Q6 | Who runs the live acceptance | **The owner, with the test accounts.** | §0, §11.2 |
| Q7 | WeCom team mode | **In 1.0 if a lane is free, otherwise 1.0.1.** | §0, §7, §12.3 |
| Q8 | Language of the fixed lines | **English, Korean, Simplified Chinese.** | §0, §12.4 |
| — | Date and lanes | **1.0 stays Sun 2026-12-27, with FOUR lanes on remote sessions from 12-01.** The bridges get what is left (§12.3). | §12 |

## 1. What each vendor offers (verified 2026-11-27)

### 1.1 KakaoTalk

| Route | Receives? | Sends first? | Who can use it | Public URL | Cost | Fit |
|---|---|---|---|---|---|---|
| **Channel chatbot** (chatbot admin center, formerly Open Builder; a *skill server*) | Yes: HTTP POST per message | No (reply within 5 s, or one callback within 5 min) | Anyone with a Kakao account; channel free, business number optional, foreign phone numbers allowed | **Yes** (Kakao POSTs to it) | Free (no price listed for replies) | **The route** (§5) |
| **Event API** (bot speaks first, by a named block) | n/a | Yes, to users who added the channel | Business-verified channel **and** a business-verified developer app ("Biz App") | n/a | 15 KRW per delivered message, VAT excluded | Not for individuals |
| **Talk Message API**, "send to me" | No (send only) | Yes, into the user's own "chat with myself" | Kakao developer app + Kakao Login with the `talk_message` consent; no count limit | No | Free | Optional notifier (§5.6), not in the first version |
| **Talk Message API**, "send to friends" | No | Yes | 30 a day before review; more needs a Biz App and review | No | Free | No |
| **AlimTalk / BrandMessage** (FriendTalk ended 2025-12-31) | No | Yes | Korean business, verified channel, an official dealer, approved templates (~2 business days) | No | ~8 KRW (AlimTalk), 15–40 KRW (BrandMessage), dealer prices | No (one-way notices) |

What matters from the chatbot docs:

- **Flow.** The user writes in the channel's chat; Kakao's bot system POSTs a JSON *skill request* to the skill server and takes the reply JSON from the same HTTP response. **Timeout 5 s.**
- **Callback.** Enabled per block. The request then carries a one-use `userRequest.callbackUrl`; the server answers `{"useCallback": true}` at once (Kakao shows the block's waiting text) and later POSTs the real reply to that URL, **once, within 5 minutes**. Older third-party posts say callback first needed an "AI chatbot" application and gave 1 minute; current Kakao docs say 5 minutes and describe no application. **Phase 0 checks it** (§11.1).
- **Reply limits.** 1 to 3 `outputs` per reply; `simpleText` up to 1,000 characters (folded after 500 behind "view all"); up to 10 `quickReplies`, each of which sends a fixed text or calls a block: Approve and Decline are two of them.
- **Who wrote.** `userRequest.user.id` is a `botUserKey` (up to 70 characters), stable per bot and different between bots for the same person. That is what pairing binds (§5.3).
- **Signatures.** None found in the docs: a skill request is not signed. Phase 0 checks whether the skill settings accept a custom header; the design does not depend on it (§5.2).
- **Deploy.** The bot is linked to an operating channel and a development channel, reaches the operating channel after its first deploy, which takes up to 30 minutes. **No approval review** is described.
- **Terms.** Kakao may restrict a bot or channel temporarily or permanently ("use restrictions"); nothing in them forbids a chatbot that answers its own owner.

### 1.2 WeChat

| Route | Receives? | Sends first? | Who can use it | Public URL | Cost | Fit |
|---|---|---|---|---|---|---|
| **ClawBot / iLink** (personal WeChat, official, Tencent-maintained) | Yes: long poll `getupdates` (~35 s) | No: replies to the user's messages (`context_token`) | Any personal WeChat user who has the ClawBot plugin (iOS 8.0.70+, gradual roll-out) | **No** (outbound only) | None seen | **Owner** (§6) |
| **WeCom intelligent bot**, long connection | Yes: WebSocket `aibot_msg_callback` | After the user has written once in that chat (`aibot_send_msg`) | Members of a WeCom organisation, internal groups only | **No** (outbound WebSocket) | Free (unverified org, 100 usable members) | **Team** (§7) |
| **Official account** (公众号 / 服务号) | Yes: callback, XML, ports 80/443 only | Customer-service API, 5 messages within 48 h of the user's last message, **verified accounts only** | Individual accounts: passive reply within 5 s only. Verified: mainland entity (300 RMB per verification) or an overseas company (99 USD, service account only, 7–15 working days) | **Yes** | 300 RMB / 99 USD a year | Later, companies (§8) |
| **WeChat Customer Service** (微信客服, via WeCom) | Yes: callback | 5 messages within 48 h | A WeCom self-built app with a callback; trusted IPs; ICP-filed domain of the same entity for verified orgs | **Yes** | Org verification 300 RMB | No |
| **Mini program** | Customer-service messages only | As above | Registered entity | Yes | — | No |

What matters:

- **iLink, from Tencent's published protocol** (`github.com/Tencent/openclaw-weixin`, MIT, created 2026-03-27, last push 2026-11-20; `docs/protocol.md`):
  - Base URL `https://ilinkai.weixin.qq.com`; JSON over HTTPS, `POST`.
  - Binding: `POST /ilink/bot/get_bot_qrcode?bot_type=3`, then poll `get_qrcode_status`; the phone scans the code and may ask for a PIN. The result is a `bot_token`, `ilink_bot_id`, a `baseurl` and the `ilink_user_id` of the person who bound it. "Self bind" needs nothing else; binding a bot to **another** owner needs that person's real-name authorisation through WeChat Pay.
  - Every request carries `Authorization: Bearer <bot_token>` and a `base_info` with `channel_version` and an optional `bot_agent` (an observability name, "not used for authentication").
  - Receiving: `POST /ilink/bot/getupdates`, a long poll of about 35 s with a cursor (`get_updates_buf`). `errcode -14` means "session paused for one hour".
  - Sending: `POST /ilink/bot/sendmessage` with the inbound `context_token` (optional since iLink 2.1, where the server uses the most recent conversation; third-party protocol notes). `sendTyping` exists.
  - **The server identifies the client from `channel_version`**: an empty or unrecognised value is stored as `0.0.0`, a value below the configured minimum is "unsupported". So Tencent can turn non-OpenClaw clients off at will (§6.5).
  - Limits from a 2026 third-party guide, not from Tencent: iOS WeChat only (Android, PC, Mac not yet); no proactive push, the user writes first; Tencent reserves control and can restrict at any time. **The ClawBot plugin's own terms were not found** (§14).
- **WeCom intelligent bot** (`developer.work.weixin.qq.com/document/path/101463`, updated 2026-10-10): API mode by callback URL **or** long connection. Long connection: `wss://openws.work.weixin.qq.com`, subscribe with BotID and Secret (`aibot_subscribe`), heartbeat about every 30 s, one connection per bot (a new one kicks the old), streaming replies, proactive `aibot_send_msg` once the user has written in that chat, **30 messages a minute and 1,000 an hour per chat**. Launched "to integrate OpenClaw" by a "locally deployed plugin + WebSocket, bypassing corporate domain/IP restrictions". Internal groups only: no external groups, so no personal-WeChat users in them.
- **Self-built WeCom apps** created after 2022-06-20 need trusted public IPv4 addresses for API calls, and a verified organisation's callback must be a domain filed (ICP) to the same entity. Not usable from a home node abroad.
- **Mainland Weixin vs international WeChat.** The same backend shows "Weixin" or "WeChat" to the user. Whether ClawBot is offered in the international app, and whether an official account reaches both, is **unconfirmed** (§11.1). Decided (the owner, Q4): mainland Weixin first, so the gate does not wait on the international app.
- **Terms.** On 2025-04-30 WeChat's Security Center announced a crackdown on third-party tools that obtain or use WeChat data by code injection, screen recording or dumping, under the software licence and the personal-account rules (§9.2).

## 2. The privacy model, compared

| | Matrix | Signal | Telegram | Discord | **KakaoTalk** | **WeChat (iLink / WeCom)** |
|---|---|---|---|---|---|---|
| Who can read the messages | Nobody else | Nobody else (E2E) | Telegram | Discord | **Kakao** | **Tencent** |
| Inbound path to the node | Overlay | None (signal-cli polls) | None (long poll) | None (Gateway) | **Through the user's relay box** | None (long poll / WebSocket) |
| Law that applies to the vendor's copy | — | — | UAE/others | US | **Korea** | **China (mainland servers)** |
| Account at risk if the bot misbehaves | — | The owner's own | The bot's | The bot's | The channel's / bot's | **The owner's own personal WeChat** (iLink binds to it) |

Setup's consent text, per adapter, in the user's language (English, Korean, Simplified Chinese):

> Everything you send to Ragon in KakaoTalk, and every answer, passes through Kakao's servers in plain text. Kakao can read and keep it under its terms and Korean law. Answers may contain your mail, files and code that Ragon reads. Type yes to turn this on.

The WeChat text names Tencent and Chinese law instead. Setup refuses without a terminal, as Telegram's does.

## 3. Architecture

```
 KakaoTalk app ── Kakao servers ──HTTPS POST──► relay box :443 (haproxy, SNI hooks.<domain>, pass-through)
                     ▲                                │ reverse SSH tunnel (ragon-remote-tunnel, second -R)
                     │ callbackUrl POST               ▼
                     └──────────────────────── ragon chat serve ──► ragon web (127.0.0.1:3100) ─► app-server
                                                (hooks listener, TLS on the node)
 WeChat app (iOS) ── Tencent iLink ◄── HTTPS long poll ──┘
 WeCom app ── Tencent WeCom ◄── outbound WebSocket ──────┘
```

- **One process, more adapters.** `ragon chat serve` (`ragon-chat.service`) runs each enabled adapter as a task with the shared back-off; one failing never stops another (CHAT §2).
- **Hub keys:** `kakao:<botUserKey>`, `wechat:<ilink_user_id>`, `wecom:<chatid>` (a single chat) or `wecom:<chatid>/<userid>` (an internal group). `threads.json` and the bindings (AUTH §14.1, `bindings.rs`) need nothing new beyond the kinds.
- **Phone threads** are Ask threads with `approval_policy = "never"` and the chat prompt, as for every phone messenger (CHAT §16.3).

## 4. The inbound path through the user's relay box (KakaoTalk only)

Kakao must reach a public HTTPS URL. The owner ruled out a project-run relay (A3); the user's relay box (REMOTE_ACCESS) already has what is needed: a public address, haproxy routing 443 by server name, a reverse tunnel from the node, and public certificates issued on the node by DNS-01.

- **A third name: `hooks.<the box's parent domain>`.** A and AAAA records = the box's, unproxied, made before the first challenge (REMOTE_ACCESS §15.5's lesson). Its own lego account `ragon-hooks` and its own CAA set (`issue "letsencrypt.org; validationmethods=dns-01; accounturi=<that account>"`, `issuewild ";"`), so neither the box nor the other accounts can get a certificate for it.
- **TLS ends on the node.** Unlike the relay's certificate, this one **never goes to the box**: haproxy passes `hooks.<domain>` through in TCP mode, like the control plane's name, so the box sees ciphertext only. (Kakao, of course, sees the plaintext: it is Kakao's message.)
- **The tunnel.** `ragon-remote-tunnel.service` gains a second `-R 127.0.0.1:8444:127.0.0.1:<hooks port>`; the box account's `authorized_keys` line gains `permitlisten="127.0.0.1:8444"` (`dreamference/remote/remote_tunnel.py:130` builds it today for one port). haproxy gets one more `use_backend` on SNI.
- **The listener.** Inside `ragon chat serve`, on `127.0.0.1:<hooks port>` (default 3110), rustls with the `hooks` certificate. It serves exactly one path, `POST /kakao/<path secret>` (32 random bytes, base64url, made at setup, stored 0600); anything else is closed after the handshake with 404. Bodies over 64 KiB are refused. Requests are answered within 4 seconds whatever happens (§5.4).
- **Commands.** `ragon-admin remote hooks on|off|status`: makes the name, the DNS records, the CAA set, the certificate, the haproxy backend and the tunnel's second port; `off` undoes them. `remote certificate renew` renews it with the others (reload of the listener only). `ragon chat status` checks the public handshake presents the node's current leaf.
- **No relay box:** `ragon chat kakao setup` refuses and names `ragon-admin remote setup`. There is no fallback to a public port on the node and no third-party tunnel.
- **Exposure.** The listener is reachable from the internet. It acts only on a request that carries the path secret, names the configured `bot.id`, and comes from a paired `botUserKey`; everything else gets at most one fixed line. Kakao's source addresses are not published (Phase 0 looks), so haproxy cannot filter by source.

## 5. KakaoTalk adapter

### 5.1 Turning it on

1. `ragon-admin remote hooks on` (once per box).
2. `ragon chat kakao setup`: the consent (§2), then asks for the bot id (shown in the chatbot admin center) and prints the **skill URL** to paste there (`https://hooks.<domain>/kakao/<secret>`) and the three blocks to create (§10.1). It waits for the first skill request and checks `bot.id`.
3. `ragon chat kakao pair` (or `ragon pair kakao`): an 8-digit code; the user writes `/pair <code>` to the channel; the `botUserKey` is bound to the Ragon user (AUTH §14.1).
4. `ragon chat kakao users | remove <key> | off` as for Telegram.

### 5.2 Requests

- Accepted: skill requests whose `bot.id` matches and whose `userRequest.user.id` is paired; one `/pair` from anyone. Unpaired senders: one fixed line ("This is a private Ragon…") at most once a day per key, nothing else.
- **No signature is relied on.** The path secret is the shared secret; if Phase 0 finds Kakao can send a custom header, a second secret goes there too.
- **Duplicates.** Kakao may retry a request that timed out; the bridge keeps the last 256 request ids (`userRequest.utterance` plus the block id and timestamp if no id is given) and answers a duplicate with the same reply.

### 5.3 Blocks in the chatbot

The chatbot admin center routes text to *blocks*. The setup page (§10.1) makes three, all calling the one skill:

| Block | Triggered by | Callback |
|---|---|---|
| Fallback (every message) | any text | **on** |
| Approve / Decline | the quick-reply texts `/approve <nonce>` and `/decline <nonce>` | on |
| Answer | the quick-reply text `/answer` | off (the answer is already on the node) |

### 5.4 A turn, in the 5-second / 5-minute windows

1. A message arrives. The bridge starts or steers the turn (hub) and answers at once `{"version":"2.0","useCallback":true}`; Kakao shows "Ragon is working…".
2. **Callback budget.** The Kakao adapter sets the turn's answer bound to 150 s (`[chat] kakao_answer_seconds`, default 150; the shared bound is 180 s), so with the 30 s grace and the 90 s final turn the answer is normally ready within 270 s, inside the 5-minute callback.
3. When the answer is ready, it is POSTed to the `callbackUrl` as up to three `simpleText` outputs of at most 1,000 characters (split on paragraphs, code blocks closed and reopened, CHAT §3). Longer: the third output ends "… (more)" with a **More** quick reply, which returns the next three.
4. **Not ready at 285 s:** the callback carries "Still working. Tap Answer in a while." with an **Answer** quick reply. The answer is stored; a tap (or any message) delivers it in the synchronous reply, which needs no callback.
5. **If Phase 0 finds callback is not available** to an individual's bot: step 1 answers "Working on it. Tap Answer in a minute." with the Answer quick reply, and every answer is fetched that way. Worse, but it works with no review, so the 1.0 gate does not depend on Kakao (§12).
6. Rendering: `simpleText` is plain text. Markdown is flattened (headings as lines, code blocks kept as text, links as "text (url)").

### 5.5 Approvals

- An approval request (CHAT §3, MOONSHOT's nonces) becomes the callback's message: what is asked, cut to 900 characters, with two quick replies **Approve** and **Decline** sending `/approve <nonce>` / `/decline <nonce>`. The tap is a new skill request with a **new callback URL**, which carries the rest of the turn.
- A typed `yes` / `no` while one approval is pending does the same (as Signal).
- If the callback was already used when the request comes, the request waits on the node; the next message or Answer tap shows it first.
- No choice in 10 minutes: declined, as everywhere.

### 5.6 Not in 1.0: "send to me" notifications (the owner, Q3)

To push "your answer is ready" or Night Shift's morning report without the user writing first, the Talk Message API can post into the owner's own "chat with myself" with the owner's OAuth token (Kakao developer app, Kakao Login, `talk_message` consent; access token 6 h, refresh token 2 months, renewed when a month or less is left). Two cautions: Kakao's message tutorial says the API is meant for user-initiated sending, not for a service sending automatically; and it adds an app, a login and a token. **Decided: not in 1.0** (the owner, Q3); answers are fetched with the Answer button (§5.4). Revisit after 1.0.

### 5.7 Team

Each member adds the channel and pairs; bindings map each `botUserKey` to its Ragon user. The same windows apply per member. Group chats: a Kakao Channel chatbot answers in one-to-one chats only, so there are no team rooms; a team is several one-to-one chats with one node.

## 6. WeChat adapter, owner mode: ClawBot / iLink

### 6.1 Turning it on

1. `ragon chat wechat setup`: the consent (§2), then `get_bot_qrcode`; the QR code is printed in the terminal (and as a PNG path); the owner scans it in WeChat (Me → Settings → Plugins → ClawBot) and enters the PIN if asked. The bridge stores `bot_token`, `ilink_bot_id`, `baseurl` and the binder's `ilink_user_id` in `~/.ragon/chat/wechat.json` (0600).
2. **Self bind only.** The other-owner path (WeChat Pay real-name ticket) is not offered: the bot is the owner's.
3. The binder is paired automatically (they scanned the code on their own phone); `ragon chat wechat users | remove | off` as for Telegram. `off` also asks the user to remove the bot in WeChat's plugin page, since unbinding there is the vendor's.

### 6.2 The loop

- `getupdates` long poll with the cursor, persisted in `wechat-state.json` after each batch. A message from anyone but a paired `ilink_user_id`: ignored (iLink delivers only the bound user's direct messages, but the check stays).
- `errcode -14`: the adapter pauses one hour and `ragon chat status` says so with the time it resumes.
- Replies: `sendmessage` with the message's `context_token`; `sendTyping` while a turn runs (every 5 s).
- **The user writes first** is not a limit in practice: a turn is always answering a message. Night Shift's morning push (CHAT §16.8) is not delivered to WeChat; `/night status` shows it when asked.
- Message size: unknown (Phase 0); the splitter starts at 2,000 characters, the WeCom text limit's neighbourhood.

### 6.3 Approvals

Plain text: the request, then "Reply YES or NO (code 4821)". `YES`/`NO` (or `是`/`否`, or the code) answers the pending one, as Signal does. No buttons (iLink's message types in the published protocol carry none).

### 6.4 The identity it presents

**Decided (the owner, Q2, against the recommendation): `channel_version` carries the known plugin's version string**, the one Tencent's ClawBot plugin for OpenClaw sends, so the server treats the adapter as that plugin. `base_info.bot_agent` stays `Ragon/<version>`: the answer covers the version string only, and if Phase 0 (W2) shows the server checks `bot_agent` too, that is a new question for the owner, not a silent change.

- The string is one constant in `wechat_ilink.rs` with a comment naming where it comes from (the plugin's published package version) and a test that pins it.
- When Tencent's plugin moves to a new version and the server starts refusing the old one, the adapter says so in `ragon chat status` ("WeChat refused this client version; update Ragon") and a release bumps the constant. No automatic fetching of the current version: that would be a call to a vendor the egress audit does not list.
- Setup's consent text says it plainly: "Ragon presents itself to WeChat as the ClawBot plugin."

### 6.5 What can break it, plainly

- Tencent can stop clients it does not recognise at any time (by `bot_agent` or other signals, even with the plugin's version string), refuse an old version string after its plugin updates (§6.4), or end the ClawBot programme.
- Availability: iOS only at the time of writing, rolled out gradually; an owner whose WeChat shows no ClawBot plugin cannot use this mode yet.
- The account at stake is the owner's personal WeChat. The route is Tencent's own, so it is not against the terms in the way §9 is, but its terms were not found (§14).

## 7. WeChat adapter, team mode: WeCom intelligent bot (long connection)

- **Setup (the organisation's admin, once):** a WeCom organisation (unverified is enough for up to 100 usable members), Intelligent zone → Intelligent bots → permissions for the member who creates it; create a bot in API mode, **long connection**; copy BotID and Secret. `ragon chat wecom setup` takes them (echo off), shows the consent, and connects.
- **Transport:** `wss://openws.work.weixin.qq.com`, `aibot_subscribe` with BotID and Secret, a heartbeat every 30 s, reconnect with the shared back-off. One connection per bot: a second `ragon chat` on another machine with the same bot kicks the first, which says so in its status instead of fighting back.
- **Who:** members of the organisation, in one-to-one chats with the bot or in internal groups where they @-mention it. Pairing binds a WeCom `userid` to a Ragon user (`ragon pair wecom`); unpaired members get one line.
- **Answers** stream (`aibot_msg_callback` replies support streaming), throttled to stay under 30 messages a minute per chat.
- **Approvals:** typed YES/NO with a code, as §6.3; template-card buttons are Phase 2 after Phase 0 confirms the bot can receive their clicks.
- **Personal WeChat users** cannot talk to it: intelligent bots join internal groups only. WeCom's "WeChat plugin" lets members read WeCom messages inside personal WeChat; whether bot chats reach them there is unconfirmed (Phase 0).

## 8. Not in the first version: official accounts

A verified **service account** is how a company in China reaches many WeChat users: callback on 80/443 (it would come through the relay box's `hooks` name, which serves on 443, so the same §4 path works, with `/wechat/<secret>` added), XML, signature and AES (`msg_signature`), 5-second passive reply plus customer-service messages (5 within 48 h of the user's last message). It needs a mainland entity (300 RMB) or an overseas company (99 USD, service account only, 7–15 working days), and an individual account can only answer within 5 seconds, too short for an agent. **Decided: after 1.0, on request** (the owner, Q5): built when a company asks for it, not before.

## 9. The unofficial personal-account mode (explicit opt-in)

### 9.1 KakaoTalk (LOCO protocol)

| Project | Language | Licence | Last change | State |
|---|---|---|---|---|
| storycraft/node-kakao | TypeScript | MIT | 2023-11-11 | 428 stars, not archived, **unmaintained**; its own docs: "Abusing this client can cause permanent service restriction"; v3 was refused for "version mismatch" |
| jincreator/kakaotalk, carpedm20 and pykakao forks | Python | various | 2013–2016 | dead |
| kuuko-re/kakaotalk-loco-client | ? | per-project licensing | created 2026-07-18 | 0 stars, claims "request signing and device attestation"; looks like a sales front; not trustworthy |

- **Sign-in** (node-kakao v4, not re-verified): e-mail and password plus a device id; the first login registers a device with a passcode confirmed on the phone and takes the PC/tablet sub-device slot. Current KakaoTalk builds add request signing and device attestation (per the 2026 repository's claim).
- **What breaks:** every client-version bump can be refused by version check; signing and attestation changes need reverse engineering each time.
- **Terms and record:** Kakao's operating policy (2025-09-30) names reverse engineering KakaoTalk and using or publishing the result as copyright infringement, and lists calling the protocol with non-permitted programs and distributing protocol bots as grounds for **immediate permanent restriction** of the whole KakaoTalk account. A mass permanent ban of LOCO-bot users was reported in 2021 (secondary source). No lawsuit was found.
- **Conclusion:** there is nothing maintained to build on. Shipping this mode means Dreamference writing and maintaining its own LOCO client, which Kakao's policy calls copyright infringement, for the users of a product sold in Korea. **Decided (the owner, Q1): nothing for KakaoTalk**, neither in the binaries nor as an add-on page.

### 9.2 WeChat (personal account)

| Project | Licence | Last push | State | How it works |
|---|---|---|---|---|
| wechaty/wechaty | Apache-2.0 | 2026-11-27 | active, 23.6k stars | framework; needs a "puppet" below |
| wechaty/puppet-xp | Apache-2.0 | 2025-07-29 | quiet | hooks the Windows desktop client |
| wechaty/puppet-padlocal | Apache-2.0 | 2023-07-04 | stale | iPad protocol, paid tokens |
| wechaty/puppet-wechat4u, nodeWechat/wechat4u | Apache-2.0 / none | 2024 | stale | web protocol (web login closed for most accounts) |
| littlecodersh/ItChat, ItChat-UOS | MIT | 2023 | stale | web protocol |
| lich0821/WeChatFerry | MIT | 2026-07-10 | **archived** | DLL injection into the Windows client |
| Devo919/Gewechat | Apache-2.0 | 2026-08-06 | **stopped**, service withdrawn citing Tencent's crackdown | iPad protocol as a service |
| cluic/wxauto | Apache-2.0 | 2026-04-13 | active | Windows UI automation of the desktop client |
| zhayujie/chatgpt-on-wechat | MIT | 2026-11-27 | active, 48.1k stars | an app over the above channels |

- **Sign-in:** a QR code scanned with the phone (web, iPad and hook routes), or a logged-in Windows desktop client that is driven or injected into.
- **What breaks:** web login is closed for most accounts; hooks break on each desktop update; iPad-protocol services depend on a vendor that can vanish (Gewechat did).
- **Terms and record:** WeChat Security Center's notice of 2025-04-30 targets exactly these tools (code injection, screen recording or dumping) under the software licence and the personal-account rules. Restrictions of accounts using such tools are widely reported; numbers and lawsuits were not confirmed (§14).
- **And:** the working ones need a Windows machine running WeChat desktop, and the bridges are Unix-only. The official iLink route (§6) does what the owner needs without any of it.
- **Decided (the owner, Q1): an add-on page only, not in Ragon's binaries.** The page is `docs/messengers-wechat-addon.md`, linked from `docs/messengers.md` after the official routes (never first). It names wechaty and wxauto as user-installed third-party tools, points them at the OpenAI-compatible endpoint `ragon web` already serves, and nothing of ours is injected into WeChat. Its first lines are the warning: "This uses an unofficial client. It is against WeChat's terms. Your WeChat account can be restricted or banned." Ragon ships no installer, no setup command and no code for it; the egress audit does not list it, because nothing of Ragon's makes those calls.

## 10. What the owner (or a user) does once

### 10.1 KakaoTalk: cost 0, about an hour, no review

1. A KakaoTalk account on a phone (any country's number, SMS verification; one number can be verified at most 5 times a day).
2. Kakao Business sign-up at business.kakao.com with that account (free).
3. Create a **Kakao Channel** (free; skip the business registration number: "no, later"; the channel shows "business info not verified", which does not matter here).
4. Join the chatbot admin center (chatbot.kakao.com) and **create a bot**; link it to the channel.
5. A relay box with `ragon-admin remote setup` done, then `ragon-admin remote hooks on` (needs the box's domain in the user's Cloudflare zone, as REMOTE_ACCESS already requires).
6. `ragon chat kakao setup` prints the skill URL: add it as a **skill**; make the three blocks of §5.3; switch callback on in the fallback and approval blocks (if the console asks for an application, note what it asks and how long: that is Phase 0's question).
7. **Deploy** the bot (up to 30 minutes). Add the channel as a friend in KakaoTalk, then `ragon chat kakao pair`.

### 10.2 WeChat owner mode: cost 0, minutes, no review

1. A personal **mainland Weixin** account (the owner, Q4; the international WeChat app once Phase 0 shows ClawBot there) on an **iPhone**, Weixin 8.0.70 or later, with **ClawBot** visible under Me → Settings → Plugins (gradual roll-out: if it is not there, this mode is not yet available to that account).
2. `ragon chat wechat setup`, scan the QR code with that phone, enter the PIN if asked.

### 10.3 WeChat team mode (WeCom): cost 0 unverified, under an hour

1. Register a WeCom organisation (free; unverified allows 100 usable members). Whether a non-mainland phone number can register one is unconfirmed (Phase 0).
2. As admin, allow bot creation (Intelligent zone → Intelligent bots → permissions), create a bot in API mode with **long connection**, copy BotID and Secret.
3. `ragon chat wecom setup`; each member `ragon pair wecom`.

### 10.4 Not needed for the first version

Official accounts (§8): an entity, 300 RMB or 99 USD a year, 7–15 working days. Kakao Event API or send-to-friends: a Korean business. None of these is on the 1.0 path.

## 11. Tests, without a real account in CI

- **Stand-in vendors in `ragon-rs/chat/tests/`,** as `telegram.rs` and `discord.rs` do:
  - `kakao.rs`: a client that POSTs skill requests (built from the documented `SkillPayload` JSON) through a local TLS listener with a test certificate, a callback receiver checking the 5-minute single use, retries and duplicates, the More and Answer flows, approvals by quick reply, unpaired and wrong-`bot.id` requests.
  - `wechat_ilink.rs`: a stand-in iLink server (QR binding, `getupdates` with cursor and timeouts, `errcode -14`, `sendmessage` and `sendTyping` checked for `context_token` and headers).
  - `wecom.rs`: a stand-in WebSocket server (`aibot_subscribe`, heartbeat, a second connection kicking the first, streamed replies, the 30-a-minute throttle).
- **Recorded messages.** When the owner's test accounts exist (§10), one session per adapter is recorded once (`RAGON_CHAT_RECORD=dir`), redacted (keys, tokens, user ids replaced), committed under `ragon-rs/chat/tests/fixtures/{kakao,wechat,wecom}/` and replayed by the stand-ins, so the tests run on the vendor's real JSON shapes.
- **The hooks path:** a Docker rehearsal like REMOTE_ACCESS §12.2 (haproxy with the SNI rule, `ssh -R` with `permitlisten`, Pebble as the ACME CA); the Kakao stand-in POSTs through it.
- **Egress:** `tests/test_egress_audit.py` gains the declared-exception lines (§12.1).
- **The release job's command** gains nothing new: the adapters are in `ragon-chat`, already in `cargo test --release -p ragon-signal -p ragon-chat -p ragon-sync -p ragon-loop-guard`.

### 11.1 Phase 0 on the real services (the owner's accounts, about two hours, can run before any code)

Answers needed before relying on the design: (K1) is callback available to an unverified individual's bot, with what application and wait; (K2) can a skill send a custom header; (K3) the skill request's retry behaviour and source addresses; (W1) ClawBot on the owner's mainland Weixin account (the gate's target, Q4), then the international WeChat app and Android (for after the gate); (W2) does iLink accept the known plugin's `channel_version` (Q2) together with `bot_agent: Ragon/<version>`; (W3) text size and rate limits on `sendmessage`; (W4) can a WeCom organisation be registered with a non-mainland number, and do intelligent-bot chats reach members through the WeChat plugin. Each is a short script under `scratch/messenger-probes/{kakao,wechat}/` (row `mk-phase0-probes`, §12.4), run by the owner on the test accounts, the output pasted into §15.

### 11.2 Who tests it live

The owner may not be able to (A1). The live acceptance is one script per adapter (`scripts/messenger_acceptance.py --kakao|--wechat|--wecom`) that a tester runs on a node with their phone: send "ping", read "pong" from the model, trigger one approval, approve it, decline the next; the script records the transcript and the bridge's status. **Decided (the owner, Q6): the owner runs it, with the test accounts of §10**, once per adapter that is in 1.0, between the adapter landing and 12-26 (§12.3). The script prints each step before it runs so a run needs no one else.

## 12. Effort, against 2026-12-27

### 12.1 Egress

- **Destinations:** KakaoTalk: the `callbackUrl` host only if it is under `kakao.com` (refused otherwise), inbound only through the box. WeChat iLink: `ilinkai.weixin.qq.com` (and the `baseurl` it returns, only under `weixin.qq.com`). WeCom: `openws.work.weixin.qq.com`.
- **Declared exceptions** (`EgressAudit.declared_exceptions`): `ragon-chat.service` gains one line per adapter that is on: "KakaoTalk (Kakao reads the messages; inbound via hooks.<domain>)", "WeChat (Tencent iLink reads the messages)", "WeCom (Tencent reads the messages)". The tunnel's second port is reported with `ragon-remote-tunnel.service`'s line.
- **Air gap:** at `/airgapped on` every one of them pauses and answers one line, as Telegram (CHAT §3).

### 12.2 Size

| Piece | Days (one engineer) | Depends on |
|---|---|---|
| iLink adapter + stand-in + setup/QR + consent strings in three languages | 4 | nothing |
| Hooks listener in `ragon chat` + `remote hooks on/off/status` (DNS, CAA, lego account, haproxy, tunnel) + Docker rehearsal | 5 | relay box code |
| Kakao adapter (callback windows, More/Answer, approvals by quick reply) + stand-in | 5 | hooks |
| WeCom long-connection adapter + stand-in | 4 | nothing |
| Egress lines, docs (`docs/messengers.md`, `docs/dev/messengers.md`), acceptance script | 2 | all |
| **Total** | **20** | |

WeCom is 1.0.1 (§12.3, Q7). Without it, the 1.0 part is 16 lane-days: iLink 4, hooks 5, Kakao 5, egress/docs/acceptance 2 (the add-on page of §9.2 is part of the docs).

- **Vendor review is not on the critical path:** none of the three first routes has a review step found in the docs. The open risk is Kakao's callback (K1); §5.4 step 5 keeps KakaoTalk working without it.
- **If no live account is ready by the gate:** the stand-ins plus recorded messages carry the gate for the code; the live run is recorded as a known issue. **The unofficial mode must not carry the gate**: it is the opposite of what the gate certifies, and for KakaoTalk it does not exist.
- **A date risk outside our hands:** an owner without ClawBot in their Weixin cannot run the WeChat live test at all (§6.5).

### 12.3 The lane plan for 1.0 (Sun 2026-12-27): four lanes, remote sessions first

The owner's ruling (2026-11-27 evening): 1.0 stays on 12-27, with **four lanes on remote sessions from Tue 12-01**. The bridges get what is left.

**What is left, plainly.**
- Remote sessions need about 57 lane-days: 12 for 0.9.10 (Mon 12-14) and 45 for 1.0 (REMOTE_SESSIONS §11, as of 2026-11-27).
- Four lanes over the 18 working days from 12-01 to Thu 12-24 give 72 lane-days. The gate itself takes 12-25 to 12-27.
- So about **15 lane-days are left**, all from Wed 12-16 on, and only if remote sessions land on estimate. That spec calls four lanes "tight". The owner's Q5 ("re-point everything") also adds the data-only paths to its re-points. So 15 is the ceiling, and any slip there takes it down to zero.
- **That is enough for WeChat (iLink) and not for KakaoTalk.**
  - The 1.0 bridge work is 16 lane-days (§12.2 without WeCom).
  - KakaoTalk alone is 10 of them: the hooks listener (5), then the adapter (5), one after the other.
  - The longest free stretch is one lane from 12-16 to 12-24 (7 working days), so the two KakaoTalk pieces cannot both finish by 12-24.
- **WeCom has no free lane, so it is 1.0.1** (Q7).

**Week by week.** L1-L4 are the four lanes. "RS" means remote-sessions work; its item names come from REMOTE_SESSIONS §7 and §11. Fill rows are small docs, tests and probe items on the cheapest model, as the 11-25 ruling allows; they take no lane.

| Week | L1 | L2 | L3 | L4 | Bridges |
|---|---|---|---|---|---|
| W1 12-01-12-07 | RS 0.9.10 core: direct attach, secondaries | RS 0.9.10 core: trust root (Q9: the installer adds the node CA), the vendor paths re-pointed (Q5) | RS relay §7.4 (controller side, Q7) | RS cloud §7.5 | **No lane.** The four fill rows of §12.4. The owner registers the §10.1 and §10.2 accounts and runs the Phase 0 probes |
| W2 12-08-12-14 | RS router §7.3 (from about 12-09) | RS re-points §7.6 (everything, Q5) | RS relay | RS cloud | No lane. Phase 0 results go into §15. Review and land the fill rows. 0.9.10 on Mon 12-14 |
| W3 12-15-12-21 | RS router (to about 12-18), then **the hooks listener** (§4) | RS phone page | RS Win/Mac, then T15-T17 | RS cloud (to about 12-15), then **the WeChat iLink adapter** from Wed 12-16 (4 days) | iLink in L4. Hooks in L1 from about 12-19 |
| W4 12-22-12-27 | Hooks listener ends about 12-24. **The Kakao adapter is not reached** | RS T15-T17 and gate fixes | RS gate fixes | iLink: egress, docs, acceptance script (2 days). The owner's live WeChat run 12-23 to 12-26 | 12-25 to 12-27: the 1.0 gate with WeChat (iLink) in. KakaoTalk is 1.0.1 unless a lane freed early |

- **KakaoTalk in 1.0 needs one of these:**
  - remote sessions free a lane by about 12-14;
  - the lanes also run the weekends 12-19/20 and 12-26 (hooks plus adapter in one lane from 12-16 would end about 12-25, with no margin);
  - a fifth lane for the bridges from 12-08.
- The owner's morning ruling put both bridges in the gate. §13 N1 asks which way, and states the default.
- **Order inside the leftover:** iLink first (no inbound path, smallest, the owner's A6 order), then the hooks listener, then Kakao. The first lane that remote sessions free takes the next item. A lane never stops remote-sessions work for a bridge.

**The two biggest risks to the date.**
1. **Remote sessions slip and take the leftover with them.** The relay's controller side has no upstream test peer, and Q5 grows the re-points. That spec's own honest date with three lanes is 2027-01-08. A slip of three days or more in any RS lane leaves no lane for iLink before 12-22, and then WeChat misses 12-27 too.
   - Mitigation: the fill rows take everything that needs no lane out of the critical path (strings, egress lines, docs, probes).
   - The coordinator checks on Fri 12-11 whether the RS lanes still free a lane by 12-16. If they do not, the owner chooses between the bridges in 1.0.1 and a fifth lane.
2. **The live acceptance depends on Tencent, Kakao and one person in Christmas week.**
   - ClawBot must be visible on the owner's Weixin account (iOS only, gradual roll-out).
   - iLink must accept the plugin's version string with our `bot_agent` (W2).
   - Kakao's callback must be available to an unverified bot (K1).
   - The owner is the only live tester for both countries (Q6), between 12-23 and 12-26.
   - Mitigation: Phase 0 runs in W1-W2, before any adapter code, so a "no" changes the plan while there is time. The stand-ins plus recorded messages carry the code part of the gate. A live run that cannot happen is recorded as a known issue, not faked.

### 12.4 First-week rows (12-01 to 12-07), ready for dispatch on Mon 12-01

Fill rows, none on a lane. Sonnet. Each row is its own worktree and branch off `origin/main`. Each lands with the full suite after the freeze, as every row does.

| id | files | test command | one line |
|---|---|---|---|
| mk-phase0-probes | `scratch/messenger-probes/kakao/probe_skill.py`, `scratch/messenger-probes/wechat/probe_ilink.py`, `scratch/messenger-probes/wechat/probe_wecom.py`, `scratch/messenger-probes/README.md` (coordinator scratch, new; not the repository) | `python3 -I scratch/messenger-probes/wechat/probe_ilink.py --dry-run` (and the same for the other two; a dry run talks to a local stand-in, never to a vendor) | The scripts for K1-K3 and W1-W4 (§11.1). Each prints its steps and writes its output to paste into §15. The owner runs them on the test accounts in W1-W2 |
| mk-fixed-lines | `ragon-rs/chat/src/lines.rs` (new), `ragon-rs/chat/src/lib.rs`, `ragon-rs/chat/tests/lines.rs` (new), `.github/apps/CODEOWNERS` | `cargo test --release -p ragon-signal -p ragon-chat -p ragon-sync -p ragon-loop-guard` in the build's export (the release step's exact command) | The new adapters' fixed lines in English, Korean and Simplified Chinese (Q8): the consent texts (including "Ragon presents itself to WeChat as the ClawBot plugin"), "working…", the approval prompt with its code, the errors. The language comes from the message's script (Hangul: Korean; Han: Simplified Chinese; otherwise English). The config override comes with the first adapter, so `Config` is not touched |
| mk-egress-exceptions | `dreamference/audit/egress_audit.py` (`declared_exceptions`, beside the Telegram and Discord lines), `tests/test_egress_audit.py` | `pytest tests/test_egress_audit.py tests/test_docs_egress_audit_runs.py -q` | The three declared-exception lines of §12.1 for `ragon-chat.service`, each shown only when its adapter is on in the chat config, and none by default |
| mk-wechat-addon-page | `docs/messengers-wechat-addon.md` (new), `docs/messengers.md`, `.github/apps/CODEOWNERS` | `pytest tests/test_docs_index.py tests/test_docs_commands.py -q` | The §9.2 add-on page (Q1): the warning first, wechaty or wxauto as user-installed tools, the `ragon web` endpoint, nothing in the binaries. Linked from `docs/messengers.md` after the official routes. There is no KakaoTalk page |

The lane rows (iLink, hooks, Kakao) are written when a lane frees (§12.3). The coordinator writes them from §6, §4 and §5 then.

## 13. Questions for the owner

All eight questions of the first round are answered (§0.2). One new question comes from the lane plan.

1. **N1: KakaoTalk and the four lanes.** With four lanes on remote sessions, what is left fits WeChat (iLink) and not KakaoTalk (§12.3). The morning ruling put both in the 1.0 gate.
   - (a) *Default, applied unless the owner says otherwise:* keep the date and the four lanes. KakaoTalk takes the first lane remote sessions free after iLink. If KakaoTalk is not green on its stand-ins by 12-24, it ships in 1.0.1 and the gate lists it as a known issue.
   - (b) The lanes also run the weekends 12-19/20 and 12-26 for the bridges (more account budget, no margin).
   - (c) A fifth lane for the bridges from 12-08: both bridges and WeCom fit, with margin.

## 14. Sources (read 2026-11-27)

**KakaoTalk** (official unless marked):
- Skill server and request flow: kakaobusiness.gitbook.io/main/tool/chatbot/skill_guide/make_skill
- Callback, 5 s timeout, 5-minute single-use URL: kakaobusiness.gitbook.io/main/tool/chatbot/skill_guide/ai_chatbot_callback_guide (third-party for the older 1-minute/application: quite-a-bit.tistory.com/19)
- Response format, limits, `botUserKey`: kakaobusiness.gitbook.io/main/tool/chatbot/skill_guide/answer_json_format
- Bot setup, deploy (30 min, no review): kakaobusiness.gitbook.io/main/tool/chatbot/start/prepare, …/main_notions/bot_setting, …/main_notions/deploy
- Event API price and requirements: kakaobusiness.gitbook.io (Event API pages)
- Talk Message API, quotas, review, tokens: developers.kakao.com/docs/ko/kakaotalk-message/common, …/tutorial/message, …/kakaologin/common
- Business messages: business.kakao.com/info/bizmessage/, …/brandmessage/; prices from a dealer blog (third-party, 2026)
- Channel creation without a business number, foreign numbers: Kakao Business help (cs.kakao.com) and innerkorea.com (third-party, 2026)
- Operating policy on abnormal use (2025-09-30): talksafety.kakao.com/policy/stability/abnormalusage
- LOCO libraries: api.github.com for each repository; node-kakao docs and discussion 616; 2021 ban: chat.axwith.com (third-party)
- Not found: a skill-request signature, Kakao's source addresses, review times for business verification, any Kakao lawsuit.

**WeChat** (official unless marked):
- iLink protocol and plugin: github.com/Tencent/openclaw-weixin (`docs/protocol.md`, README), npm `@tencent-weixin/openclaw-weixin` 2.4.10; launch 2026-03-22 and limits (iOS only, user first, Tencent control): zhihu and smzdm articles (third-party); `context_token` optional since 2.1: github.com/nightsailer/wechat-clawbot docs (third-party)
- WeCom intelligent bot, long connection: developer.work.weixin.qq.com/document/path/101463; open.work.weixin.qq.com/help2/pc/21669, 21663
- WeCom trusted IP / domain: open.work.weixin.qq.com/help2/pc/21711, 21316, 21611; callback retries: document/path/90930
- WeChat Customer Service: developer.work.weixin.qq.com/document/path/94677, 94638
- Official accounts: developers.weixin.qq.com/doc/subscription/guide/dev/push/, …/Receiving_standard_messages.html, …/api_sendcustommessage.html (changelog 2025-11-26), …/api_getaccesstoken.html, …/limit.html; overseas registration and fees: theredscroll.com (third-party, 2026-10-06) citing kf.qq.com
- WeCom org limits and verification fee: open.work.weixin.qq.com/help2/pc/18496, 19734
- Crackdown notice 2025-04-30: mp.weixin.qq.com/s/A6h4ZLTE2EPrY7kJ5fHE2g
- Unofficial libraries: api.github.com for each repository
- Not found: ClawBot's own terms, iLink rate limits, ClawBot in the international app, cross-reach of official accounts between Weixin and WeChat, ban numbers.

## 15. Phase 0 results

Empty until the owner's accounts exist (§10, §11.1).

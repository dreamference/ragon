# Ragon 0.9.8.1 "Illuyanka" — release notes

Version 0.9.8.1, a patch release of 0.9.8 with one fix: the chat bridge's pairings (`scratch/reports/fix-098-pairings-dropped.md`). The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. A patch release keeps its release's codename. There is no new engine image: the 0.9.7 image stays pinned (`ragon-serve@sha256:f4391e2e`, ragon-engine 46ab4259). Everything in `specs/DREAMFERENCE_RELEASE_0.9.8.md` still applies, except where this file corrects it.

---

## Ragon 0.9.8.1 "Illuyanka"

**Ragon 0.9.8.1 makes paired phones work again.** 0.9.8 dropped every Telegram and Discord pairing the first time its chat bridge started and then accepted only pairings to a Ragon account. A node without accounts, which is every node unless you turned accounts on, cannot make those, so paired phones were refused and `ragon pair` answered "Not signed in". Everything else is the same as in 0.9.8; the engine is unchanged.

### Who is affected

Only nodes that started a phone bridge (Telegram, Discord or Matrix) on 0.9.8. A node that never ran 0.9.8, or ran it with no bridge turned on, is not affected; it can install 0.9.8.1 like any update.

### What to do

1. Install 0.9.8.1 (`ragon update`, or the installer).
2. **Matrix:** nothing to do. Its list of allowed users was never changed, so Matrix chats answer again once the bridge runs 0.9.8.1.
3. **Telegram and Discord:** pair each phone again, as in 0.9.7. 0.9.8 deleted these pairings and they cannot be restored. Run `ragon chat telegram pair` or `ragon chat discord pair` on the node, then send the `/pair` line it prints to your bot (in Discord, in a direct message) within 10 minutes. Each code works once.

### Since 0.9.8

- **Pairing works as in 0.9.7.** Without accounts, the bridges again keep their own list of paired users and pair with codes made on the node. Account pairing is used only on a node with accounts turned on.

### Known limits

The limits of 0.9.8 apply unchanged.

---

**Checklist:** not part of the release text.

- [ ] Engine image pin on main is still `f4391e2e`.
- [ ] The release date in `CHANGELOG.md` matches the dispatch day.
- [ ] The dry run of the chain is green.

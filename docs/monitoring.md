# Node monitoring

A Ragon node watches itself. The node monitor, `ragon-node-monitor`, checks the model server, the gate in front of it, memory, disk, the GPU, the services Ragon runs and the node's own certificates and keys. It reports what it finds; it does not change anything on the machine. Everything stays on the node.

## What you can look at today

**From a terminal**, on the node:

```bash
ragon-admin monitor status          # the node's state and its checks
ragon-admin monitor status --json   # the monitor's JSON, unchanged
```

`status` prints the node's state (`ok`, `degraded` or `down`) and each check with its state and a short reason. It takes one sample when it runs; it shows no history.

**In a browser**, on a paired device: `ragon web` serves a status page at `/status`. It shows the node's state and its checks, one per line, with the same state and reason as the terminal. The page reads `/api/monitor/status`, which returns the JSON of `monitor status --json`. The monitor's answer is reused for 10 seconds, however many pages ask. A device that is not paired sees nothing at these addresses, as with the rest of `ragon web`.

**In the desktop app**, a status chip beside the model chip shows the node's state, coloured by it. Clicking it opens the status page. The chip reads the node the app is using, so a client shows its own node's health.

## How it runs

`ragon-node-monitor` runs as the user service `ragon-node-monitor.service`. `ragon-admin node enable` installs and enables the unit, with a small memory cap and a watchdog. The service samples the node's checks at fixed intervals and writes the samples to a local database:

```text
~/.local/share/dreamference/monitor/node/monitor.sqlite
```

The monitor reads the node's state from the machine and from the model server's own counters. It has no network access of its own, no message channel and no way to restart a unit or power-cycle the machine.

## What is not there yet

- **No history.** There is no `monitor history` command and no chart on the status page yet. The samples are stored, but nothing on the node reads them back for you.
- **No phone alerts.** The monitor does not send messages to your phone.
- **No fleet view.** `status` covers the one node you run it on.

## Where to look when a check is not `ok`

1. Run `ragon-admin monitor status`; the line for the check gives the reason.
2. Check the service with `systemctl --user status ragon-node-monitor.service`.
3. For the model server, see [Models](models.md); for the admin commands, see [ragon-admin](admin.md).

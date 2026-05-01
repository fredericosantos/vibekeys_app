---
name: vibekeys-setup
description: First-run configuration for VibeKeys. Asks once whether Claude Code runs locally or over SSH, then runs `vibekeys bootstrap` to install the bridge service, write config, and (on remote hosts) set $VIBEKEYS_REMOTE. Use when ~/.config/vibekeys/config.json is missing or the user explicitly invokes /vibekeys-setup.
---

# VibeKeys Setup

This skill bootstraps the VibeKeys plugin so hooks reach the keyboard whether Claude Code is running locally or on a remote SSH host.

## When to run

- The session-start nudge printed `[vibekeys] not configured yet`.
- The user typed `/vibekeys-setup`.
- The user wants to reconfigure (e.g. moved to a new machine, switched between local/SSH workflows).

## Detect first, ask second

Before asking anything, run `vibekeys bootstrap --dry-run` and read the printed `detected:` line. It reports three signals: BLE adapter present, inside SSH session, bridge port reachable.

Use the table below to decide whether you can skip the question:

| BLE | SSH | Action |
|-----|-----|--------|
| yes | no  | Run `vibekeys bootstrap --mode local`. No prompt. |
| no  | yes | Run `vibekeys bootstrap --mode ssh-remote`. No prompt. |
| yes | yes | Ambiguous — ask. |
| no  | no  | Ambiguous — ask. |

## When you must ask

Ask exactly one question:

> **How are you using Claude Code with this VibeKeys keyboard?**
>
> 1. **Locally** — Claude Code runs on this machine and the keyboard is paired to it.
> 2. **Over SSH** — Claude Code runs on a different (remote) machine; the keyboard is paired to my laptop.
> 3. **Both, depending on the project.**

Map the answer to a bootstrap call:

- **1** → `vibekeys bootstrap --mode local`
- **2 (and we're on the laptop)** → `vibekeys bootstrap --mode ssh-laptop`
- **2 (and we're on the remote)** → `vibekeys bootstrap --mode ssh-remote`
- **3** → run *both* `--mode ssh-laptop` (on the laptop) and `--mode ssh-remote` (on the remote). The dispatch wrapper falls back to BLE when `$VIBEKEYS_REMOTE` is unset, so both can coexist on one machine.

To decide laptop vs remote in option 2/3 when the detection is ambiguous, ask the follow-up: *"Is this the machine your VibeKeys keyboard is paired to (your laptop), or the remote you're SSH'd into?"*

## After bootstrap

Show the user what bootstrap printed. Two cases need a manual step:

1. **`ssh-laptop` mode** — bootstrap prints an SSH config snippet. Tell the user to paste it into `~/.ssh/config` on the laptop:
    ```
    Host <your-remote>
        RemoteForward 7777 127.0.0.1:7777
    ```
2. **`ssh-remote` mode** — bootstrap appended `export VIBEKEYS_REMOTE=...` to the user's shell rc. Tell them to either open a new shell or `source ~/.zshrc` so it takes effect.

After both sides are set up, run a quick smoke test from the remote: `curl -s http://127.0.0.1:7777` should print `ok`. If it doesn't, the tunnel isn't live — confirm the user reconnected SSH after editing the config.

## Reconfigure / uninstall hints

- Reconfigure: just re-run `/vibekeys-setup`. `vibekeys bootstrap` is idempotent.
- Stop the bridge service (macOS): `launchctl unload ~/Library/LaunchAgents/sh.secondstate.vibekeys.plist`
- Stop the bridge service (Linux): `systemctl --user disable --now vibekeys.service`
- Remove remote env var: delete the `VIBEKEYS_REMOTE` line from `~/.zshrc` / `~/.bashrc`.

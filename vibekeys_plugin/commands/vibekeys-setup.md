---
description: Configure VibeKeys for this machine — local BLE or SSH-remote workflow.
---

Use the `vibekeys-setup` skill to bootstrap VibeKeys on this machine.

Detect first (`vibekeys bootstrap --dry-run`); only ask the user a question if the signals are ambiguous. Then run the appropriate `vibekeys bootstrap --mode ...` command and surface any manual steps it printed (SSH `RemoteForward` line, `source ~/.zshrc`, etc.).

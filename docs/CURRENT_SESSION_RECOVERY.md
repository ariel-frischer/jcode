# Current session recovery snapshot

Captured: 2026-08-06 02:30 UTC

Use `scripts/recover-current-sessions.sh` from the repository checkout after a Kitty/Jcode crash. It opens each session in a new Kitty tab with a normal `zsh` shell, starts Jcode with `--resume`, and leaves the shell at the session's working directory after Jcode exits or is interrupted.

| Label | Session ID | Working directory | Role |
|---|---|---|---|
| Autospec / deer | `session_deer_1785959709238_1af8ca4ca2345274` | `/home/ari/repos/autospec` | Autospec |
| Locus / penguin | `session_penguin_1785963770473_43f44b84e3b74796` | `/home/ari/repos/locus` | Locus |
| Jcode / sheep | `session_sheep_1785963902901_190ce18535a3e20b` | `/home/ari/repos/jcode` | Jcode |

Manual equivalent for one entry:

```bash
cd /home/ari/repos/jcode
jcode --resume session_sheep_1785963902901_190ce18535a3e20b
```

Do not mass-resume the other historical session files. Update this snapshot and the script deliberately when the active recovery set changes.

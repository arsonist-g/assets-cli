# Install, repair, and configuration

Read this when a variable reads empty in a fresh shell, when `assets doctor` reports a failing check, or when the tool must be installed or repaired. The common path in `SKILL.md` never needs it.

## First run

The person installs the binary and puts `assets` on PATH; nothing is configured before the first command, because the tool reads and writes the registry directly.

`assets init` installs all three parts below, and it is idempotent: a second run walks every file back to the expected state and reports one outcome per file, `新建` (created), `已存在` (already present), or `已修复` (repaired).

`assets skill install [--json]` installs only the third part, the skill package, and reports the same way. Use it to refresh this package on its own when the hooks are already in place.

| Step | Lands | Purpose |
|---|---|---|
| hooks | `~/.assets-cli/hydrate.ps1`, `~/.assets-cli/emit-sh.ps1`, `~/.assets-cli/hydrate.sh` | read the registry at shell start and export the `ASSETS_CLI_*` variables |
| profile blocks | the PowerShell profile, `~/.bashrc`, and the Git Bash login profile (`.bash_profile`, `.bash_login`, or `.profile`) | call the hook, as one line inside a `# >>> assets-cli >>>` marker block |
| skill | `~/.agents/skills/assets/` | this skill package |

- Neither command touches PATH. Where the binary sits, and how it got on PATH, belongs to the person's install step rather than to this tool.
- A marker block whose content is out of date is replaced whole, so an edit made inside the block is lost on the next run.
- The hooks export only names matching `^ASSETS_CLI_[^_]+_.+$`, which is three segments or more. A single segment switch such as the ones below never reaches a session.
- Exit codes here are 0, or 4 when the registry or the data directory could not be written.

## Self check

`assets doctor [--json]` reports one line per check, each `ok` or `未通过` with a stated fix. It repairs nothing.

| Check | Passes when | A failure means |
|---|---|---|
| `hook_installed` | the hook scripts exist and both shell profiles carry the marker block | run `assets init` |
| `session_fresh` | a sample of declared variables matches the registry, compared by hash and never printed | the shell you are in started before the last write, so its environment is stale; start a new shell |
| `registry_writable` | the user registry key opens for writing, opened only and never written | the process is not the user who owns the ledger |
| `data_dir_writable` | the data directory can be created, written, and cleaned | a disk or permission problem |

Exit 0 means every check passed and exit 7 means at least one did not. `session_fresh` reports ok while no variable is declared, since there is nothing to compare.

## Configuration switches

Two environment variables redirect the storage. They exist for isolated development and test runs, and the person does not set them in normal use.

| Switch | Default | Meaning |
|---|---|---|
| `ASSETS_CLI_DATA` | `~/.assets-cli` | the data directory: the declarations, and the snapshots taken before every write. A leading `~` is expanded |
| `ASSETS_CLI_REGISTRY` | `HKCU\Environment` | the registry key that holds the values, written as `<hive>\<subkey>` |

A run that sets both writes to that key and that directory alone, which is how a test reaches the ledger without touching the real one.
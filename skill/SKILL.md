---
name: assets
description: Reach for this before a call that needs a credential from this machine, and before storing one. It drives the `assets` CLI, the local credential ledger that keeps API tokens, proxies, and base URLs as Windows user level environment variables. Use it to find which assets exist, to store a new token for an account, or when a credential variable reads as empty.
---

# assets: the local credential ledger

`assets` keeps this machine's platform credentials in Windows user level environment variables, under one naming rule per platform and account. A shell hook exports them when a shell starts, so a credential is used by referencing its variable, never by fetching or printing it. Installing and repairing the hook, the profiles, and this skill is `assets init`, documented in `references/install-and-config.md`.

## Output boundary

- It calls no platform API and knows no platform's semantics. It stores a base URL; which endpoint to hit with it stays your decision.
- It never prints a value. No command returns one, by design.
- It deletes nothing. No command removes a platform, an account, a declaration, or a snapshot; removing is the person's own action in the GUI.
- It does not test whether a credential still works. An expired token surfaces as the API call that fails, not here.

## How to read this skill

Rules are marked must, do not, or the criterion is. Command names, flags, exit codes, and the tool output shown in fenced blocks are behavior observed by running the tool on this machine. The loop below is a sequence to follow, not a transcript: its variable names and addresses stand for what this ledger actually holds. Tables and unmarked examples are reference material: read the row you need and skip the rest.

The package also carries a Chinese copy of every file, `skill-zh.md` and `references/*-zh.md`. Those exist for the person to proofread; the English file is the one in force, and you never need to load a copy.

Two files sit behind this one, and neither is loaded with it:

| File | Read it when |
|---|---|
| `references/install-and-config.md` | a variable reads empty in a fresh shell, you install or repair the tool, or you refresh the skill package alone with `assets skill install` |
| `references/errors.md` | a command exits nonzero and the next action is not obvious |

## What must be true before you call

The hook exports the ledger when a shell starts, so a command you start now sees the values as they are now. There is no session to open, no daemon to start, and no lock to take.

- A value written a moment ago is already in effect for the next command you start. Do not ask for a re-read after a write.
- A shell process that was already running when the write happened keeps the environment it exported at its own start. `assets doctor`'s `session_fresh` check reports that case, and the fix is a new shell, not a re-read.

A value can only be stored where its owner already exists, and the order is fixed:

1. the platform exists (`assets platform add`), otherwise every write below exits 2;
2. for an account level variable the account entry exists (`assets account add`); `var set --account` does not create one, and exits 2 on an unknown alias;
3. then the value goes in (`assets var set`).

Reading needs none of that: `assets list` works on an empty ledger, and a name you cannot find is a name that does not exist yet, not an error in your call.

## The loop

```bash
# 1. what exists. --platform exits 2 when the platform was never registered
assets list --platform cloudflare

# 2. store a value. The account entry must exist already
printf '%s' "$TOKEN" | assets var set --platform cloudflare --account new --term API_TOKEN

# 3. use it. The platform level API_BASE_URL is the address the person stored
curl -H "Authorization: Bearer $ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN" "$ASSETS_CLI_CLOUDFLARE_API_BASE_URL/v1/items"
```

The same two writes on PowerShell, where `printf` does not exist:

```powershell
$TOKEN | assets var set --platform cloudflare --account new --term API_TOKEN
# the credential is now readable as $env:ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN
```

Under PowerShell the same variables read as `$env:ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN`; a bare `$ASSETS_CLI_...` in a PowerShell command expands to nothing and would send an empty credential.

Read the exit code, not the prose: `list --platform <name>` exits 2 when that platform was never registered, and 0 when it was, even with no account under it. That is the answer to whether this platform has assets here.

When the entry you need is not in `list`, it is not registered yet, and the value itself can only come from the person. Ask them for it instead of inventing one or reaching for a same-purpose credential that belongs to another account.

How you join the stored base URL with a path is your decision. The tool keeps the address as data and never inspects it.

## Command usage

Shape: `assets <group> <verb> [positional...] [--flag <value>]`; `account rename` is the one command that takes its new alias after the flags. Flags are kebab case, and an option a command does not declare is a usage error (exit 2), never a silent ignore. There are no global flags; `--json` belongs to the commands that list it.

Run `assets --help` for the whole surface and the exit code table instead of guessing a flag. A bare `assets` prints the same text and exits 2.

A value never travels through argv. `assets var set` reads the value from stdin and has no `--value` flag, so piping into it is the only way to write one. Piping nothing is an empty value, exit 3; at a terminal with no pipe at all it refuses with exit 2. The trailing newline both shells add when piping, LF or CRLF, is trimmed, and everything before it is stored exactly as it arrived. Which shell you are in decides the wording: `printf '%s' "$VALUE" | ...` under bash and Git Bash, `$VALUE | ...` under PowerShell.

## The command surface

| You want to | Run |
|---|---|
| learn what this machine has | `assets list` |
| learn whether one platform is registered | `assets list --platform <name>` |
| register a platform | `assets platform add <name> [--note <text>]` |
| rename a platform and recompute every name under it | `assets platform rename <old> <new>` |
| register an account, or a server entry with no credential | `assets account add --platform <name> --alias <alias> [metadata flags]` |
| change an account's metadata | `assets account edit --platform <name> --alias <alias> [metadata flags]` |
| rename an account and recompute its names | `assets account rename --platform <name> --alias <old> <new>` |
| store or replace a value | `printf '%s' "$VALUE" \| assets var set --platform <name> [--account <alias>] --term <TERM>` |
| install or repair hooks, profiles, and this skill | `assets init`, in `references/install-and-config.md` |
| check the hook and the storage | `assets doctor`, in `references/install-and-config.md` |

| Flag | Used by | Meaning |
|---|---|---|
| `--platform <name>` | list, account add/edit/rename, var set | which platform |
| `--account <alias>` | list, var set | which account; omit it for a platform level variable. `list` needs `--platform` beside it and exits 2 without it |
| `--alias <alias>` | account add/edit/rename | the account's own name |
| `--term <TERM>` | var set | the last segment of the variable name |
| `--note <text>` | platform add, account add/edit | free text metadata |
| `--email`, `--purpose`, `--host`, `--user` | account add/edit only | free text metadata; `platform add` takes none of these |
| `--json` | list, doctor, init, skill install | machine readable output |

| Command | Behavior worth knowing |
|---|---|
| `assets list` | markdown by default: a platform heading, its note, its platform level variables, then one row per account. A platform with no account still gets a heading and the line `（该平台暂无账号条目）` |
| `assets list --json` | the same content as `platforms[]`, each carrying `variables[]` of `{term, name}` and `accounts[]` with the metadata and its own `variables[]`. Values appear in neither form |
| `assets var set` | upsert on the term: a term that is not declared yet is declared, one that exists is overwritten. The platform, and the account when one is given, must already exist. The term is yours to choose, `API_TOKEN` being the usual one for a credential; what comes back on success is the derived name, so a write is also how you confirm a name you could not read before it existed |
| `assets account edit` | changes only the fields you pass, and an empty string clears a field (`--note ""`). To change the alias, use `account rename` |
| `assets platform rename`, `assets account rename` | recompute names: the platform form covers its platform level variables and every account under it, the account form covers only that account's own variables. Both snapshot first and roll back whole |
| `assets account add` | an entry may declare no variable at all, which is how a server entry (host, user, purpose only) looks |

## The naming space

Every variable this tool owns starts with `ASSETS_CLI_`, and the segments after that prefix are the address.

| Level | Shape | Example |
|---|---|---|
| platform level | `ASSETS_CLI_<PLATFORM>_<TERM>` | `ASSETS_CLI_CLOUDFLARE_API_BASE_URL` |
| account level | `ASSETS_CLI_<PLATFORM>_<ALIAS>_<TERM>` | `ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN` |

Three rules decide what you may pass, and they are what callers get wrong:

- The charset is `A-Za-z0-9_`. A hyphen, a dot, an `@`, or a space is rejected with exit 3, so `cf-new` is not a usable alias while `cf_new` is.
- Each segment is at most 64 characters, beyond which the exit is also 3.
- The derived name is uppercased, so `cloudflare`/`new`/`api_token` and `CLOUDFLARE`/`NEW`/`API_TOKEN` name one variable.

An underscore is legal inside a segment, so a name cannot be parsed back into platform, alias, and term by splitting on underscores. The pattern tells you the shape; the name itself comes from `assets list`, which reports the derived name of every declared variable.

Two terms carry a fixed meaning for connectivity: `API_BASE_URL` at platform level, the address to point that platform's API at, and `PROXY` at account level, the proxy to use for that account. Seeing one in `list` means that path is configured; not seeing it means it is not configured, which is not the same as unknown.

## Red lines

- Do not print, echo, log, or paste a value, and do not dump the environment (`env`, `printenv`, `Get-ChildItem Env:`) to find one: a dump publishes every credential at once. Reference the variable at the place the value is consumed.
- Do not commit a name you worked out from the pattern where an exact name is required. Read it from `assets list`; the pattern gives you the shape, and `list` gives you the fact.
- Do not look for a way to put a value on the command line. That flag does not exist, and a value given that way would land in the process list and the shell history.
- Do not work around a refusal by editing the registry or the data files, and do not hunt for a delete command. A refusal here protects a declaration the tool cannot account for, and deleting stays the person's action.

## Errors

| Exit | Meaning | Action |
|---|---|---|
| 0 | success | read stdout |
| 2 | usage error, or the platform, account, or variable is not registered | correct the reference, or create the platform or account first; `list` shows what exists |
| 3 | a segment breaks the charset or the 64 character limit, or the value is empty | fix the argument and retry |
| 5 | a name collides, or the registry holds a variable this tool did not declare | the message names the variable; pick another name, or leave it to the person |
| 6 | the value, or the whole environment block, is over budget | shorten the value |

The table above is the common path. `references/errors.md` holds the closed set, exit 4 and exit 7 included, with the literal messages and with how to tell a failure worth a retry from one that will repeat.
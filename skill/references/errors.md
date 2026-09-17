# Exit codes and failures

Read this when a command exits nonzero and the next action is not obvious.

## The closed set

| Exit | Meaning | Action |
|---|---|---|
| 0 | success | read stdout |
| 2 | usage error, or a referenced platform, account, or variable is not registered | correct the argument, or create the missing platform or account first. Retrying unchanged cannot help |
| 3 | a segment breaks the charset or the 64 character limit, or the value is empty | correct the argument. Retrying unchanged cannot help |
| 4 | the registry or the data file could not be read or written, or the ledger and the registry disagree | read the message: see below, the two cases need different actions |
| 5 | a name collision, or a variable in the registry that this tool did not declare | the message names the variable, so choose another name or leave it to the person |
| 6 | the value, or the environment block total, is over budget | shorten the value, or free space in the GUI |
| 7 | `assets doctor` had a failing check | the detail column names the check and its fix |

Exit 4 carries two situations, and the message separates them:

- a message that names a variable means the ledger and the registry disagree about it. No retry helps; restore the value with `assets var set` and repeat the command.
- any other 4 is a storage failure. Retry once, then run `assets doctor`.

Retry applies there and nowhere else. Exit 2, 3, and 5 repeat identically until an argument or a name changes, 6 needs a shorter value, and 7 needs the fix its detail names.

## The messages you will meet

| Message | Exit | What to do |
|---|---|---|
| `平台不存在：<name>` | 2 | the platform was never registered, or the name differs in spelling. There is no fuzzy match, so either correct the name or register the platform. `var set` and the two `rename`s print the bare form |
| `平台不存在：<name>（用 assets list 看已登记的平台）` | 2 | the same case, as `list` words it |
| `平台不存在：<name>（先用 assets platform add 建平台）` | 2 | an account needs an owner, so register the platform first. `account add` words it this way |
| `平台 <name> 下没有条目：<alias>` | 2 | the alias does not belong to that platform, and `var set --account` does not create it |
| `--account 必须与 --platform 同用` | 2 | pass both, or pass `--platform` alone |
| `平台名只能包含 A-Z a-z 0-9 和 _：<value>` | 3 | the same charset applies to an alias and a term; a hyphen is the usual cause |
| `别名过长：最多 64 个字符，当前 <n>` | 3 | shorten the segment |
| `值不能为空：不声明即不存在，空值会让变量名没有意义` | 3 | have the person delete in the GUI instead of storing an empty value |
| `值过长：超过单个用户级变量上限（32767 字符）` | 6 | shorten the value |
| `预算提示：环境块已用 <n> / 262144 字符（<p>%），接近上限` | 0 | a warning printed on a write that succeeded, at 80 percent of the budget and above. Nothing is wrong yet; the write that would cross the budget is the one that fails with exit 6 |
| `平台已存在：<name>（重名检测不区分大小写）` | 5 | the name exists under a different case |
| `注册表里已有同名变量 <name>，但台账里没有对应声明；为免覆盖手工创建的变量，拒绝写入` | 5 | a hand made or foreign variable holds the name. The person decides whether to take it over in the GUI or rename it |
| `台账与注册表不一致：<name> 在注册表里没有值，已拒绝改名` | 4 | a desync, not a storage fault. Restore the value with `assets var set`, then repeat the rename. The tool refuses rather than move a declaration it cannot account for |
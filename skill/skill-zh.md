---
name: assets
description: 需要这台机器上的凭证、或要把凭证存进来之前，先看它。它驱动 `assets` CLI，也就是本机的凭证台账：把各平台各账号的 API 令牌、代理、base URL 存成 Windows 用户级环境变量。要查本机登记了哪些资产、要给某账号存一个新令牌、或某个凭证变量读出为空时用它。
---

> 本文件是中文翻译版，供人阅读校对；实际生效的是同目录的英文版 `SKILL.md`。两版内容一一对应。

# assets：本机凭证台账

`assets` 把本机的平台凭证存在 Windows 用户级环境变量里，命名规则每个平台、每个账号一套。shell 启动时由钩子把它们导出，所以用凭证就是引用它的变量名，永远不是去取它或打印它。安装与修复钩子、profile 和这份 skill 是 `assets init`，写在 `references/install-and-config.md`。

## 输出边界

- 它不调用任何平台 API，也不知道任何平台的语义。它存的是 base URL；拿它去打哪个端点仍然是你的事。
- 它从不打印值。没有任何一条命令会返回值，这是设计如此。
- 它什么都不删。没有任何命令能删掉平台、条目、声明或快照；删除是人在 GUI 里的动作。
- 它不检验凭证是否还能用。令牌过期表现为某次 API 调用失败，而不是在这里。

## 怎么读这份 skill

带"必须""不要""判据是"的句子是规则。命令名、选项、退出码，以及围栏代码块里展示的工具输出，都是在这台机器上真跑出来的行为。下面那一轮用法是照着做的顺序，不是一次真实会话的记录：里面的变量名和地址代表这份台账里实际有的东西。表格和没标注的示例是参考材料：读你要的那一行，其余跳过。

包里每个文件都另有一份中文版本（`skill-zh.md` 与 `references/*-zh.md`）。它们只为供人校对而存在；生效的是英文版，你永远不需要加载这些副本。

这份文件背后还有两份，都不与它一起加载：

| 文件 | 什么时候读 |
|---|---|
| `references/install-and-config.md` | 新开的 shell 里变量读出为空，或者要安装、修复这套东西，或用 `assets skill install` 单独刷新 skill 包 |
| `references/errors.md` | 命令以非零退出，而下一步该做什么并不明显 |

## 调用之前必须成立的事

钩子在 shell 启动时把台账导出，所以你现在启动的命令看到的就是现在的值。没有会话要开、没有守护进程要起、没有锁要拿。

- 刚刚写进去的值，你下一条启动的命令就已经生效。写完值之后不要要求重读。
- 写入发生时已经在跑的那个 shell 进程，用的是它自己启动时导出的那份环境。这种情况由 `assets doctor` 的 `session_fresh` 检查指出，办法是开一个新 shell，不是重读。

值只能存在它的归属已经存在的地方，顺序是固定的：

1. 平台已存在（`assets platform add`），否则下面每一条写命令都退出 2；
2. 账号级变量还要求条目已存在（`assets account add`）；`var set --account` 不会顺手建条目，别名不认识就退出 2；
3. 然后才把值写进去（`assets var set`）。

读不需要这些前提：空台账上 `assets list` 照样能跑，而一个你找不到的名字，是"还没有这个名字"，不是你这次调用写错了。

## 一轮完整用法

```bash
# 1. 先看有什么。--platform 在该平台从未登记过时退出 2
assets list --platform cloudflare

# 2. 存值。条目必须已经存在
printf '%s' "$TOKEN" | assets var set --platform cloudflare --account new --term API_TOKEN

# 3. 用它。平台级的 API_BASE_URL 就是人存进去的那个地址
curl -H "Authorization: Bearer $ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN" "$ASSETS_CLI_CLOUDFLARE_API_BASE_URL/v1/items"
```

同样的两步写入在 PowerShell 里（那里没有 `printf`）：

```powershell
$TOKEN | assets var set --platform cloudflare --account new --term API_TOKEN
# 之后这个凭证可以用 $env:ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN 读到
```

同一个变量在 PowerShell 里读作 `$env:ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN`；在 PowerShell 命令里裸写 `$ASSETS_CLI_...` 会展开成空，等于发出去一个空凭证。

看退出码，不要看文案：`list --platform <名称>` 在该平台从未登记过时退出 2，登记过时退出 0，哪怕它下面一个账号都没有。这就是"这个平台在这里有没有资产"的答案。

list 里没有你要的那个条目，说明它还没登记，而那个值本身只能由人给。去问人要，不要自己编一个，也不要拿别的账号的同用途凭证顶上。

存下来的 base URL 怎么和具体路径拼，是你的事。工具把地址当数据存着，从不解析它。

## 命令用法

形状：`assets <组> <动词> [位置参数…] [--选项 <值>]`；唯一把位置参数放在选项后面的是 `account rename`（新别名跟在选项后）。选项名是 kebab-case，命令没有声明的选项一律是用法错误（退出码 2），不会被静默忽略。没有全局选项；`--json` 属于列了它的那些命令。

要看完整命令面和退出码表就跑 `assets --help`，不要猜选项。裸跑 `assets` 打印同一段文字并退出 2。

值从不经过 argv。`assets var set` 从 stdin 读值，没有 `--value` 这样的选项，所以管道是唯一的写入方式。管道里什么都不给就是空值（退出码 3）；在终端上完全不带管道，它按用法错误拒绝（退出码 2）。两种 shell 用管道时都会补的那个行尾换行（LF 或 CRLF）会被裁掉，它之前的内容原样保存。用哪种写法看你所在的 shell：bash 与 Git Bash 写 `printf '%s' "$VALUE" | …`，PowerShell 写 `$VALUE | …`。

## 命令面

| 你想做 | 跑 |
|---|---|
| 查这台机器有什么 | `assets list` |
| 查某个平台登记过没有 | `assets list --platform <名称>` |
| 登记一个平台 | `assets platform add <名称> [--note <文本>]` |
| 给平台改名，并重算它下面所有名字 | `assets platform rename <旧名> <新名>` |
| 登记一个账号，或一个不含凭证的服务器条目 | `assets account add --platform <名称> --alias <别名> [元数据选项]` |
| 改一个账号的元数据 | `assets account edit --platform <名称> --alias <别名> [元数据选项]` |
| 给账号改名，并重算它的名字 | `assets account rename --platform <名称> --alias <旧别名> <新别名>` |
| 存值或换值 | `printf '%s' "$VALUE" \| assets var set --platform <名称> [--account <别名>] --term <术语>` |
| 安装或修复钩子、profile 与这份 skill | `assets init`，见 `references/install-and-config.md` |
| 自检钩子与存储 | `assets doctor`，见 `references/install-and-config.md` |

| 选项 | 谁在用 | 含义 |
|---|---|---|
| `--platform <名称>` | list、account add/edit/rename、var set | 哪个平台 |
| `--account <别名>` | list、var set | 哪个账号；平台级变量就不给。`list` 要求它和 `--platform` 同时给，缺了退出 2 |
| `--alias <别名>` | account add/edit/rename | 账号自己的名字 |
| `--term <术语>` | var set | 变量名最后一段 |
| `--note <文本>` | platform add、account add/edit | 自由文本元数据 |
| `--email`、`--purpose`、`--host`、`--user` | 只有 account add/edit | 自由文本元数据；`platform add` 一个都不收 |
| `--json` | list、doctor、init、skill install | 机器可读输出 |

| 命令 | 值得知道的行为 |
|---|---|
| `assets list` | 默认 markdown：一个平台一个标题、它的备注、它的平台级变量，然后每个账号一行。没有账号的平台照样有标题和 `（该平台暂无账号条目）` 这一行 |
| `assets list --json` | 同样的内容装进 `platforms[]`，每项带 `variables[]`（`{term, name}`）和 `accounts[]`（元数据加它自己的 `variables[]`）。两种形态都不出值 |
| `assets var set` | 按术语 upsert：还没声明的术语就声明，已存在的就覆盖。平台必须已存在，给了账号时该账号也必须已存在。术语由你选，凭证惯用 `API_TOKEN`；成功输出里回的就是派生名，所以"写"本身也是确认名字的办法（在它存在之前你读不到它） |
| `assets account edit` | 只改你给的那些字段，给空串即清空该字段（`--note ""`）。要改别名请用 `account rename` |
| `assets platform rename`、`assets account rename` | 重算名字：平台那条覆盖它的平台级变量和它下面的每一个账号，账号那条只覆盖该账号自己的变量。两者都先建快照、整体回滚 |
| `assets account add` | 条目允许一个变量都不声明，服务器类条目（只有 host、user、purpose）就长这样 |

## 命名空间

本工具拥有的每个变量都以 `ASSETS_CLI_` 开头，前缀之后的分段就是地址。

| 层级 | 形状 | 例 |
|---|---|---|
| 平台级 | `ASSETS_CLI_<平台>_<术语>` | `ASSETS_CLI_CLOUDFLARE_API_BASE_URL` |
| 账号级 | `ASSETS_CLI_<平台>_<别名>_<术语>` | `ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN` |

三条规则决定你允许传什么，也正是调用方最容易搞错的：

- 字符集是 `A-Za-z0-9_`。连字符、点、`@`、空格一律被拒并退出 3，所以 `cf-new` 不能当别名，`cf_new` 可以。
- 每一段最多 64 个字符，超了同样退出 3。
- 派生出来的名字一律大写，所以 `cloudflare`/`new`/`api_token` 与 `CLOUDFLARE`/`NEW`/`API_TOKEN` 是同一个变量。

下划线在段内合法，所以一个名字无法按定界下划线反解出平台、别名和术语。规则只告诉你形状；名字本身来自 `assets list`，它给出每一条已声明变量派生出来的名字。

有两个术语带着固定的连通性含义：平台级的 `API_BASE_URL`（这个平台的 API 该指向哪里），账号级的 `PROXY`（这个账号该走哪个代理）。在 `list` 里看到它，就表示那条路径配了；没看到表示没配，而"没配"不等于"不知道"。

## 红线

- 不要打印、回显、记日志或粘贴值，也不要为了找一个值去 dump 环境（`env`、`printenv`、`Get-ChildItem Env:`）：一次 dump 会把所有凭证一起公开。在消费这个值的地方引用变量。
- 不要把按规则推出来的名字用在必须精确的地方。从 `assets list` 读；规则给你形状，`list` 给你事实。
- 不要去找把值放到命令行上的办法。那个选项不存在，而且那样给的值会落进进程列表和 shell 历史。
- 不要靠改注册表或数据文件绕过拒绝，也不要找删除命令。这里的拒绝是在保护一条它无法核对的声明，删除始终是人的动作。

## 错误

| 退出码 | 含义 | 动作 |
|---|---|---|
| 0 | 成功 | 读 stdout |
| 2 | 用法错误，或引用的平台、账号、变量没登记过 | 改正引用，或先把缺的平台、账号建出来；`list` 显示已有什么 |
| 3 | 某一段违反字符集或 64 字符上限，或值是空的 | 改参数再试 |
| 5 | 名字撞车，或注册表里有本工具没声明的同名变量 | 文案里点了名；换个名字，或交给人处理 |
| 6 | 值或整个环境块超预算 | 把值改短 |

上面这张表是常用路径。`references/errors.md` 里有封闭的完整集合（含退出码 4 与 7）、实际会遇到的文案，以及怎么区分"值得重试"和"重试也不会变"。
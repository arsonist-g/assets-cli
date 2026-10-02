# assets-cli

Windows 上的本地资产 / 凭证台账。

它把多个平台、多个账号的 API token、base URL、代理等值统一成有规则的用户级环境变量，同时让人和 AI 都能先知道“这台机器上有什么”，再按变量名取用。

- 给 AI：`assets` CLI + 常驻 skill，只暴露“查看、增加、修改”，不提供取值和删除命令。
- 给人：`assets-gui.exe` 图形界面，可增删改、管理平台图标、查看和恢复快照。
- 给 shell：启动钩子把台账中的变量注入新会话，变量名统一以 `ASSETS_CLI_` 开头。

> `assets-cli` 是本地台账，不是密钥保险箱、不是各平台官方 CLI 的替代品，也不会同步到云端。

## 工作方式

台账的层级是：

```text
平台（Platform）
├─ 平台级变量
└─ 账号条目（Account）
   └─ 账号级变量
```

变量名的形状固定为：

| 层级 | 命名规则 | 示例 |
|---|---|---|
| 平台级 | `ASSETS_CLI_<PLATFORM>_<TERM>` | `ASSETS_CLI_EXAMPLE_API_BASE_URL` |
| 账号级 | `ASSETS_CLI_<PLATFORM>_<ACCOUNT>_<TERM>` | `ASSETS_CLI_EXAMPLE_MAIN_API_TOKEN` |

平台名、账号别名和术语只允许 `A-Za-z0-9_`，每段最长 64 个字符；比较和派生变量名时统一转大写。

值的存储和配置分开：

| 数据 | 位置 |
|---|---|
| 变量值 | Windows 用户级注册表，默认 `HKCU\Environment` |
| 平台 / 账号 / 变量声明 | `%USERPROFILE%\.assets-cli\data.json` |
| 写入前快照 | `%USERPROFILE%\.assets-cli\snapshots\` |
| 平台图标 | `%USERPROFILE%\.assets-cli\icons\` |
| 导出钩子 | `%USERPROFILE%\.assets-cli\hydrate.ps1`、`emit-sh.ps1`、`hydrate.sh` |
| AI skill | `%USERPROFILE%\.claude\skills\assets\`、`%USERPROFILE%\.codex\skills\assets\`（只装进已存在的家族根） |

`data.json` 只保存名字和元数据，不保存凭证值。每次写入前都会先建快照；容量滚动时会保留最新快照。

## 安装

当前只提供 Windows 独立安装，不支持通过 npm 安装。npm 上的 `assets-cli` 不是这个项目。

### 方式一：下载 Release

从 [GitHub Releases](https://github.com/arsonist-g/assets-cli/releases) 下载最新的 Windows 压缩包，解压后将 `assets.exe` 和 `assets-gui.exe` 放到同一个用户级目录，例如：

```powershell
$dir = "$env:LOCALAPPDATA\Programs\assets-cli"
New-Item -ItemType Directory -Force $dir
Expand-Archive -LiteralPath .\assets-cli-v0.1.1-windows-x86_64.zip -DestinationPath $dir -Force

# 打开 Windows 的“编辑账户环境变量”，把 $dir 追加到用户 PATH
& "$dir\assets.exe" init
& "$dir\assets.exe" doctor
```

完成后新开一个终端，运行 `assets` 或双击 `assets-gui.exe`。

### 方式二：从源码安装

前置条件：

- Windows 10/11
- Rust stable，目标为 `x86_64-pc-windows-msvc`
- Visual Studio Build Tools（Rust MSVC 工具链需要 C++ 构建工具）

```powershell
git clone https://github.com/arsonist-g/assets-cli.git
cd assets-cli
cargo build --release

# 普通用户建议显式指定安装目录；脚本会复制两个 exe、配置 PATH、运行 init、创建快捷方式
powershell -ExecutionPolicy Bypass -File .\install.ps1 -SkipBuild -Target "$env:LOCALAPPDATA\Programs\assets-cli"
```

也可以使用仓库脚本：

```powershell
.\build.ps1
.\build.ps1 -Install
```

`install.ps1` 默认目标目录是开发机上的 `D:\Dev\Global\bin`；其他机器请传 `-Target`。卸载可使用 `-Uninstall`，它只删自己放进目标目录的两个 exe 和快捷方式，不删除台账、快照或 PATH 项。

## 快速开始

以下示例使用平台 `EXAMPLE`、账号 `MAIN`，请替换成自己的名字。

### PowerShell

```powershell
assets platform add EXAMPLE --note "示例平台"
assets account add --platform EXAMPLE --alias MAIN --purpose "示例账号"

# 凭证值只从 stdin 进，不要放进命令行参数
$TOKEN | assets var set --platform EXAMPLE --account MAIN --term API_TOKEN
$BASE_URL | assets var set --platform EXAMPLE --term API_BASE_URL

assets list --platform EXAMPLE
```

### Git Bash

```bash
assets platform add EXAMPLE --note "示例平台"
assets account add --platform EXAMPLE --alias MAIN --purpose "示例账号"

printf '%s' "$TOKEN" | assets var set --platform EXAMPLE --account MAIN --term API_TOKEN
printf '%s' "$BASE_URL" | assets var set --platform EXAMPLE --term API_BASE_URL

assets list --platform EXAMPLE
```

新开的 shell 会注入变量。使用时直接引用变量，不要先打印它：

```powershell
$env:ASSETS_CLI_EXAMPLE_MAIN_API_TOKEN
$env:ASSETS_CLI_EXAMPLE_API_BASE_URL
```

```bash
"$ASSETS_CLI_EXAMPLE_MAIN_API_TOKEN"
"$ASSETS_CLI_EXAMPLE_API_BASE_URL"
```

Windows 用户级环境变量对同一用户的所有进程可见，因此它保护的是“AI 不需要看到明文”这个使用约定，不是加密存储。

## CLI 命令

运行 `assets --help` 查看完整帮助。

| 命令 | 用途 |
|---|---|
| `assets list [--platform <名称>] [--account <别名>] [--json]` | 查看台账；不输出任何值 |
| `assets platform add <名称> [--note <文本>]` | 新增平台 |
| `assets platform rename <旧名> <新名>` | 平台改名，并级联重算其下变量名 |
| `assets account add --platform <名称> --alias <别名> [元数据]` | 新增账号或服务器条目 |
| `assets account edit --platform <名称> --alias <别名> [元数据]` | 修改非密元数据；空字符串可清空字段 |
| `assets account rename --platform <名称> --alias <旧别名> <新别名>` | 账号改名，并级联重算变量名 |
| `printf '%s' "$VALUE" \| assets var set --platform <名称> [--account <别名>] --term <术语>` | 新增或覆盖变量值 |
| `assets doctor [--json]` | 检查钩子、会话新鲜度、注册表和数据目录 |
| `assets init [--json]` | 安装或修复钩子、profile 标记块和 skill 包 |
| `assets skill install [--json]` | 只刷新 AI skill 包 |

`account add` / `edit` 支持 `--email`、`--purpose`、`--note`、`--host`、`--user`。

命令面刻意没有删除、取值、快照恢复和快照删除。删除与恢复只存在于 GUI，这是权限边界，不是尚未实现。

退出码：

| 退出码 | 含义 |
|---|---|
| `0` | 成功 |
| `2` | 用法错误，或引用的平台 / 账号 / 变量不存在 |
| `3` | 校验失败 |
| `4` | 存储失败 |
| `5` | 命名冲突 |
| `6` | 预算超限 |
| `7` | `doctor` 自检未通过 |

## 图形界面

`assets-gui.exe` 提供：

- 平台、账号、变量的树形台账和详情页
- 新增、编辑、改名和删除
- 平台图标：从网址获取、选择本地图片或移除
- 快照查看、恢复和删除
- 环境自检
- 值默认遮蔽

删除平台时会级联删除其账号和变量；删除或改名平台时，显示层图标会一并处理。图标不进入台账和历史快照，删除图标不会影响任何凭证。

## 安全边界

- CLI 不提供任何打印值的命令，`list` 和 JSON 输出也不包含值。
- `assets var set` 只从 stdin 读取值，不接受 `--value`，避免值进入进程列表和 shell 历史。
- 变量值存放在用户级环境变量中，任何同用户进程都可以读取；它不是加密保险箱。
- 不要把值写入日志、提交、脚本或聊天记录。
- 不要为了查值而 `env` / `printenv` / `Get-ChildItem Env:`；直接在消费凭证的命令中引用变量。
- shell 钩子只导出三段及以上的 `ASSETS_CLI_*` 变量，两个内部配置开关 `ASSETS_CLI_DATA`、`ASSETS_CLI_REGISTRY` 不会被注入资产会话。

`ASSETS_CLI_DATA` 和 `ASSETS_CLI_REGISTRY` 用于隔离开发与测试，日常使用不要设置。

## 仓库结构

```text
core/     台账模型、存储、快照、安装、自检、平台图标
cli/      assets.exe 命令面
gui/      assets-gui.exe，Slint 软件渲染
skill/    嵌入 CLI、可安装到 ~/.claude/skills/assets/ 与 ~/.codex/skills/assets/ 的 skill 包
```

## 开发与验证

```powershell
cargo fmt --all -- --check
cargo test --all
cargo build --release
```

应用图标母版是 `gui/assets/assets-gui-source.png`；更新母版后运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\gui\assets\make-icon.ps1
```

## 已知边界

- 仅支持 Windows；WSL 不会自动继承 Windows 任意用户环境变量。
- 当前不发布 npm 包；GUI 和 CLI 都通过 GitHub Releases 或源码安装。
- 不提供云同步、团队共享、审计日志或加密保险箱能力。
- `assets` 只保存 base URL 和凭证变量，不判断某平台应该调用哪个 API，也不验证凭证是否仍然有效。


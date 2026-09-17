# 安装、修复与配置

新开的 shell 里变量读出为空、`assets doctor` 报出未通过的检查、或这套东西需要安装或修复时读这份。`SKILL.md` 里的常用路径永远用不到它。

## 首次运行

二进制由人安装，`assets` 已在 PATH 上；第一条命令之前没有要配置的东西，因为工具直接读写注册表。

`assets init` 把下面三部分一次装齐，而且幂等：再跑一次会把每个文件走回期望状态，并逐个文件报一种结果，`新建`、`已存在` 或 `已修复`。

`assets skill install [--json]` 只装第三部分（skill 包），报告方式相同。钩子已经在位、只想单独刷新这份包时用它。

| 步骤 | 落到哪 | 干什么 |
|---|---|---|
| 钩子 | `~/.assets-cli/hydrate.ps1`、`~/.assets-cli/emit-sh.ps1`、`~/.assets-cli/hydrate.sh` | shell 启动时读注册表，导出 `ASSETS_CLI_*` 变量 |
| profile 标记块 | PowerShell profile、`~/.bashrc`，以及 Git Bash 的登录 profile（`.bash_profile`、`.bash_login` 或 `.profile`） | 调用钩子，内容是 `# >>> assets-cli >>>` 标记块里的一行 |
| skill | `~/.agents/skills/assets/` | 这份 skill 包 |

- 两条命令都不碰 PATH。二进制放在哪、怎么进的 PATH，属于人自己的安装步骤，不属于这个工具。
- 内容过旧的标记块会被整块替换，所以写进块里的改动会在下次运行时丢掉。
- 钩子只导出匹配 `^ASSETS_CLI_[^_]+_.+$` 的名字，也就是三段及以上。像下面那种单段开关永远进不了会话。
- 这一步的退出码是 0，或在注册表、数据目录写不动时是 4。

## 自检

`assets doctor [--json]` 逐项报一行，每项是 `ok` 或 `未通过`，并给出该怎么修。它什么都不修。

| 检查 | 什么时候通过 | 不通过意味着 |
|---|---|---|
| `hook_installed` | 钩子脚本在位，且两个 shell profile 都带标记块 | 跑 `assets init` |
| `session_fresh` | 取样几个已声明变量与注册表一致，比哈希、绝不打印 | 你所在的那个 shell 启动早于最后一次写入，环境是旧的；开一个新 shell |
| `registry_writable` | 用户注册表键能以写权限打开，只打开、绝不写入 | 当前进程不是拥有这份台账的那个用户 |
| `data_dir_writable` | 数据目录能创建、写入、清理 | 磁盘或权限问题 |

退出 0 表示每项都过，退出 7 表示至少一项没过。还没有声明任何变量时 `session_fresh` 报 ok，因为没有可比对的东西。

## 配置开关

两个环境变量用来改落点。它们是为隔离的开发与测试跑准备的，正常使用时人不设它们。

| 开关 | 默认 | 含义 |
|---|---|---|
| `ASSETS_CLI_DATA` | `~/.assets-cli` | 数据目录：声明，以及每次写入前建的快照。开头的 `~` 会展开 |
| `ASSETS_CLI_REGISTRY` | `HKCU\Environment` | 存值的注册表键，写成 `<hive>\<subkey>` |

两个都设上的一次运行只写那个键和那个目录，测试就是这样够到台账而不碰真实那一份的。
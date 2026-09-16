//! 安装与修复：钩子脚本 → profile 标记块 → skill 描述。三步各自幂等，重复执行只是回到期望状态。

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::paths::Layout;

/// 要安装的钩子脚本（Git Bash 侧两个：取值 + eval）。
pub const HOOK_SCRIPTS: &[&str] = &["hydrate.ps1", "emit-sh.ps1", "hydrate.sh"];

/// 标记块的边界。
pub const MARKER_BEGIN: &str = "# >>> assets-cli >>>";
pub const MARKER_END: &str = "# <<< assets-cli <<<";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepResult {
    Created,
    Existing,
    Repaired,
}

impl StepResult {
    pub fn label(self) -> &'static str {
        match self {
            StepResult::Created => "新建",
            StepResult::Existing => "已存在",
            StepResult::Repaired => "已修复",
        }
    }

    /// 一个步骤里含多个文件时，取最"重"的那个结果报告。
    fn merge(self, other: StepResult) -> StepResult {
        use StepResult::*;
        match (self, other) {
            (Repaired, _) | (_, Repaired) => Repaired,
            (Created, _) | (_, Created) => Created,
            _ => Existing,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub title: &'static str,
    pub result: StepResult,
    /// 每个落点一行：`<路径>（<结果>）`。
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub steps: Vec<Step>,
}

pub fn run(layout: &Layout) -> Result<Report> {
    Ok(Report {
        steps: vec![
            install_hook_scripts(layout)?,
            install_profile_blocks(layout)?,
            install_skill(layout)?,
        ],
    })
}

pub fn render_markdown(report: &Report) -> String {
    let mut out = String::from("# 安装与修复\n\n| 步骤 | 结果 | 明细 |\n|---|---|---|\n");
    for step in &report.steps {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            step.title,
            step.result.label(),
            step.detail.join("；")
        ));
    }
    out
}

pub fn render_json(report: &Report) -> Result<String> {
    serde_json::to_string_pretty(report)
        .map_err(|e| CoreError::storage_from("安装结果序列化失败", e))
}

fn install_hook_scripts(layout: &Layout) -> Result<Step> {
    let mut result = StepResult::Existing;
    let mut detail = Vec::new();
    for name in HOOK_SCRIPTS {
        let path = layout.hook_dir.join(name);
        let outcome = write_file(&path, hook_script(name))?;
        result = result.merge(outcome);
        detail.push(format!("{}（{}）", path.display(), outcome.label()));
    }
    Ok(Step {
        id: "hook_scripts",
        title: "钩子脚本",
        result,
        detail,
    })
}

fn install_profile_blocks(layout: &Layout) -> Result<Step> {
    let mut result = StepResult::Existing;
    let mut detail = Vec::new();

    let ps_outcome = upsert_block(&layout.ps_profile, &ps_block())?;
    result = result.merge(ps_outcome);
    detail.push(format!(
        "{}（{}）",
        layout.ps_profile.display(),
        ps_outcome.label()
    ));

    for profile in layout.bash_profiles() {
        let outcome = upsert_block(&profile, &bash_block())?;
        result = result.merge(outcome);
        detail.push(format!("{}（{}）", profile.display(), outcome.label()));
    }

    Ok(Step {
        id: "profile_blocks",
        title: "profile 标记块",
        result,
        detail,
    })
}

fn install_skill(layout: &Layout) -> Result<Step> {
    let outcome = write_file(&layout.skill_file, SKILL_CONTENT)?;
    Ok(Step {
        id: "skill",
        title: "skill 描述",
        result: outcome,
        detail: vec![format!(
            "{}（{}）",
            layout.skill_file.display(),
            outcome.label()
        )],
    })
}

/// 内容一致 = 已存在；不一致 = 整块替换。
fn write_file(path: &Path, content: &str) -> Result<StepResult> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            CoreError::storage_from(&format!("目录创建失败（{}）", dir.display()), e)
        })?;
    }
    match fs::read_to_string(path) {
        Ok(existing) if existing == content => Ok(StepResult::Existing),
        Ok(_) => {
            fs::write(path, content).map_err(|e| {
                CoreError::storage_from(&format!("文件写入失败（{}）", path.display()), e)
            })?;
            Ok(StepResult::Repaired)
        }
        Err(_) => {
            fs::write(path, content).map_err(|e| {
                CoreError::storage_from(&format!("文件写入失败（{}）", path.display()), e)
            })?;
            Ok(StepResult::Created)
        }
    }
}

/// 幂等写入标记块：没有就追加，有且一致就不动，有但过旧就整块替换。
fn upsert_block(path: &Path, block: &str) -> Result<StepResult> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let updated = match find_block(&existing) {
        Some((start, end)) => {
            if existing[start..end].trim_end() == block.trim_end() {
                return Ok(StepResult::Existing);
            }
            format!("{}{}{}", &existing[..start], block, &existing[end..])
        }
        None => {
            let mut merged = existing.clone();
            if !merged.is_empty() {
                if !merged.ends_with('\n') {
                    merged.push('\n');
                }
                merged.push('\n');
            }
            merged.push_str(block);
            merged
        }
    };
    let created = existing.is_empty();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            CoreError::storage_from(&format!("目录创建失败（{}）", dir.display()), e)
        })?;
    }
    fs::write(path, updated)
        .map_err(|e| CoreError::storage_from(&format!("文件写入失败（{}）", path.display()), e))?;
    Ok(if created {
        StepResult::Created
    } else {
        StepResult::Repaired
    })
}

/// 找标记块的字节区间（含结束标记那一行的换行）。
fn find_block(text: &str) -> Option<(usize, usize)> {
    let hit = text.find(MARKER_BEGIN)?;
    let start = text[..hit].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end_marker = text[start..].find(MARKER_END)? + start;
    let end = text[end_marker..]
        .find('\n')
        .map(|i| end_marker + i + 1)
        .unwrap_or(text.len());
    Some((start, end))
}

/// 块内只有一行调用（改这块内容等于改变安装期望，所以写成常量而非模板）。
fn ps_block() -> String {
    format!(
        "{MARKER_BEGIN}\nif (Test-Path \"$HOME/.assets-cli/hydrate.ps1\") {{ . \"$HOME/.assets-cli/hydrate.ps1\" }}\n{MARKER_END}\n"
    )
}

fn bash_block() -> String {
    format!(
        "{MARKER_BEGIN}\n[ -f \"$HOME/.assets-cli/hydrate.sh\" ] && . \"$HOME/.assets-cli/hydrate.sh\"\n{MARKER_END}\n"
    )
}

/// 钩子脚本内容。只导出「≥3 段」的资产变量 —— 同前缀的单段覆写开关不会被导出。
fn hook_script(name: &str) -> &'static str {
    match name {
        "hydrate.ps1" => HYDRATE_PS1,
        "emit-sh.ps1" => EMIT_SH_PS1,
        _ => HYDRATE_SH,
    }
}

const HYDRATE_PS1: &str = r#"# assets-cli 启动钩子（PowerShell）：把用户级 ASSETS_CLI_* 资产变量注入当前会话。
# 由 assets init 安装；内容会在下次 init 时被覆盖，不要手工改。
$ErrorActionPreference = 'SilentlyContinue'
$userEnv = [System.Environment]::GetEnvironmentVariables('User')
foreach ($name in $userEnv.Keys) {
    if ($name -match '^ASSETS_CLI_[^_]+_.+$') {
        [System.Environment]::SetEnvironmentVariable([string]$name, [string]$userEnv[$name], 'Process')
    }
}
"#;

const EMIT_SH_PS1: &str = r#"# assets-cli 启动钩子（Git Bash 侧取值）：把 ASSETS_CLI_* 资产变量输出成 export 行。
# 由 hydrate.sh 经 eval 读入，值只走管道、不落到控制台。由 assets init 安装。
$ErrorActionPreference = 'SilentlyContinue'
$userEnv = [System.Environment]::GetEnvironmentVariables('User')
foreach ($name in $userEnv.Keys) {
    if ($name -match '^ASSETS_CLI_[^_]+_.+$') {
        $value = [string]$userEnv[$name]
        "export {0}='{1}'" -f $name, $value.Replace("'", "'\''")
    }
}
"#;

const HYDRATE_SH: &str = r#"# assets-cli 启动钩子（Git Bash）：经 emit-sh.ps1 取值并 eval 进当前 shell。
# 由 assets init 安装；内容会在下次 init 时被覆盖，不要手工改。
if command -v powershell.exe >/dev/null 2>&1 && [ -f "$HOME/.assets-cli/emit-sh.ps1" ]; then
    eval "$(powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$HOME/.assets-cli/emit-sh.ps1" 2>/dev/null)"
fi
"#;

const SKILL_CONTENT: &str = r#"---
name: assets
description: 本机登记的凭证资产台账（多平台 / 多账号，平台由用户自定义）。当任务需要某个平台的 API 令牌、代理或接口地址时用它：变量名形如 ASSETS_CLI_<平台>_<账号别名>_<术语>（平台级为 ASSETS_CLI_<平台>_<术语>）；先跑 `assets list` 看有什么，值已在当前会话的环境里，直接引用 $VAR。
---

# assets：本机资产台账

## 先看清单

- `assets list` 是清单的唯一真相：只出变量名与元数据，**不出值**。
- `assets list --platform <名称>` 只看一个平台；再加 `--account <别名>` 只看一个条目（必须与 `--platform` 同用）。
- 退出码 **2 = 该平台 / 条目没登记过**，0 = 登记过（哪怕一个账号都没有）。判断"我有没有这个平台的资产"就看退出码。

## 命名规则（变量名不手写）

- 账号级：`ASSETS_CLI_<平台>_<账号别名>_<术语>`；平台级：`ASSETS_CLI_<平台>_<术语>`。
- 平台名 / 别名 / 术语：只允许 `A-Z a-z 0-9 _`，各自不超过 64 字符；输出一律大写。
- 连通性的固定术语：账号级 `PROXY`（代理）、平台级 `API_BASE_URL`（接口地址）。没有声明就是没有配置。
- 拿不准变量名时先 `assets list` 确认，不要凭猜。

## 用值

- 值已经在会话环境里，直接引用，例如 `curl -H "Authorization: Bearer $ASSETS_CLI_GITHUB_WORK_API_TOKEN" ...`。
- **禁止打印或回显值**：不要 `echo $VAR`、不要 `printenv` / `env` 全量 dump、不要把值写进文件、日志、提交信息或回复里。
- 值的时新性由 shell 启动钩子保证：人刚改过的值，你的下一条命令就是新的，不需要重启任何东西。

## 改台账（可增可改，不可删）

- 写值：`printf '%s' "$VALUE" | assets var set --platform <名称> [--account <别名>] --term <术语>`（值只从 stdin 进）。
- 新增：`assets platform add <名称>`；`assets account add --platform <名称> --alias <别名> [--email ... --purpose ... --note ... --host ... --user ...]`。
- 修改：`assets account edit`（同一批选项，只改给出的字段，给空串即清空）；`assets platform rename <旧名> <新名>`、`assets account rename --platform <名称> --alias <旧别名> <新别名>`（级联重算变量名）。
- **删除不是你的动作**：CLI 里没有删除平台、删除条目、删除声明、恢复快照的命令 —— 那是人自己的操作，不要试图绕过。

## 自检

- 值看着不对、或怀疑钩子失效时跑 `assets doctor`：退出码 7 = 有检查没过（钩子缺失就跑 `assets init`，会话不新鲜就重开 shell）。
- 退出码表：0 成功 / 2 用法或引用不存在 / 3 校验失败 / 4 存储失败 / 5 冲突 / 6 预算超限 / 7 自检未通过。
"#;

//! 自检：四项检查，**只报不修**。
//!
//! 钩子是否在位、当前会话的值是否与注册表一致、注册表可写、数据目录可写。
//! 任一项不过 → 退出码 7；连前提都拿不到（如注册表打不开）也算不过，并在该项说明原因。

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::init::{MARKER_BEGIN, MARKER_END};
use crate::model::DeclaredVariable;
use crate::ops::Ctx;
use crate::paths::Layout;
use crate::store;

/// 取样比对的变量个数（只比哈希，不打印任何值）。
const SAMPLE: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ok: bool,
    pub checks: Vec<Check>,
}

/// 跑四项检查，不做任何修改。
pub fn run(ctx: &Ctx, layout: &Layout) -> Report {
    let checks = vec![
        hook_installed(layout),
        session_fresh(ctx),
        registry_writable(ctx),
        data_dir_writable(ctx),
    ];
    let ok = checks.iter().all(|check| check.ok);
    Report { ok, checks }
}

pub fn render_markdown(report: &Report) -> String {
    let passed = report.checks.iter().filter(|c| c.ok).count();
    let mut out = format!(
        "# 自检 · {}/{} 项通过\n\n| 检查 | 结果 | 说明 |\n|---|---|---|\n",
        passed,
        report.checks.len()
    );
    for check in &report.checks {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            check.id,
            if check.ok { "ok" } else { "未通过" },
            check.detail
        ));
    }
    out
}

pub fn render_json(report: &Report) -> String {
    match serde_json::to_string_pretty(report) {
        Ok(text) => text,
        Err(err) => format!("{{\"error\":\"{err}\"}}"),
    }
}

fn hook_installed(layout: &Layout) -> Check {
    let mut problems = Vec::new();
    for script in layout.hook_scripts() {
        if !script.is_file() {
            problems.push(format!("钩子脚本缺失：{}", script.display()));
        }
    }
    if !has_block(&layout.ps_profile) {
        problems.push(format!(
            "PowerShell profile 里没有标记块：{}",
            layout.ps_profile.display()
        ));
    }
    for profile in layout.bash_profiles() {
        if !has_block(&profile) {
            problems.push(format!(
                "Git Bash profile 里没有标记块：{}",
                profile.display()
            ));
        }
    }
    if problems.is_empty() {
        Check {
            id: "hook_installed",
            ok: true,
            detail: "钩子脚本与两个 shell 的标记块都在位".to_string(),
        }
    } else {
        Check {
            id: "hook_installed",
            ok: false,
            detail: format!("{}；跑 assets init 修复", problems.join("；")),
        }
    }
}

fn session_fresh(ctx: &Ctx) -> Check {
    let id = "session_fresh";
    let doc = match store::load(&ctx.paths) {
        Ok(doc) => doc,
        Err(err) => {
            return Check {
                id,
                ok: false,
                detail: format!("台账不可读，无法取样比对：{err}"),
            }
        }
    };
    let declared = match doc.declarations() {
        Ok(declared) => declared,
        Err(err) => {
            return Check {
                id,
                ok: false,
                detail: format!("台账里有无法派生的变量名：{err}"),
            }
        }
    };
    if declared.is_empty() {
        return Check {
            id,
            ok: true,
            detail: "台账里还没有已声明的变量，无可比对项".to_string(),
        };
    }

    let sample: Vec<&DeclaredVariable> = declared.iter().take(SAMPLE).collect();
    let mut stale = Vec::new();
    let mut troubles = Vec::new();
    for decl in &sample {
        let in_registry = match ctx.registry.get(&decl.name) {
            Ok(value) => value,
            Err(err) => {
                troubles.push(format!("{}（{err}）", decl.name));
                continue;
            }
        };
        let in_session = std::env::var(&decl.name).ok();
        if fingerprint(in_registry.as_deref()) != fingerprint(in_session.as_deref()) {
            stale.push(decl.name.clone());
        }
    }

    if !troubles.is_empty() {
        return Check {
            id,
            ok: false,
            detail: format!("取样的变量读不到注册表值：{}", troubles.join("；")),
        };
    }
    if stale.is_empty() {
        Check {
            id,
            ok: true,
            detail: format!(
                "取样的 {} 个变量与注册表一致（当前会话是最新的）",
                sample.len()
            ),
        }
    } else {
        Check {
            id,
            ok: false,
            detail: format!(
                "当前会话里有 {} 个取样变量不是最新：{}；重开 shell 即可刷新",
                stale.len(),
                stale.join(", ")
            ),
        }
    }
}

fn registry_writable(ctx: &Ctx) -> Check {
    let id = "registry_writable";
    match ctx.registry.writable() {
        Ok(()) => Check {
            id,
            ok: true,
            detail: format!("{} 可写", ctx.registry.spec().raw()),
        },
        Err(err) => Check {
            id,
            ok: false,
            detail: format!("{err}；确认以当前用户身份运行"),
        },
    }
}

fn data_dir_writable(ctx: &Ctx) -> Check {
    let id = "data_dir_writable";
    let dir = &ctx.paths.data_dir;
    if let Err(err) = fs::create_dir_all(dir) {
        return Check {
            id,
            ok: false,
            detail: format!("数据目录创建失败（{}）：{err}", dir.display()),
        };
    }
    let probe = dir.join("write-probe.tmp");
    let result = fs::File::create(&probe).and_then(|mut file| file.write_all(b"ok"));
    let _ = fs::remove_file(&probe);
    match result {
        Ok(()) => Check {
            id,
            ok: true,
            detail: format!("数据目录可写：{}", dir.display()),
        },
        Err(err) => Check {
            id,
            ok: false,
            detail: format!("数据目录不可写（{}）：{err}", dir.display()),
        },
    }
}

fn has_block(path: &Path) -> bool {
    match fs::read_to_string(path) {
        Ok(text) => text.contains(MARKER_BEGIN) && text.contains(MARKER_END),
        Err(_) => false,
    }
}

/// 只留下指纹，用于比对"会话里的值"与"注册表里的值"是否同一个，不打印也不留痕。
fn fingerprint(value: Option<&str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

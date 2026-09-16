//! 清单产出：默认 Markdown，`--json` 可选。
//!
//! **永不输出值** —— 只有变量名与元数据。排序固定：平台名 → 别名 → 术语，大写归一化升序。

use serde::Serialize;

use crate::error::{CoreError, Result};
use crate::model::{Account, DataDoc, Platform};
use crate::naming;

/// 过滤条件：`--account` 必须与 `--platform` 同用。
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub platform: Option<String>,
    pub account: Option<String>,
}

/// 输出形态（Markdown 与 JSON 共用同一份视图）。
#[derive(Debug, Clone, Serialize)]
pub struct ListView {
    pub platforms: Vec<PlatformView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformView {
    pub name: String,
    pub note: Option<String>,
    pub variables: Vec<VarView>,
    pub accounts: Vec<AccountView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VarView {
    pub term: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub alias: String,
    pub email: Option<String>,
    pub purpose: Option<String>,
    pub note: Option<String>,
    pub host: Option<String>,
    pub user: Option<String>,
    pub variables: Vec<VarView>,
}

impl Filter {
    /// 过滤条件自检：引用的平台 / 条目不存在 → 退出码 2。
    pub fn validate(&self) -> Result<()> {
        if self.account.is_some() && self.platform.is_none() {
            return Err(CoreError::usage("--account 必须与 --platform 同用"));
        }
        Ok(())
    }
}

/// 按过滤条件取视图（同时完成排序与"引用是否存在"的判定）。
pub fn view(doc: &DataDoc, filter: &Filter) -> Result<ListView> {
    filter.validate()?;
    let mut platforms: Vec<&Platform> = match &filter.platform {
        Some(name) => {
            let platform = doc.find_platform(name).ok_or_else(|| {
                CoreError::usage(format!(
                    "平台不存在：{name}（用 assets list 看已登记的平台）"
                ))
            })?;
            vec![platform]
        }
        None => doc.platforms.iter().collect(),
    };
    platforms.sort_by_key(|p| naming::normalize(&p.name));

    let mut out = Vec::with_capacity(platforms.len());
    for platform in platforms {
        let mut variables = Vec::with_capacity(platform.variables.len());
        for decl in &platform.variables {
            variables.push(VarView {
                term: decl.term.clone(),
                name: naming::platform_var_name(&platform.name, &decl.term)?,
            });
        }
        variables.sort_by(|a, b| naming::normalize(&a.term).cmp(&naming::normalize(&b.term)));

        let mut accounts: Vec<&Account> = match &filter.account {
            Some(alias) => {
                let account = platform.find_account(alias).ok_or_else(|| {
                    CoreError::usage(format!("平台 {} 下没有条目：{alias}", platform.name))
                })?;
                vec![account]
            }
            None => platform.accounts.iter().collect(),
        };
        accounts.sort_by_key(|a| naming::normalize(&a.alias));

        let mut account_views = Vec::with_capacity(accounts.len());
        for account in accounts {
            let mut variables = Vec::with_capacity(account.variables.len());
            for decl in &account.variables {
                variables.push(VarView {
                    term: decl.term.clone(),
                    name: naming::account_var_name(&platform.name, &account.alias, &decl.term)?,
                });
            }
            variables.sort_by(|a, b| naming::normalize(&a.term).cmp(&naming::normalize(&b.term)));
            account_views.push(AccountView {
                alias: account.alias.clone(),
                email: account.email.clone(),
                purpose: account.purpose.clone(),
                note: account.note.clone(),
                host: account.host.clone(),
                user: account.user.clone(),
                variables,
            });
        }

        out.push(PlatformView {
            name: platform.name.clone(),
            note: platform.note.clone(),
            variables,
            accounts: account_views,
        });
    }
    Ok(ListView { platforms: out })
}

/// Markdown 清单（AI 的主要读物）。
pub fn render_markdown(view: &ListView) -> String {
    let accounts: usize = view.platforms.iter().map(|p| p.accounts.len()).sum();
    let mut out = format!(
        "# 资产清单 · {} 个平台 / {} 个账号\n",
        view.platforms.len(),
        accounts
    );
    for platform in &view.platforms {
        out.push_str(&format!("\n## {}\n", platform.name));
        if let Some(note) = &platform.note {
            out.push_str(&format!("备注：{note}\n"));
        }
        if !platform.variables.is_empty() {
            let names: Vec<&str> = platform.variables.iter().map(|v| v.name.as_str()).collect();
            out.push_str(&format!("平台变量：{}\n", names.join(", ")));
        }
        if platform.accounts.is_empty() {
            out.push_str("（该平台暂无账号条目）\n");
            continue;
        }
        out.push_str("\n| 账号 | 变量 | 元数据 |\n|---|---|---|\n");
        for account in &platform.accounts {
            let names: Vec<&str> = account.variables.iter().map(|v| v.name.as_str()).collect();
            let variables = if names.is_empty() {
                "—".to_string()
            } else {
                names.join(", ")
            };
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                account.alias,
                variables,
                meta_summary(account)
            ));
        }
    }
    out
}

/// JSON 清单（给真有程序要解析的场合）。
pub fn render_json(view: &ListView) -> Result<String> {
    serde_json::to_string_pretty(view).map_err(|e| CoreError::storage_from("清单序列化失败", e))
}

fn meta_summary(account: &AccountView) -> String {
    let fields = [
        ("purpose", &account.purpose),
        ("email", &account.email),
        ("note", &account.note),
        ("host", &account.host),
        ("user", &account.user),
    ];
    let parts: Vec<String> = fields
        .into_iter()
        .filter_map(|(kind, value)| value.as_ref().map(|v| format!("{kind}={v}")))
        .collect();
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

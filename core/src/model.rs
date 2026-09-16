//! 数据模型：文档形态的元数据（值不住在这里）+ 快照。
//!
//! 变量名不落盘 —— 它由 (平台, 别名?, 术语) 现算（见 `naming`）。

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::naming;

/// 元数据文档版本；读到更高版本即拒绝启动并提示升级。
pub const DOC_VERSION: u32 = 1;

/// 当前时刻（ISO 8601 UTC）。
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataDoc {
    pub version: u32,
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Platform {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub variables: Vec<VarDecl>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub alias: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub variables: Vec<VarDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarDecl {
    pub term: String,
}

/// 一条已声明的变量（名字现算），清单、自检、预算都基于它。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredVariable {
    pub platform: String,
    pub account: Option<String>,
    pub term: String,
    pub name: String,
}

impl DataDoc {
    pub fn empty() -> DataDoc {
        DataDoc {
            version: DOC_VERSION,
            platforms: Vec::new(),
        }
    }

    pub fn find_platform(&self, name: &str) -> Option<&Platform> {
        let key = naming::normalize(name);
        self.platforms
            .iter()
            .find(|p| naming::normalize(&p.name) == key)
    }

    pub fn find_platform_mut(&mut self, name: &str) -> Option<&mut Platform> {
        let key = naming::normalize(name);
        self.platforms
            .iter_mut()
            .find(|p| naming::normalize(&p.name) == key)
    }

    pub fn has_platform(&self, name: &str) -> bool {
        self.find_platform(name).is_some()
    }

    /// 全部已声明变量，按变量名升序。
    pub fn declarations(&self) -> Result<Vec<DeclaredVariable>> {
        let mut out = Vec::new();
        for platform in &self.platforms {
            for decl in &platform.variables {
                out.push(DeclaredVariable {
                    platform: platform.name.clone(),
                    account: None,
                    term: decl.term.clone(),
                    name: naming::platform_var_name(&platform.name, &decl.term)?,
                });
            }
            for account in &platform.accounts {
                for decl in &account.variables {
                    out.push(DeclaredVariable {
                        platform: platform.name.clone(),
                        account: Some(account.alias.clone()),
                        term: decl.term.clone(),
                        name: naming::account_var_name(&platform.name, &account.alias, &decl.term)?,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// 某个平台的平台级变量名，按名字升序。
    pub fn platform_var_names(&self, platform: &Platform) -> Result<Vec<String>> {
        let mut names = Vec::with_capacity(platform.variables.len());
        for decl in &platform.variables {
            names.push(naming::platform_var_name(&platform.name, &decl.term)?);
        }
        names.sort();
        Ok(names)
    }

    /// 某个账号的账号级变量名，按名字升序。
    pub fn account_var_names(&self, platform: &Platform, account: &Account) -> Result<Vec<String>> {
        let mut names = Vec::with_capacity(account.variables.len());
        for decl in &account.variables {
            names.push(naming::account_var_name(
                &platform.name,
                &account.alias,
                &decl.term,
            )?);
        }
        names.sort();
        Ok(names)
    }

    /// 落盘前统一排序：平台名 → 别名 → 术语，一律大写归一化升序。
    pub fn sort(&mut self) {
        self.platforms.sort_by_key(|p| naming::normalize(&p.name));
        for platform in &mut self.platforms {
            platform
                .variables
                .sort_by_key(|v| naming::normalize(&v.term));
            platform
                .accounts
                .sort_by_key(|a| naming::normalize(&a.alias));
            for account in &mut platform.accounts {
                account
                    .variables
                    .sort_by_key(|v| naming::normalize(&v.term));
            }
        }
    }
}

impl Platform {
    pub fn find_account(&self, alias: &str) -> Option<&Account> {
        let key = naming::normalize(alias);
        self.accounts
            .iter()
            .find(|a| naming::normalize(&a.alias) == key)
    }

    pub fn find_account_mut(&mut self, alias: &str) -> Option<&mut Account> {
        let key = naming::normalize(alias);
        self.accounts
            .iter_mut()
            .find(|a| naming::normalize(&a.alias) == key)
    }

    pub fn has_account(&self, alias: &str) -> bool {
        self.find_account(alias).is_some()
    }

    pub fn has_term(&self, term: &str) -> bool {
        let key = naming::normalize(term);
        self.variables
            .iter()
            .any(|v| naming::normalize(&v.term) == key)
    }
}

impl Account {
    pub fn has_term(&self, term: &str) -> bool {
        let key = naming::normalize(term);
        self.variables
            .iter()
            .any(|v| naming::normalize(&v.term) == key)
    }
}

/// 快照：某一时刻的全量（元数据 + 全部变量名与值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub created_at: String,
    pub data: DataDoc,
    /// 含明文值 —— 快照与注册表同级敏感，只能落在用户目录。
    pub variables: Vec<SnapshotVar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotVar {
    pub name: String,
    pub value: String,
}

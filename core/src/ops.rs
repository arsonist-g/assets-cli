//! 写入路径：事务、级联、预算。
//!
//! 一次写入 = 建快照 → 应用注册表改动 → 落盘元数据；任一步失败整体回退，不留半途状态。

use std::collections::HashSet;

use crate::error::{CoreError, Result};
use crate::model::{now_iso, Account, DataDoc, Platform, VarDecl};
use crate::naming::{self, ENV_BLOCK_BUDGET, ENV_BLOCK_WARN_AT};
use crate::paths::Paths;
use crate::registry::{self, Registry};
use crate::snapshot;
use crate::store;

/// 引擎上下文：两个落点 + 注册表句柄。
#[derive(Debug, Clone)]
pub struct Ctx {
    pub paths: Paths,
    pub registry: Registry,
}

impl Ctx {
    pub fn new(paths: Paths) -> Ctx {
        let registry = Registry::new(paths.registry.clone());
        Ctx { paths, registry }
    }

    /// 默认值即生产；开发 / 测试用开关显式指定落点。
    pub fn from_env() -> Result<Ctx> {
        Ok(Ctx::new(Paths::from_env()?))
    }
}

/// 条目元数据的改动意图：`None` = 不动，`Some("")` = 清空，`Some(v)` = 设为 v。
#[derive(Debug, Clone, Default)]
pub struct MetaPatch {
    pub email: Option<String>,
    pub purpose: Option<String>,
    pub note: Option<String>,
    pub host: Option<String>,
    pub user: Option<String>,
}

impl MetaPatch {
    pub fn is_empty(&self) -> bool {
        self.email.is_none()
            && self.purpose.is_none()
            && self.note.is_none()
            && self.host.is_none()
            && self.user.is_none()
    }

    /// 本次真正改到的字段名（按固定顺序）。
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (name, value) in [
            ("email", &self.email),
            ("purpose", &self.purpose),
            ("note", &self.note),
            ("host", &self.host),
            ("user", &self.user),
        ] {
            if value.is_some() {
                out.push(name);
            }
        }
        out
    }

    pub fn validate(&self) -> Result<()> {
        for (kind, value) in [
            ("邮箱", &self.email),
            ("用途", &self.purpose),
            ("备注", &self.note),
            ("host", &self.host),
            ("user", &self.user),
        ] {
            if let Some(text) = value {
                naming::validate_meta(kind, text)?;
            }
        }
        Ok(())
    }

    fn apply_to(&self, account: &mut Account) {
        if let Some(value) = &self.email {
            account.email = clean(Some(value.clone()));
        }
        if let Some(value) = &self.purpose {
            account.purpose = clean(Some(value.clone()));
        }
        if let Some(value) = &self.note {
            account.note = clean(Some(value.clone()));
        }
        if let Some(value) = &self.host {
            account.host = clean(Some(value.clone()));
        }
        if let Some(value) = &self.user {
            account.user = clean(Some(value.clone()));
        }
    }
}

/// 注册表改动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegOp {
    Set { name: String, value: String },
    Delete { name: String },
}

impl RegOp {
    fn name(&self) -> &str {
        match self {
            RegOp::Set { name, .. } | RegOp::Delete { name } => name,
        }
    }
}

/// 新增平台。
pub fn platform_add(ctx: &Ctx, name: &str, note: Option<String>) -> Result<String> {
    naming::validate_segment("平台名", name)?;
    let note = clean(note);
    if let Some(note) = &note {
        naming::validate_meta("平台备注", note)?;
    }
    let before = store::load(&ctx.paths)?;
    if let Some(existing) = before.find_platform(name) {
        return Err(CoreError::conflict(format!(
            "平台已存在：{}（重名检测不区分大小写）",
            existing.name
        )));
    }
    let mut after = before.clone();
    after.platforms.push(Platform {
        name: name.to_string(),
        note,
        created_at: now_iso(),
        variables: Vec::new(),
        accounts: Vec::new(),
    });
    commit(ctx, &before, &after, &[])?;
    Ok(format!(
        "已新增平台 {name}（变量名前缀 {}{}_）",
        naming::PREFIX,
        naming::normalize(name)
    ))
}

/// 平台改名：连带平台级变量 + 该平台全部账号级变量。
pub fn platform_rename(ctx: &Ctx, old: &str, new: &str) -> Result<String> {
    naming::validate_segment("平台名", new)?;
    let before = store::load(&ctx.paths)?;
    let old_name = before
        .find_platform(old)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{old}")))?
        .name
        .clone();
    if naming::normalize(&old_name) != naming::normalize(new) && before.has_platform(new) {
        return Err(CoreError::conflict(format!(
            "平台已存在：{new}（重名检测不区分大小写）"
        )));
    }

    let platform = before.find_platform(old).expect("平台刚查到");
    let mut moves: Vec<(String, String)> = Vec::new();
    for decl in &platform.variables {
        moves.push((
            naming::platform_var_name(&old_name, &decl.term)?,
            naming::platform_var_name(new, &decl.term)?,
        ));
    }
    let account_count = platform.accounts.len();
    for account in &platform.accounts {
        for decl in &account.variables {
            moves.push((
                naming::account_var_name(&old_name, &account.alias, &decl.term)?,
                naming::account_var_name(new, &account.alias, &decl.term)?,
            ));
        }
    }

    let ops = rename_ops(&ctx.registry, &moves)?;
    let mut after = before.clone();
    after.find_platform_mut(old).expect("平台刚查到").name = new.to_string();
    commit(ctx, &before, &after, &ops)?;
    Ok(format!(
        "已重命名 {old_name} → {new}：{account_count} 个账号 / {} 个变量名已更新",
        moves.len()
    ))
}

/// 新增账号条目（允许一条含值声明都没有 —— 服务器类条目就是这样）。
pub fn account_add(ctx: &Ctx, platform_name: &str, alias: &str, meta: MetaPatch) -> Result<String> {
    naming::validate_segment("别名", alias)?;
    meta.validate()?;
    let before = store::load(&ctx.paths)?;
    let platform = before.find_platform(platform_name).ok_or_else(|| {
        CoreError::usage(format!(
            "平台不存在：{platform_name}（先用 assets platform add 建平台）"
        ))
    })?;
    if platform.has_account(alias) {
        return Err(CoreError::conflict(format!(
            "平台 {} 下已有条目：{alias}（重名检测不区分大小写）",
            platform.name
        )));
    }
    let display = platform.name.clone();
    let mut after = before.clone();
    let target = after.find_platform_mut(platform_name).expect("平台刚查到");
    let mut account = Account {
        alias: alias.to_string(),
        email: None,
        purpose: None,
        note: None,
        host: None,
        user: None,
        created_at: now_iso(),
        variables: Vec::new(),
    };
    meta.apply_to(&mut account);
    target.accounts.push(account);
    commit(ctx, &before, &after, &[])?;
    Ok(format!("已新增条目 {display}/{alias}"))
}

/// 改条目的非密元数据：只改给出的字段，给空串即清空。
pub fn account_edit(
    ctx: &Ctx,
    platform_name: &str,
    alias: &str,
    patch: MetaPatch,
) -> Result<String> {
    patch.validate()?;
    if patch.is_empty() {
        return Err(CoreError::usage(
            "至少要给一个要改的字段：--email / --purpose / --note / --host / --user",
        ));
    }
    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(platform_name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{platform_name}")))?;
    if !platform.has_account(alias) {
        return Err(CoreError::usage(format!(
            "平台 {} 下没有条目：{alias}",
            platform.name
        )));
    }
    let display = platform.name.clone();
    let old_alias = platform
        .find_account(alias)
        .expect("条目刚查到")
        .alias
        .clone();
    let mut after = before.clone();
    let account = after
        .find_platform_mut(platform_name)
        .expect("平台刚查到")
        .find_account_mut(alias)
        .expect("条目刚查到");
    patch.apply_to(account);
    commit(ctx, &before, &after, &[])?;
    Ok(format!(
        "已更新 {display}/{old_alias}：{}",
        patch.changed_fields().join(", ")
    ))
}

/// 别名改名：**只连带这个条目自己的变量**（平台级变量的名字里没有账号段）。
pub fn account_rename(
    ctx: &Ctx,
    platform_name: &str,
    old_alias: &str,
    new_alias: &str,
) -> Result<String> {
    naming::validate_segment("别名", new_alias)?;
    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(platform_name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{platform_name}")))?;
    let account = platform.find_account(old_alias).ok_or_else(|| {
        CoreError::usage(format!("平台 {} 下没有条目：{old_alias}", platform.name))
    })?;
    if naming::normalize(old_alias) != naming::normalize(new_alias)
        && platform.has_account(new_alias)
    {
        return Err(CoreError::conflict(format!(
            "平台 {} 下已有条目：{new_alias}（重名检测不区分大小写）",
            platform.name
        )));
    }

    let mut moves: Vec<(String, String)> = Vec::new();
    for decl in &account.variables {
        moves.push((
            naming::account_var_name(&platform.name, &account.alias, &decl.term)?,
            naming::account_var_name(&platform.name, new_alias, &decl.term)?,
        ));
    }
    let display = platform.name.clone();
    let current_alias = account.alias.clone();
    let ops = rename_ops(&ctx.registry, &moves)?;
    let mut after = before.clone();
    after
        .find_platform_mut(platform_name)
        .expect("平台刚查到")
        .find_account_mut(old_alias)
        .expect("条目刚查到")
        .alias = new_alias.to_string();
    commit(ctx, &before, &after, &ops)?;
    Ok(format!(
        "已重命名 {display}/{current_alias} → {new_alias}：{} 个变量名已更新",
        moves.len()
    ))
}

/// 声明变量并写入值（upsert）。值只从 stdin 进来，不进 argv。
pub fn var_set(
    ctx: &Ctx,
    platform_name: &str,
    account_alias: Option<&str>,
    term: &str,
    value: &str,
) -> Result<String> {
    naming::validate_segment("术语", term)?;
    naming::validate_value(value)?;

    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(platform_name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{platform_name}")))?;

    let (name, declared) = match account_alias {
        Some(alias) => {
            let account = platform.find_account(alias).ok_or_else(|| {
                CoreError::usage(format!("平台 {} 下没有条目：{alias}", platform.name))
            })?;
            (
                naming::account_var_name(&platform.name, &account.alias, term)?,
                account.has_term(term),
            )
        }
        None => (
            naming::platform_var_name(&platform.name, term)?,
            platform.has_term(term),
        ),
    };

    if !declared && ctx.registry.get(&name)?.is_some() {
        return Err(CoreError::conflict(format!(
            "注册表里已有同名变量 {name}，但台账里没有对应声明；为免覆盖手工创建的变量，拒绝写入。\
             请先确认它是否属于本系统（属于就用 GUI 接管，不属于就改名）"
        )));
    }

    let mut after = before.clone();
    if !declared {
        match account_alias {
            Some(alias) => after
                .find_platform_mut(platform_name)
                .expect("平台刚查到")
                .find_account_mut(alias)
                .expect("条目刚查到")
                .variables
                .push(VarDecl {
                    term: term.to_string(),
                }),
            None => after
                .find_platform_mut(platform_name)
                .expect("平台刚查到")
                .variables
                .push(VarDecl {
                    term: term.to_string(),
                }),
        }
    }

    let usage = env_block_usage(ctx, &after, (&name, value))?;
    if usage > ENV_BLOCK_BUDGET {
        return Err(CoreError::budget(format!(
            "环境块预算超限：本次写入后为 {usage} 字符，超过上限 {ENV_BLOCK_BUDGET}；已拒绝写入"
        )));
    }

    let ops = vec![RegOp::Set {
        name: name.clone(),
        value: value.to_string(),
    }];
    commit(ctx, &before, &after, &ops)?;

    let mut message = format!("已写入 {name}");
    if usage >= ENV_BLOCK_WARN_AT {
        message.push_str(&format!(
            "\n预算提示：环境块已用 {usage} / {ENV_BLOCK_BUDGET} 字符（{}%），接近上限",
            usage * 100 / ENV_BLOCK_BUDGET
        ));
    }
    Ok(message)
}

/// 事务：建快照 → 应用注册表改动 → 落盘元数据；任一步失败整体回退。
/// 删除一个变量声明（连带注册表里的值）。
///
/// 删除是人类专属动作：命令面里没有它，只有界面层调用（DEC-008）。
pub fn var_delete(
    ctx: &Ctx,
    platform_name: &str,
    account_alias: Option<&str>,
    term: &str,
) -> Result<String> {
    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(platform_name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{platform_name}")))?;
    let display = platform.name.clone();

    let (var_name, scope) = match account_alias {
        None => {
            let decl = platform
                .variables
                .iter()
                .find(|v| naming::normalize(&v.term) == naming::normalize(term))
                .ok_or_else(|| CoreError::usage(format!("平台级变量不存在：{display}/{term}")))?;
            (
                naming::platform_var_name(&display, &decl.term)?,
                format!("{display}（平台级）"),
            )
        }
        Some(alias) => {
            let target = platform
                .find_account(alias)
                .ok_or_else(|| CoreError::usage(format!("条目不存在：{display}/{alias}")))?;
            let decl = target
                .variables
                .iter()
                .find(|v| naming::normalize(&v.term) == naming::normalize(term))
                .ok_or_else(|| {
                    CoreError::usage(format!("变量不存在：{display}/{}/{term}", target.alias))
                })?;
            (
                naming::account_var_name(&display, &target.alias, &decl.term)?,
                format!("{display}/{}", target.alias),
            )
        }
    };

    let mut after = before.clone();
    let target = after.find_platform_mut(&display).expect("平台刚查到");
    match account_alias {
        None => target
            .variables
            .retain(|v| naming::normalize(&v.term) != naming::normalize(term)),
        Some(alias) => target
            .find_account_mut(alias)
            .expect("条目刚查到")
            .variables
            .retain(|v| naming::normalize(&v.term) != naming::normalize(term)),
    }

    commit(ctx, &before, &after, &[RegOp::Delete { name: var_name.clone() }])?;
    Ok(format!("已删除 {scope} 的变量 {var_name}"))
}

/// 删除账号条目：只连带这个账号自己的变量（平台级变量不参与）。
///
/// 删除是人类专属动作：命令面里没有它，只有界面层调用（DEC-008）。
pub fn account_delete(ctx: &Ctx, platform_name: &str, alias: &str) -> Result<String> {
    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(platform_name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{platform_name}")))?;
    let target = platform
        .find_account(alias)
        .ok_or_else(|| CoreError::usage(format!("条目不存在：{platform_name}/{alias}")))?;
    let display = platform.name.clone();
    let target_alias = target.alias.clone();

    let mut names = Vec::new();
    for decl in &target.variables {
        names.push(naming::account_var_name(&display, &target_alias, &decl.term)?);
    }
    let var_count = names.len();

    let mut after = before.clone();
    if let Some(platform) = after.find_platform_mut(&display) {
        platform
            .accounts
            .retain(|a| naming::normalize(&a.alias) != naming::normalize(&target_alias));
    }
    commit(ctx, &before, &after, &delete_ops(&names))?;
    Ok(format!("已删除条目 {display}/{target_alias}：{var_count} 个变量"))
}

/// 删除平台：连带它的平台级变量与全部账号级变量。
///
/// 删除是人类专属动作：命令面里没有它，只有界面层调用（DEC-008）。
pub fn platform_delete(ctx: &Ctx, name: &str) -> Result<String> {
    let before = store::load(&ctx.paths)?;
    let platform = before
        .find_platform(name)
        .ok_or_else(|| CoreError::usage(format!("平台不存在：{name}")))?;
    let display = platform.name.clone();

    let mut names = Vec::new();
    for decl in &platform.variables {
        names.push(naming::platform_var_name(&display, &decl.term)?);
    }
    for account in &platform.accounts {
        for decl in &account.variables {
            names.push(naming::account_var_name(&display, &account.alias, &decl.term)?);
        }
    }
    let account_count = platform.accounts.len();
    let var_count = names.len();

    let mut after = before.clone();
    after
        .platforms
        .retain(|p| naming::normalize(&p.name) != naming::normalize(&display));
    commit(ctx, &before, &after, &delete_ops(&names))?;
    Ok(format!(
        "已删除平台 {display}：{account_count} 个账号 / {var_count} 个变量"
    ))
}

/// 删除对应的注册表改动；注册表里本来就没有这个值也算成功（幂等）。
fn delete_ops(names: &[String]) -> Vec<RegOp> {
    names
        .iter()
        .map(|name| RegOp::Delete { name: name.clone() })
        .collect()
}

/// 事务：建快照 → 应用注册表改动 → 落盘元数据；任一步失败整体回退。
fn commit(ctx: &Ctx, before: &DataDoc, after: &DataDoc, ops: &[RegOp]) -> Result<()> {
    snapshot::create(&ctx.paths, &ctx.registry, before)?;

    let mut undo: Vec<(String, Option<String>)> = Vec::with_capacity(ops.len());
    for op in ops {
        undo.push((op.name().to_string(), ctx.registry.get(op.name())?));
    }

    let mut applied = 0usize;
    for op in ops {
        let result = match op {
            RegOp::Set { name, value } => ctx.registry.set(name, value),
            RegOp::Delete { name } => ctx.registry.delete(name),
        };
        if let Err(err) = result {
            let trouble = rollback(ctx, &undo[..applied]);
            return Err(with_rollback_note(err, trouble));
        }
        applied += 1;
    }

    if let Err(err) = store::save(&ctx.paths, after) {
        let trouble = rollback(ctx, &undo[..applied]);
        return Err(with_rollback_note(err, trouble));
    }

    if !ops.is_empty() {
        registry::broadcast_environment_change();
    }
    Ok(())
}

/// 回退已应用的注册表改动（逆序还原）。返回没能还原的部分。
fn rollback(ctx: &Ctx, undo: &[(String, Option<String>)]) -> Option<String> {
    let mut failures = Vec::new();
    for (name, before) in undo.iter().rev() {
        let result = match before {
            Some(value) => ctx.registry.set(name, value),
            None => ctx.registry.delete(name),
        };
        if let Err(err) = result {
            failures.push(format!("{name}（{err}）"));
        }
    }
    if failures.is_empty() {
        None
    } else {
        Some(failures.join("；"))
    }
}

fn with_rollback_note(err: CoreError, trouble: Option<String>) -> CoreError {
    let Some(trouble) = trouble else {
        return err;
    };
    let message = format!("{err}；回退未完成，仍需人工恢复：{trouble}");
    match err {
        CoreError::Usage(_) => CoreError::Usage(message),
        CoreError::Validation(_) => CoreError::Validation(message),
        CoreError::Storage(_) => CoreError::Storage(message),
        CoreError::Conflict(_) => CoreError::Conflict(message),
        CoreError::Budget(_) => CoreError::Budget(message),
    }
}

/// 级联改名对应的注册表改动。
///
/// 台账里声明了、注册表里却没有值 → 拒绝改名（错误状态先修，不带着走）。
fn rename_ops(registry: &Registry, moves: &[(String, String)]) -> Result<Vec<RegOp>> {
    let mut ops = Vec::new();
    let mut missing = Vec::new();
    for (old_name, new_name) in moves {
        match registry.get(old_name)? {
            Some(value) => ops.push(RegOp::Set {
                name: new_name.clone(),
                value,
            }),
            None => missing.push(old_name.clone()),
        }
        if naming::normalize(old_name) != naming::normalize(new_name) {
            ops.push(RegOp::Delete {
                name: old_name.clone(),
            });
        }
    }
    if !missing.is_empty() {
        return Err(CoreError::storage(format!(
            "台账与注册表不一致：{} 在注册表里没有值，已拒绝改名（先跑 assets doctor；\
             用 assets var set 重新写入这些变量后再试）",
            missing.join(", ")
        )));
    }

    let moving: HashSet<String> = moves
        .iter()
        .map(|(old_name, _)| naming::normalize(old_name))
        .collect();
    for (_, new_name) in moves {
        if moving.contains(&naming::normalize(new_name)) {
            continue;
        }
        if registry.get(new_name)?.is_some() {
            return Err(CoreError::conflict(format!(
                "注册表里已存在同名变量 {new_name}（不是本次改名要腾出来的），拒绝覆盖"
            )));
        }
    }
    Ok(ops)
}

/// 环境块用量：全部已声明变量的「名字 + 值 + 2」之和；`written` 是本次要写的那个。
fn env_block_usage(ctx: &Ctx, doc: &DataDoc, written: (&str, &str)) -> Result<usize> {
    let mut total = 0usize;
    for decl in doc.declarations()? {
        let value_len = if decl.name == written.0 {
            written.1.chars().count()
        } else {
            ctx.registry
                .get(&decl.name)?
                .map(|value| value.chars().count())
                .unwrap_or(0)
        };
        total += decl.name.len() + value_len + 2;
    }
    Ok(total)
}

/// 空串与 `None` 同义：元数据里没有"空字符串"这种状态。
fn clean(value: Option<String>) -> Option<String> {
    match value {
        Some(text) if text.is_empty() => None,
        other => other,
    }
}

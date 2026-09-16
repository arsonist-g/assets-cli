//! 元数据文档的读写：临时文件 + 原子替换；读入时做一次完整性校验。

use std::collections::HashSet;
use std::fs;

use crate::error::{CoreError, Result};
use crate::model::{Account, DataDoc, Platform, DOC_VERSION};
use crate::naming;
use crate::paths::Paths;

/// 读入台账；文件不存在即空台账（合法的初始状态）。
pub fn load(paths: &Paths) -> Result<DataDoc> {
    let file = paths.data_file();
    if !file.exists() {
        return Ok(DataDoc::empty());
    }
    let text = fs::read_to_string(&file).map_err(|e| {
        CoreError::storage_from(&format!("数据文件读取失败（{}）", file.display()), e)
    })?;
    let doc: DataDoc = serde_json::from_str(&text).map_err(|e| {
        CoreError::storage_from(&format!("数据文件无法解析（{}）", file.display()), e)
    })?;
    if doc.version > DOC_VERSION {
        return Err(CoreError::storage(format!(
            "数据文件版本 {} 高于本程序支持的 {DOC_VERSION}（{}）：请升级 assets-cli",
            doc.version,
            file.display()
        )));
    }
    validate_doc(&doc).map_err(|e| {
        CoreError::storage_from(&format!("数据文件内容非法（{}）", file.display()), e)
    })?;
    Ok(doc)
}

/// 落盘：写临时文件 → 覆盖改名，不存在"写一半"的中间态。
pub fn save(paths: &Paths, doc: &DataDoc) -> Result<()> {
    ensure_data_dir(paths)?;
    let mut stable = doc.clone();
    stable.sort();
    let text = serde_json::to_string_pretty(&stable)
        .map_err(|e| CoreError::storage_from("元数据序列化失败", e))?;
    let tmp = paths.tmp_file();
    fs::write(&tmp, format!("{text}\n")).map_err(|e| {
        CoreError::storage_from(&format!("临时文件写入失败（{}）", tmp.display()), e)
    })?;
    let file = paths.data_file();
    fs::rename(&tmp, &file).map_err(|e| {
        CoreError::storage_from(
            &format!("原子替换失败（{} → {}）", tmp.display(), file.display()),
            e,
        )
    })?;
    Ok(())
}

pub fn ensure_data_dir(paths: &Paths) -> Result<()> {
    let dir = &paths.data_dir;
    if dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dir)
        .map_err(|e| CoreError::storage_from(&format!("数据目录创建失败（{}）", dir.display()), e))
}

/// 完整性校验：字符集、长度、唯一性（大写归一化）、派生变量名长度。
///
/// 手改过文件导致的非法状态在**读入阶段**就被拒绝，不扩散到运行时。
pub fn validate_doc(doc: &DataDoc) -> Result<()> {
    if doc.version == 0 || doc.version > DOC_VERSION {
        return Err(CoreError::validation(format!(
            "不支持的文档版本：{}（当前支持 {DOC_VERSION}）",
            doc.version
        )));
    }
    let mut platform_keys = HashSet::new();
    for platform in &doc.platforms {
        validate_platform(platform)?;
        if !platform_keys.insert(naming::normalize(&platform.name)) {
            return Err(CoreError::validation(format!(
                "平台重名（大小写不敏感）：{}",
                platform.name
            )));
        }
    }
    doc.declarations()?;
    Ok(())
}

fn validate_platform(platform: &Platform) -> Result<()> {
    naming::validate_segment("平台名", &platform.name)?;
    if let Some(note) = &platform.note {
        naming::validate_meta("平台备注", note)?;
    }
    let mut term_keys = HashSet::new();
    for decl in &platform.variables {
        naming::validate_segment("术语", &decl.term)?;
        if !term_keys.insert(naming::normalize(&decl.term)) {
            return Err(CoreError::validation(format!(
                "平台 {} 的术语重复：{}",
                platform.name, decl.term
            )));
        }
    }
    let mut alias_keys = HashSet::new();
    for account in &platform.accounts {
        validate_account(account)?;
        if !alias_keys.insert(naming::normalize(&account.alias)) {
            return Err(CoreError::validation(format!(
                "平台 {} 内别名重复：{}",
                platform.name, account.alias
            )));
        }
    }
    Ok(())
}

fn validate_account(account: &Account) -> Result<()> {
    naming::validate_segment("别名", &account.alias)?;
    for (kind, value) in [
        ("邮箱", &account.email),
        ("用途", &account.purpose),
        ("备注", &account.note),
        ("host", &account.host),
        ("user", &account.user),
    ] {
        if let Some(text) = value {
            naming::validate_meta(kind, text)?;
        }
    }
    let mut term_keys = HashSet::new();
    for decl in &account.variables {
        naming::validate_segment("术语", &decl.term)?;
        if !term_keys.insert(naming::normalize(&decl.term)) {
            return Err(CoreError::validation(format!(
                "条目 {} 的术语重复：{}",
                account.alias, decl.term
            )));
        }
    }
    Ok(())
}

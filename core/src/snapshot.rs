//! 快照：写入前建全量副本（元数据 + 全部变量名与值），超容量按最旧优先滚动。
//!
//! 快照文件只新建、永不改写；列表从目录派生（不建索引）。
//! 恢复语义 = 让状态等于快照：① 覆盖元数据 ② 写回快照里的值 ③ 删掉本系统名下、快照里没有的变量。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::model::{now_iso, DataDoc, Snapshot, SnapshotVar, DOC_VERSION};
use crate::paths::Paths;
use crate::registry::{self, Registry};
use crate::store;

/// 快照目录的容量上限：超过就删最旧的（按容量，不按条数、不按时间）。
pub const QUOTA_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub file: PathBuf,
    pub created_at: String,
    pub bytes: u64,
    pub variables: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub file: PathBuf,
    pub created_at: String,
    pub variables_written: usize,
    pub variables_removed: Vec<String>,
}

/// 建一份快照并滚动配额，返回快照文件路径。
pub fn create(paths: &Paths, registry: &Registry, doc: &DataDoc) -> Result<PathBuf> {
    let entries = registry.asset_entries()?;
    let snapshot = Snapshot {
        version: DOC_VERSION,
        created_at: now_iso(),
        data: doc.clone(),
        variables: entries
            .into_iter()
            .map(|(name, value)| SnapshotVar { name, value })
            .collect(),
    };
    let dir = paths.snapshots_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        CoreError::storage_from(&format!("快照目录创建失败（{}）", dir.display()), e)
    })?;
    let file = dir.join(file_name());
    let text = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| CoreError::storage_from("快照序列化失败", e))?;
    fs::write(&file, format!("{text}\n"))
        .map_err(|e| CoreError::storage_from(&format!("快照写入失败（{}）", file.display()), e))?;
    prune(paths)?;
    Ok(file)
}

/// 列出全部快照，最旧在前。
pub fn list(paths: &Paths) -> Result<Vec<SnapshotInfo>> {
    let dir = paths.snapshots_dir();
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let entries = fs::read_dir(&dir).map_err(|e| {
        CoreError::storage_from(&format!("快照目录读取失败（{}）", dir.display()), e)
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            CoreError::storage_from(&format!("快照目录读取失败（{}）", dir.display()), e)
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let text = fs::read_to_string(&path).map_err(|e| {
            CoreError::storage_from(&format!("快照读取失败（{}）", path.display()), e)
        })?;
        let snapshot: Snapshot = serde_json::from_str(&text).map_err(|e| {
            CoreError::storage_from(&format!("快照无法解析（{}）", path.display()), e)
        })?;
        out.push(SnapshotInfo {
            file: path,
            created_at: snapshot.created_at,
            bytes,
            variables: snapshot.variables.len(),
        });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

/// 按默认配额滚动（删最旧的直到回到配额内），返回被删掉的文件。
pub fn prune(paths: &Paths) -> Result<Vec<PathBuf>> {
    prune_with_quota(paths, QUOTA_BYTES)
}

/// 按指定配额滚动（测试用；生产走 `QUOTA_BYTES`）。最新的那一份永远保留。
pub fn prune_with_quota(paths: &Paths, quota: u64) -> Result<Vec<PathBuf>> {
    let dir = paths.snapshots_dir();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| {
        CoreError::storage_from(&format!("快照目录读取失败（{}）", dir.display()), e)
    })? {
        let entry = entry.map_err(|e| {
            CoreError::storage_from(&format!("快照目录读取失败（{}）", dir.display()), e)
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        files.push((path, size));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total: u64 = files.iter().map(|(_, size)| *size).sum();
    let mut removed = Vec::new();
    while total > quota && files.len() > 1 {
        let (path, size) = files.remove(0);
        fs::remove_file(&path).map_err(|e| {
            CoreError::storage_from(&format!("快照删除失败（{}）", path.display()), e)
        })?;
        total -= size.min(total);
        removed.push(path);
    }
    Ok(removed)
}

/// 恢复：让状态等于快照（三步缺一不可）。
pub fn restore(paths: &Paths, registry: &Registry, file: &Path) -> Result<RestoreReport> {
    let text = fs::read_to_string(file)
        .map_err(|e| CoreError::storage_from(&format!("快照读取失败（{}）", file.display()), e))?;
    let snapshot: Snapshot = serde_json::from_str(&text)
        .map_err(|e| CoreError::storage_from(&format!("快照无法解析（{}）", file.display()), e))?;
    if snapshot.version > DOC_VERSION {
        return Err(CoreError::storage(format!(
            "快照版本 {} 高于本程序支持的 {DOC_VERSION}（{}）：请升级 assets-cli",
            snapshot.version,
            file.display()
        )));
    }
    store::validate_doc(&snapshot.data).map_err(|e| {
        CoreError::storage_from(&format!("快照里的元数据非法（{}）", file.display()), e)
    })?;

    store::save(paths, &snapshot.data)?;

    for variable in &snapshot.variables {
        registry.set(&variable.name, &variable.value)?;
    }

    let keep: HashSet<&str> = snapshot.variables.iter().map(|v| v.name.as_str()).collect();
    let mut variables_removed = Vec::new();
    for name in registry.asset_entries()?.keys() {
        if !keep.contains(name.as_str()) {
            registry.delete(name)?;
            variables_removed.push(name.clone());
        }
    }

    registry::broadcast_environment_change();
    prune(paths)?;

    Ok(RestoreReport {
        file: file.to_path_buf(),
        created_at: snapshot.created_at,
        variables_written: snapshot.variables.len(),
        variables_removed,
    })
}

/// 文件名用 `-` 代替 `:`（Windows 文件名不允许冒号），并带毫秒以免同一秒内互相覆盖。
fn file_name() -> String {
    format!(
        "{}.json",
        chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ")
    )
}

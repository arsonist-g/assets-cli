//! 错误类型与退出码。
//!
//! 退出码是命令面契约的一部分：调用方（AI / GUI）靠退出码分流，不解析文案。
//! 0 成功 / 2 用法或引用不存在 / 3 校验失败 / 4 存储失败 / 5 冲突 / 6 预算超限 / 7 自检未通过。

use std::fmt;

/// 成功。
pub const EXIT_OK: i32 = 0;
/// 参数缺失、未知子命令、引用的平台或条目不存在。
pub const EXIT_USAGE: i32 = 2;
/// 字符集非法、超长、空值。
pub const EXIT_VALIDATION: i32 = 3;
/// 注册表或数据文件不可读写。
pub const EXIT_STORAGE: i32 = 4;
/// 重名；或注册表里已有同名变量但台账里没有声明。
pub const EXIT_CONFLICT: i32 = 5;
/// 变量数或环境块总量超预算。
pub const EXIT_BUDGET: i32 = 6;
/// 自检未通过（仅 doctor）。
pub const EXIT_DOCTOR: i32 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// 参数缺失 / 未知子命令 / 引用的平台或条目不存在。
    Usage(String),
    /// 字符集、长度、空值等校验失败。
    Validation(String),
    /// 注册表或数据文件不可读写。
    Storage(String),
    /// 重名，或注册表已有同名变量但无声明。
    Conflict(String),
    /// 变量数或环境块总量超预算。
    Budget(String),
}

impl CoreError {
    pub fn code(&self) -> i32 {
        match self {
            CoreError::Usage(_) => EXIT_USAGE,
            CoreError::Validation(_) => EXIT_VALIDATION,
            CoreError::Storage(_) => EXIT_STORAGE,
            CoreError::Conflict(_) => EXIT_CONFLICT,
            CoreError::Budget(_) => EXIT_BUDGET,
        }
    }

    pub fn usage(msg: impl Into<String>) -> Self {
        CoreError::Usage(msg.into())
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        CoreError::Validation(msg.into())
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        CoreError::Storage(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        CoreError::Conflict(msg.into())
    }

    pub fn budget(msg: impl Into<String>) -> Self {
        CoreError::Budget(msg.into())
    }

    /// 存储类错误统一带上底层原因，避免丢信息。
    pub fn storage_from(context: &str, err: impl fmt::Display) -> Self {
        CoreError::Storage(format!("{context}：{err}"))
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Usage(m)
            | CoreError::Validation(m)
            | CoreError::Storage(m)
            | CoreError::Conflict(m)
            | CoreError::Budget(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type Result<T> = std::result::Result<T, CoreError>;

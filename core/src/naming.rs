//! 变量名规则：生成、校验、段数判定。
//!
//! 变量名永不落盘，永远由 (平台, 别名?, 术语) 现算；比较与唯一性一律**大写归一化**。

use crate::error::{CoreError, Result};

/// 所有资产变量共用的前缀。
pub const PREFIX: &str = "ASSETS_CLI_";
/// 变量名长度上限：Windows 用户级变量名 ≥255 字符非法。
pub const MAX_VAR_NAME_LEN: usize = 254;
/// 平台名 / 别名 / 术语各自的长度上限；三段各 64 字符可稳定落在变量名上限内。
pub const MAX_SEGMENT_LEN: usize = 64;
/// 备注 / 邮箱 / 用途 / host / user 的长度上限。
pub const MAX_META_LEN: usize = 256;
/// 单个变量值的长度上限（Windows 用户级变量值上限 32767 字符）。
pub const MAX_VALUE_LEN: usize = 32767;
/// 环境块预算：全部已声明变量的「名字 + 值 + 2」之和的硬上限，超过即拒绝写入。
pub const ENV_BLOCK_BUDGET: usize = 256 * 1024;
/// 用量到此比例即出声提示（不静默）。
pub const ENV_BLOCK_WARN_AT: usize = ENV_BLOCK_BUDGET * 8 / 10;

/// 唯一性与冲突检测一律用这个形式比较。
pub fn normalize(value: &str) -> String {
    value.to_ascii_uppercase()
}

/// 平台名 / 别名 / 术语的公共校验。
pub fn validate_segment(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation(format!("{kind}不能为空")));
    }
    if value.chars().count() > MAX_SEGMENT_LEN {
        return Err(CoreError::validation(format!(
            "{kind}过长：最多 {MAX_SEGMENT_LEN} 个字符，当前 {}",
            value.chars().count()
        )));
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(CoreError::validation(format!(
            "{kind}只能包含 A-Z a-z 0-9 和 _：{value}"
        )));
    }
    Ok(())
}

/// 文本类元数据（备注 / 邮箱 / 用途 / host / user）的校验：只限长度，允许中文。
pub fn validate_meta(kind: &str, value: &str) -> Result<()> {
    if value.chars().count() > MAX_META_LEN {
        return Err(CoreError::validation(format!(
            "{kind}过长：最多 {MAX_META_LEN} 个字符，当前 {}",
            value.chars().count()
        )));
    }
    Ok(())
}

/// 平台级变量名：`ASSETS_CLI_<平台>_<术语>`（3 段）。
pub fn platform_var_name(platform: &str, term: &str) -> Result<String> {
    compose(&[platform, term])
}

/// 账号级变量名：`ASSETS_CLI_<平台>_<别名>_<术语>`（4 段）。
pub fn account_var_name(platform: &str, alias: &str, term: &str) -> Result<String> {
    compose(&[platform, alias, term])
}

fn compose(parts: &[&str]) -> Result<String> {
    let name = format!("{PREFIX}{}", parts.join("_")).to_ascii_uppercase();
    if name.chars().count() > MAX_VAR_NAME_LEN {
        return Err(CoreError::validation(format!(
            "派生出的变量名超过 Windows 上限（{MAX_VAR_NAME_LEN} 字符）：{} 字符。请缩短{}\u{3002}",
            name.chars().count(),
            if parts.len() > 2 {
                "别名或术语"
            } else {
                "平台名或术语"
            }
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(CoreError::validation(format!("变量名含有非法字符：{name}")));
    }
    Ok(name)
}

/// 是不是本系统的变量名：`ASSETS_CLI_` 前缀 + **≥3 段**（前缀后至少两段）。
///
/// 同前缀的单段覆写开关（如 `ASSETS_CLI_DATA`）不是资产，钩子与清单都不认它。
pub fn is_asset_var_name(name: &str) -> bool {
    let Some(rest) = strip_prefix(name) else {
        return false;
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    let mut segments = rest.split('_');
    let first = segments.next().unwrap_or("");
    let tail: Vec<&str> = segments.collect();
    !first.is_empty() && !tail.is_empty() && tail.iter().all(|s| !s.is_empty())
}

/// 值的校验：非空即可（`setx` 的 1024 截断问题不存在，因为写入走注册表 API），长度另受预算约束。
pub fn validate_value(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CoreError::validation(
            "值不能为空：不声明即不存在，空值会让变量名没有意义",
        ));
    }
    if value.chars().count() > MAX_VALUE_LEN {
        return Err(CoreError::budget(format!(
            "值过长：超过单个用户级变量上限（{MAX_VALUE_LEN} 字符）"
        )));
    }
    Ok(())
}

fn strip_prefix(name: &str) -> Option<&str> {
    let head = name.get(..PREFIX.len())?;
    if head.eq_ignore_ascii_case(PREFIX) {
        name.get(PREFIX.len()..)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_platform_level_name() {
        assert_eq!(
            platform_var_name("cloudflare", "API_BASE_URL").unwrap(),
            "ASSETS_CLI_CLOUDFLARE_API_BASE_URL"
        );
    }

    #[test]
    fn derives_account_level_name_uppercased() {
        assert_eq!(
            account_var_name("GitHub", "work", "api_token").unwrap(),
            "ASSETS_CLI_GITHUB_WORK_API_TOKEN"
        );
    }

    #[test]
    fn rejects_non_ascii_segments() {
        assert_eq!(validate_segment("平台名", "云flare").unwrap_err().code(), 3);
        assert_eq!(validate_segment("平台名", "a-b").unwrap_err().code(), 3);
        assert_eq!(validate_segment("平台名", "").unwrap_err().code(), 3);
    }

    #[test]
    fn rejects_overlong_segments_and_guards_the_derived_name() {
        let long = "a".repeat(MAX_SEGMENT_LEN + 1);
        assert_eq!(validate_segment("平台名", &long).unwrap_err().code(), 3);

        // 段长上限让正常输入的派生名不可能超限；这里验证兜底那条判断本身有效
        let capped = "a".repeat(MAX_SEGMENT_LEN);
        assert!(account_var_name(&capped, &capped, &capped).is_ok());
        assert_eq!(
            compose(&[&capped, &capped, &capped, &capped])
                .unwrap_err()
                .code(),
            3
        );
    }

    #[test]
    fn recognises_asset_names_only() {
        assert!(is_asset_var_name("ASSETS_CLI_CLOUDFLARE_API_BASE_URL"));
        assert!(is_asset_var_name("assets_cli_cf_x_y"));
        assert!(!is_asset_var_name("ASSETS_CLI_DATA"));
        assert!(!is_asset_var_name("ASSETS_CLI_"));
        assert!(!is_asset_var_name("ASSETS_CLI_A_"));
        assert!(!is_asset_var_name("PATH"));
    }

    #[test]
    fn rejects_empty_value_with_validation_code() {
        assert_eq!(validate_value("").unwrap_err().code(), 3);
    }
}

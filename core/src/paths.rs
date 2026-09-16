//! 落点解析：数据目录、注册表根、宿主目录布局。
//!
//! 默认值即生产；两个覆写开关只在自己被显式设置时生效（开发 / 测试隔离用）。
//! 开关名是 `ASSETS_CLI_` 前缀下的**单段**名（两段变量名），钩子按「≥3 段」过滤，因此不会被当成资产导出。

use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// 数据目录覆写开关。
pub const DATA_DIR_ENV: &str = "ASSETS_CLI_DATA";
/// 注册表根覆写开关。
pub const REGISTRY_ENV: &str = "ASSETS_CLI_REGISTRY";
/// 数据目录与钩子脚本的目录名（位于用户主目录下）。
pub const HOOK_DIR_NAME: &str = ".assets-cli";
/// 默认注册表根。
pub const DEFAULT_REGISTRY_SPEC: &str = r"HKCU\Environment";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn parse(s: &str) -> Option<Hive> {
        match s.to_ascii_uppercase().as_str() {
            "HKCU" | "HKEY_CURRENT_USER" => Some(Hive::CurrentUser),
            "HKLM" | "HKEY_LOCAL_MACHINE" => Some(Hive::LocalMachine),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
        }
    }
}

/// 注册表根的写法：`<hive>\<subkey>`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySpec {
    pub hive: Hive,
    pub subkey: String,
}

impl RegistrySpec {
    pub fn parse(spec: &str) -> Result<RegistrySpec> {
        let trimmed = spec.trim();
        let (hive_part, subkey_part) = trimmed.split_once('\\').ok_or_else(|| {
            CoreError::storage(format!(
                "注册表根写法不合法：{trimmed}（应形如 HKCU\\Environment）"
            ))
        })?;
        let hive = Hive::parse(hive_part).ok_or_else(|| {
            CoreError::storage(format!(
                "不支持的注册表根：{hive_part}（只支持 HKCU / HKLM）"
            ))
        })?;
        let subkey = subkey_part.trim_matches('\\').to_string();
        if subkey.is_empty() {
            return Err(CoreError::storage(format!("注册表根缺少子键：{trimmed}")));
        }
        Ok(RegistrySpec { hive, subkey })
    }

    pub fn raw(&self) -> String {
        format!("{}\\{}", self.hive.name(), self.subkey)
    }
}

/// core 的两个落点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub registry: RegistrySpec,
}

impl Paths {
    /// 显式指定两个落点（测试一律走这条，不依赖默认值）。
    pub fn explicit(data_dir: impl Into<PathBuf>, registry_spec: &str) -> Result<Paths> {
        Ok(Paths {
            data_dir: data_dir.into(),
            registry: RegistrySpec::parse(registry_spec)?,
        })
    }

    /// 默认值即生产：只有开关被显式设置时才偏离。
    pub fn from_env() -> Result<Paths> {
        let data_dir = match env_var(DATA_DIR_ENV) {
            Some(v) => expand_user_path(&v)?,
            None => default_data_dir()?,
        };
        let registry = match env_var(REGISTRY_ENV) {
            Some(v) => RegistrySpec::parse(&v)?,
            None => RegistrySpec::parse(DEFAULT_REGISTRY_SPEC)?,
        };
        Ok(Paths { data_dir, registry })
    }

    pub fn data_file(&self) -> PathBuf {
        self.data_dir.join("data.json")
    }

    pub fn tmp_file(&self) -> PathBuf {
        self.data_dir.join("data.json.tmp")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.data_dir.join("snapshots")
    }
}

/// 用户主目录。
pub fn home_dir() -> Result<PathBuf> {
    if let Some(profile) = env_var("USERPROFILE") {
        return Ok(PathBuf::from(profile));
    }
    if let (Some(drive), Some(path)) = (env_var("HOMEDRIVE"), env_var("HOMEPATH")) {
        return Ok(PathBuf::from(format!("{drive}{path}")));
    }
    Err(CoreError::storage(
        "无法确定用户主目录（USERPROFILE 与 HOMEDRIVE+HOMEPATH 都为空）",
    ))
}

/// 默认数据目录：`<用户主目录>/.assets-cli`。
pub fn default_data_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(HOOK_DIR_NAME))
}

/// 展开路径里的 `~`（只处理开头那一个 `~`）。
pub fn expand_user_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return home_dir();
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return Ok(home_dir()?.join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

/// 宿主目录布局：钩子脚本、两个 shell 的 profile、skill 描述。
///
/// 这不是"配置"，而是安装与自检要落到的固定位置；因此由主目录直接推导，不受两个覆写开关影响。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub home: PathBuf,
    pub hook_dir: PathBuf,
    pub ps_profile: PathBuf,
    pub bashrc: PathBuf,
    /// Git Bash 登录 shell 读的 profile：`.bash_profile` → `.bash_login` → `.profile`。
    pub bash_login_profile: PathBuf,
    pub skill_file: PathBuf,
}

impl Layout {
    /// 由主目录推导布局；Git Bash 登录 profile 按存在性挑（都不存在时用 `.profile`）。
    pub fn for_home(home: impl Into<PathBuf>) -> Layout {
        let home = home.into();
        let bash_profile = home.join(".bash_profile");
        let bash_login = home.join(".bash_login");
        let bash_login_profile = if bash_profile.exists() {
            bash_profile
        } else if bash_login.exists() {
            bash_login
        } else {
            home.join(".profile")
        };
        Layout {
            hook_dir: home.join(HOOK_DIR_NAME),
            ps_profile: home
                .join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
            bashrc: home.join(".bashrc"),
            bash_login_profile,
            skill_file: home
                .join(".agents")
                .join("skills")
                .join("assets")
                .join("SKILL.md"),
            home,
        }
    }

    /// 真实宿主布局。
    pub fn detect() -> Result<Layout> {
        Ok(Layout::for_home(home_dir()?))
    }

    /// 需要写标记块的 bash profile（`.bashrc` 覆盖非登录交互 shell，登录 profile 覆盖登录 shell）。
    pub fn bash_profiles(&self) -> Vec<PathBuf> {
        let mut out = vec![self.bashrc.clone()];
        if self.bash_login_profile != self.bashrc {
            out.push(self.bash_login_profile.clone());
        }
        out
    }

    /// 钩子脚本的安装位置。
    pub fn hook_scripts(&self) -> Vec<PathBuf> {
        crate::init::HOOK_SCRIPTS
            .iter()
            .map(|name| self.hook_dir.join(name))
            .collect()
    }

    pub fn ps_profile_dir(&self) -> PathBuf {
        match self.ps_profile.parent() {
            Some(dir) => dir.to_path_buf(),
            None => PathBuf::from(Path::new(".")),
        }
    }
}

fn env_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

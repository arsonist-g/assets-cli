//! 注册表读写（默认 `HKCU\Environment`）与设置变更广播。
//!
//! 值就是明文，直接住在这里 —— 因此除了本模块，没有第二处能碰注册表。

use std::collections::BTreeMap;
use std::io;

use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ,
};
use winreg::types::FromRegValue;
use winreg::RegKey;

use crate::error::{CoreError, Result};
use crate::naming::is_asset_var_name;
use crate::paths::{Hive, RegistrySpec};

#[derive(Debug, Clone)]
pub struct Registry {
    spec: RegistrySpec,
}

impl Registry {
    pub fn new(spec: RegistrySpec) -> Registry {
        Registry { spec }
    }

    pub fn spec(&self) -> &RegistrySpec {
        &self.spec
    }

    /// 以写权限打开（子键不存在则创建，测试用的一次性子键靠这一步建立）。只打开，不改值。
    pub fn writable(&self) -> Result<()> {
        self.key_write().map(|_| ())
    }

    /// 读一个值；不存在返回 `None`。
    pub fn get(&self, name: &str) -> Result<Option<String>> {
        let key = match self.key_read()? {
            Some(key) => key,
            None => return Ok(None),
        };
        match key.get_raw_value(name) {
            Ok(raw) => Ok(Some(decode(name, &raw)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CoreError::storage_from(
                &format!("注册表读取失败（{}\\{}）", self.spec.raw(), name),
                e,
            )),
        }
    }

    /// 写一个 REG_SZ 值（覆盖同名值）。
    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        let key = self.key_write()?;
        key.set_value(name, &value.to_string()).map_err(|e| {
            CoreError::storage_from(
                &format!("注册表写入失败（{}\\{}）", self.spec.raw(), name),
                e,
            )
        })
    }

    /// 删一个值；本来就不存在视为成功（幂等）。
    pub fn delete(&self, name: &str) -> Result<()> {
        let key = self.key_write()?;
        match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::storage_from(
                &format!("注册表删除失败（{}\\{}）", self.spec.raw(), name),
                e,
            )),
        }
    }

    /// 本系统名下的全部变量（名字 → 值），按名字升序。
    ///
    /// 只认「≥3 段」的名字：同前缀的单段覆写开关不是资产。
    pub fn asset_entries(&self) -> Result<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        let Some(key) = self.key_read()? else {
            return Ok(out);
        };
        for item in key.enum_values() {
            let (name, raw) = item.map_err(|e| {
                CoreError::storage_from(&format!("注册表枚举失败（{}）", self.spec.raw()), e)
            })?;
            if !is_asset_var_name(&name) {
                continue;
            }
            let value = decode(&name, &raw)?;
            out.insert(name, value);
        }
        Ok(out)
    }

    fn key_read(&self) -> Result<Option<RegKey>> {
        match self
            .predef()
            .open_subkey_with_flags(&self.spec.subkey, KEY_READ)
        {
            Ok(key) => Ok(Some(key)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CoreError::storage_from(
                &format!("注册表打开失败（{}）", self.spec.raw()),
                e,
            )),
        }
    }

    fn key_write(&self) -> Result<RegKey> {
        let predef = self.predef();
        match predef.open_subkey_with_flags(&self.spec.subkey, KEY_READ | KEY_WRITE) {
            Ok(key) => Ok(key),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let (key, _) = predef
                    .create_subkey_with_flags(&self.spec.subkey, KEY_READ | KEY_WRITE)
                    .map_err(|e| {
                        CoreError::storage_from(
                            &format!("注册表子键创建失败（{}）", self.spec.raw()),
                            e,
                        )
                    })?;
                Ok(key)
            }
            Err(e) => Err(CoreError::storage_from(
                &format!("注册表打开失败（{},需要写权限）", self.spec.raw()),
                e,
            )),
        }
    }

    fn predef(&self) -> RegKey {
        match self.spec.hive {
            Hive::CurrentUser => RegKey::predef(HKEY_CURRENT_USER),
            Hive::LocalMachine => RegKey::predef(HKEY_LOCAL_MACHINE),
        }
    }
}

fn decode(name: &str, raw: &winreg::RegValue<'_>) -> Result<String> {
    if raw.vtype != REG_SZ && raw.vtype != REG_EXPAND_SZ {
        return Err(CoreError::storage(format!(
            "注册表值 {name} 不是字符串类型（类型码 {:?}），拒绝当成资产",
            raw.vtype
        )));
    }
    String::from_reg_value(raw)
        .map_err(|e| CoreError::storage_from(&format!("注册表值 {name} 解码失败"), e))
}

/// 通知系统"环境变了"：此后新开的窗口能看到新值。
///
/// 已经存在的进程（含 AI 当前会话）不受影响 —— 它们的环境块在创建时就固定了，
/// "改完立刻可用"由 shell 启动钩子负责。
pub fn broadcast_environment_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let parameter: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            parameter.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

//! 手写的 argv 解析：只认契约里写明的选项，别的一律用法错误（退出码 2）。

use std::collections::{BTreeSet, HashMap};

use assets_core::{CoreError, Result};

#[derive(Debug, Default)]
pub struct Args {
    pub positional: Vec<String>,
    values: HashMap<String, String>,
    flags: BTreeSet<String>,
}

impl Args {
    pub fn parse(
        tokens: &[String],
        usage: &str,
        value_opts: &[&str],
        flags: &[&str],
    ) -> Result<Args> {
        let mut args = Args::default();
        let mut index = 0;
        while index < tokens.len() {
            let token = tokens[index].clone();
            if let Some(raw) = token.strip_prefix("--") {
                let (name, inline) = match raw.split_once('=') {
                    Some((name, value)) => (name.to_string(), Some(value.to_string())),
                    None => (raw.to_string(), None),
                };
                if value_opts.contains(&name.as_str()) {
                    let value = match inline {
                        Some(value) => value,
                        None => {
                            index += 1;
                            tokens.get(index).cloned().ok_or_else(|| {
                                CoreError::usage(format!("--{name} 需要一个值\n用法：{usage}"))
                            })?
                        }
                    };
                    args.values.insert(name, value);
                } else if flags.contains(&name.as_str()) {
                    if inline.is_some() {
                        return Err(CoreError::usage(format!(
                            "--{name} 是开关，不接受值\n用法：{usage}"
                        )));
                    }
                    args.flags.insert(name);
                } else {
                    return Err(CoreError::usage(format!(
                        "不认识的选项：--{name}\n用法：{usage}"
                    )));
                }
            } else {
                args.positional.push(token);
            }
            index += 1;
        }
        Ok(args)
    }

    pub fn take(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    /// 第 `index` 个位置参数；缺了就报用法错误。
    pub fn positional_at(&self, index: usize, what: &str, usage: &str) -> Result<&str> {
        self.positional
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| CoreError::usage(format!("缺少{what}\n用法：{usage}")))
    }

    /// 位置参数个数必须正好是 `count`。
    pub fn expect_positional(&self, count: usize, usage: &str) -> Result<()> {
        if self.positional.len() == count {
            return Ok(());
        }
        Err(CoreError::usage(format!(
            "位置参数个数不对（要 {count} 个，给了 {} 个）\n用法：{usage}",
            self.positional.len()
        )))
    }
}

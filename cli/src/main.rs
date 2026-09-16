//! assets：本机资产台账的命令面。
//!
//! 只有清单与管理两类动作。删除、取值、快照恢复**不在**这条命令面上 —— 这是权限边界本身，
//! 不是还没实现。

mod args;
mod commands;

use assets_core::{CoreError, Result};

const USAGE: &str = r#"assets — 本机资产台账

清单
  assets list [--platform <名称>] [--account <别名>] [--json]

管理（可增可改；删除是人的动作，命令面里没有删除）
  assets platform add <名称> [--note <文本>]
  assets platform rename <旧名> <新名>
  assets account add --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]
  assets account edit --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]
  assets account rename --platform <名称> --alias <旧别名> <新别名>
  printf '%s' "$VALUE" | assets var set --platform <名称> [--account <别名>] --term <术语>

环境
  assets doctor [--json]
  assets init [--json]

退出码
  0 成功 / 2 用法或引用不存在 / 3 校验失败 / 4 存储失败 / 5 冲突 / 6 预算超限 / 7 自检未通过
"#;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

fn run(argv: &[String]) -> i32 {
    match dispatch(argv) {
        Ok(outcome) => {
            if !outcome.text.is_empty() {
                println!("{}", outcome.text);
            }
            outcome.code
        }
        Err(err) => {
            eprintln!("assets: {err}");
            err.code()
        }
    }
}

fn dispatch(argv: &[String]) -> Result<commands::Outcome> {
    if argv.iter().any(|token| token == "--help" || token == "-h") {
        return Ok(commands::Outcome::ok(USAGE));
    }
    let (command, rest) = argv
        .split_first()
        .ok_or_else(|| CoreError::usage(USAGE.to_string()))?;
    match command.as_str() {
        "list" => commands::list(rest),
        "platform" => commands::platform(rest),
        "account" => commands::account(rest),
        "var" => commands::var(rest),
        "doctor" => commands::doctor(rest),
        "init" => commands::init(rest),
        other => Err(CoreError::usage(format!("未知子命令：{other}\n\n{USAGE}"))),
    }
}

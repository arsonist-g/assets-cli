//! 九个命令的实现：解析 → 调 core → 一行给人（和 AI）读的结果。
//!
//! 命令面里没有删除、没有取值、没有快照 —— 那是人（GUI）的动作，不是这里漏了。

use std::io::{IsTerminal, Read};

use assets_core::error::EXIT_DOCTOR;
use assets_core::manifest::{self, Filter};
use assets_core::ops::{self, Ctx, MetaPatch};
use assets_core::paths::Layout;
use assets_core::{doctor, init, store, CoreError, Result};

use crate::args::Args;

pub const USAGE_LIST: &str = "assets list [--platform <名称>] [--account <别名>] [--json]";
pub const USAGE_PLATFORM: &str =
    "assets platform <add|rename> …\n  assets platform add <名称> [--note <文本>]\n  assets platform rename <旧名> <新名>";
pub const USAGE_ACCOUNT: &str = "assets account <add|edit|rename> …\n  assets account add --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]\n  assets account edit --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]\n  assets account rename --platform <名称> --alias <旧别名> <新别名>";
pub const USAGE_VAR: &str =
    "printf '%s' \"$VALUE\" | assets var set --platform <名称> [--account <别名>] --term <术语>";
pub const USAGE_DOCTOR: &str = "assets doctor [--json]";
pub const USAGE_INIT: &str = "assets init [--json]";

/// 成功结果：`text` 写 stdout，`code` 是退出码（只有 doctor 会非 0）。
pub struct Outcome {
    pub text: String,
    pub code: i32,
}

impl Outcome {
    pub fn ok(text: impl Into<String>) -> Outcome {
        Outcome {
            text: text.into(),
            code: 0,
        }
    }

    pub fn with_code(text: impl Into<String>, code: i32) -> Outcome {
        Outcome {
            text: text.into(),
            code,
        }
    }
}

pub fn list(tokens: &[String]) -> Result<Outcome> {
    let args = Args::parse(tokens, USAGE_LIST, &["platform", "account"], &["json"])?;
    args.expect_positional(0, USAGE_LIST)?;
    let filter = Filter {
        platform: args.take("platform"),
        account: args.take("account"),
    };
    let ctx = Ctx::from_env()?;
    let doc = store::load(&ctx.paths)?;
    let view = manifest::view(&doc, &filter)?;
    let text = if args.has("json") {
        manifest::render_json(&view)?
    } else {
        manifest::render_markdown(&view)
    };
    Ok(Outcome::ok(text))
}

pub fn platform(tokens: &[String]) -> Result<Outcome> {
    let (sub, rest) = tokens
        .split_first()
        .ok_or_else(|| CoreError::usage(format!("platform 缺少子命令\n用法：{USAGE_PLATFORM}")))?;
    match sub.as_str() {
        "add" => {
            let usage = "assets platform add <名称> [--note <文本>]";
            let args = Args::parse(rest, usage, &["note"], &[])?;
            args.expect_positional(1, usage)?;
            let name = args.positional_at(0, "平台名", usage)?;
            let ctx = Ctx::from_env()?;
            ops::platform_add(&ctx, name, args.take("note")).map(Outcome::ok)
        }
        "rename" => {
            let usage = "assets platform rename <旧名> <新名>";
            let args = Args::parse(rest, usage, &[], &[])?;
            args.expect_positional(2, usage)?;
            let old = args.positional_at(0, "旧平台名", usage)?;
            let new = args.positional_at(1, "新平台名", usage)?;
            let ctx = Ctx::from_env()?;
            ops::platform_rename(&ctx, old, new).map(Outcome::ok)
        }
        other => Err(CoreError::usage(format!(
            "platform 不认识子命令：{other}\n用法：{USAGE_PLATFORM}"
        ))),
    }
}

pub fn account(tokens: &[String]) -> Result<Outcome> {
    let (sub, rest) = tokens
        .split_first()
        .ok_or_else(|| CoreError::usage(format!("account 缺少子命令\n用法：{USAGE_ACCOUNT}")))?;
    let account_opts = [
        "platform", "alias", "email", "purpose", "note", "host", "user",
    ];
    match sub.as_str() {
        "add" => {
            let usage = "assets account add --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]";
            let args = Args::parse(rest, usage, &account_opts, &[])?;
            args.expect_positional(0, usage)?;
            let platform = require(&args, "platform", usage)?;
            let alias = require(&args, "alias", usage)?;
            let ctx = Ctx::from_env()?;
            ops::account_add(&ctx, &platform, &alias, meta_from(&args)).map(Outcome::ok)
        }
        "edit" => {
            let usage = "assets account edit --platform <名称> --alias <别名> [--email <文本>] [--purpose <文本>] [--note <文本>] [--host <文本>] [--user <文本>]";
            let args = Args::parse(rest, usage, &account_opts, &[])?;
            args.expect_positional(0, usage)?;
            let platform = require(&args, "platform", usage)?;
            let alias = require(&args, "alias", usage)?;
            let ctx = Ctx::from_env()?;
            ops::account_edit(&ctx, &platform, &alias, meta_from(&args)).map(Outcome::ok)
        }
        "rename" => {
            let usage = "assets account rename --platform <名称> --alias <旧别名> <新别名>";
            let args = Args::parse(rest, usage, &["platform", "alias"], &[])?;
            args.expect_positional(1, usage)?;
            let platform = require(&args, "platform", usage)?;
            let old_alias = require(&args, "alias", usage)?;
            let new_alias = args.positional_at(0, "新别名", usage)?;
            let ctx = Ctx::from_env()?;
            ops::account_rename(&ctx, &platform, &old_alias, new_alias).map(Outcome::ok)
        }
        other => Err(CoreError::usage(format!(
            "account 不认识子命令：{other}\n用法：{USAGE_ACCOUNT}"
        ))),
    }
}

pub fn var(tokens: &[String]) -> Result<Outcome> {
    let (sub, rest) = tokens
        .split_first()
        .ok_or_else(|| CoreError::usage(format!("var 缺少子命令\n用法：{USAGE_VAR}")))?;
    match sub.as_str() {
        "set" => {
            let args = Args::parse(rest, USAGE_VAR, &["platform", "account", "term"], &[])?;
            args.expect_positional(0, USAGE_VAR)?;
            let platform = require(&args, "platform", USAGE_VAR)?;
            let term = require(&args, "term", USAGE_VAR)?;
            let account = args.take("account");
            let value = read_stdin_value()?;
            let ctx = Ctx::from_env()?;
            ops::var_set(&ctx, &platform, account.as_deref(), &term, &value).map(Outcome::ok)
        }
        other => Err(CoreError::usage(format!(
            "var 不认识子命令：{other}\n用法：{USAGE_VAR}"
        ))),
    }
}

pub fn doctor(tokens: &[String]) -> Result<Outcome> {
    let args = Args::parse(tokens, USAGE_DOCTOR, &[], &["json"])?;
    args.expect_positional(0, USAGE_DOCTOR)?;
    let ctx = Ctx::from_env()?;
    let layout = Layout::detect()?;
    let report = doctor::run(&ctx, &layout);
    let text = if args.has("json") {
        doctor::render_json(&report)
    } else {
        doctor::render_markdown(&report)
    };
    Ok(Outcome::with_code(
        text,
        if report.ok { 0 } else { EXIT_DOCTOR },
    ))
}

pub fn init(tokens: &[String]) -> Result<Outcome> {
    let args = Args::parse(tokens, USAGE_INIT, &[], &["json"])?;
    args.expect_positional(0, USAGE_INIT)?;
    let layout = Layout::detect()?;
    let report = init::run(&layout)?;
    let text = if args.has("json") {
        init::render_json(&report)?
    } else {
        init::render_markdown(&report)
    };
    Ok(Outcome::ok(text))
}

fn meta_from(args: &Args) -> MetaPatch {
    MetaPatch {
        email: args.take("email"),
        purpose: args.take("purpose"),
        note: args.take("note"),
        host: args.take("host"),
        user: args.take("user"),
    }
}

fn require(args: &Args, name: &str, usage: &str) -> Result<String> {
    args.take(name)
        .ok_or_else(|| CoreError::usage(format!("缺少 --{name}\n用法：{usage}")))
}

/// 值只从 stdin 进：不进 argv，也就进不了进程命令行与 shell 历史。
fn read_stdin_value() -> Result<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CoreError::usage(format!(
            "值必须从 stdin 进来（例如 printf '%s' \"$VALUE\" | …）\n用法：{USAGE_VAR}"
        )));
    }
    let mut raw = String::new();
    stdin
        .read_to_string(&mut raw)
        .map_err(|e| CoreError::storage_from("stdin 读取失败", e))?;
    Ok(raw.trim_end_matches(['\r', '\n']).to_string())
}

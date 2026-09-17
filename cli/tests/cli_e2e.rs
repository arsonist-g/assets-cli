//! CLI 端到端：九个命令的成功路径 + 每个退出码至少一条用例。
//!
//! 一律用环境变量把两个落点指到临时位置（USERPROFILE / ASSETS_CLI_DATA / ASSETS_CLI_REGISTRY），
//! 不碰真实台账、真实 profile 与 HKCU\Environment。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

struct Env {
    base: PathBuf,
    home: PathBuf,
    data_dir: PathBuf,
    registry_key: String,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Env {
    fn new(tag: &str) -> Env {
        let unique = format!("{}-{tag}", std::process::id());
        let base = std::env::temp_dir().join(format!("assets-cli-e2e-{unique}"));
        let home = base.join("home");
        let data_dir = base.join("data");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        Env {
            base,
            home,
            data_dir,
            registry_key: format!("Software\\assets-cli\\e2e-{unique}"),
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_assets"));
        for (name, _) in std::env::vars() {
            if name.starts_with("ASSETS_CLI_") {
                cmd.env_remove(&name);
            }
        }
        cmd.env("USERPROFILE", &self.home)
            .env("ASSETS_CLI_DATA", &self.data_dir)
            .env(
                "ASSETS_CLI_REGISTRY",
                format!("HKCU\\{}", self.registry_key),
            );
        cmd
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Run {
        self.run_with(args, stdin, &[])
    }

    fn run_with(&self, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Run {
        let mut cmd = self.command();
        for (name, value) in env {
            cmd.env(name, value);
        }
        let mut child = cmd
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("启动 assets");
        if let Some(text) = stdin {
            child
                .stdin
                .as_mut()
                .expect("stdin 管道")
                .write_all(text.as_bytes())
                .expect("写 stdin");
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("等待 assets");
        Run {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.delete_subkey_all(&self.registry_key) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("一次性注册表子键（{}）删除失败：{err}", self.registry_key),
        }
    }
}

const SECRET: &str = "cf-token-placeholder-0001";

#[test]
fn every_command_has_a_working_success_path() {
    let env = Env::new("happy");

    let add = env.run(
        &["platform", "add", "cloudflare", "--note", "CDN 与域名"],
        None,
    );
    assert_eq!(add.code, 0, "{}", add.stderr);
    assert!(
        add.stdout.contains("已新增平台 cloudflare"),
        "{}",
        add.stdout
    );

    let account = env.run(
        &[
            "account",
            "add",
            "--platform",
            "cloudflare",
            "--alias",
            "new",
            "--purpose",
            "新站",
            "--email",
            "ops@example.com",
        ],
        None,
    );
    assert_eq!(account.code, 0, "{}", account.stderr);
    assert!(
        account.stdout.contains("已新增条目 cloudflare/new"),
        "{}",
        account.stdout
    );

    let set = env.run(
        &[
            "var",
            "set",
            "--platform",
            "cloudflare",
            "--account",
            "new",
            "--term",
            "API_TOKEN",
        ],
        Some(SECRET),
    );
    assert_eq!(set.code, 0, "{}", set.stderr);
    assert!(
        set.stdout
            .contains("已写入 ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN"),
        "{}",
        set.stdout
    );

    let set_platform = env.run(
        &[
            "var",
            "set",
            "--platform",
            "cloudflare",
            "--term",
            "API_BASE_URL",
        ],
        Some("https://api.example.test\n"),
    );
    assert_eq!(set_platform.code, 0, "{}", set_platform.stderr);

    let edit = env.run(
        &[
            "account",
            "edit",
            "--platform",
            "cloudflare",
            "--alias",
            "new",
            "--note",
            "换个备注",
        ],
        None,
    );
    assert_eq!(edit.code, 0, "{}", edit.stderr);
    assert!(
        edit.stdout.contains("已更新 cloudflare/new：note"),
        "{}",
        edit.stdout
    );

    let rename_account = env.run(
        &[
            "account",
            "rename",
            "--platform",
            "cloudflare",
            "--alias",
            "new",
            "work",
        ],
        None,
    );
    assert_eq!(rename_account.code, 0, "{}", rename_account.stderr);
    assert!(
        rename_account
            .stdout
            .contains("已重命名 cloudflare/new → work"),
        "{}",
        rename_account.stdout
    );

    let rename_platform = env.run(&["platform", "rename", "cloudflare", "cf"], None);
    assert_eq!(rename_platform.code, 0, "{}", rename_platform.stderr);
    assert!(
        rename_platform.stdout.contains("已重命名 cloudflare → cf"),
        "{}",
        rename_platform.stdout
    );

    let list = env.run(&["list"], None);
    assert_eq!(list.code, 0, "{}", list.stderr);
    assert!(
        list.stdout.contains("# 资产清单 · 1 个平台 / 1 个账号"),
        "{}",
        list.stdout
    );
    assert!(
        list.stdout.contains("ASSETS_CLI_CF_WORK_API_TOKEN"),
        "{}",
        list.stdout
    );
    assert!(
        list.stdout
            .contains("purpose=新站, email=ops@example.com, note=换个备注"),
        "{}",
        list.stdout
    );
    assert!(!list.stdout.contains(SECRET), "清单永不输出值");

    let json = env.run(
        &["list", "--platform", "CF", "--account", "work", "--json"],
        None,
    );
    assert_eq!(json.code, 0, "{}", json.stderr);
    assert!(
        json.stdout
            .contains("\"name\": \"ASSETS_CLI_CF_WORK_API_TOKEN\""),
        "{}",
        json.stdout
    );
    assert!(!json.stdout.contains(SECRET), "JSON 清单同样不带值");

    let skill = env.run(&["skill", "install"], None);
    assert_eq!(skill.code, 0, "{}", skill.stderr);
    assert!(skill.stdout.contains("skill 包"), "{}", skill.stdout);
    let skill_dir = env.home.join(".agents").join("skills").join("assets");
    assert!(skill_dir.join("SKILL.md").is_file());
    assert!(skill_dir.join("skill-zh.md").is_file());
    assert!(skill_dir.join("references").join("errors.md").is_file());

    let skill_again = env.run(&["skill", "install"], None);
    assert_eq!(skill_again.code, 0, "{}", skill_again.stderr);
    assert!(
        !skill_again.stdout.contains("（新建）") && !skill_again.stdout.contains("（已修复）"),
        "第二次必须全部报已存在：{}",
        skill_again.stdout
    );
}

#[test]
fn exit_code_2_covers_usage_and_missing_references() {
    let env = Env::new("code2");
    assert_eq!(env.run(&[], None).code, 2);
    assert_eq!(env.run(&["nonsense"], None).code, 2);
    assert_eq!(env.run(&["skill"], None).code, 2);
    assert_eq!(env.run(&["skill", "nonsense"], None).code, 2);
    assert_eq!(env.run(&["list", "--account", "work"], None).code, 2);
    assert_eq!(env.run(&["list", "--platform", "nope"], None).code, 2);
    assert_eq!(env.run(&["list", "--bogus"], None).code, 2);
    assert_eq!(env.run(&["platform"], None).code, 2);
    assert_eq!(env.run(&["platform", "rename", "only-one"], None).code, 2);
    assert_eq!(env.run(&["account", "add", "--alias", "new"], None).code, 2);
    assert_eq!(env.run(&["var", "set", "--platform", "cf"], None).code, 2);

    env.run(&["platform", "add", "cf"], None);
    assert_eq!(
        env.run(
            &["account", "edit", "--platform", "cf", "--alias", "x"],
            None
        )
        .code,
        2
    );
}

#[test]
fn exit_code_3_is_validation_failure() {
    let env = Env::new("code3");
    let bad = env.run(&["platform", "add", "bad name"], None);
    assert_eq!(bad.code, 3, "{}", bad.stderr);
    assert!(bad.stderr.contains("只能包含"), "{}", bad.stderr);

    env.run(&["platform", "add", "cf"], None);
    let empty = env.run(
        &["var", "set", "--platform", "cf", "--term", "API_TOKEN"],
        Some(""),
    );
    assert_eq!(empty.code, 3, "{}", empty.stderr);
}

#[test]
fn exit_code_5_is_conflict() {
    let env = Env::new("code5");
    assert_eq!(env.run(&["platform", "add", "cf"], None).code, 0);
    let again = env.run(&["platform", "add", "CF"], None);
    assert_eq!(again.code, 5, "{}", again.stderr);
    assert!(again.stderr.contains("已存在"), "{}", again.stderr);
}

#[test]
fn exit_code_6_is_budget() {
    let env = Env::new("code6");
    env.run(&["platform", "add", "cf"], None);
    let huge = "x".repeat(32768);
    let run = env.run(
        &["var", "set", "--platform", "cf", "--term", "API_TOKEN"],
        Some(&huge),
    );
    assert_eq!(run.code, 6, "{}", run.stderr);
}

#[test]
fn exit_code_4_is_storage_failure() {
    let env = Env::new("code4");
    let blocker = env.base.join("blocker");
    fs::write(&blocker, "not a directory").unwrap();

    let run = env.run_with(
        &["platform", "add", "cf"],
        None,
        &[("ASSETS_CLI_DATA", blocker.join("sub").to_str().unwrap())],
    );
    assert_eq!(run.code, 4, "{}{}", run.stderr, run.stdout);
}

#[test]
fn init_is_idempotent_and_doctor_passes_afterwards() {
    let env = Env::new("init");

    let first = env.run(&["init"], None);
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert!(first.stdout.contains("# 安装与修复"), "{}", first.stdout);
    assert!(env.home.join(".assets-cli").join("hydrate.ps1").is_file());
    assert!(
        env.home.join(".profile").is_file(),
        "登录 profile 落在 .profile"
    );
    assert!(env.home.join(".bashrc").is_file());
    assert!(env
        .home
        .join("Documents")
        .join("PowerShell")
        .join("Microsoft.PowerShell_profile.ps1")
        .is_file());
    let skill_dir = env.home.join(".agents").join("skills").join("assets");
    assert!(skill_dir.join("SKILL.md").is_file());
    assert!(skill_dir.join("skill-zh.md").is_file());
    assert!(skill_dir
        .join("references")
        .join("install-and-config.md")
        .is_file());
    assert!(skill_dir.join("references").join("errors.md").is_file());

    let second = env.run(&["init"], None);
    assert_eq!(second.code, 0, "{}", second.stderr);
    let existing = second.stdout.matches("（已存在）").count();
    assert_eq!(
        existing, 12,
        "第二次每个落点都要报已存在：{}",
        second.stdout
    );

    let doctor = env.run(&["doctor"], None);
    assert_eq!(doctor.code, 0, "{}{}", doctor.stderr, doctor.stdout);
    assert!(doctor.stdout.contains("4/4 项通过"), "{}", doctor.stdout);

    let json = env.run(&["doctor", "--json"], None);
    assert_eq!(json.code, 0, "{}", json.stderr);
    assert!(
        json.stdout.contains("\"id\": \"hook_installed\""),
        "{}",
        json.stdout
    );
}

#[test]
fn doctor_fails_with_code_7_when_the_hook_is_missing() {
    let env = Env::new("doctor7");
    let run = env.run(&["doctor"], None);
    assert_eq!(run.code, 7, "{}{}", run.stderr, run.stdout);
    assert!(run.stdout.contains("hook_installed"), "{}", run.stdout);
    assert!(run.stdout.contains("assets init"), "{}", run.stdout);
}

#[test]
fn doctor_detects_a_session_that_is_not_fresh() {
    let env = Env::new("doctorfresh");
    env.run(&["init"], None);
    env.run(&["platform", "add", "cf"], None);
    env.run(
        &["var", "set", "--platform", "cf", "--term", "API_TOKEN"],
        Some(SECRET),
    );

    // 钩子照常工作：会话里的值就是注册表里的值
    let fresh = env.run_with(&["doctor"], None, &[("ASSETS_CLI_CF_API_TOKEN", SECRET)]);
    assert_eq!(fresh.code, 0, "{}{}", fresh.stderr, fresh.stdout);

    // 会话里拿到的是旧值：检查必须发现
    let stale = env.run_with(
        &["doctor"],
        None,
        &[("ASSETS_CLI_CF_API_TOKEN", "an-older-token")],
    );
    assert_eq!(stale.code, 7, "{}{}", stale.stderr, stale.stdout);
    assert!(stale.stdout.contains("不是最新"), "{}", stale.stdout);
    assert!(!stale.stdout.contains(SECRET), "自检不打印值");
    assert!(!stale.stdout.contains("an-older-token"), "自检不打印值");
}

#[test]
fn data_dir_override_expands_tilde() {
    let env = Env::new("tilde");
    let run = env.run_with(
        &["platform", "add", "cf"],
        None,
        &[("ASSETS_CLI_DATA", "~/assets-data")],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(env.home.join("assets-data").join("data.json").is_file());
}

#[test]
fn help_lists_the_command_surface_without_touching_anything() {
    let env = Env::new("help");
    let run = env.run(&["--help"], None);
    assert_eq!(run.code, 0, "{}", run.stderr);
    for expected in [
        "assets list",
        "assets platform add",
        "assets platform rename",
        "assets account add",
        "assets account edit",
        "assets account rename",
        "assets var set",
        "assets doctor",
        "assets init",
        "assets skill install",
    ] {
        assert!(run.stdout.contains(expected), "帮助里缺 {expected}");
    }
    assert!(!env.data_dir.join("data.json").exists());
}

//! core 的端到端行为测试。
//!
//! 一律显式指定数据目录与注册表根（一次性子键），绝不碰真实台账与 HKCU\Environment。

use std::fs;
use std::path::PathBuf;

use assets_core::manifest::{self, Filter};
use assets_core::ops::{self, Ctx, MetaPatch};
use assets_core::paths::{self, Layout, Paths};
use assets_core::{doctor, init, snapshot, store};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

/// 每个测试一套隔离落点：临时数据目录 + 一次性注册表子键。
struct Fixture {
    paths: Paths,
    home: PathBuf,
    registry_key: String,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        let unique = format!("{}-{}", std::process::id(), tag);
        let base = std::env::temp_dir().join(format!("assets-cli-test-{unique}"));
        let data_dir = base.join("data");
        let home = base.join("home");
        fs::create_dir_all(&data_dir).expect("建临时数据目录");
        fs::create_dir_all(&home).expect("建临时主目录");
        let registry_key = format!("Software\\assets-cli\\test-{unique}");
        Fixture {
            paths: Paths::explicit(&data_dir, &format!("HKCU\\{registry_key}")).expect("落点解析"),
            home,
            registry_key,
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx::new(self.paths.clone())
    }

    fn layout(&self) -> Layout {
        Layout::for_home(&self.home)
    }

    fn data_text(&self) -> String {
        fs::read_to_string(self.paths.data_file()).expect("读 data.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(base) = self.paths.data_dir.parent() {
            let _ = fs::remove_dir_all(base);
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.delete_subkey_all(&self.registry_key) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("一次性注册表子键（{}）删除失败：{err}", self.registry_key),
        }
    }
}

#[test]
fn lifecycle_writes_names_but_never_values_into_the_ledger() {
    let fixture = Fixture::new("lifecycle");
    let ctx = fixture.ctx();

    let message = ops::platform_add(&ctx, "cloudflare", Some("CDN 与域名".into())).unwrap();
    assert!(message.contains("已新增平台 cloudflare"), "{message}");

    ops::account_add(
        &ctx,
        "cloudflare",
        "new",
        MetaPatch {
            purpose: Some("新站".into()),
            email: Some("ops@example.com".into()),
            ..MetaPatch::default()
        },
    )
    .unwrap();

    let secret = "cf-token-placeholder-0001";
    ops::var_set(&ctx, "cloudflare", Some("new"), "API_TOKEN", secret).unwrap();
    ops::var_set(
        &ctx,
        "cloudflare",
        None,
        "API_BASE_URL",
        "https://api.example.test",
    )
    .unwrap();

    assert_eq!(
        ctx.registry
            .get("ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN")
            .unwrap()
            .as_deref(),
        Some(secret)
    );

    let ledger = fixture.data_text();
    assert!(!ledger.contains("ASSETS_CLI"), "变量名不该落盘");
    assert!(!ledger.contains(secret), "值不该进元数据文件");
    assert!(ledger.contains("\"term\": \"API_TOKEN\""));

    let doc = store::load(&fixture.paths).unwrap();
    let view = manifest::view(&doc, &Filter::default()).unwrap();
    let markdown = manifest::render_markdown(&view);
    assert!(
        markdown.contains("ASSETS_CLI_CLOUDFLARE_NEW_API_TOKEN"),
        "{markdown}"
    );
    assert!(markdown.contains("purpose=新站"), "{markdown}");
    assert!(!markdown.contains(secret), "清单永不输出值");

    let json = manifest::render_json(&view).unwrap();
    assert!(json.contains("\"term\": \"API_BASE_URL\""));
    assert!(json.contains("\"name\": \"ASSETS_CLI_CLOUDFLARE_API_BASE_URL\""));
    assert!(!json.contains(secret));
}

#[test]
fn list_filter_reports_missing_reference_as_usage_error() {
    let fixture = Fixture::new("filter");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "github", None).unwrap();
    ops::account_add(&ctx, "github", "work", MetaPatch::default()).unwrap();

    let doc = store::load(&fixture.paths).unwrap();
    let view = manifest::view(
        &doc,
        &Filter {
            platform: Some("github".into()),
            account: Some("WORK".into()),
        },
    )
    .unwrap();
    assert_eq!(view.platforms.len(), 1);
    assert_eq!(view.platforms[0].accounts.len(), 1);

    let missing = manifest::view(
        &doc,
        &Filter {
            platform: Some("gitlab".into()),
            account: None,
        },
    )
    .unwrap_err();
    assert_eq!(missing.code(), 2);

    let orphan_account = manifest::view(
        &doc,
        &Filter {
            platform: None,
            account: Some("work".into()),
        },
    )
    .unwrap_err();
    assert_eq!(orphan_account.code(), 2);
}

#[test]
fn exit_codes_for_validation_conflict_and_missing_reference() {
    let fixture = Fixture::new("codes");
    let ctx = fixture.ctx();

    assert_eq!(
        ops::platform_add(&ctx, "cloud flare", None)
            .unwrap_err()
            .code(),
        3
    );
    assert_eq!(
        ops::account_add(&ctx, "nope", "x", MetaPatch::default())
            .unwrap_err()
            .code(),
        2
    );
    assert_eq!(
        ops::platform_rename(&ctx, "nope", "other")
            .unwrap_err()
            .code(),
        2
    );

    ops::platform_add(&ctx, "cloudflare", None).unwrap();
    assert_eq!(
        ops::platform_add(&ctx, "CloudFlare", None)
            .unwrap_err()
            .code(),
        5
    );
    assert_eq!(
        ops::var_set(&ctx, "cloudflare", None, "API BASE", "v")
            .unwrap_err()
            .code(),
        3
    );
    assert_eq!(
        ops::var_set(&ctx, "cloudflare", None, "API_TOKEN", "")
            .unwrap_err()
            .code(),
        3
    );

    let too_long = "x".repeat(assets_core::naming::MAX_VALUE_LEN + 1);
    assert_eq!(
        ops::var_set(&ctx, "cloudflare", None, "API_TOKEN", &too_long)
            .unwrap_err()
            .code(),
        6
    );

    ops::account_add(&ctx, "cloudflare", "new", MetaPatch::default()).unwrap();
    assert_eq!(
        ops::account_add(&ctx, "cloudflare", "NEW", MetaPatch::default())
            .unwrap_err()
            .code(),
        5
    );
    assert_eq!(
        ops::account_edit(&ctx, "cloudflare", "new", MetaPatch::default())
            .unwrap_err()
            .code(),
        2
    );
}

#[test]
fn var_set_refuses_to_clobber_an_unmanaged_registry_value() {
    let fixture = Fixture::new("r4");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "cloudflare", None).unwrap();

    ctx.registry
        .set("ASSETS_CLI_CLOUDFLARE_MANUAL_TOKEN", "hand-made")
        .unwrap();

    let err = ops::var_set(&ctx, "cloudflare", None, "MANUAL_TOKEN", "new").unwrap_err();
    assert_eq!(err.code(), 5);
    assert_eq!(
        ctx.registry
            .get("ASSETS_CLI_CLOUDFLARE_MANUAL_TOKEN")
            .unwrap()
            .as_deref(),
        Some("hand-made"),
        "拒绝写入时原值必须原样保留"
    );
}

#[test]
fn rename_cascades_by_scope() {
    let fixture = Fixture::new("cascade");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "cf", None).unwrap();
    ops::account_add(&ctx, "cf", "new", MetaPatch::default()).unwrap();
    ops::var_set(&ctx, "cf", None, "API_BASE_URL", "https://api.example.test").unwrap();
    ops::var_set(&ctx, "cf", Some("new"), "API_TOKEN", "token-0001").unwrap();

    let message = ops::platform_rename(&ctx, "cf", "cf2").unwrap();
    assert!(message.contains("已重命名 cf → cf2"), "{message}");
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF2_API_BASE_URL")
        .unwrap()
        .is_some());
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF2_NEW_API_TOKEN")
        .unwrap()
        .is_some());
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF_API_BASE_URL")
        .unwrap()
        .is_none());
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF_NEW_API_TOKEN")
        .unwrap()
        .is_none());

    ops::account_rename(&ctx, "cf2", "new", "work").unwrap();
    assert_eq!(
        ctx.registry
            .get("ASSETS_CLI_CF2_WORK_API_TOKEN")
            .unwrap()
            .as_deref(),
        Some("token-0001")
    );
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF2_NEW_API_TOKEN")
        .unwrap()
        .is_none());
    assert_eq!(
        ctx.registry
            .get("ASSETS_CLI_CF2_API_BASE_URL")
            .unwrap()
            .as_deref(),
        Some("https://api.example.test"),
        "平台级变量不参与账号改名"
    );
}

#[test]
fn rename_refuses_when_ledger_and_registry_disagree() {
    let fixture = Fixture::new("desync");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "cf", None).unwrap();
    ops::var_set(&ctx, "cf", None, "API_BASE_URL", "https://api.example.test").unwrap();

    ctx.registry.delete("ASSETS_CLI_CF_API_BASE_URL").unwrap();

    let err = ops::platform_rename(&ctx, "cf", "cf2").unwrap_err();
    assert_eq!(err.code(), 4, "{err}");
    assert!(ctx
        .registry
        .get("ASSETS_CLI_CF2_API_BASE_URL")
        .unwrap()
        .is_none());
    assert!(store::load(&fixture.paths).unwrap().has_platform("cf"));
}

#[test]
fn snapshots_are_taken_before_every_write_and_pruned_by_capacity() {
    let fixture = Fixture::new("snapshots");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "cf", None).unwrap();
    ops::var_set(&ctx, "cf", None, "API_TOKEN", "token-0001").unwrap();
    ops::var_set(&ctx, "cf", None, "API_TOKEN", "token-0002").unwrap();

    let listed = snapshot::list(&fixture.paths).unwrap();
    assert!(
        listed.len() >= 3,
        "每次写入前都建一份，实际 {}",
        listed.len()
    );

    let removed = snapshot::prune_with_quota(&fixture.paths, 0).unwrap();
    assert!(!removed.is_empty());
    let left = snapshot::list(&fixture.paths).unwrap();
    assert_eq!(left.len(), 1, "最新的那一份永远保留");
}

#[test]
fn restore_makes_state_equal_to_the_snapshot() {
    let fixture = Fixture::new("restore");
    let ctx = fixture.ctx();
    ops::platform_add(&ctx, "cf", None).unwrap();
    ops::var_set(&ctx, "cf", None, "API_TOKEN", "token-0001").unwrap();

    let doc = store::load(&fixture.paths).unwrap();
    let before_file = snapshot::create(&fixture.paths, &ctx.registry, &doc).unwrap();

    ops::var_set(&ctx, "cf", None, "PROXY", "http://proxy.example.test:8080").unwrap();
    ops::account_add(&ctx, "cf", "later", MetaPatch::default()).unwrap();
    assert!(ctx.registry.get("ASSETS_CLI_CF_PROXY").unwrap().is_some());

    let report = snapshot::restore(&fixture.paths, &ctx.registry, &before_file).unwrap();
    assert_eq!(report.variables_written, 1);
    assert_eq!(report.variables_removed, vec!["ASSETS_CLI_CF_PROXY"]);
    assert!(ctx.registry.get("ASSETS_CLI_CF_PROXY").unwrap().is_none());
    assert_eq!(
        ctx.registry
            .get("ASSETS_CLI_CF_API_TOKEN")
            .unwrap()
            .as_deref(),
        Some("token-0001")
    );
    let restored = store::load(&fixture.paths).unwrap();
    assert!(!restored.find_platform("cf").unwrap().has_account("later"));
}

#[test]
fn init_is_idempotent_and_doctor_sees_the_hook() {
    let fixture = Fixture::new("init");
    let ctx = fixture.ctx();
    let layout = fixture.layout();

    let before = doctor::run(&ctx, &layout);
    assert!(!before.ok);
    assert!(!check(&before, "hook_installed").ok);
    assert!(check(&before, "registry_writable").ok);
    assert!(check(&before, "data_dir_writable").ok);

    let first = init::run(&layout).unwrap();
    for step in &first.steps {
        assert_eq!(step.result, init::StepResult::Created, "{}", step.title);
    }
    assert!(layout.hook_dir.join("hydrate.ps1").is_file());
    assert!(layout.hook_dir.join("emit-sh.ps1").is_file());
    assert!(layout.hook_dir.join("hydrate.sh").is_file());
    assert!(layout.skill_file.is_file());

    let second = init::run(&layout).unwrap();
    for step in &second.steps {
        assert_eq!(
            step.result,
            init::StepResult::Existing,
            "第二次必须全部报已存在：{} {}",
            step.title,
            step.detail.join("；")
        );
    }

    let after = doctor::run(&ctx, &layout);
    assert!(check(&after, "hook_installed").ok, "{:?}", after);
    assert!(after.ok, "{:?}", after);
}

#[test]
fn init_repairs_a_stale_block_without_touching_the_rest_of_the_profile() {
    let fixture = Fixture::new("repair");
    let layout = fixture.layout();
    fs::create_dir_all(layout.ps_profile.parent().unwrap()).unwrap();
    fs::write(
        &layout.ps_profile,
        "# 用户自己的 profile 内容\n# >>> assets-cli >>>\n旧的调用\n# <<< assets-cli <<<\nafter = 1\n",
    )
    .unwrap();

    let report = init::run(&layout).unwrap();
    let profile_step = report
        .steps
        .iter()
        .find(|step| step.id == "profile_blocks")
        .unwrap();
    assert_eq!(profile_step.result, init::StepResult::Repaired);

    let text = fs::read_to_string(&layout.ps_profile).unwrap();
    assert!(text.starts_with("# 用户自己的 profile 内容\n"));
    assert!(text.contains("after = 1"));
    assert!(!text.contains("旧的调用"));
    assert!(text.contains(init::MARKER_BEGIN) && text.contains(init::MARKER_END));
}

#[test]
fn chinese_metadata_and_paths_with_spaces_round_trip() {
    let fixture = Fixture::new("chinese paths");
    let ctx = fixture.ctx();
    assert!(fixture.paths.data_dir.to_string_lossy().contains(' '));

    ops::platform_add(&ctx, "cloudflare", Some("测试平台".into())).unwrap();
    ops::account_add(
        &ctx,
        "cloudflare",
        "new",
        MetaPatch {
            purpose: Some("新站 · 备注里带标点".into()),
            note: Some("换行\n与制表符\t都要能存".into()),
            host: Some("server.example.test".into()),
            user: Some("deploy".into()),
            ..MetaPatch::default()
        },
    )
    .unwrap();

    let doc = store::load(&fixture.paths).unwrap();
    let account = doc
        .find_platform("cloudflare")
        .unwrap()
        .find_account("new")
        .unwrap();
    assert_eq!(account.purpose.as_deref(), Some("新站 · 备注里带标点"));
    assert_eq!(account.note.as_deref(), Some("换行\n与制表符\t都要能存"));

    ops::account_edit(
        &ctx,
        "cloudflare",
        "new",
        MetaPatch {
            note: Some(String::new()),
            ..MetaPatch::default()
        },
    )
    .unwrap();
    let doc = store::load(&fixture.paths).unwrap();
    assert!(doc
        .find_platform("cloudflare")
        .unwrap()
        .find_account("new")
        .unwrap()
        .note
        .is_none());
}

#[test]
fn tilde_paths_expand_to_the_user_home() {
    let home = paths::home_dir().unwrap();
    assert_eq!(paths::expand_user_path("~/x").unwrap(), home.join("x"));
    assert_eq!(paths::expand_user_path("~").unwrap(), home);
    assert_eq!(
        paths::expand_user_path("D:\\some dir").unwrap(),
        PathBuf::from("D:\\some dir")
    );
}

fn check<'a>(report: &'a doctor::Report, id: &str) -> &'a doctor::Check {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("没有这项检查：{id}"))
}

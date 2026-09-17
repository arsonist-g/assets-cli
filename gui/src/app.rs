//! 界面层：把 core 的台账读成界面需要的形状，把界面上的动作写回 core。
//!
//! 这里不做命名 / 事务 / 预算的判断 —— 那些全在 `assets-core` 里，
//! 界面只负责「取数据 → 摆到屏幕上」与「把人的意图交给 core」。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use assets_core::model::{Account, DataDoc, Platform, Snapshot, VarDecl};
use assets_core::naming;
use assets_core::ops::{self, Ctx, MetaPatch};
use assets_core::paths::{Hive, Layout, Paths, RegistrySpec};
use assets_core::{doctor, snapshot, store, CoreError};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, CheckLine, ListLine, SnapshotLine, TreeRow};

/// 界面形态：S1 ~ S12 的落点。
#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    Overview,
    Platform(String),
    Account(String, String),
    Form,
    Snapshots,
    Doctor,
    Empty,
    Fault,
}

impl View {
    fn name(&self) -> &'static str {
        match self {
            View::Overview => "overview",
            View::Platform(_) => "platform",
            View::Account(_, _) => "account",
            View::Form => "form",
            View::Snapshots => "snapshots",
            View::Doctor => "doctor",
            View::Empty => "empty",
            View::Fault => "fault",
        }
    }
}

/// 表单在编辑什么：新建平台 / 新建条目 / 新增变量 / 改值。
///
/// 改名走浮层（S7）：它的核心是「旧名 → 新名」的级联对照表，不是普通表单。
#[derive(Debug, Clone, PartialEq, Eq)]
enum FormKind {
    NewPlatform,
    NewAccount(String),
    NewVar(String, Option<String>),
    EditVar(String, Option<String>, String),
}

/// 待确认的动作（S6 / S7 / S9）。
#[derive(Debug, Clone)]
enum Confirm {
    DeletePlatform(String),
    DeleteAccount(String, String),
    DeleteVar(String, Option<String>, String),
    RestoreSnapshot(usize),
    DeleteSnapshot(usize),
    RenamePlatform(String),
    RenameAccount(String, String),
}

struct App {
    ctx: Ctx,
    layout: Layout,
    doc: DataDoc,
    values: BTreeMap<String, String>,
    snaps: Vec<snapshot::SnapshotInfo>,
    report: doctor::Report,
    fault: Option<String>,
    view: View,
    /// 表单/浮层之外的返回点（打开表单前停在哪一屏）。
    back: View,
    selection: String,
    form: Option<FormKind>,
    form_error: Option<String>,
    form_value: String,
    form_nonce: i32,
    pushed_nonce: i32,
    confirm: Option<Confirm>,
    confirm_error: Option<String>,
    rename_input: String,
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let app = Rc::new(RefCell::new(App::load()));
    app.borrow_mut().render(&ui);
    wire(&ui, &app);
    ui.run()
}

fn wire(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    // `state` / `win` 必须从调用点传进来：宏内部新造的标识符对 `$body` 不可见（macro_rules 的卫生规则）。
    macro_rules! bind {
        ($setter:ident, $state:ident, $win:ident, || $body:block) => {{
            let app = app.clone();
            let weak = ui.as_weak();
            ui.$setter(move || {
                let Some($win) = weak.upgrade() else { return };
                let mut $state = app.borrow_mut();
                $body
                $state.render(&$win);
            });
        }};
        ($setter:ident, $state:ident, $win:ident, |$arg:ident : $ty:ty| $body:block) => {{
            let app = app.clone();
            let weak = ui.as_weak();
            ui.$setter(move |$arg: $ty| {
                let Some($win) = weak.upgrade() else { return };
                let mut $state = app.borrow_mut();
                $body
                $state.render(&$win);
            });
        }};
    }

    bind!(on_select, state, _win, |key: slint::SharedString| { state.select(key.as_str()); });
    bind!(on_nav, state, _win, |view: slint::SharedString| { state.nav(view.as_str()); });
    bind!(on_new_platform, state, _win, || { state.start_new_platform(); });
    bind!(on_new_account, state, _win, || { state.start_new_account(); });
    bind!(on_new_var, state, _win, || { state.start_new_var(); });
    bind!(on_rename, state, _win, || { state.start_rename(); });
    bind!(on_remove, state, _win, || { state.start_remove(); });
    bind!(on_edit_var, state, _win, |key: slint::SharedString| { state.start_edit_var(key.as_str()); });
    bind!(on_remove_var, state, _win, |key: slint::SharedString| { state.start_remove_var(key.as_str()); });
    // 这两处要读界面上的输入，所以把升级后的窗口交给回调体（叫 win）。
    bind!(on_submit_form, state, win, || { state.submit_form(&win); });
    bind!(on_confirm_yes, state, win, || { state.confirm_yes(&win); });
    bind!(on_cancel_form, state, _win, || { state.cancel_form(); });
    bind!(on_restore_snapshot, state, _win, |index: i32| { state.start_restore(index); });
    bind!(on_delete_snapshot, state, _win, |index: i32| { state.start_delete_snapshot(index); });
    bind!(on_rerun_doctor, state, _win, || { state.rerun_doctor(); });
    bind!(on_confirm_no, state, _win, || { state.confirm = None; state.confirm_error = None; });
    bind!(on_cf_input_changed, state, _win, |text: slint::SharedString| { state.rename_input = text.to_string(); });
}

/// 落点解析失败时的兜底：只为让界面能起来并报出故障，不做任何写入。
fn fallback_paths() -> Paths {
    Paths {
        data_dir: PathBuf::from(".assets-cli"),
        registry: RegistrySpec {
            hive: Hive::CurrentUser,
            subkey: "Environment".to_string(),
        },
    }
}

/// 变量名的分段呈现：常量前缀与实体段分开（MASTER §6 的签名元素）。
fn signature(name: &str) -> (String, String) {
    let prefix = naming::PREFIX.to_string();
    let head = name.get(..naming::PREFIX.len());
    let entity = match head {
        Some(head) if head.eq_ignore_ascii_case(naming::PREFIX) => {
            name[naming::PREFIX.len()..].to_string()
        }
        _ => name.to_string(),
    };
    (prefix, entity)
}

/// 变量行的键：`v|<平台>|<别名或空>|<术语>`。
fn var_key(platform: &str, alias: Option<&str>, term: &str) -> String {
    format!("v|{platform}|{}|{term}", alias.unwrap_or(""))
}

fn parse_var_key(key: &str) -> Option<(String, Option<String>, String)> {
    let mut parts = key.split('|');
    if parts.next()? != "v" {
        return None;
    }
    let platform = parts.next()?.to_string();
    let alias = parts.next()?;
    let term = parts.next()?;
    Some((
        platform,
        if alias.is_empty() {
            None
        } else {
            Some(alias.to_string())
        },
        term.to_string(),
    ))
}

/// `2026-09-17T12:34:56Z` → `2026-09-17 12:34:56`。
fn stamp(iso: &str) -> String {
    iso.trim_end_matches('Z').replace('T', " ")
}

fn human_size(bytes: u64) -> String {
    let value = bytes as f64;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", value / 1048576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", value / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn trimmed(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn blank(kind: &str, text: impl Into<slint::SharedString>) -> ListLine {
    ListLine {
        key: "".into(),
        kind: kind.into(),
        text: text.into(),
        prefix: "".into(),
        entity: "".into(),
        meta: "".into(),
        has_value: false,
    }
}

fn field(label: &str, value: impl Into<slint::SharedString>) -> ListLine {
    ListLine {
        key: "".into(),
        kind: "field".into(),
        text: label.into(),
        prefix: "".into(),
        entity: "".into(),
        meta: value.into(),
        has_value: false,
    }
}

/// 变量行：键可回指到具体变量（改值 / 删除都靠它）。
fn var_line(platform: &str, alias: Option<&str>, term: &str, name: &str, meta: &str, has_value: bool) -> ListLine {
    let (prefix, entity) = signature(name);
    ListLine {
        key: var_key(platform, alias, term).into(),
        kind: "var".into(),
        text: name.into(),
        prefix: prefix.into(),
        entity: entity.into(),
        meta: meta.into(),
        has_value,
    }
}
impl App {
    fn load() -> App {
        let (paths, mut fault) = match Paths::from_env() {
            Ok(paths) => (paths, None),
            Err(err) => (fallback_paths(), Some(format!("落点解析失败：{err}"))),
        };
        let ctx = Ctx::new(paths);
        let layout = Layout::detect().unwrap_or_else(|_| Layout::for_home(PathBuf::from(".")));
        let doc = match store::load(&ctx.paths) {
            Ok(doc) => doc,
            Err(err) => {
                fault = Some(err.to_string());
                DataDoc::empty()
            }
        };
        let values = ctx.registry.asset_entries().unwrap_or_default();
        let mut snaps = snapshot::list(&ctx.paths).unwrap_or_default();
        snaps.reverse();
        let report = doctor::run(&ctx, &layout);
        let view = if fault.is_some() {
            View::Fault
        } else if doc.platforms.is_empty() {
            View::Empty
        } else {
            View::Overview
        };
        App {
            ctx,
            layout,
            doc,
            values,
            snaps,
            report,
            fault,
            view,
            back: View::Overview,
            selection: String::new(),
            form: None,
            form_error: None,
            form_value: String::new(),
            form_nonce: 0,
            pushed_nonce: 0,
            confirm: None,
            confirm_error: None,
            rename_input: String::new(),
        }
    }

    /// 重新读台账 / 注册表 / 快照 / 自检；每次成功写入之后都走这里。
    fn refresh(&mut self) {
        match store::load(&self.ctx.paths) {
            Ok(doc) => {
                self.doc = doc;
                self.fault = None;
            }
            Err(err) => self.fault = Some(err.to_string()),
        }
        self.values = self.ctx.registry.asset_entries().unwrap_or_default();
        let mut snaps = snapshot::list(&self.ctx.paths).unwrap_or_default();
        snaps.reverse();
        self.snaps = snaps;
        self.report = doctor::run(&self.ctx, &self.layout);
    }

    /// 台账变了之后，把视图落回一个还存在的地方。
    fn settle_view(&mut self) {
        self.selection.clear();
        if self.fault.is_some() {
            self.view = View::Fault;
            return;
        }
        if self.doc.platforms.is_empty() {
            self.view = View::Empty;
            return;
        }
        match self.view.clone() {
            View::Empty | View::Fault => self.view = View::Overview,
            View::Platform(name) => {
                if !self.doc.has_platform(&name) {
                    self.view = View::Overview;
                }
            }
            View::Account(platform, alias) => {
                let alive = self
                    .doc
                    .find_platform(&platform)
                    .map(|p| p.has_account(&alias))
                    .unwrap_or(false);
                if !alive {
                    self.view = View::Overview;
                }
            }
            _ => {}
        }
    }

    fn var_name_of(&self, platform: &str, alias: Option<&str>, term: &str) -> String {
        match alias {
            Some(alias) => naming::account_var_name(platform, alias, term),
            None => naming::platform_var_name(platform, term),
        }
        .unwrap_or_default()
    }

    // ── 导航 ──

    fn select(&mut self, key: &str) {
        // 变量行：选中它的宿主，并直接打开改值表单（S4）。
        if let Some((platform, alias, term)) = parse_var_key(key) {
            self.start_edit_var_at(&platform, alias.as_deref(), &term);
            return;
        }
        let mut parts = key.split('|');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("p"), Some(name), None) if self.doc.has_platform(name) => {
                self.selection = key.to_string();
                self.view = View::Platform(name.to_string());
            }
            (Some("a"), Some(platform), Some(alias))
                if self
                    .doc
                    .find_platform(platform)
                    .map(|p| p.has_account(alias))
                    .unwrap_or(false) =>
            {
                self.selection = key.to_string();
                self.view = View::Account(platform.to_string(), alias.to_string());
            }
            _ => {}
        }
    }

    fn nav(&mut self, target: &str) {
        match target {
            "doctor" => {
                self.report = doctor::run(&self.ctx, &self.layout);
                self.view = View::Doctor;
            }
            "snapshots" => {
                let mut snaps = snapshot::list(&self.ctx.paths).unwrap_or_default();
                snaps.reverse();
                self.snaps = snaps;
                self.view = View::Snapshots;
            }
            "overview" => {
                self.view = if self.fault.is_some() {
                    View::Fault
                } else if self.doc.platforms.is_empty() {
                    View::Empty
                } else {
                    View::Overview
                };
            }
            _ => {}
        }
    }

    // ── 开表单 / 开浮层 ──

    fn open_form(&mut self, kind: FormKind, value: String) {
        if !matches!(self.view, View::Form) {
            self.back = self.view.clone();
        }
        self.form = Some(kind);
        self.form_error = None;
        self.form_value = value;
        self.form_nonce += 1;
        self.view = View::Form;
    }

    fn open_confirm(&mut self, confirm: Confirm) {
        self.rename_input = match &confirm {
            Confirm::RenamePlatform(name) => name.clone(),
            Confirm::RenameAccount(_, alias) => alias.clone(),
            _ => String::new(),
        };
        self.confirm_error = None;
        self.confirm = Some(confirm);
    }

    fn start_new_platform(&mut self) {
        self.open_form(FormKind::NewPlatform, String::new());
    }

    fn start_new_account(&mut self) {
        if let View::Platform(name) = self.view.clone() {
            self.open_form(FormKind::NewAccount(name), String::new());
        }
    }

    fn start_new_var(&mut self) {
        match self.view.clone() {
            View::Platform(name) => self.open_form(FormKind::NewVar(name, None), String::new()),
            View::Account(platform, alias) => {
                self.open_form(FormKind::NewVar(platform, Some(alias)), String::new())
            }
            _ => {}
        }
    }

    fn start_edit_var(&mut self, key: &str) {
        if let Some((platform, alias, term)) = parse_var_key(key) {
            self.start_edit_var_at(&platform, alias.as_deref(), &term);
        }
    }

    fn start_edit_var_at(&mut self, platform: &str, alias: Option<&str>, term: &str) {
        let name = self.var_name_of(platform, alias, term);
        let value = self.values.get(&name).cloned().unwrap_or_default();
        self.open_form(
            FormKind::EditVar(
                platform.to_string(),
                alias.map(str::to_string),
                term.to_string(),
            ),
            value,
        );
    }

    fn start_remove_var(&mut self, key: &str) {
        if let Some((platform, alias, term)) = parse_var_key(key) {
            self.open_confirm(Confirm::DeleteVar(platform, alias, term));
        }
    }

    fn start_rename(&mut self) {
        match self.view.clone() {
            View::Platform(name) => self.open_confirm(Confirm::RenamePlatform(name)),
            View::Account(platform, alias) => {
                self.open_confirm(Confirm::RenameAccount(platform, alias))
            }
            _ => {}
        }
    }

    fn start_remove(&mut self) {
        match self.view.clone() {
            View::Platform(name) => self.open_confirm(Confirm::DeletePlatform(name)),
            View::Account(platform, alias) => {
                self.open_confirm(Confirm::DeleteAccount(platform, alias))
            }
            _ => {}
        }
    }

    fn start_restore(&mut self, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if index < self.snaps.len() {
            self.open_confirm(Confirm::RestoreSnapshot(index));
        }
    }

    fn start_delete_snapshot(&mut self, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if index < self.snaps.len() {
            self.open_confirm(Confirm::DeleteSnapshot(index));
        }
    }

    fn rerun_doctor(&mut self) {
        self.report = doctor::run(&self.ctx, &self.layout);
    }

    fn cancel_form(&mut self) {
        self.form = None;
        self.form_error = None;
        self.view = self.back.clone();
    }

    // ── 提交 ──

    fn submit_form(&mut self, ui: &AppWindow) {
        let Some(kind) = self.form.clone() else {
            return;
        };
        let term = ui.get_fm_term().to_string();
        let value = ui.get_fm_value().to_string();
        let meta = MetaPatch {
            email: trimmed(ui.get_fm_email().as_str()),
            purpose: trimmed(ui.get_fm_purpose().as_str()),
            note: trimmed(ui.get_fm_note().as_str()),
            host: trimmed(ui.get_fm_host().as_str()),
            user: trimmed(ui.get_fm_user().as_str()),
        };
        let outcome = match kind {
            FormKind::NewPlatform => ops::platform_add(&self.ctx, &term, meta.note.clone()),
            FormKind::NewAccount(platform) => ops::account_add(&self.ctx, &platform, &term, meta),
            FormKind::NewVar(platform, alias) => {
                ops::var_set(&self.ctx, &platform, alias.as_deref(), &term, &value)
            }
            FormKind::EditVar(platform, alias, old_term) => {
                ops::var_set(&self.ctx, &platform, alias.as_deref(), &old_term, &value)
            }
        };
        match outcome {
            Ok(_) => {
                self.form = None;
                self.form_error = None;
                self.view = self.back.clone();
                self.refresh();
                self.settle_view();
            }
            Err(err) => self.form_error = Some(err.to_string()),
        }
    }

    fn confirm_yes(&mut self, ui: &AppWindow) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };
        let proposed = ui.get_cf_input().to_string();
        let outcome: assets_core::Result<String> = match &confirm {
            Confirm::DeletePlatform(name) => ops::platform_delete(&self.ctx, name),
            Confirm::DeleteAccount(platform, alias) => {
                ops::account_delete(&self.ctx, platform, alias)
            }
            Confirm::DeleteVar(platform, alias, term) => {
                ops::var_delete(&self.ctx, platform, alias.as_deref(), term)
            }
            Confirm::RenamePlatform(name) => ops::platform_rename(&self.ctx, name, &proposed),
            Confirm::RenameAccount(platform, alias) => {
                ops::account_rename(&self.ctx, platform, alias, &proposed)
            }
            Confirm::RestoreSnapshot(index) => match self.snaps.get(*index) {
                Some(info) => snapshot::restore(&self.ctx.paths, &self.ctx.registry, &info.file)
                    .map(|report| {
                        format!(
                            "已恢复到 {}：写回 {} 个变量，清掉 {} 个",
                            stamp(&report.created_at),
                            report.variables_written,
                            report.variables_removed.len()
                        )
                    }),
                None => Err(CoreError::usage("快照已经不在列表里了，重新打开快照屏")),
            },
            Confirm::DeleteSnapshot(index) => match self.snaps.get(*index) {
                Some(info) => snapshot::delete(&self.ctx.paths, &info.file)
                    .map(|_| format!("已删除快照 {}", stamp(&info.created_at))),
                None => Err(CoreError::usage("快照已经不在列表里了，重新打开快照屏")),
            },
        };
        match outcome {
            Ok(_) => {
                self.confirm = None;
                self.confirm_error = None;
                self.refresh();
                self.settle_view();
            }
            // 整体已回退，浮层不关：把原因就地摆出来，人可以改输入或取消。
            Err(err) => self.confirm_error = Some(err.to_string()),
        }
    }
}
impl App {
    // ── 渲染 ──

    fn render(&mut self, ui: &AppWindow) {
        ui.set_view(self.view.name().into());
        ui.set_data_dir(self.ctx.paths.data_dir.display().to_string().into());

        let (tone, label) = self.doctor_badge();
        ui.set_doctor_tone(tone.into());
        ui.set_doctor_label(label.into());

        let (latest, quota, warn) = self.footer();
        ui.set_foot_snapshot(latest.into());
        ui.set_foot_quota(quota.into());
        ui.set_quota_warning(warn);

        ui.set_tree(ModelRc::new(VecModel::from(self.tree_rows())));
        ui.set_selection(self.selection.clone().into());

        match self.view.clone() {
            View::Overview => self.render_overview(ui),
            View::Platform(_) | View::Account(_, _) => self.render_detail(ui),
            View::Form => self.render_form(ui),
            View::Snapshots => self.render_snapshots(ui),
            View::Doctor => self.render_doctor(ui),
            View::Empty => self.render_empty(ui),
            View::Fault => self.render_fault(ui),
        }

        self.render_overlay(ui);
    }

    fn render_overview(&self, ui: &AppWindow) {
        let platforms = self.doc.platforms.len();
        let accounts: usize = self.doc.platforms.iter().map(|p| p.accounts.len()).sum();
        ui.set_ov_heading("资产总览".into());
        ui.set_ov_summary(
            format!(
                "{platforms} 个平台 · {accounts} 个账号 · {} 个变量；清单永远是真相源，进程环境只是快照。",
                self.declared_count()
            )
            .into(),
        );
        ui.set_ov_lines(ModelRc::new(VecModel::from(self.overview_lines())));
    }

    fn render_detail(&self, ui: &AppWindow) {
        match self.view.clone() {
            View::Platform(name) => {
                let Some(platform) = self.doc.find_platform(&name) else {
                    return;
                };
                let account_vars: usize =
                    platform.accounts.iter().map(|a| a.variables.len()).sum();
                ui.set_dt_heading(platform.name.clone().into());
                ui.set_dt_summary(
                    format!(
                        "{} 个平台级变量 · {} 个账号 · 账号级 {} 个变量",
                        platform.variables.len(),
                        platform.accounts.len(),
                        account_vars
                    )
                    .into(),
                );
                ui.set_dt_can_delete(true);
                ui.set_dt_can_add_account(true);

                let mut lines = vec![
                    field("备注", platform.note.clone().unwrap_or_default()),
                    field("创建", stamp(&platform.created_at)),
                    blank(
                        "group",
                        format!("平台级变量 · {} 个", platform.variables.len()),
                    ),
                ];
                for decl in sorted_terms(&platform.variables) {
                    let var = naming::platform_var_name(&platform.name, &decl.term).unwrap_or_default();
                    let has = self.has_value(&var);
                    lines.push(var_line(
                        &platform.name,
                        None,
                        &decl.term,
                        &var,
                        if has { "值已就位" } else { "无值" },
                        has,
                    ));
                }
                if platform.variables.is_empty() {
                    lines.push(field("平台级变量", "（一个都没有）"));
                }
                lines.push(blank(
                    "group",
                    format!("账号 · {} 个", platform.accounts.len()),
                ));
                for account in self.sorted_accounts(platform) {
                    let mut meta = format!("{} 个变量", account.variables.len());
                    if let Some(purpose) = &account.purpose {
                        meta = format!("{purpose} · {meta}");
                    } else if let Some(email) = &account.email {
                        meta = format!("{email} · {meta}");
                    }
                    lines.push(ListLine {
                        key: format!("a|{}|{}", platform.name, account.alias).into(),
                        kind: "account".into(),
                        text: account.alias.clone().into(),
                        prefix: "".into(),
                        entity: "".into(),
                        meta: meta.into(),
                        has_value: false,
                    });
                }
                if platform.accounts.is_empty() {
                    lines.push(field("账号", "（还没有账号条目）"));
                }
                ui.set_dt_lines(ModelRc::new(VecModel::from(lines)));
            }
            View::Account(platform_name, alias) => {
                let Some(platform) = self.doc.find_platform(&platform_name) else {
                    return;
                };
                let Some(account) = platform.find_account(&alias) else {
                    return;
                };
                ui.set_dt_heading(account.alias.clone().into());
                ui.set_dt_summary(
                    format!(
                        "{} · {} 个变量（平台级变量不归它）",
                        platform.name,
                        account.variables.len()
                    )
                    .into(),
                );
                ui.set_dt_can_delete(true);
                ui.set_dt_can_add_account(false);

                let mut lines = vec![
                    field("别名", account.alias.clone()),
                    field("邮箱", account.email.clone().unwrap_or_default()),
                    field("用途", account.purpose.clone().unwrap_or_default()),
                    field("备注", account.note.clone().unwrap_or_default()),
                    field("host", account.host.clone().unwrap_or_default()),
                    field("user", account.user.clone().unwrap_or_default()),
                    blank("group", format!("变量 · {} 个", account.variables.len())),
                ];
                let mut missing = 0usize;
                for decl in sorted_terms(&account.variables) {
                    let var = naming::account_var_name(&platform.name, &account.alias, &decl.term)
                        .unwrap_or_default();
                    let has = self.has_value(&var);
                    if !has {
                        missing += 1;
                    }
                    lines.push(var_line(
                        &platform.name,
                        Some(&account.alias),
                        &decl.term,
                        &var,
                        if has { "值已就位" } else { "无值" },
                        has,
                    ));
                }
                if account.variables.is_empty() {
                    lines.push(field("变量", "（这个账号还没有声明任何变量）"));
                }
                if missing > 0 {
                    lines.push(field(
                        "不一致",
                        format!("{missing} 个声明在注册表里没有值：点变量行的「改值」写回去"),
                    ));
                }
                ui.set_dt_lines(ModelRc::new(VecModel::from(lines)));
            }
            _ => {}
        }
    }

    fn render_form(&mut self, ui: &AppWindow) {
        let Some(kind) = self.form.clone() else {
            return;
        };
        // 只在「刚打开」这一次回填可编辑字段：之后回写会吞掉人正在打的字。
        let fresh = self.form_nonce != self.pushed_nonce;
        if fresh {
            self.pushed_nonce = self.form_nonce;
            // 清掉上一张表单留下的输入。清理必须放在这里：Slint 那边用 `changed` 监听
            // 打开动作再清空，而 changed 处理器在本函数赋值之后才跑，会把刚回填的值抹掉。
            ui.set_fm_term("".into());
            ui.set_fm_value("".into());
            ui.set_fm_purpose("".into());
            ui.set_fm_note("".into());
            ui.set_fm_email("".into());
            ui.set_fm_host("".into());
            ui.set_fm_user("".into());
        }
        ui.set_fm_preview_prefix(naming::PREFIX.into());

        match &kind {
            FormKind::NewPlatform => {
                ui.set_fm_heading("登记平台".into());
                ui.set_fm_hint(
                    "平台名会成为变量名的第一段，只允许 A-Z a-z 0-9 和 _。".into(),
                );
                ui.set_fm_name_label("平台名".into());
                ui.set_fm_show_term(true);
                ui.set_fm_term_readonly(false);
                ui.set_fm_show_value(false);
                ui.set_fm_show_purpose(false);
                ui.set_fm_show_note(true);
                ui.set_fm_show_contact(false);
                ui.set_fm_show_target(false);
                ui.set_fm_target("".into());
                ui.set_fm_preview_entity("".into());
            }
            FormKind::NewAccount(platform) => {
                ui.set_fm_heading("登记账号条目".into());
                ui.set_fm_hint(
                    "别名只允许 A-Z a-z 0-9 和 _；邮箱 / 用途 / 备注可以写中文。".into(),
                );
                ui.set_fm_name_label("别名".into());
                ui.set_fm_show_term(true);
                ui.set_fm_term_readonly(false);
                ui.set_fm_show_value(false);
                ui.set_fm_show_purpose(true);
                ui.set_fm_show_note(true);
                ui.set_fm_show_contact(true);
                ui.set_fm_show_target(true);
                ui.set_fm_target(platform.clone().into());
                ui.set_fm_preview_entity("".into());
            }
            FormKind::NewVar(platform, alias) => {
                ui.set_fm_heading("新增变量".into());
                ui.set_fm_hint("术语是变量名的最后一段；值只写进注册表，不进元数据。".into());
                ui.set_fm_name_label("术语".into());
                ui.set_fm_show_term(true);
                ui.set_fm_term_readonly(false);
                ui.set_fm_show_value(true);
                ui.set_fm_show_purpose(false);
                ui.set_fm_show_note(false);
                ui.set_fm_show_contact(false);
                ui.set_fm_show_target(true);
                ui.set_fm_target(self.target_label(platform, alias.as_deref()).into());
                ui.set_fm_preview_entity(
                    self.owner_segment(platform, alias.as_deref()).into(),
                );
            }
            FormKind::EditVar(platform, alias, term) => {
                ui.set_fm_heading("改值".into());
                ui.set_fm_hint(
                    "术语与变量名不可在这里改；改名请用「改名」并为新名腾出变量名。".into(),
                );
                ui.set_fm_name_label("术语".into());
                ui.set_fm_show_term(true);
                ui.set_fm_term_readonly(true);
                ui.set_fm_show_value(true);
                ui.set_fm_show_purpose(false);
                ui.set_fm_show_note(false);
                ui.set_fm_show_contact(false);
                ui.set_fm_show_target(true);
                ui.set_fm_target(self.target_label(platform, alias.as_deref()).into());
                ui.set_fm_preview_entity(
                    self.owner_segment(platform, alias.as_deref()).into(),
                );
                if fresh {
                    ui.set_fm_term(term.clone().into());
                }
            }
        }

        ui.set_fm_error(self.form_error.clone().unwrap_or_default().into());
        if fresh {
            ui.set_fm_value(self.form_value.clone().into());
        }
    }

    /// 表单里「归属」一行的文字。
    fn target_label(&self, platform: &str, alias: Option<&str>) -> String {
        match alias {
            Some(alias) => format!("{platform} / {alias}"),
            None => format!("{platform}（平台级）"),
        }
    }

    /// 变量名里平台 / 别名那一段（大写，带尾下划线），术语由界面实时接在后面。
    fn owner_segment(&self, platform: &str, alias: Option<&str>) -> String {
        match alias {
            Some(alias) => format!("{}_{}_", naming::normalize(platform), naming::normalize(alias)),
            None => format!("{}_", naming::normalize(platform)),
        }
    }

    fn render_snapshots(&self, ui: &AppWindow) {
        let lines: Vec<SnapshotLine> = self
            .snaps
            .iter()
            .enumerate()
            .map(|(index, info)| SnapshotLine {
                index: index as i32,
                created_at: stamp(&info.created_at).into(),
                size: human_size(info.bytes).into(),
                variables: info.variables as i32,
            })
            .collect();
        ui.set_sn_lines(ModelRc::new(VecModel::from(lines)));
    }

    fn render_doctor(&self, ui: &AppWindow) {
        let lines: Vec<CheckLine> = self
            .report
            .checks
            .iter()
            .map(|check| CheckLine {
                id: check.id.into(),
                ok: check.ok,
                detail: check.detail.clone().into(),
            })
            .collect();
        ui.set_ck_ok(self.report.ok);
        ui.set_ck_lines(ModelRc::new(VecModel::from(lines)));
    }

    fn render_empty(&self, ui: &AppWindow) {
        ui.set_em_hint(
            "登记第一个平台：变量名会以 ASSETS_CLI_<平台>_ 开头，之后任何新开的终端里都能读到。".into(),
        );
    }

    fn render_fault(&self, ui: &AppWindow) {
        let detail = self.fault.clone().unwrap_or_default();
        ui.set_ft_heading("台账读入失败".into());
        ui.set_ft_detail(
            format!(
                "{detail}\n\n数据文件：{}\n修好这份文件后按「重新自检」重读；也可以用快照整体回退。",
                self.ctx.paths.data_file().display()
            )
            .into(),
        );
    }
}
impl App {
    // ── 数据装配 ──

    fn declared_count(&self) -> usize {
        self.doc.declarations().map(|list| list.len()).unwrap_or(0)
    }

    fn has_value(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    fn sorted_platforms(&self) -> Vec<&Platform> {
        let mut list: Vec<&Platform> = self.doc.platforms.iter().collect();
        list.sort_by_key(|platform| naming::normalize(&platform.name));
        list
    }

    fn sorted_accounts<'a>(&self, platform: &'a Platform) -> Vec<&'a Account> {
        let mut list: Vec<&Account> = platform.accounts.iter().collect();
        list.sort_by_key(|account| naming::normalize(&account.alias));
        list
    }

    /// 左资产树：平台 → 平台级变量 → 账号 → 账号级变量，一段一层缩进。
    fn tree_rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        for platform in self.sorted_platforms() {
            let account_vars: usize = platform.accounts.iter().map(|a| a.variables.len()).sum();
            rows.push(TreeRow {
                key: format!("p|{}", platform.name).into(),
                kind: "platform".into(),
                depth: 1,
                label: platform.name.clone().into(),
                meta: format!("{} 个变量", platform.variables.len() + account_vars).into(),
                platform: platform.name.clone().into(),
                account: "".into(),
                term: "".into(),
                prefix: "".into(),
                entity: "".into(),
                has_value: false,
            });
            for decl in sorted_terms(&platform.variables) {
                let name =
                    naming::platform_var_name(&platform.name, &decl.term).unwrap_or_default();
                let (prefix, entity) = signature(&name);
                let has = self.has_value(&name);
                rows.push(TreeRow {
                    key: var_key(&platform.name, None, &decl.term).into(),
                    kind: "variable".into(),
                    depth: 2,
                    label: decl.term.clone().into(),
                    meta: "平台级".into(),
                    platform: platform.name.clone().into(),
                    account: "".into(),
                    term: decl.term.clone().into(),
                    prefix: prefix.into(),
                    entity: entity.into(),
                    has_value: has,
                });
            }
            for account in self.sorted_accounts(platform) {
                rows.push(TreeRow {
                    key: format!("a|{}|{}", platform.name, account.alias).into(),
                    kind: "account".into(),
                    depth: 2,
                    label: account.alias.clone().into(),
                    meta: format!("{} 个变量", account.variables.len()).into(),
                    platform: platform.name.clone().into(),
                    account: account.alias.clone().into(),
                    term: "".into(),
                    prefix: "".into(),
                    entity: "".into(),
                    has_value: false,
                });
                for decl in sorted_terms(&account.variables) {
                    let name = naming::account_var_name(&platform.name, &account.alias, &decl.term)
                        .unwrap_or_default();
                    let (prefix, entity) = signature(&name);
                    let has = self.has_value(&name);
                    rows.push(TreeRow {
                        key: var_key(&platform.name, Some(&account.alias), &decl.term).into(),
                        kind: "variable".into(),
                        depth: 3,
                        label: decl.term.clone().into(),
                        meta: "".into(),
                        platform: platform.name.clone().into(),
                        account: account.alias.clone().into(),
                        term: decl.term.clone().into(),
                        prefix: prefix.into(),
                        entity: entity.into(),
                        has_value: has,
                    });
                }
            }
        }
        rows
    }

    /// S1：人看到的清单与 AI 在终端里看到的是同一份。
    fn overview_lines(&self) -> Vec<ListLine> {
        let mut lines = Vec::new();
        for platform in self.sorted_platforms() {
            let account_vars: usize = platform.accounts.iter().map(|a| a.variables.len()).sum();
            lines.push(blank(
                "group",
                format!(
                    "{} · {} 个账号 · {} 个变量",
                    platform.name,
                    platform.accounts.len(),
                    platform.variables.len() + account_vars
                ),
            ));
            if let Some(note) = &platform.note {
                lines.push(blank("note", format!("备注：{note}")));
            }
            for decl in sorted_terms(&platform.variables) {
                let name =
                    naming::platform_var_name(&platform.name, &decl.term).unwrap_or_default();
                let has = self.has_value(&name);
                lines.push(var_line(
                    &platform.name,
                    None,
                    &decl.term,
                    &name,
                    "平台级",
                    has,
                ));
            }
            for account in self.sorted_accounts(platform) {
                let mut head = format!("账号 {}", account.alias);
                if let Some(purpose) = &account.purpose {
                    head.push_str(&format!(" · {purpose}"));
                } else if let Some(email) = &account.email {
                    head.push_str(&format!(" · {email}"));
                }
                lines.push(blank("note", head));
                if account.variables.is_empty() {
                    lines.push(blank("note", "（这个账号还没有声明任何变量）"));
                }
                for decl in sorted_terms(&account.variables) {
                    let name = naming::account_var_name(&platform.name, &account.alias, &decl.term)
                        .unwrap_or_default();
                    let has = self.has_value(&name);
                    lines.push(var_line(
                        &platform.name,
                        Some(&account.alias),
                        &decl.term,
                        &name,
                        if has { "值已就位" } else { "无值" },
                        has,
                    ));
                }
            }
        }
        lines
    }

    /// 底部信息条：最近快照 + 配额水位（超 80% 出声）。
    fn footer(&self) -> (String, String, bool) {
        let used: u64 = self.snaps.iter().map(|info| info.bytes).sum();
        let latest = match self.snaps.first() {
            Some(info) => format!("最近快照 {}", stamp(&info.created_at)),
            None => "还没有快照".to_string(),
        };
        let quota = format!("配额 {:.1} / 10 MB", used as f64 / 1048576.0);
        (latest, quota, used * 5 > snapshot::QUOTA_BYTES * 4)
    }

    /// 顶部条那个自检点：ok / warn / fail 三态文案。
    fn doctor_badge(&self) -> (&'static str, String) {
        if self.fault.is_some() {
            return ("fail", "台账不可读".to_string());
        }
        let failed: Vec<&doctor::Check> =
            self.report.checks.iter().filter(|check| !check.ok).collect();
        if failed.is_empty() {
            return ("ok", "自检通过".to_string());
        }
        let fatal = failed
            .iter()
            .any(|check| check.id == "hook_installed" || check.id == "data_dir_writable");
        let label = if fatal {
            format!("自检未通过（{} 项）", failed.len())
        } else {
            format!("自检有 {} 条待处理", failed.len())
        };
        (if fatal { "fail" } else { "warn" }, label)
    }

    // ── 浮层 ──

    fn render_overlay(&self, ui: &AppWindow) {
        let Some(confirm) = &self.confirm else {
            ui.set_cf_open(false);
            return;
        };
        ui.set_cf_open(true);
        ui.set_cf_heading(self.confirm_heading(confirm).into());
        let lines: Vec<ListLine> = self
            .confirm_lines(confirm)
            .into_iter()
            .map(|text| blank("note", text))
            .collect();
        ui.set_cf_lines(ModelRc::new(VecModel::from(lines)));
        ui.set_cf_destructive(confirm_destructive(confirm));
        ui.set_cf_confirm_label(confirm_label(confirm).into());
        ui.set_cf_error(self.confirm_error.clone().unwrap_or_default().into());

        let rename = matches!(
            confirm,
            Confirm::RenamePlatform(_) | Confirm::RenameAccount(_, _)
        );
        ui.set_cf_show_input(rename);
        if rename {
            let label = match confirm {
                Confirm::RenamePlatform(_) => "新平台名",
                _ => "新别名",
            };
            ui.set_cf_input_label(label.into());
            ui.set_cf_input(self.rename_input.clone().into());
        }
    }

    fn confirm_heading(&self, confirm: &Confirm) -> String {
        match confirm {
            Confirm::DeletePlatform(name) => format!("删除平台 {name}"),
            Confirm::DeleteAccount(platform, alias) => format!("删除条目 {platform} / {alias}"),
            Confirm::DeleteVar(platform, alias, term) => {
                format!("删除变量 {}", self.var_name_of(platform, alias.as_deref(), term))
            }
            Confirm::RestoreSnapshot(index) => match self.snaps.get(*index) {
                Some(info) => format!("恢复到 {}", stamp(&info.created_at)),
                None => "恢复快照".to_string(),
            },
            Confirm::DeleteSnapshot(index) => match self.snaps.get(*index) {
                Some(info) => format!("删除快照 {}", stamp(&info.created_at)),
                None => "删除快照".to_string(),
            },
            Confirm::RenamePlatform(name) => format!("平台改名 · {name}"),
            Confirm::RenameAccount(platform, alias) => format!("条目改名 · {platform} / {alias}"),
        }
    }

    fn confirm_lines(&self, confirm: &Confirm) -> Vec<String> {
        let mut lines = Vec::new();
        match confirm {
            Confirm::DeletePlatform(name) => {
                if let Some(platform) = self.doc.find_platform(name) {
                    let mut names = Vec::new();
                    for decl in sorted_terms(&platform.variables) {
                        names.push(
                            naming::platform_var_name(&platform.name, &decl.term)
                                .unwrap_or_default(),
                        );
                    }
                    for account in self.sorted_accounts(platform) {
                        for decl in sorted_terms(&account.variables) {
                            names.push(
                                naming::account_var_name(
                                    &platform.name,
                                    &account.alias,
                                    &decl.term,
                                )
                                .unwrap_or_default(),
                            );
                        }
                    }
                    lines.push(format!(
                        "{} 个账号 / {} 个变量会被一起清除：",
                        platform.accounts.len(),
                        names.len()
                    ));
                    lines.extend(names);
                }
                lines.push("注册表里的值也会删掉。写入前会自动建一份快照（含明文值）。".into());
            }
            Confirm::DeleteAccount(platform_name, alias) => {
                if let Some(account) = self
                    .doc
                    .find_platform(platform_name)
                    .and_then(|platform| platform.find_account(alias))
                {
                    lines.push(format!(
                        "条目 {platform_name} / {} 的 {} 个变量会被清除：",
                        account.alias,
                        account.variables.len()
                    ));
                    for decl in sorted_terms(&account.variables) {
                        lines.push(
                            naming::account_var_name(platform_name, &account.alias, &decl.term)
                                .unwrap_or_default(),
                        );
                    }
                }
                lines.push("平台级变量的名字里没有账号段，不参与这次删除。".into());
                lines.push("注册表里的值也会删掉。写入前会自动建一份快照。".into());
            }
            Confirm::DeleteVar(platform, alias, term) => {
                lines.push(self.var_name_of(platform, alias.as_deref(), term));
                if alias.is_some() {
                    lines.push("平台级变量不参与。".into());
                }
                lines.push("注册表里的值也会一起删掉。写入前会自动建一份快照。".into());
            }
            Confirm::RestoreSnapshot(index) => {
                lines.push(
                    "① 用快照里的台账覆盖现在这份；② 写回快照里的值；③ 删掉不属于快照的变量。"
                        .into(),
                );
                match self.snaps.get(*index).and_then(|info| read_snapshot(&info.file)) {
                    Some(shot) => {
                        lines.push(format!(
                            "这份快照是 {} 的全量：{} 个变量。",
                            stamp(&shot.created_at),
                            shot.variables.len()
                        ));
                        let known: BTreeSet<&str> =
                            shot.variables.iter().map(|var| var.name.as_str()).collect();
                        let lost: Vec<&String> = self
                            .values
                            .keys()
                            .filter(|name| !known.contains(name.as_str()))
                            .collect();
                        if lost.is_empty() {
                            lines.push("按现在这份台账，第 ③ 步没有变量会消失。".into());
                        } else {
                            lines.push(format!("第 ③ 步会让这 {} 个变量消失：", lost.len()));
                            lines.extend(lost.into_iter().cloned());
                        }
                    }
                    None => lines.push("快照文件读不出来：先确认文件还在，再恢复。".into()),
                }
            }
            Confirm::DeleteSnapshot(index) => {
                if let Some(info) = self.snaps.get(*index) {
                    lines.push(format!(
                        "{}（{}，{} 个变量）",
                        stamp(&info.created_at),
                        human_size(info.bytes),
                        info.variables
                    ));
                }
                lines.push("快照删了就没有第二份兜底了；台账本身不动。".into());
            }
            Confirm::RenamePlatform(_) => {
                let pairs = self.rename_pairs(confirm);
                lines.push(format!("{} 个变量名会跟着改：", pairs.len()));
                lines.extend(pairs);
                lines.push("平台级变量与全部账号级变量都在名单里（账号级变量名含平台段）。".into());
                if self.rename_input.trim().is_empty() {
                    lines.push("新名不能为空。".into());
                }
            }
            Confirm::RenameAccount(_, _) => {
                let pairs = self.rename_pairs(confirm);
                lines.push(format!("{} 个变量名会跟着改：", pairs.len()));
                lines.extend(pairs);
                lines.push("平台级变量的名字里没有账号段，不参与这次改名。".into());
                if self.rename_input.trim().is_empty() {
                    lines.push("新名不能为空。".into());
                }
            }
        }
        lines
    }

    /// 改名对照表：旧名 → 新名（新名跟着输入实时变）。
    fn rename_pairs(&self, confirm: &Confirm) -> Vec<String> {
        let mut pairs: Vec<String> = Vec::new();
        let new_segment = naming::normalize(self.rename_input.trim());
        match confirm {
            Confirm::RenamePlatform(old) => {
                if let Some(platform) = self.doc.find_platform(old) {
                    for decl in sorted_terms(&platform.variables) {
                        pairs.push(format!(
                            "{} → {}",
                            naming::platform_var_name(&platform.name, &decl.term)
                                .unwrap_or_default(),
                            naming::platform_var_name(&new_segment, &decl.term).unwrap_or_default()
                        ));
                    }
                    for account in self.sorted_accounts(platform) {
                        for decl in sorted_terms(&account.variables) {
                            pairs.push(format!(
                                "{} → {}",
                                naming::account_var_name(
                                    &platform.name,
                                    &account.alias,
                                    &decl.term
                                )
                                .unwrap_or_default(),
                                naming::account_var_name(
                                    &new_segment,
                                    &account.alias,
                                    &decl.term
                                )
                                .unwrap_or_default()
                            ));
                        }
                    }
                }
            }
            Confirm::RenameAccount(platform_name, alias) => {
                if let Some(platform) = self.doc.find_platform(platform_name) {
                    if let Some(account) = platform.find_account(alias) {
                        for decl in sorted_terms(&account.variables) {
                            pairs.push(format!(
                                "{} → {}",
                                naming::account_var_name(
                                    &platform.name,
                                    &account.alias,
                                    &decl.term
                                )
                                .unwrap_or_default(),
                                naming::account_var_name(
                                    &platform.name,
                                    &new_segment,
                                    &decl.term
                                )
                                .unwrap_or_default()
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        pairs
    }
}

fn confirm_destructive(confirm: &Confirm) -> bool {
    matches!(
        confirm,
        Confirm::DeletePlatform(_) | Confirm::DeleteAccount(_, _) | Confirm::DeleteVar(_, _, _)
    )
}

fn confirm_label(confirm: &Confirm) -> &'static str {
    match confirm {
        Confirm::DeleteSnapshot(_) => "删除快照",
        Confirm::DeletePlatform(_) => "删除平台",
        Confirm::DeleteAccount(_, _) => "删除条目",
        Confirm::DeleteVar(_, _, _) => "删除变量",
        Confirm::RestoreSnapshot(_) => "恢复",
        Confirm::RenamePlatform(_) | Confirm::RenameAccount(_, _) => "确认改名",
    }
}

fn sorted_terms(decls: &[VarDecl]) -> Vec<&VarDecl> {
    let mut list: Vec<&VarDecl> = decls.iter().collect();
    list.sort_by_key(|decl| naming::normalize(&decl.term));
    list
}

fn read_snapshot(file: &std::path::Path) -> Option<Snapshot> {
    let text = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&text).ok()
}

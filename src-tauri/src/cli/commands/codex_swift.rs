use clap::Subcommand;

use crate::cli::ui::{create_table, info, success};
use crate::codex_swift::service;
use crate::error::AppError;
use crate::settings;
use crate::store::AppState;

#[derive(Subcommand, Debug, Clone)]
pub enum CodexSwiftCommand {
    /// 登录 Codex Swift 账号
    Login {
        /// 服务地址（留空则交互输入，默认 https://cs.lksin.top）
        #[arg(long, alias = "url")]
        base_url: Option<String>,
        /// API Key（留空则交互输入）
        #[arg(long)]
        api_key: Option<String>,
    },
    /// 注销 Codex Swift 账号
    Logout,
    /// 显示账号信息与余额
    Status {
        /// 输出机器可读 JSON
        #[arg(long)]
        json: bool,
    },
    /// 列出可用的模型分组
    Groups {
        /// 输出机器可读 JSON
        #[arg(long)]
        json: bool,
    },
    /// 将指定分组的供应商配置应用到本地
    Apply {
        /// 群组 ID
        group_id: String,
        /// 目标应用（可多选，逗号分隔，如 claude,codex）
        /// 不指定则应用到所有已启用的应用
        #[arg(long, value_delimiter = ',')]
        apps: Vec<String>,
    },
}

fn create_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Message(format!("创建 tokio 运行时失败: {e}")))
}

pub fn execute(cmd: CodexSwiftCommand, state: &AppState) -> Result<(), AppError> {
    let runtime = create_runtime()?;
    match cmd {
        CodexSwiftCommand::Login { base_url, api_key } => {
            let base_url = match base_url {
                Some(u) if !u.trim().is_empty() => u,
                _ => {
                    let input = inquire::Text::new("Server URL:")
                        .with_default("https://cs.lksin.top")
                        .prompt()
                        .map_err(|e| AppError::Message(e.to_string()))?;
                    if input.trim().is_empty() {
                        "https://cs.lksin.top".to_string()
                    } else {
                        input
                    }
                }
            };
            let api_key = match api_key {
                Some(k) => k,
                None => inquire::Password::new("API Key:")
                    .without_confirmation()
                    .prompt()
                    .map_err(|e| AppError::Message(e.to_string()))?,
            };
            let account = runtime.block_on(service::validate_and_login(&base_url, &api_key))?;
            println!(
                "{}",
                success(&format!(
                    "已连接 Codex Swift，欢迎 {} ({})",
                    account.username, account.role
                ))
            );
            println!("余额：{}", account.balance);
        }

        CodexSwiftCommand::Logout => {
            runtime.block_on(service::logout(state))?;
            println!("{}", success("已注销 Codex Swift 账号"));
        }

        CodexSwiftCommand::Status { json } => {
            match runtime.block_on(service::get_account())? {
                None => println!("{}", info("未登录 Codex Swift")),
                Some(account) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&account)
                                .map_err(|source| AppError::JsonSerialize { source })?
                        );
                    } else {
                        let mut table = create_table();
                        table.set_header(vec!["字段", "值"]);
                        table.add_row(vec!["用户名", &account.username]);
                        table.add_row(vec!["角色", &account.role]);
                        table.add_row(vec!["余额", &account.balance.to_string()]);
                        if let Some(key_name) = &account.key_name {
                            table.add_row(vec!["Key 名称", key_name]);
                        }
                        if let Some(aff) = &account.aff_code {
                            table.add_row(vec!["邀请码", aff]);
                        }
                        println!("{table}");
                    }
                }
            }
        }

        CodexSwiftCommand::Groups { json } => {
            let groups = runtime.block_on(service::list_groups())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&groups)
                        .map_err(|source| AppError::JsonSerialize { source })?
                );
            } else if groups.is_empty() {
                println!("{}", info("暂无可用群组"));
            } else {
                let mut table = create_table();
                table.set_header(vec!["ID", "名称", "状态", "用户数", "倍率"]);
                for g in &groups {
                    table.add_row(vec![
                        g.id.as_str(),
                        g.name.as_str(),
                        g.status.as_str(),
                        &g.active_users.to_string(),
                        &format!("×{}", g.multiplier),
                    ]);
                }
                println!("{table}");
            }
        }

        CodexSwiftCommand::Apply { group_id, apps } => {
            let apps = if apps.is_empty() {
                let s = settings::get_settings();
                let va = &s.visible_apps;
                let mut available = vec![];
                if va.claude {
                    available.push("claude");
                }
                if va.codex {
                    available.push("codex");
                }
                if va.gemini {
                    available.push("gemini");
                }
                if available.is_empty() {
                    return Err(AppError::InvalidInput(
                        "没有已启用的目标应用，请先在设置中启用 Claude、Codex 或 Gemini".to_string(),
                    ));
                }
                let selected = inquire::MultiSelect::new("选择要应用的 Agents 应用（空格选中，回车确认）:", available.clone())
                    .with_all_selected_by_default()
                    .prompt()
                    .map_err(|e| AppError::Message(e.to_string()))?;
                if selected.is_empty() {
                    return Err(AppError::InvalidInput("未选择任何应用".to_string()));
                }
                selected.into_iter().map(str::to_string).collect()
            } else {
                apps
            };

            let session = runtime.block_on(service::apply_group(state, &group_id, apps))?;
            println!(
                "{}",
                success(&format!(
                    "已应用群组「{}」的供应商配置（session: {}）",
                    session.group_name, session.session_id
                ))
            );
        }
    }
    Ok(())
}

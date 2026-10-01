// SPDX-FileCopyrightText: Copyright (c) 2026 owu <wqh@live.com>
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use tokio::sync::Mutex;
use slint::{ModelRc, VecModel};
use crate::{AppWindow, AppState, SchedulerTaskUI};
use tracing::{info, error};

// Extract short filename from full command path
// D:\program\Docker\get-ip.bat → get-ip.bat
// C:\Windows\System32\cmd.exe → cmd.exe
fn short_filename(command: &str) -> String {
    let trimmed = command.trim();
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.len() > 32 {
        crate::utils::truncate_bytes_ellipsis(name, 29)
    } else {
        name.to_string()
    }
}

// Convert backend SchedulerTask to Slint SchedulerTaskUI
fn to_ui_task(task: &crate::app::scheduler_task::SchedulerTask) -> SchedulerTaskUI {
    let unwrapped_cmd = crate::app::scheduler_task::schtasks::unwrap_command(&task.command);
    SchedulerTaskUI {
        task_id: task.task_id.clone().into(),
        display_name: task.display_name.clone().into(),
        command: unwrapped_cmd.clone().into(),
        command_short: short_filename(&unwrapped_cmd).into(),
        arguments: task.arguments.clone().into(),
        cron_expr: task.cron_expr.clone().into(),
        enabled: task.enabled,
        status: task.status.clone().into(),
        last_run: task.last_run.clone().into(),
        next_run: task.next_run.clone().into(),
        trigger_summary: task.trigger_summary.clone().into(),
        admin_mode: task.admin_mode,

    }
}

// Refresh scheduler task list from backend
pub fn refresh_tasks(app_handle: slint::Weak<AppWindow>) {
    let ah = app_handle.clone();
    // Set loading state synchronously before spawning async task
    if let Some(app) = ah.upgrade() {
        app.set_scheduler_loading(true);
        app.set_scheduler_tasks(ModelRc::new(VecModel::from(Vec::<SchedulerTaskUI>::new())));
    }
    tokio::spawn(async move {
        let result = crate::app::scheduler_task::schtasks::query_tasks();

        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_loading(false);
                match result {
                    Ok(tasks) => {
                        let ui_tasks: Vec<SchedulerTaskUI> = tasks.iter().map(to_ui_task).collect();
                        let model = ModelRc::new(VecModel::from(ui_tasks));
                        app.set_scheduler_tasks(model);
                    }
                    Err(e) => {
                        error!("Failed to query scheduler tasks: {}", e);
                        let err_msg = crate::i18n::t("scheduler.error_query_failed");
                        app.set_scheduler_error(format!("{}: {}", err_msg, e).into());
                    }
                }
            }
        });
    });
}

fn show_toast(ah: slint::Weak<AppWindow>, msg: String) {
    super::network::utils::show_toast(ah, msg);
}

fn set_name_error(ah: &slint::Weak<AppWindow>, msg: &str) {
    if let Some(app) = ah.upgrade() {
        app.set_scheduler_dialog_name_error(msg.into());
    }
}

fn set_command_error(ah: &slint::Weak<AppWindow>, msg: &str) {
    if let Some(app) = ah.upgrade() {
        app.set_scheduler_dialog_command_error(msg.into());
    }
}

fn set_cron_error(ah: &slint::Weak<AppWindow>, msg: &str) {
    if let Some(app) = ah.upgrade() {
        app.set_scheduler_dialog_cron_error(msg.into());
    }
}

fn clear_all_errors(ah: &slint::Weak<AppWindow>) {
    if let Some(app) = ah.upgrade() {
        app.set_scheduler_dialog_name_error("".into());
        app.set_scheduler_dialog_command_error("".into());
        app.set_scheduler_dialog_cron_error("".into());
    }
}

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, _app_state: Arc<Mutex<AppState>>) {
    // Refresh scheduler tasks
    let ah_refresh = app_handle.clone();
    app.on_refresh_scheduler_tasks(move || {
        refresh_tasks(ah_refresh.clone());
    });

    // Add task - open dialog in add mode
    let ah_add = app_handle.clone();
    app.on_add_scheduler_task(move || {
        if let Some(app) = ah_add.upgrade() {
            app.set_scheduler_dialog_is_edit(false);
            app.set_scheduler_dialog_task_id("".into());
            app.set_scheduler_dialog_task_name("".into());
            app.set_scheduler_dialog_command("".into());
            app.set_scheduler_dialog_arguments("".into());
            app.set_scheduler_dialog_cron_expr("* * * * *".into());
            clear_all_errors(&ah_add);
            app.set_scheduler_dialog_submitting(false);

            let ah_url = ah_add.clone();
            tokio::spawn(async move {
                let data = crate::api::common::wslui_helper_scheduler();
                let cron_url = data.cron_expression
                    .map(|link| link.url)
                    .unwrap_or_default();
                let cmd_url = data.command_docs
                    .map(|link| link.url)
                    .unwrap_or_default();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_url.upgrade() {
                        app.set_scheduler_dialog_cron_help_url(cron_url.into());
                        app.set_scheduler_dialog_command_help_url(cmd_url.into());
                    }
                });
            });

            app.set_scheduler_dialog_admin_mode(false);

            app.set_show_scheduler_dialog(true);
        }
    });

    // Edit task - open dialog in edit mode
    let ah_edit = app_handle.clone();
    app.on_edit_scheduler_task(move |task_id| {
        let ah = ah_edit.clone();
        let task_id_str = task_id.to_string();
        tokio::spawn(async move {
            let result = crate::app::scheduler_task::schtasks::query_tasks();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    match result {
                        Ok(tasks) => {
                            if let Some(task) = tasks.iter().find(|t| t.task_id == task_id_str) {
                                app.set_scheduler_dialog_is_edit(true);
                                app.set_scheduler_dialog_task_id(task.task_id.clone().into());
                                app.set_scheduler_dialog_task_name(task.display_name.clone().into());
                                app.set_scheduler_dialog_command(
                                    crate::app::scheduler_task::schtasks::unwrap_command(&task.command).into()
                                );
                                app.set_scheduler_dialog_arguments(task.arguments.clone().into());
                                app.set_scheduler_dialog_cron_expr(task.cron_expr.clone().into());
                                app.set_scheduler_dialog_admin_mode(task.admin_mode);

                                clear_all_errors(&ah);
                                app.set_scheduler_dialog_submitting(false);
                                app.set_show_scheduler_dialog(true);

                                let ah_url = ah.clone();
                                tokio::spawn(async move {
                                    let data = crate::api::common::wslui_helper_scheduler();
                                    let cron_url = data.cron_expression
                                        .map(|link| link.url)
                                        .unwrap_or_default();
                                    let cmd_url = data.command_docs
                                        .map(|link| link.url)
                                        .unwrap_or_default();
                                    let _ = slint::invoke_from_event_loop(move || {
                                        if let Some(app) = ah_url.upgrade() {
                                            app.set_scheduler_dialog_cron_help_url(cron_url.into());
                                            app.set_scheduler_dialog_command_help_url(cmd_url.into());
                                        }
                                    });
                                });
                            }
                        }
                        Err(e) => {
                            error!("Failed to query tasks for edit: {}", e);
                        }
                    }
                }
            });
        });
    });

    // Delete task
    let ah_delete = app_handle.clone();
    app.on_delete_scheduler_task(move |task_id| {
        let ah = ah_delete.clone();
        let task_id_str = task_id.to_string();
        info!("Deleting scheduler task: {}", task_id_str);
        if let Some(app) = ah.upgrade() {
            app.set_scheduler_busy_task_id(task_id_str.clone().into());
        }
        tokio::spawn(async move {
            let result = crate::app::scheduler_task::schtasks::query_tasks();
            let delete_result = match result {
                Ok(tasks) => {
                    if tasks.iter().any(|t| t.task_id == task_id_str) {
                        let needs_elevation = true;
                        crate::app::scheduler_task::schtasks::delete_task(&task_id_str, needs_elevation)
                    } else {
                        // Task not found in list (e.g., manually deleted from Windows).
                        // Silently clean up our local VBS file and return success.
                        crate::app::scheduler_task::schtasks::cleanup_vbs_launcher(&task_id_str);
                        Ok(())
                    }
                }
                Err(e) => Err(e),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    app.set_scheduler_busy_task_id("".into());
                    match delete_result {
                        Ok(()) => {
                            let msg = crate::i18n::t("scheduler.task_deleted");
                            show_toast(ah.clone(), msg);
                            refresh_tasks(ah.clone());
                        }
                        Err(e) => {
                            error!("Failed to delete scheduler task: {}", e);
                            let msg = crate::i18n::t("scheduler.error_delete_failed");
                            show_toast(ah.clone(), format!("{}: {}", msg, e));
                        }
                    }
                }
            });
        });
    });

    // Toggle task (enable/disable)
    let ah_toggle = app_handle.clone();
    app.on_toggle_scheduler_task(move |task_id| {
        let ah = ah_toggle.clone();
        let task_id_str = task_id.to_string();
        if let Some(app) = ah.upgrade() {
            app.set_scheduler_busy_task_id(task_id_str.clone().into());
        }
        tokio::spawn(async move {
            let tasks_result = crate::app::scheduler_task::schtasks::query_tasks();
            let toggle_result = match tasks_result {
                Ok(tasks) => {
                    if let Some(task) = tasks.iter().find(|t| t.task_id == task_id_str) {
                        let new_enable = !task.enabled;
                        let needs_elevation = true;
                        crate::app::scheduler_task::schtasks::toggle_task(&task_id_str, new_enable, needs_elevation)
                    } else {
                        Err("Task not found".to_string())
                    }
                }
                Err(e) => Err(e),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    app.set_scheduler_busy_task_id("".into());
                    match toggle_result {
                        Ok(()) => {
                            let msg = crate::i18n::t("scheduler.task_toggled");
                            show_toast(ah.clone(), msg);
                            refresh_tasks(ah.clone());
                        }
                        Err(e) => {
                            error!("Failed to toggle scheduler task: {}", e);
                            show_toast(ah.clone(), format!("Toggle failed: {}", e));
                        }
                    }
                }
            });
        });
    });

    // Run task now
    let ah_run = app_handle.clone();
    app.on_run_scheduler_task(move |task_id| {
        let ah = ah_run.clone();
        let task_id_str = task_id.to_string();
        info!("Running scheduler task: {}", task_id_str);
        if let Some(app) = ah.upgrade() {
            app.set_scheduler_busy_task_id(task_id_str.clone().into());
        }
        tokio::spawn(async move {
            let result = crate::app::scheduler_task::schtasks::query_tasks();
            let run_result = match result {
                Ok(tasks) => {
                    if tasks.iter().any(|t| t.task_id == task_id_str) {
                        let needs_elevation = true;
                        crate::app::scheduler_task::schtasks::run_task(&task_id_str, needs_elevation)
                    } else {
                        Err("Task not found".to_string())
                    }
                }
                Err(e) => Err(e),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    app.set_scheduler_busy_task_id("".into());
                    match run_result {
                        Ok(()) => {
                            let msg = crate::i18n::t("scheduler.task_started");
                            show_toast(ah.clone(), msg);
                            refresh_tasks(ah.clone());
                        }
                        Err(e) => {
                            error!("Failed to run scheduler task: {}", e);
                            show_toast(ah.clone(), format!("Run failed: {}", e));
                        }
                    }
                }
            });
        });
    });

    // Save task (create or update)
    let ah_save = app_handle.clone();
    app.on_save_scheduler_task(move |name, cmd, args, cron| {
        let ah = ah_save.clone();
        let name_str = name.to_string();
        let cmd_str = cmd.to_string();
        let args_str = args.to_string();
        let cron_str = cron.to_string();
        let is_edit = if let Some(app) = ah.upgrade() {
            app.get_scheduler_dialog_is_edit()
        } else {
            false
        };

        // Validation
        clear_all_errors(&ah);
        if name_str.is_empty() {
            set_name_error(&ah, &crate::i18n::t("scheduler.error_name_required"));
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }
        if name_str.len() > 60 {
            set_name_error(&ah, &crate::i18n::t("scheduler.error_name_invalid"));
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }
        if !name_str.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            set_name_error(&ah, &crate::i18n::t("scheduler.error_name_invalid"));
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }
        if cmd_str.is_empty() {
            set_command_error(&ah, &crate::i18n::t("scheduler.error_command_required"));
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }
        if cron_str.is_empty() {
            set_cron_error(&ah, &crate::i18n::t("scheduler.cron_error_required"));
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }

        // Validate cron expression
        if let Err(e) = crate::app::scheduler_task::schtasks::parse_cron(&cron_str) {
            let msg = if e.starts_with("Unsupported") {
                crate::i18n::t("scheduler.cron_error_unsupported")
            } else if e.starts_with("Cron expression must have") {
                crate::i18n::t("scheduler.cron_error_fields")
            } else if e.starts_with("Comma-separated") {
                crate::i18n::t("scheduler.cron_error_comma")
            } else if e.starts_with("Step must be") || e.starts_with("Invalid step") {
                crate::i18n::t("scheduler.cron_error_step")
            } else if e.starts_with("Value") || e.starts_with("Invalid value") {
                crate::i18n::t("scheduler.cron_error_range")
            } else {
                e
            };
            set_cron_error(&ah, &msg);
            if let Some(app) = ah.upgrade() {
                app.set_scheduler_dialog_submitting(false);
            }
            return;
        }



        let new_admin_mode = if let Some(app) = ah.upgrade() {
            app.get_scheduler_dialog_admin_mode()
        } else {
            false
        };

        // Get old task ID for edit mode (will be passed to create_task for batch deletion)
        let old_id = if is_edit {
            if let Some(app) = ah.upgrade() {
                app.get_scheduler_dialog_task_id().to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let requires_elevation = true;

        tokio::spawn(async move {
            let params = crate::app::scheduler_task::CreateTaskParams {
                display_name: name_str,
                command: cmd_str,
                arguments: args_str,
                cron_expr: cron_str,
                admin_mode: new_admin_mode,
            };
            let old_task = if old_id.is_empty() { None } else { Some(old_id.as_str()) };
            let result = crate::app::scheduler_task::schtasks::create_task(&params, requires_elevation, old_task);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    match result {
                        Ok(()) => {
                            app.set_show_scheduler_dialog(false);
                            let msg = if is_edit {
                                crate::i18n::t("scheduler.task_updated")
                            } else {
                                crate::i18n::t("scheduler.task_created")
                            };
                            show_toast(ah.clone(), msg);
                            refresh_tasks(ah.clone());
                        }
                        Err(e) => {
                            app.set_scheduler_dialog_submitting(false);
                            error!("Failed to create/update scheduler task: {}", e);
                            let msg = crate::i18n::t("scheduler.error_create_failed");
                            show_toast(ah.clone(), format!("{}: {}", msg, e));
                        }
                    }
                }
            });
        });
    });

    // Close dialog
    let ah_close = app_handle.clone();
    app.on_close_scheduler_dialog(move || {
        if let Some(app) = ah_close.upgrade() {
            app.set_scheduler_dialog_submitting(false);
            app.set_show_scheduler_dialog(false);
        }
        clear_all_errors(&ah_close);
    });

    // Browse command (file picker)
    let ah_browse = app_handle.clone();
    app.on_browse_scheduler_command(move || {
        let ah = ah_browse.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(|| {
                rfd::FileDialog::new()
                    .add_filter("Scripts", &["ps1", "bat", "cmd", "exe", "py"])
                    .set_title("Select Command")
                    .pick_file()
            }).await;

            if let Ok(Some(path)) = result {
                let path_str = path.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        app.set_scheduler_dialog_command(path_str.into());
                    }
                });
            }
        });
    });

    // Open cron help URL
    let _ah_help = app_handle.clone();
    app.on_open_scheduler_help(move |url| {
        let url_str = url.to_string();
        if !url_str.is_empty() {
            let _ = open::that(&url_str);
        }
    });

    // Dialog field change callbacks (for future validation)
    app.on_scheduler_name_changed(move |_name| {});
    app.on_scheduler_command_changed(move |_cmd| {});
    app.on_scheduler_arguments_changed(move |_args| {});
    app.on_scheduler_cron_changed(move |_cron| {});
}

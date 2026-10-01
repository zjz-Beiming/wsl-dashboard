// SPDX-FileCopyrightText: Copyright (c) 2026 owu <wqh@live.com>
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, error, debug};
use crate::{AppState, AppWindow};
use crate::wsl::ops::config::{get_wsl_conf, check_wsl_version_support, validate_wsl_conf, save_wsl_conf, serialize_wsl_conf, WslVersionMeta, WslConf};

static IS_LOADING_CONFIG: AtomicBool = AtomicBool::new(false);

pub async fn handle_configs_clicked(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    name: String,
) {
    if IS_LOADING_CONFIG.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        debug!("handle_configs_clicked: Already loading, ignoring request for '{}'", name);
        return;
    }

    let _guard = scopeguard::guard((), |_| {
        IS_LOADING_CONFIG.store(false, Ordering::SeqCst);
    });

    info!("Operation: Loading wsl.conf for '{}'", name);

    // Show loading status
    let _ = slint::invoke_from_event_loop({
        let ah = ah.clone();
        move || {
            if let Some(app) = ah.upgrade() {
                app.set_task_status_text(crate::i18n::t("wsl_conf.loading").into());
                app.set_task_status_visible(true);
            }
        }
    });

    let executor = {
        // Use a timeout for lock acquisition on UI thread to prevent freezing if background task is stuck
        let lock_timeout = std::time::Duration::from_millis(500);
        match tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
            Ok(state) => state.wsl_dashboard.executor().clone(),
            Err(_) => {
                error!("handle_configs_clicked: AppState lock timeout, UI might be unresponsive");
                return;
            }
        }
    };

    // 1. Check version support
    let version_meta = check_wsl_version_support(&executor).await;

    // 2. Read wsl.conf
    let conf = get_wsl_conf(&executor, &name).await;

    // 3. Update UI
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            // Hide loading status
            app.set_task_status_visible(false);
            
            // Set version info
            app.set_wsl_config_version_string(version_meta.version_string.clone().into());
            app.set_wsl_config_boot_supported(version_meta.boot_supported);
            app.set_wsl_config_gpu_supported(version_meta.gpu_supported);
            app.set_wsl_config_time_supported(version_meta.time_supported);
            app.set_wsl_config_version_detection_failed(version_meta.detection_failed);

            // Set distro name
            app.set_wsl_config_distro_name(name.into());

            // Clear errors
            app.set_wsl_config_user_error("".into());
            app.set_wsl_config_command_error("".into());
            app.set_wsl_config_preview_content("".into());

            // Map [automount]
            app.set_wsl_config_automount_enabled(conf.automount.enabled.unwrap_or(true));
            app.set_wsl_config_automount_mount_fs_tab(conf.automount.mount_fs_tab.unwrap_or(true));
            app.set_wsl_config_automount_root(conf.automount.root.unwrap_or_else(|| "/mnt/".to_string()).into());
            app.set_wsl_config_automount_options(conf.automount.options.unwrap_or_default().into());

            // Map [network]
            app.set_wsl_config_network_generate_hosts(conf.network.generate_hosts.unwrap_or(true));
            app.set_wsl_config_network_generate_resolv_conf(conf.network.generate_resolv_conf.unwrap_or(true));
            app.set_wsl_config_network_hostname(conf.network.hostname.unwrap_or_default().into());

            // Map [interop]
            app.set_wsl_config_interop_enabled(conf.interop.enabled.unwrap_or(true));
            app.set_wsl_config_interop_append_windows_path(conf.interop.append_windows_path.unwrap_or(true));

            // Map [user]
            app.set_wsl_config_user_default(conf.user.default.unwrap_or_default().into());

            // Map [boot]
            app.set_wsl_config_boot_systemd(conf.boot.systemd.unwrap_or(false));
            app.set_wsl_config_boot_command(conf.boot.command.unwrap_or_default().into());
            app.set_wsl_config_boot_protect_binfmt(conf.boot.protect_binfmt.unwrap_or(true));

            // Map [gpu]
            app.set_wsl_config_gpu_enabled(conf.gpu.enabled.unwrap_or(true));

            // Map [time]
            app.set_wsl_config_time_use_windows_timezone(conf.time.use_windows_timezone.unwrap_or(true));

            // Show dialog
            app.set_show_wsl_config(true);
        }
    });
}

fn collect_conf_from_ui(app: &AppWindow) -> (WslConf, WslVersionMeta) {
    let conf = WslConf {
        automount: crate::wsl::ops::config::AutomountSection {
            enabled: Some(app.get_wsl_config_automount_enabled()),
            mount_fs_tab: Some(app.get_wsl_config_automount_mount_fs_tab()),
            root: Some(app.get_wsl_config_automount_root().to_string()),
            options: Some(app.get_wsl_config_automount_options().to_string()),
        },
        network: crate::wsl::ops::config::NetworkSection {
            generate_hosts: Some(app.get_wsl_config_network_generate_hosts()),
            generate_resolv_conf: Some(app.get_wsl_config_network_generate_resolv_conf()),
            hostname: Some(app.get_wsl_config_network_hostname().to_string()),
        },
        interop: crate::wsl::ops::config::InteropSection {
            enabled: Some(app.get_wsl_config_interop_enabled()),
            append_windows_path: Some(app.get_wsl_config_interop_append_windows_path()),
        },
        user: crate::wsl::ops::config::UserSection {
            default: Some(app.get_wsl_config_user_default().to_string()),
        },
        boot: crate::wsl::ops::config::BootSection {
            systemd: Some(app.get_wsl_config_boot_systemd()),
            command: Some(app.get_wsl_config_boot_command().to_string()),
            protect_binfmt: Some(app.get_wsl_config_boot_protect_binfmt()),
        },
        gpu: crate::wsl::ops::config::GpuSection {
            enabled: Some(app.get_wsl_config_gpu_enabled()),
        },
        time: crate::wsl::ops::config::TimeSection {
            use_windows_timezone: Some(app.get_wsl_config_time_use_windows_timezone()),
        },
    };

    let meta = WslVersionMeta {
        version_string: app.get_wsl_config_version_string().to_string(),
        boot_supported: app.get_wsl_config_boot_supported(),
        gpu_supported: app.get_wsl_config_gpu_supported(),
        time_supported: app.get_wsl_config_time_supported(),
        detection_failed: app.get_wsl_config_version_detection_failed(),
        // Only the section flags above are consumed by `serialize_wsl_conf`;
        // capability probes are irrelevant here.
        ..Default::default()
    };

    (conf, meta)
}

pub async fn handle_request_preview(ah: slint::Weak<AppWindow>) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            let (conf, meta) = collect_conf_from_ui(&app);
            let preview = serialize_wsl_conf(&conf, &meta);
            app.set_wsl_config_preview_content(preview.into());
        }
    });
}

pub async fn handle_save_wsl_config(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    restart: bool,
) {
    let _guard = crate::ui::data::BusyGuard::new();
    // 1. Collect all necessary data from UI thread first
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = slint::invoke_from_event_loop({
        let ah = ah.clone();
        move || {
            let res = ah.upgrade().map(|app| {
                let name = app.get_wsl_config_distro_name().to_string();
                let (conf, meta) = collect_conf_from_ui(&app);
                (name, conf, meta)
            });
            let _ = tx.send(res);
        }
    });

    let (name, conf, meta) = match rx.await {
        Ok(Some(res)) => res,
        _ => return,
    };

    info!("Operation: Saving wsl.conf for '{}' (restart={})", name, restart);

    // 2. Acquire dashboard and executor
    let wsl_dashboard = {
        let lock_timeout = std::time::Duration::from_millis(500);
        match tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
            Ok(state) => state.wsl_dashboard.clone(),
            Err(_) => {
                error!("handle_save_wsl_config: AppState lock timeout");
                return;
            }
        }
    };
    
    // Set manual operation to prevent background refresh from hiding progress toast
    wsl_dashboard.increment_manual_operation();
    let dashboard_c = wsl_dashboard.clone();
    let _op_guard = scopeguard::guard((), move |_| {
        dashboard_c.decrement_manual_operation();
    });

    let executor = wsl_dashboard.executor().clone();

    // 3. Validation
    let validation = validate_wsl_conf(&executor, &name, &conf).await;
    if !validation.success {
        let ah_c = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_c.upgrade() {
                app.set_wsl_config_user_error(validation.user_error.unwrap_or_default().into());
                app.set_wsl_config_command_error(validation.command_error.unwrap_or_default().into());
            }
        });
        return;
    }

    // 2. Save
    match save_wsl_conf(&executor, &name, &conf, &meta).await {
        Ok(_) => {
            let ah_inner = ah.clone();
            let name_inner = name.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_inner.upgrade() {
                    app.set_show_wsl_config(false);
                    // Show success toast
                    app.set_task_status_text(crate::i18n::t("wsl_conf.save_success").into());
                    app.set_task_status_visible(true);
                    
                    // Auto-hide toast
                    let ah_timer = ah_inner.clone();
                    slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
                        if let Some(app) = ah_timer.upgrade() {
                            app.set_task_status_visible(false);
                        }
                    });
                }
            });

            // 3. Optional Restart
            if restart {
                info!("Restarting distro '{}' to apply changes", name_inner);
                
                // Update status to restarting
                let ah_c = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_c.upgrade() {
                        app.set_task_status_text(crate::i18n::t("operation.restarting").into());
                        app.set_task_status_visible(true);
                    }
                });

                let _ = wsl_dashboard.restart_distro(&name_inner).await;

                // Hide status
                let ah_final = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_final.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            }
        }
        Err(e) => {
            error!("Failed to save wsl.conf for '{}': {}", name, e);
            let ah_c = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_c.upgrade() {
                    let msg = crate::i18n::tr("wsl_conf.save_failed", &[e.clone()]);
                    app.set_current_message(msg.into());
                    app.set_show_message_dialog(true);
                }
            });
        }
    }
}

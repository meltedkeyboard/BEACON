mod commands;
mod state;

use beacon_core::config::{default_config_path, LauncherConfig};
use tauri::Manager;

use commands::{accounts, instances, launch, mod_browser, modloader, settings, skins};
use state::AppState;

/// First launch (no saved `window-state` file yet): tauri.conf.json's static fallback size is
/// tiny on anything above ~1080p, and with no explicit position the OS/webview has been observed
/// pinning the window to the screen's top-left corner. Size and center it against the *actual*
/// primary monitor instead, before the window (created `"visible": false`) is ever shown. Any
/// later launch just restores whatever `tauri-plugin-window-state` saved last time --
/// `skip_initial_state` on the plugin builder keeps it from restoring (and showing the window)
/// before this runs.
fn position_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri_plugin_window_state::{StateFlags, WindowExt as _};

    let window = app
        .get_webview_window("main")
        .expect("the \"main\" window is declared in tauri.conf.json");

    let has_saved_state = app
        .path()
        .app_config_dir()
        .map(|dir| dir.join(tauri_plugin_window_state::DEFAULT_FILENAME).exists())
        .unwrap_or(false);

    if !has_saved_state {
        if let Ok(Some(monitor)) = window.primary_monitor() {
            let logical = monitor.size().to_logical::<f64>(monitor.scale_factor());
            let width = (logical.width * 0.7).clamp(900.0, 1600.0);
            let height = (logical.height * 0.75).clamp(640.0, 1000.0);
            let _ = window.set_size(tauri::LogicalSize::new(width, height));
        }
        let _ = window.center();
    }

    // Applies saved position/size/maximized when `has_saved_state`; otherwise a no-op on those
    // (nothing on disk to restore), leaving the sizing above in place.
    let _ = window.restore_state(StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED);
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// Brings the existing window to the front instead of letting a second `beacon-desktop.exe`
/// start a second process against the same `config.json`/`game_dir` -- two instances writing to
/// those concurrently (especially mid-`relocate_directory`, mid-wipe, or mid-install) would race.
fn focus_existing_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_existing_window(app);
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            // `skip_initial_state` because we want first-launch sizing (below) to run before
            // *anything* touches the window, restored or not -- see the `setup` closure.
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .skip_initial_state("main")
                .build(),
        )
        .setup(|app| {
            let config_path = default_config_path();
            let config = tauri::async_runtime::block_on(LauncherConfig::load_or_default(&config_path))?;
            instances::allow_existing_icons(app.handle(), &config);
            app.manage(AppState::new(beacon_core::http_client(), config, config_path));

            position_main_window(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            launch::list_versions,
            launch::install_version_cmd,
            launch::launch_instance_cmd,
            accounts::list_accounts,
            accounts::select_account_cmd,
            accounts::move_account_cmd,
            accounts::add_offline_account_cmd,
            accounts::rename_offline_account_cmd,
            accounts::login_microsoft_cmd,
            accounts::logout_cmd,
            instances::list_instances,
            instances::create_instance_cmd,
            instances::select_instance_cmd,
            instances::rename_instance_cmd,
            instances::set_instance_version_cmd,
            instances::set_instance_icon_cmd,
            instances::delete_instance_cmd,
            instances::list_worlds_cmd,
            instances::list_resource_packs_cmd,
            instances::list_shader_packs_cmd,
            instances::delete_world_cmd,
            instances::delete_datapack_cmd,
            instances::delete_resource_pack_cmd,
            instances::delete_shader_pack_cmd,
            instances::list_mods_cmd,
            instances::toggle_mod_cmd,
            instances::delete_mod_cmd,
            instances::add_mods_cmd,
            instances::list_screenshots_cmd,
            instances::delete_screenshot_cmd,
            instances::set_pinned_screenshot_cmd,
            instances::export_instance_cmd,
            instances::import_instance_cmd,
            modloader::list_loader_versions_cmd,
            modloader::install_loader_cmd,
            modloader::remove_loader_cmd,
            mod_browser::search_mods_cmd,
            mod_browser::list_mod_versions_cmd,
            mod_browser::get_mod_description_cmd,
            mod_browser::preview_mod_install_cmd,
            mod_browser::install_selected_mods_cmd,
            mod_browser::list_mod_provenance_cmd,
            mod_browser::remove_mod_source_cmd,
            mod_browser::set_curseforge_api_key_cmd,
            mod_browser::has_curseforge_api_key_cmd,
            settings::get_directory_settings,
            settings::set_game_dir_cmd,
            settings::set_instances_dir_cmd,
            settings::wipe_all_data_cmd,
            skins::get_skin_profile_cmd,
            skins::upload_skin_cmd,
            skins::reset_skin_cmd,
            skins::set_cape_cmd,
            skins::clear_cape_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

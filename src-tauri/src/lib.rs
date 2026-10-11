mod auth;
mod game;

#[derive(serde::Serialize)]
struct LauncherInfo {
    version: String,
    os: String,
    arch: String,
}

#[tauri::command]
fn launcher_info(app: tauri::AppHandle) -> LauncherInfo {
    LauncherInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(auth::AuthState::new())
        .manage(game::GameState::new())
        .setup(|app| {
            // Los avisos del launcher salen en la terminal donde corre
            // `npm run tauri dev` y además se guardan en un archivo, dentro de
            // la carpeta "logs" de los datos del launcher: si a un amigo le
            // falla algo, ese archivo dice qué pasó. Nunca incluyen tokens.
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            launcher_info,
            auth::restore_session,
            auth::sign_in,
            auth::cancel_sign_in,
            auth::sign_out,
            game::server_status,
            game::modpack_status,
            game::prepare,
            game::play,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

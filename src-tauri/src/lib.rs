use std::process::Command;
use std::sync::Mutex;
use tauri::Manager;

struct ServerProcess(Mutex<Option<std::process::Child>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // In dev: CARGO_MANIFEST_DIR = src-tauri/, parent = project root
            // In prod: use executable dir
            let project_root = if cfg!(debug_assertions) {
                let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
                    .unwrap_or_else(|_| ".".to_string());
                std::path::PathBuf::from(manifest_dir)
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf()
            } else {
                std::env::current_exe()
                    .unwrap_or_default()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf()
            };
            let server_script = project_root.join("src").join("server").join("index.js");

            println!("[aterm] Starting Node.js server: {:?}", server_script);

            let server: std::process::Child = Command::new("node")
                .arg(&server_script)
                .spawn()
                .expect("Failed to start aterm Node.js server");

            app.manage(ServerProcess(Mutex::new(Some(server))));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = &event {
                if let Some(state) = app_handle.try_state::<ServerProcess>() {
                    if let Ok(mut guard) = state.0.lock() {
                        if let Some(ref mut child) = *guard {
                            let pid = child.id();
                            println!("[aterm] Stopping Node.js server (pid: {})", pid);
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                        *guard = None;
                    }
                }
            }
        });
}

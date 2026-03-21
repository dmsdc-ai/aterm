use std::process::Command;
use std::sync::Mutex;
use tauri::Manager;

struct ServerProcess(Mutex<Option<std::process::Child>>);

/// Wait for the Node.js server to become ready (HTTP 200 on /api/status)
fn wait_for_server(port: u16, max_attempts: u32) -> bool {
    for i in 0..max_attempts {
        if let Ok(output) = Command::new("curl")
            .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &format!("http://localhost:{}/", port)])
            .output()
        {
            let code = String::from_utf8_lossy(&output.stdout);
            if code.trim() == "200" {
                println!("[aterm] Server ready after {} attempts", i + 1);
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    println!("[aterm] Server not ready after {} attempts, proceeding anyway", max_attempts);
    false
}

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

            // Wait for server to be ready before WebView loads content
            wait_for_server(3849, 20); // max 6 seconds (20 * 300ms)

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

//! Portward desktop app: a thin Tauri shell that talks to the local daemon.

use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

const SYSTEM_DIR: &str = "/Library/Application Support/Portward";

fn api_base() -> String {
    std::env::var("PORTWARD_API").unwrap_or_else(|_| "http://127.0.0.1:817".into())
}

fn token_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(p) = std::env::var("PORTWARD_TOKEN_FILE") {
        v.push(PathBuf::from(p));
    }
    v.push(PathBuf::from(SYSTEM_DIR).join("api-token"));
    if let Ok(h) = std::env::var("HOME") {
        v.push(PathBuf::from(h).join("Library/Application Support/Portward/api-token"));
    }
    v
}

fn token() -> Option<String> {
    token_paths()
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("client")
    })
}

async fn call(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let token = token().ok_or_else(|| "NO_TOKEN".to_string())?;
    let url = format!("{}{}", api_base(), path);
    let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut req = client().request(m, url).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() {
            "DAEMON_DOWN".to_string()
        } else {
            e.to_string()
        }
    })?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("request failed");
        return Err(format!("{status}: {msg}"));
    }
    Ok(v)
}

#[tauri::command]
async fn api(method: String, path: String, body: Option<Value>) -> Result<Value, String> {
    call(&method, &path, body).await
}

const ICON_JXA: &str = r#"
ObjC.import('AppKit');
function run(argv) {
  var img = $.NSWorkspace.sharedWorkspace.iconForFile(argv[0]);
  var size = $.NSMakeSize(64, 64);
  var rep = $.NSBitmapImageRep.alloc.initWithBitmapDataPlanesPixelsWidePixelsHighBitsPerSampleSamplesPerPixelHasAlphaIsPlanarColorSpaceNameBytesPerRowBitsPerPixel(
    null, 64, 64, 8, 4, true, false, $.NSDeviceRGBColorSpace, 0, 0);
  $.NSGraphicsContext.saveGraphicsState;
  $.NSGraphicsContext.setCurrentContext($.NSGraphicsContext.graphicsContextWithBitmapImageRep(rep));
  img.drawInRectFromRectOperationFraction($.NSMakeRect(0, 0, 64, 64), $.NSZeroRect, $.NSCompositingOperationSourceOver, 1.0);
  $.NSGraphicsContext.restoreGraphicsState;
  var png = rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $({}));
  png.writeToFileAtomically(argv[1], true);
}
"#;

fn icon_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Caches/Portward/icons")
}

/// The real Finder icon of an app or executable, as a PNG data URL (cached).
#[tauri::command]
async fn app_icon(path: String) -> Option<String> {
    use base64::Engine;
    use std::hash::{Hash, Hasher};
    if path.is_empty() || !std::path::Path::new(&path).exists() {
        return None;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    let dir = icon_cache_dir();
    let file = dir.join(format!("{:016x}.png", h.finish()));
    if !file.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        let out = file.clone();
        tauri::async_runtime::spawn_blocking(move || {
            std::process::Command::new("/usr/bin/osascript")
                .args(["-l", "JavaScript", "-e", ICON_JXA, &path, &out.to_string_lossy()])
                .output()
        })
        .await
        .ok()?
        .ok()?;
    }
    let bytes = std::fs::read(&file).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command]
fn reveal(path: String) {
    let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api, reveal, app_icon])
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "Abrir Portward", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "Pausar proteção", true, None::<&str>)?;
            let resume = MenuItem::with_id(app, "resume", "Retomar proteção", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Fechar janela do Portward", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &sep, &pause, &resume, &sep, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Portward")
                .menu(&menu)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "open" => show_main(app),
                    "pause" | "resume" => {
                        let paused = ev.id.as_ref() == "pause";
                        tauri::async_runtime::spawn(async move {
                            let _ = call("POST", "/api/v1/pause", Some(serde_json::json!({ "paused": paused }))).await;
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone()).icon_as_template(false);
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|w, ev| {
            // Closing the window keeps the app in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = ev {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, ev| {
            if let tauri::RunEvent::Reopen { .. } = ev {
                show_main(app);
            }
        });
}

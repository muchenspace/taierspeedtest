#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod net;
mod speed;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use net::Node;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct SavedConfig {
    #[serde(default)]
    province: String,
    #[serde(default)]
    oper: String,
    #[serde(default)]
    hostip: String,
    #[serde(default)]
    port: u16,
}

fn config_file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建配置目录: {e}"))?;
    Ok(dir.join("config.json"))
}

#[tauri::command]
fn load_config(app: AppHandle) -> Option<SavedConfig> {
    let path = config_file(&app).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[tauri::command]
fn save_config(app: AppHandle, cfg: SavedConfig) -> Result<(), String> {
    let path = config_file(&app)?;
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("写入配置失败: {e}"))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PhasePayload {
    idx: i32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PingStepPayload {
    rtt: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PingPayload {
    ping: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct InstantPayload {
    mbps: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SamplePayload {
    mbps: f64,
    preheat: bool,
    phase: i32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ResultPayload {
    down: Option<f64>,
    up: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestMode {
    Full,
    Ping,
    Down,
    Up,
}

impl TestMode {
    fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("full") {
            "ping" => TestMode::Ping,
            "down" => TestMode::Down,
            "up" => TestMode::Up,
            _ => TestMode::Full,
        }
    }

    fn wants_down(self) -> bool {
        matches!(self, TestMode::Full | TestMode::Down)
    }

    fn wants_up(self) -> bool {
        matches!(self, TestMode::Full | TestMode::Up)
    }
}

#[derive(Serialize, Clone)]
struct DonePayload {
    error: Option<String>,
}

struct AppState {
    nodes: Mutex<Vec<Node>>,
    running: Arc<AtomicBool>,
}

fn emit_phase(app: &AppHandle, idx: i32) {
    let _ = app.emit("phase", PhasePayload { idx });
}

fn store_nodes(app: &AppHandle, list: Vec<Node>) -> usize {
    let n = list.len();
    let state = app.state::<AppState>();
    if let Ok(mut g) = state.nodes.lock() {
        *g = list;
    }
    n
}

#[tauri::command]
fn get_nodes(state: State<'_, AppState>) -> Vec<Node> {
    state.nodes.lock().map(|g| g.clone()).unwrap_or_default()
}

#[tauri::command]
fn refresh_nodes(app: AppHandle) -> Result<usize, String> {
    let list = net::refresh_nodes()?;
    Ok(store_nodes(&app, list))
}

#[tauri::command]
fn start_test(
    app: AppHandle,
    state: State<'_, AppState>,
    node: Node,
    mode: Option<String>,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("已有测速任务在进行".into());
    }
    let running = Arc::clone(&state.running);
    let mode = TestMode::parse(mode.as_deref());

    std::thread::spawn(move || run_test(app, node, mode, running));
    Ok(())
}

fn run_test(app: AppHandle, node: Node, mode: TestMode, running: Arc<AtomicBool>) {
    macro_rules! bail {
        ($msg:expr) => {{
            running.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "done",
                DonePayload {
                    error: Some($msg.to_string()),
                },
            );
            return;
        }};
    }

    emit_phase(&app, 0);
    let ping = net::measure_latency(&node, &mut |rtt| {
        let _ = app.emit("ping-step", PingStepPayload { rtt });
    });
    let _ = app.emit("ping", PingPayload { ping });

    if mode == TestMode::Ping {
        running.store(false, Ordering::SeqCst);
        let _ = app.emit("done", DonePayload { error: None });
        return;
    }

    emit_phase(&app, 1);
    let uuid = match net::enqueue(&node) {
        Ok(u) => u,
        Err(e) => bail!(e),
    };

    let ctx = speed::PhaseCtx::new(&node, uuid.clone());

    let mut down: Option<speed::PhaseResult> = None;
    let mut up: Option<speed::PhaseResult> = None;

    if mode.wants_down() {
        emit_phase(&app, 2);
        let r = {
            let mut on_sample = |pre: bool, mbps: f64| {
                let _ = app.emit(
                    "sample",
                    SamplePayload {
                        mbps,
                        preheat: pre,
                        phase: 2,
                    },
                );
            };
            let mut on_instant = |mbps: f64| {
                let _ = app.emit("instant", InstantPayload { mbps });
            };
            speed::run_phase(&ctx, false, &mut on_sample, &mut on_instant)
        };

        if !r.has_samples {
            net::dequeue(&node, &uuid);
            bail!("下载失败: 节点未返回有效数据（可能被限流或该节点不支持测速）");
        }
        down = Some(r);
    }

    if mode.wants_up() {
        emit_phase(&app, 3);
        let r = {
            let mut on_sample = |pre: bool, mbps: f64| {
                let _ = app.emit(
                    "sample",
                    SamplePayload {
                        mbps,
                        preheat: pre,
                        phase: 3,
                    },
                );
            };
            let mut on_instant = |mbps: f64| {
                let _ = app.emit("instant", InstantPayload { mbps });
            };
            speed::run_phase(&ctx, true, &mut on_sample, &mut on_instant)
        };

        if !r.has_samples {
            net::dequeue(&node, &uuid);
            bail!("上传失败: 节点未接收有效数据");
        }
        up = Some(r);
    }

    net::dequeue(&node, &uuid);

    let _ = app.emit(
        "result",
        ResultPayload {
            down: down.as_ref().map(|d| d.avg),
            up: up.as_ref().map(|u| u.avg),
        },
    );
    running.store(false, Ordering::SeqCst);
    let _ = app.emit("done", DonePayload { error: None });
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState {
                nodes: Mutex::new(Vec::new()),
                running: Arc::new(AtomicBool::new(false)),
            });

            std::thread::spawn({
                let handle = handle.clone();
                move || {
                    match net::refresh_nodes() {
                        Ok(list) => {
                            let n = store_nodes(&handle, list);
                            let _ = handle.emit("nodes-updated", n);
                        }
                        Err(e) => {
                            let _ = handle.emit("nodes-error", e);
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_nodes,
            refresh_nodes,
            start_test,
            load_config,
            save_config
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

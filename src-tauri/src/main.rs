// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use tauri::Window;
use tauri::Manager;
use tauri_plugin_log::LogTarget;

use lazy_static::lazy_static;

mod clipboard;
mod producer;
mod xs;

lazy_static! {
    static ref PRODUCER: producer::Producer = producer::Producer::new();
    static ref PROCESS_MAP: Mutex<HashMap<String, Arc<AtomicBool>>> = Mutex::new(HashMap::new());
    static ref DATADIR: Mutex<PathBuf> = Mutex::new(PathBuf::new());
}

#[derive(Clone, serde::Serialize)]
struct Payload {
    message: String,
}

#[derive(Clone, serde::Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[tauri::command]
fn init_process(window: Window) -> Result<Vec<String>, String> {
    let label = window.label().to_string();
    log::info!("WINDOW: {:?}", label);

    // If there's an existing process for this window, stop it
    let mut process_map = PROCESS_MAP.lock().unwrap();

    if let Some(should_continue) = process_map.get(&label) {
        should_continue.store(false, Ordering::SeqCst);
    } else {
        // only setup an event listener the first time we see this window
        window.on_window_event(move |event| log::info!("EVENT: {:?}", event));
    }

    let should_continue = Arc::new(AtomicBool::new(true));
    process_map.insert(label.clone(), should_continue.clone());
    drop(process_map); // Explicitly drop the lock

    let (initial_data, consumer) = PRODUCER.add_consumer();

    std::thread::spawn(move || {
        for line in consumer.iter() {
            if !should_continue.load(Ordering::SeqCst) {
                log::info!("Window closed, ending thread.");
                break;
            }

            window.emit("item", Payload { message: line }).unwrap();
        }
    });

    Ok(initial_data)
}

#[tauri::command]
fn run_command(command: &str) -> Result<CommandOutput, String> {
    let parts = shlex::split(command).ok_or("Failed to parse command")?;
    let program = parts.get(0).ok_or("No program specified")?;
    let args = &parts[1..];

    let output = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute command: {}", e))?;

    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|_| String::new());
    let stderr = String::from_utf8(output.stderr).unwrap_or_else(|_| String::new());
    let exit_code = output.status.code().unwrap_or(-1);

    let output = CommandOutput {
        stdout,
        stderr,
        exit_code,
    };

    let json_data = serde_json::json!({
        "command": command,
        "output": output
    });

    let json_string = json_data.to_string();

    let data_dir = DATADIR.lock().unwrap();

    let mut child = std::process::Command::new("xs")
        .arg(&*data_dir)
        .arg("put")
        .arg("--topic")
        .arg("command")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to execute xs command: {}", e))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(json_string.as_bytes())
            .map_err(|e| format!("Failed to write to xs stdin: {}", e))?;
    }

    // Wait for the subprocess to finish
    let _ = child.wait();

    Ok(output)
}

fn start_child_process(path: &PathBuf) {
    let path = path.clone();
    std::thread::spawn(move || {
        let mut last_id = None;
        let mut counter = 0;
        loop {
            let env = xs::store_open(&path);
            let frames = xs::store_cat(&env, last_id);
            for frame in frames {
                last_id = Some(frame.id);
                let data = serde_json::to_string(&frame).unwrap();
                PRODUCER.send_data(data);
            }
            if counter % 1000 == 0 {
                log::info!("start_child_process::last_id: {:?}", last_id);
            }
            counter += 1;
            std::thread::sleep(std::time::Duration::from_millis(xs::POLL_INTERVAL));
        }
    });
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![init_process, run_command])
        .plugin(tauri_plugin_spotlight::init(Some(
            tauri_plugin_spotlight::PluginConfig {
                windows: Some(vec![tauri_plugin_spotlight::WindowConfig {
                    label: String::from("main"),
                    shortcut: String::from("Control+Space"),
                    macos_window_level: Some(20), // Default 24
                }]),
                global_close_shortcut: None,
            },
        )))
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([LogTarget::LogDir, LogTarget::Stdout, LogTarget::Webview])
                .level_for("tao", log::LevelFilter::Debug)
                .build(),
        )
        .setup(|app| {
            let _window = app.get_window("main").unwrap();
            // window.open_devtools();
            // window.close_devtools();

            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let data_dir = app.path_resolver().app_data_dir().unwrap();
            let data_dir = data_dir.join("stream");
            log::info!("PR: {:?}", data_dir);
            let mut shared = DATADIR.lock().unwrap();
            *shared = data_dir;

            clipboard::start(&*shared);
            start_child_process(&*shared);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

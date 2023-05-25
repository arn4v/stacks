use std::path::PathBuf;

use tauri::api::process::{Command, CommandEvent};

pub fn start(path: &PathBuf) {
    let path = path.clone();
    let (mut rx, _child) = Command::new_sidecar("x-macos-pasteboard")
        .unwrap()
        .spawn()
        .unwrap();

    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Stdout(line) = event {
                let path = path.clone();
                let env = xs_lib::store_open(&path);
                log::info!("{}", xs_lib::store_put(&env, Some("clipboard".into()), None, line));
            }
        }
    });
}

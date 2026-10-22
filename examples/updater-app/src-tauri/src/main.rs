//! Tauri v2 self-hosted updater drill sample (T-M9-12).
//!
//! A normal, tiny Tauri v2 window plus a headless `--smoke-update` mode:
//!
//! 1. ask the updater plugin to check the self-hosted static `latest.json`;
//! 2. download the platform artifact and verify its Tauri `.sig` (the plugin
//!    refuses to install bytes that do not match the baked public key);
//! 3. install it (on Windows the plugin exits the app and the NSIS installer
//!    relaunches it with the same arguments);
//! 4. append every phase to `--marker <path>` so CI can assert versions.
//!
//! The version comparator accepts *any different* version. That is a drill-only
//! switch so the rollback leg can install v1 over v2 from a restored static
//! manifest; production apps keep the default "must be newer" check (or gate
//! downgrades behind an explicit channel).
//!
//! Release builds use the Windows GUI subsystem, so the marker file is the only
//! reliable evidence channel (no console attached).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Value of `--flag <value>` (the drill passes `--marker <path>`).
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

/// Append one phase line to the marker file (best effort: markers are evidence,
/// never a reason to fail the update).
fn record(marker: Option<&Path>, line: &str) {
    let Some(path) = marker else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

async fn check_and_update(
    app: AppHandle,
    marker: Option<PathBuf>,
    current: String,
) -> Result<(), String> {
    let updater = app
        .updater_builder()
        // Drill-only: static manifests do not downgrade by default. Accept a
        // different advertised version so the restored v1 manifest can roll v2 back.
        .version_comparator(|current, remote| remote.version != current)
        .build()
        .map_err(|err| format!("updater build failed: {err}"))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|err| format!("check failed: {err}"))?
    else {
        return Ok(());
    };
    record(
        marker.as_deref(),
        &format!("detected current={current} remote={}", update.version),
    );
    update
        .download_and_install(
            |_chunk, _total| {},
            || record(marker.as_deref(), &format!("downloaded current={current}")),
        )
        .await
        .map_err(|err| format!("download/install failed: {err}"))?;
    // Windows install() exits the process after spawning the installer, so this
    // line is only reached on other hosts.
    record(marker.as_deref(), &format!("installed current={current}"));
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let smoke_update = args.iter().any(|arg| arg == "--smoke-update");
    let marker = arg_value(&args, "--marker").map(PathBuf::from);

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if smoke_update {
                let handle = app.handle().clone();
                let marker = marker.clone();
                let current = app.package_info().version.to_string();
                record(marker.as_deref(), &format!("start version={current}"));
                tauri::async_runtime::spawn(async move {
                    match check_and_update(handle.clone(), marker.clone(), current.clone()).await {
                        Ok(()) => {
                            record(marker.as_deref(), &format!("up-to-date version={current}"));
                            handle.exit(0);
                        }
                        Err(err) => {
                            record(marker.as_deref(), &format!("error version={current} {err}"));
                            handle.exit(2);
                        }
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("polypack-updater-sample: Tauri runtime failed");
}

#[cfg(test)]
mod tests {
    use super::arg_value;

    #[test]
    fn parses_flag_value_and_rejects_missing_values() {
        let args = [
            "polypack-updater-sample.exe",
            "--smoke-update",
            "--marker",
            "C:\\evidence\\update.marker",
        ]
        .map(String::from);
        assert_eq!(
            arg_value(&args, "--marker").as_deref(),
            Some("C:\\evidence\\update.marker")
        );
        assert_eq!(arg_value(&args, "--missing"), None);
        assert_eq!(
            arg_value(&["app".into(), "--marker".into()], "--marker"),
            None
        );
    }
}

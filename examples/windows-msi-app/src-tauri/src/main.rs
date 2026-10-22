//! Minimal Tauri v2 sample app for the PolyPack real-MSI test (T-M9-53/54).
//!
//! One window, one committed static HTML/JS asset, no external services.
//! `--smoke` starts the normal Tauri runtime (the configured window is created
//! before the setup hook runs, so reaching the hook proves WebView2 initialized),
//! writes an `ok` marker, and exits automatically a moment later. The marker path
//! comes from `POLYPACK_MSI_SMOKE_MARKER`, falling back to the user temp directory.
//!
//! The test launches the *installed* copy from `%ProgramFiles%\Polypack MSI Sample`
//! and uses the marker file rather than stdout because release builds use the
//! Windows GUI subsystem (no console attached).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

/// Where `--smoke` records successful startup.
fn smoke_marker() -> PathBuf {
    std::env::var_os("POLYPACK_MSI_SMOKE_MARKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("polypack-msi-sample-smoke.txt"))
}

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let mut builder = tauri::Builder::default();

    if smoke {
        builder = builder.setup(|app| {
            let marker = smoke_marker();
            std::fs::write(&marker, "ok\n")?;
            println!(
                "polypack-msi-sample: smoke marker written to {}",
                marker.display()
            );
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1200));
                handle.exit(0);
            });
            Ok(())
        });
    }

    builder
        .run(tauri::generate_context!())
        .expect("polypack-msi-sample: Tauri runtime failed");
}

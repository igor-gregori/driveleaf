//! M0 interface spike: synthetic treemap tiles over binary IPC.
//!
//! Generation and transfer are separate commands so the frontend can time the
//! IPC round trip alone. The report path comes from the environment only, never
//! from JavaScript. This module goes away when the real commands land in M2.

use std::sync::Mutex;
use std::time::Instant;

use driveleaf_treemap::{Tile, synthetic_tiles};
use serde::Serialize;
use tauri::ipc::Response;

const REPORT_PATH_VAR: &str = "DRIVELEAF_SPIKE_OUT";

#[derive(Default)]
pub struct SpikeState {
    bytes: Mutex<Vec<u8>>,
}

#[derive(Serialize)]
pub struct Prepared {
    tiles: usize,
    bytes: usize,
    generate_ms: f64,
}

/// True when the app was launched for an unattended benchmark.
#[tauri::command]
pub fn spike_mode() -> bool {
    std::env::var_os(REPORT_PATH_VAR).is_some()
}

#[tauri::command]
pub fn spike_prepare(
    state: tauri::State<'_, SpikeState>,
    count: u32,
    width: f32,
    height: f32,
    seed: u32,
) -> Prepared {
    let start = Instant::now();
    let tiles = synthetic_tiles(count, width, height, seed);
    let mut bytes = Vec::with_capacity(tiles.len() * Tile::BYTES);
    for tile in &tiles {
        tile.write_le(&mut bytes);
    }
    let generate_ms = start.elapsed().as_secs_f64() * 1000.0;
    let prepared = Prepared {
        tiles: tiles.len(),
        bytes: bytes.len(),
        generate_ms,
    };
    *state.bytes.lock().expect("spike state poisoned") = bytes;
    prepared
}

#[tauri::command]
pub fn spike_fetch(state: tauri::State<'_, SpikeState>) -> Response {
    Response::new(state.bytes.lock().expect("spike state poisoned").clone())
}

/// Writes the benchmark report and quits. Does nothing outside bench mode.
#[tauri::command]
pub fn spike_report(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let Some(path) = std::env::var_os(REPORT_PATH_VAR) else {
        return Ok(());
    };
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

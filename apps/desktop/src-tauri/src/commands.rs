//! IPC commands. The tree stays in Rust and the frontend only ever receives what
//! is on screen. Actions take node IDs from the scan, never paths from JavaScript.

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use driveleaf_core::model::{Model, NodeId, ROOT, ScanMode};
use driveleaf_core::query::{Page, SortKey};
use driveleaf_core::safety::check_removable;
use driveleaf_core::scanner::{ScanError, scan_drive};
use driveleaf_ntfs::ScanControl;
use driveleaf_treemap::{LayoutOptions, Tile, layout_tree};
use driveleaf_win::path_from_wtf8;
use driveleaf_win::volumes::{DriveKind, list_volumes as system_volumes};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Manager, State, WebviewWindow};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const NO_SCAN: &str = "no scan loaded";
const UNKNOWN_NODE: &str = "unknown node";

#[derive(Default)]
pub struct AppState {
    model: RwLock<Option<Model>>,
    running: Mutex<Option<Arc<ScanControl>>>,
}

impl AppState {
    fn read<T>(&self, query: impl FnOnce(&Model) -> Result<T, String>) -> Result<T, String> {
        let guard = self.model.read().expect("no command panicked");
        query(guard.as_ref().ok_or(NO_SCAN)?)
    }
}

fn check_node(model: &Model, node: NodeId) -> Result<(), String> {
    if model.contains(node) {
        Ok(())
    } else {
        Err(UNKNOWN_NODE.to_owned())
    }
}

/// Milliseconds since 1970 for JavaScript, from 100 ns ticks since 1601.
fn to_unix_millis(ticks: u64) -> Option<u64> {
    const TICKS_PER_MILLI: u64 = 10_000;
    const MILLIS_TO_UNIX_EPOCH: u64 = 11_644_473_600_000;
    (ticks / TICKS_PER_MILLI).checked_sub(MILLIS_TO_UNIX_EPOCH)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    id: NodeId,
    name: String,
    logical: u64,
    allocated: u64,
    files: u32,
    modified: Option<u64>,
    flags: u16,
    category: &'static str,
    has_children: bool,
    /// Share of the parent's space, from 0 to 1.
    share: f64,
}

fn row(model: &Model, node: NodeId) -> Row {
    let whole = model.allocated(model.parent(node));
    Row {
        id: node,
        name: model.name(node),
        logical: model.logical(node),
        allocated: model.allocated(node),
        files: model.files(node),
        modified: to_unix_millis(model.modified(node)),
        flags: model.flags(node),
        category: model.category(node).key(),
        has_children: model
            .raw_children(node)
            .iter()
            .any(|&child| model.is_visible(child)),
        share: if whole == 0 {
            0.0
        } else {
            model.weight(node) as f64 / whole as f64
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowPage {
    rows: Vec<Row>,
    total: usize,
}

fn row_page(model: &Model, page: &Page) -> RowPage {
    RowPage {
        rows: page.nodes.iter().map(|&node| row(model, node)).collect(),
        total: page.total,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeRow {
    letter: char,
    label: String,
    file_system: String,
    kind: &'static str,
    total_bytes: u64,
    free_bytes: u64,
}

#[tauri::command]
pub fn list_volumes() -> Vec<VolumeRow> {
    system_volumes()
        .into_iter()
        .map(|volume| VolumeRow {
            letter: volume.letter,
            label: volume.label,
            file_system: volume.file_system,
            kind: match volume.kind {
                DriveKind::Fixed => "fixed",
                DriveKind::Removable => "removable",
                DriveKind::Network => "network",
                DriveKind::Optical => "optical",
                DriveKind::Memory => "memory",
                DriveKind::Unknown => "unknown",
            },
            total_bytes: volume.total_bytes,
            free_bytes: volume.free_bytes,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    elevated: bool,
    mica: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        elevated: driveleaf_win::elevation::is_elevated(),
        mica: driveleaf_win::system::supports_mica(),
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    mode: &'static str,
    used_bytes: Option<u64>,
    total_bytes: Option<u64>,
    problems: u64,
    allocated_bytes: u64,
    logical_bytes: u64,
    files: u32,
    entries: usize,
    seconds: f64,
}

fn summary_of(model: &Model, seconds: f64) -> ScanSummary {
    let summary = model.summary();
    ScanSummary {
        mode: match summary.mode {
            ScanMode::Mft => "mft",
            ScanMode::Walk => "walk",
        },
        used_bytes: summary.used_bytes,
        total_bytes: summary.total_bytes,
        problems: summary.problems,
        allocated_bytes: model.allocated(ROOT),
        logical_bytes: model.logical(ROOT),
        files: model.files(ROOT),
        entries: model.len(),
        seconds,
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ScanEvent {
    Progress { done: u64, total: u64 },
    Finished { summary: ScanSummary },
    Cancelled,
    Failed { message: String },
}

#[tauri::command]
pub fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    letter: char,
    on_event: Channel<ScanEvent>,
) -> Result<(), String> {
    let known = system_volumes()
        .iter()
        .any(|volume| volume.letter.eq_ignore_ascii_case(&letter));
    if !known {
        return Err("unknown drive".to_owned());
    }
    let control = Arc::new(ScanControl::default());
    {
        let mut running = state.running.lock().expect("no command panicked");
        if running.is_some() {
            return Err("a scan is already running".to_owned());
        }
        *running = Some(Arc::clone(&control));
    }

    std::thread::spawn(move || {
        let started = Instant::now();
        let finished = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(Ordering::Relaxed) {
                    let (done, total) = control.progress();
                    let _ = on_event.send(ScanEvent::Progress { done, total });
                    std::thread::sleep(PROGRESS_INTERVAL);
                }
            });
            let result = scan_drive(letter, &control);
            finished.store(true, Ordering::Relaxed);
            result
        });

        let state = app.state::<AppState>();
        *state.running.lock().expect("no command panicked") = None;
        let event = match result {
            Ok(model) => {
                let summary = summary_of(&model, started.elapsed().as_secs_f64());
                *state.model.write().expect("no command panicked") = Some(model);
                ScanEvent::Finished { summary }
            }
            Err(ScanError::Cancelled) => ScanEvent::Cancelled,
            Err(ScanError::Failed(message)) => ScanEvent::Failed { message },
        };
        let _ = on_event.send(event);
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    if let Some(control) = state.running.lock().expect("no command panicked").as_ref() {
        control.cancel();
    }
}

#[tauri::command]
pub fn get_node(state: State<'_, AppState>, node: NodeId) -> Result<Row, String> {
    state.read(|model| {
        check_node(model, node)?;
        Ok(row(model, node))
    })
}

/// The node and its ancestors, from the root down.
#[tauri::command]
pub fn get_ancestry(state: State<'_, AppState>, node: NodeId) -> Result<Vec<Row>, String> {
    state.read(|model| {
        check_node(model, node)?;
        Ok(model
            .ancestry(node)
            .into_iter()
            .map(|step| row(model, step))
            .collect())
    })
}

#[tauri::command]
pub fn get_path(state: State<'_, AppState>, node: NodeId) -> Result<String, String> {
    state.read(|model| {
        check_node(model, node)?;
        Ok(model.display_path(node))
    })
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortColumn {
    Allocated,
    Logical,
    Name,
    Files,
    Modified,
}

impl From<SortColumn> for SortKey {
    fn from(column: SortColumn) -> Self {
        match column {
            SortColumn::Allocated => SortKey::Allocated,
            SortColumn::Logical => SortKey::Logical,
            SortColumn::Name => SortKey::Name,
            SortColumn::Files => SortKey::Files,
            SortColumn::Modified => SortKey::Modified,
        }
    }
}

#[tauri::command]
pub fn get_children(
    state: State<'_, AppState>,
    node: NodeId,
    sort: SortColumn,
    descending: bool,
    offset: usize,
    limit: usize,
) -> Result<RowPage, String> {
    state.read(|model| {
        check_node(model, node)?;
        let page = model.list_children(node, sort.into(), descending, offset, limit);
        Ok(row_page(model, &page))
    })
}

/// Tiles for one folder as a binary buffer. See `TileBuffer` in the frontend.
#[tauri::command]
pub fn get_treemap(
    state: State<'_, AppState>,
    node: NodeId,
    width: f32,
    height: f32,
) -> Result<Response, String> {
    state.read(|model| {
        check_node(model, node)?;
        let width = width.clamp(1.0, 16_384.0);
        let height = height.clamp(1.0, 16_384.0);
        let tiles = layout_tree(model, node, width, height, &LayoutOptions::default());
        let mut bytes = Vec::with_capacity(tiles.len() * Tile::BYTES);
        for tile in &tiles {
            tile.write_le(&mut bytes);
        }
        Ok(Response::new(bytes))
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRow {
    #[serde(flatten)]
    row: Row,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRowPage {
    rows: Vec<PathRow>,
    total: usize,
}

fn path_page(model: &Model, page: &Page) -> PathRowPage {
    PathRowPage {
        rows: page
            .nodes
            .iter()
            .map(|&node| PathRow {
                row: row(model, node),
                path: model.display_path(model.parent(node)),
            })
            .collect(),
        total: page.total,
    }
}

#[tauri::command]
pub fn top_files(
    state: State<'_, AppState>,
    offset: usize,
    limit: usize,
) -> Result<PathRowPage, String> {
    state.read(|model| Ok(path_page(model, &model.top_files(offset, limit))))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeRow {
    extension: String,
    category: &'static str,
    files: u64,
    logical: u64,
    allocated: u64,
}

#[tauri::command]
pub fn file_types(state: State<'_, AppState>) -> Result<Vec<TypeRow>, String> {
    state.read(|model| {
        Ok(model
            .file_types()
            .into_iter()
            .map(|row| TypeRow {
                extension: row.extension,
                category: row.category.key(),
                files: row.files,
                logical: row.logical,
                allocated: row.allocated,
            })
            .collect())
    })
}

#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<PathRowPage, String> {
    state.read(|model| Ok(path_page(model, &model.search(&query, limit))))
}

/// The real path of a node that exists on disk.
fn disk_path(model: &Model, node: NodeId) -> Result<OsString, String> {
    check_node(model, node)?;
    let flags = model.flags(node);
    if flags & driveleaf_core::model::FLAG_SYNTHETIC != 0 {
        return Err("not on disk".to_owned());
    }
    Ok(path_from_wtf8(&model.path_bytes(node)))
}

#[tauri::command]
pub fn reveal_in_explorer(state: State<'_, AppState>, node: NodeId) -> Result<(), String> {
    let path = state.read(|model| disk_path(model, node))?;
    driveleaf_win::shell::reveal_in_explorer(&path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn show_properties(state: State<'_, AppState>, node: NodeId) -> Result<(), String> {
    let path = state.read(|model| disk_path(model, node))?;
    driveleaf_win::shell::show_properties(&path).map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    id: NodeId,
    name: String,
    reason: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecycleResult {
    removed: Vec<NodeId>,
    freed_bytes: u64,
    refused: Vec<Refused>,
    /// The user or Windows stopped part of the operation.
    aborted: bool,
}

/// One item cleared to go to the Recycle Bin.
struct Target {
    node: NodeId,
    path: OsString,
    allocated: u64,
}

/// Sends nodes to the Recycle Bin, after re-checking each one against the rules
/// and against what is on disk now. The interface has already asked the user.
#[tauri::command]
pub async fn recycle(
    app: AppHandle,
    window: WebviewWindow,
    nodes: Vec<NodeId>,
) -> Result<RecycleResult, String> {
    let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut targets = Vec::new();
        let mut refused = Vec::new();
        state.read(|model| {
            for &node in &nodes {
                check_node(model, node)?;
                match check_removable(model, node) {
                    Err(refusal) => refused.push(Refused {
                        id: node,
                        name: model.name(node),
                        reason: refusal.key(),
                    }),
                    Ok(()) => {
                        let path = disk_path(model, node)?;
                        // A path can name something else by now: the scan is a
                        // snapshot, and files keep changing after it.
                        let same = driveleaf_win::shell::file_id(&path)
                            .is_ok_and(|id| id as u32 == model.file_id(node));
                        if same {
                            targets.push(Target {
                                node,
                                path,
                                allocated: model.weight(node),
                            });
                        } else {
                            refused.push(Refused {
                                id: node,
                                name: model.name(node),
                                reason: "changed",
                            });
                        }
                    }
                }
            }
            Ok(())
        })?;

        if targets.is_empty() {
            return Ok(RecycleResult {
                removed: Vec::new(),
                freed_bytes: 0,
                refused,
                aborted: false,
            });
        }
        let paths = targets.iter().map(|target| target.path.clone()).collect();
        let completed =
            driveleaf_win::shell::recycle(paths, owner).map_err(|error| error.to_string())?;

        // Whatever the outcome, trust the disk about what is gone.
        let mut model = state.model.write().expect("no command panicked");
        let model = model.as_mut().ok_or(NO_SCAN)?;
        let mut removed = Vec::new();
        let mut freed_bytes = 0;
        for target in &targets {
            if !Path::new(&target.path).exists() {
                model.mark_deleted(target.node);
                removed.push(target.node);
                freed_bytes += target.allocated;
            }
        }
        Ok(RecycleResult {
            removed,
            freed_bytes,
            refused,
            aborted: !completed,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Reopens the app with administrator rights, through the Windows prompt.
#[tauri::command]
pub fn relaunch_elevated(app: AppHandle) -> Result<(), String> {
    driveleaf_win::elevation::relaunch_elevated().map_err(|error| error.to_string())?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_times_convert_to_unix_milliseconds() {
        assert_eq!(to_unix_millis(116_444_736_000_000_000), Some(0));
        assert_eq!(to_unix_millis(116_444_736_010_000_000), Some(1000));
        assert_eq!(to_unix_millis(0), None);
    }
}

use std::sync::atomic::Ordering;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;

use lc::app::job_runner::{RunningJob, start_confirmed_action, start_search_job};
use lc::app::shell;
use lc::app::types::*;
use lc::fs;
use lc::ops;
use lc::ops::archive::ArchiveFormat;
use lc::ui::{dialogs, viewer};

use super::EventContext;
use crate::app::panel_ops::{
    panel_visible_height, rebuild_visible_entries, refresh_active, refresh_both, set_active_panel,
};

/// Upper bound on the byte length of any dialog text field (paths, names, octal
/// modes, search queries). Caps memory for pathological pasted input and keeps
/// the single-line input renderable; enforced per inserted char in
/// [`apply_dialog_text_edit`].
const MAX_DIALOG_INPUT_BYTES: usize = 4096;

pub(crate) fn parse_octal_mode(input: &str) -> Option<u32> {
    let mode = u32::from_str_radix(input.trim(), 8).ok()?;
    if mode <= 0o7777 { Some(mode) } else { None }
}

enum ValidationResult {
    Valid,
    EmptyInput,
    InvalidPath(String),
}

fn validate_non_empty(input: &str) -> ValidationResult {
    if input.trim().is_empty() {
        ValidationResult::EmptyInput
    } else {
        ValidationResult::Valid
    }
}

fn contains_parent_dir(input: &str) -> bool {
    std::path::Path::new(input)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn validate_path_name(input: &str) -> ValidationResult {
    match validate_non_empty(input) {
        ValidationResult::Valid => {}
        other => return other,
    }
    if input.contains('/') || (cfg!(windows) && input.contains('\\')) {
        return ValidationResult::InvalidPath(format!("Name contains path separator: {input}"));
    }
    if contains_parent_dir(input) {
        ValidationResult::InvalidPath(format!("'..' not allowed in '{input}'"))
    } else {
        ValidationResult::Valid
    }
}

fn reset_dialog_state(state: &mut AppState) {
    state.mode = AppMode::Normal;
    state.ui.pending_action = None;
    state.ui.pending_menu_command = None;
    state.ui.pending_hotlist_delete = None;
    state.ui.pending_archive_list = None;
    state.ui.pending_tree_build = None;
    state.ui.status_message = None;
    state.input.dialog_selection = 0;
    if let Some(panel) = state.ui.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

pub(crate) fn finish_confirmed_action(state: &mut AppState) {
    state.input.dialog_selection = 0;
    if state.ui.status_message.is_some()
        && !matches!(state.mode, AppMode::Dialog(DialogKind::Progress { .. }))
    {
        let msg = state.ui.status_message.take();
        dismiss_dialog(state);
        state.ui.status_message = msg;
        refresh_both(state);
    }
}

fn dispatch_with_overwrite_check(state: &mut AppState, running_job: &mut Option<RunningJob>) {
    if let Some(conflicting) = check_overwrite_conflict(state) {
        state.input.dialog_selection = 0;
        state.mode = AppMode::Dialog(DialogKind::OverwriteConfirm(Box::new(
            OverwriteConfirmDetails { conflicting },
        )));
        return;
    }
    start_confirmed_action(state, running_job);
    finish_confirmed_action(state);
}

pub(crate) fn dismiss_dialog(state: &mut AppState) {
    reset_dialog_state(state);
}

#[cfg(unix)]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(src_meta) = std::fs::symlink_metadata(src) else {
        return false;
    };
    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        return false;
    };
    src_meta.dev() == dest_meta.dev() && src_meta.ino() == dest_meta.ino()
}

#[cfg(not(unix))]
fn is_same_file(src: &std::path::Path, dest: &std::path::Path) -> bool {
    match (src.canonicalize(), dest.canonicalize()) {
        (Ok(s), Ok(d)) => s == d,
        _ => crate::fs::path::clean_path(src) == crate::fs::path::clean_path(dest),
    }
}

/// Reports which destination names already exist, so the caller can prompt
/// before overwriting.
///
/// This is a pre-flight check only: the actual copy/move runs asynchronously in
/// `job_runner`, so a file can appear (or vanish) in the window between this
/// `symlink_metadata` probe and the operation. The check therefore drives the
/// confirmation UX, not safety — atomicity is the operation layer's job:
/// non-overwrite copies open the target with `create_new`/`O_NOFOLLOW` and moves
/// call `ensure_destination_absent` immediately before `fs::rename`. A fully
/// race-free guarantee would need OS-specific atomics (e.g. `renameat2` with
/// `RENAME_NOREPLACE`), which the codebase's non-adversarial-filesystem contract
/// deliberately does not pursue.
pub(crate) fn check_overwrite_conflict(state: &AppState) -> Option<Vec<String>> {
    let action = state.ui.pending_action.as_ref()?;
    match action {
        PendingAction::Copy(t) | PendingAction::Move(t) => {
            if t.overwrite {
                return None;
            }
            let conflicting: Vec<String> = t
                .sources
                .iter()
                .filter_map(|s| {
                    let name = s.file_name()?;
                    let target = t.dest.join(name);
                    if is_same_file(s, &target) {
                        return None;
                    }
                    if std::fs::symlink_metadata(&target).is_ok() {
                        Some(name.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .collect();
            (!conflicting.is_empty()).then_some(conflicting)
        }
        PendingAction::CreateArchive {
            dest, overwrite, ..
        } => {
            if *overwrite {
                return None;
            }
            if std::fs::symlink_metadata(dest).is_ok() {
                let name = dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(vec![name])
            } else {
                None
            }
        }
        PendingAction::ExtractArchive {
            source,
            dest,
            overwrite,
        } => {
            if *overwrite {
                return None;
            }
            let entries = ops::archive::list::list_archive(source).ok()?;
            let mut seen = std::collections::HashSet::new();
            let conflicting: Vec<String> = entries
                .iter()
                .filter_map(|e| {
                    let top = e.name.split('/').next()?;
                    if top.is_empty() || top == ".." || !seen.insert(top.to_owned()) {
                        return None;
                    }
                    let target = dest.join(top);
                    if std::fs::symlink_metadata(&target).is_ok() {
                        Some(top.to_owned())
                    } else {
                        None
                    }
                })
                .collect();
            (!conflicting.is_empty()).then_some(conflicting)
        }
        PendingAction::Delete { .. } => None,
    }
}

/// Flip the two-button dialog selection (0 <-> 1). Shared by the confirmation
/// and archive dialogs so the Left/Right toggle stays identical in both.
fn toggle_dialog_selection(state: &mut AppState) {
    state.input.dialog_selection = if state.input.dialog_selection == 0 {
        1
    } else {
        0
    };
}

fn confirm_dialog_key(state: &mut AppState, key: KeyCode) -> Option<bool> {
    match key {
        KeyCode::Char('y' | 'Y') => Some(true),
        KeyCode::Char('n' | 'N') => Some(false),
        KeyCode::Enter => Some(state.input.dialog_selection == 0),
        KeyCode::Esc => {
            dismiss_dialog(state);
            None
        }
        KeyCode::Left | KeyCode::Right => {
            toggle_dialog_selection(state);
            None
        }
        _ => None,
    }
}

fn handle_confirm_dialog(state: &mut AppState, running_job: &mut Option<RunningJob>, key: KeyCode) {
    let Some(confirmed) = confirm_dialog_key(state, key) else {
        return;
    };

    // A pending hotlist deletion returns to the hotlist picker either way.
    if state.ui.pending_hotlist_delete.is_some() {
        super::pickers::resolve_hotlist_delete(state, confirmed);
        return;
    }

    if confirmed {
        if state.ui.pending_action.is_some() {
            dispatch_with_overwrite_check(state, running_job);
        } else if let Some(cmd) = state.ui.pending_menu_command.take() {
            state.mode = AppMode::Normal;
            shell::run_shell_command(state, &cmd, true, refresh_active);
        } else {
            dismiss_dialog(state);
            refresh_both(state);
        }
    } else {
        dismiss_dialog(state);
    }
}

fn handle_overwrite_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    match key {
        KeyCode::Esc => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Left => {
            state.input.dialog_selection = state.input.dialog_selection.saturating_sub(1);
            return;
        }
        KeyCode::Right => {
            state.input.dialog_selection = (state.input.dialog_selection + 1).min(1);
            return;
        }
        KeyCode::Char('o' | 'O') => {
            if let Some(a) = &mut state.ui.pending_action {
                a.set_overwrite();
            }
        }
        KeyCode::Char('c' | 'C') => {
            dismiss_dialog(state);
            return;
        }
        KeyCode::Enter => match state.input.dialog_selection {
            0 => {
                if let Some(a) = &mut state.ui.pending_action {
                    a.set_overwrite();
                }
            }
            1 => {
                dismiss_dialog(state);
                return;
            }
            _ => return,
        },
        _ => return,
    }
    start_confirmed_action(state, running_job);
    finish_confirmed_action(state);
}

fn handle_find_file(state: &mut AppState, running_job: &mut Option<RunningJob>, input: &str) {
    start_search_job(state, running_job, input);
}

fn handle_quick_cd(state: &mut AppState, input: &str) {
    let expanded = fs::path::resolve_user_path(state.active_panel().path(), input);

    if expanded.is_dir() {
        let panel = state.active_panel_mut();
        panel.push_history(panel.path().to_path_buf());
        panel.set_path(expanded.clone());
        panel.cursor = 0;
        panel.scroll_offset = 0;
        // The input epilogue (ResetWithRefresh) refreshes the active panel, so
        // no explicit refresh here — it would double the directory read.
        if !state.hotlist().iter().any(|p| p == &expanded) {
            state.hotlist_push(expanded);
        }
    } else if expanded.exists() {
        state.ui.status_message = Some(format!("Not a directory: {input}"));
    } else {
        state.ui.status_message = Some(format!("Directory not found: {input}"));
    }
}

/// What the common epilogue should do after a per-action handler runs.
enum InputOutcome {
    /// The handler already finalized the dialog; skip the common epilogue.
    Finalized,
    /// Reset to Normal mode and refresh the active panel.
    ResetWithRefresh,
    /// Reset to Normal mode without refreshing (Filter rebuilds in place).
    ResetNoRefresh,
}

fn input_action_viewer_search(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    input: &str,
    terminal_height: u16,
) -> InputOutcome {
    if let Some(vs) = viewer_state.as_mut() {
        vs.search(input, terminal_height.saturating_sub(3) as usize);
    }
    state.mode = AppMode::Viewing;
    state.input.dialog_input.clear();
    InputOutcome::Finalized
}

fn input_action_create_directory(state: &mut AppState, input: &str) -> InputOutcome {
    // Clear any stale message up front so a success leaves no leftover error from
    // a prior attempt; the validation/failure paths below set a fresh one.
    state.ui.status_message = None;
    if let Err(msg) = validate_create_or_rename(input, "Directory name") {
        state.ui.status_message = Some(msg);
        return InputOutcome::Finalized;
    }
    let target = fs::path::resolve_user_path(state.active_panel().path(), input);
    if let Err(err) = ops::create_directory(&target) {
        state.ui.status_message = Some(format!("Create directory failed: {err}"));
    }
    InputOutcome::ResetWithRefresh
}

fn input_action_rename(state: &mut AppState, input: &str) -> InputOutcome {
    state.ui.status_message = None;
    if let Err(msg) = validate_create_or_rename(input, "New name") {
        state.ui.status_message = Some(msg);
        return InputOutcome::Finalized;
    }
    if let Some(entry) = state.active_panel().current_entry()
        && input != entry.name
        && let Err(err) = ops::rename_entry(&entry.path, input)
    {
        state.ui.status_message = Some(format!("Rename failed: {err}"));
    }
    InputOutcome::ResetWithRefresh
}

fn input_action_chmod(state: &mut AppState, input: &str) -> InputOutcome {
    state.ui.status_message = None;
    let Some(mode) = parse_octal_mode(input) else {
        if input.trim().is_empty() {
            state.ui.status_message = Some("Octal mode cannot be empty".to_string());
        } else {
            state.ui.status_message = Some(format!("Invalid octal mode '{input}'"));
        }
        return InputOutcome::Finalized;
    };
    #[cfg(unix)]
    if let Some(entry) = state.active_panel().current_entry()
        && let Err(err) = ops::chmod(&entry.path, mode)
    {
        state.ui.status_message = Some(format!("Chmod failed: {err}"));
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        state.ui.status_message = Some("Chmod is not supported on this platform".to_string());
    }
    InputOutcome::ResetWithRefresh
}

fn input_action_filter(state: &mut AppState, input: String, terminal_height: u16) -> InputOutcome {
    let panel = state.active_panel_mut();
    panel.set_filter((!input.trim().is_empty()).then_some(input));
    if panel.listing.needs_full_read() || panel.listing.unfiltered().is_empty() {
        refresh_active(state);
    } else {
        rebuild_visible_entries(panel, panel_visible_height(terminal_height));
    }
    InputOutcome::ResetNoRefresh
}

fn handle_input_action(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    action: &InputAction,
    terminal_height: u16,
) {
    // Raw input is preserved verbatim for the search actions (ViewerSearch,
    // FindFile) where leading/trailing whitespace is meaningful (e.g. searching
    // indented code like `    fn`). All filesystem-mutating / navigation actions
    // trim it so pasted whitespace cannot create entries named `" foo "` and so
    // whitespace-only input fails the non-empty validation as empty.
    let input = state.input.dialog_input.text().to_owned();
    let trimmed = input.trim();
    let outcome = match action {
        InputAction::ViewerSearch => {
            input_action_viewer_search(state, viewer_state, &input, terminal_height)
        }
        InputAction::CreateDirectory => input_action_create_directory(state, trimmed),
        InputAction::Rename => input_action_rename(state, trimmed),
        InputAction::Chmod => input_action_chmod(state, trimmed),
        InputAction::Filter => input_action_filter(state, trimmed.to_owned(), terminal_height),
        InputAction::QuickCd => {
            handle_quick_cd(state, trimmed);
            InputOutcome::ResetWithRefresh
        }
        InputAction::FindFile => {
            handle_find_file(state, running_job, &input);
            InputOutcome::Finalized
        }
    };

    let refresh = match outcome {
        InputOutcome::Finalized => return,
        InputOutcome::ResetWithRefresh => true,
        InputOutcome::ResetNoRefresh => false,
    };
    state.mode = AppMode::Normal;
    state.input.dialog_input.clear();
    if refresh {
        refresh_active(state);
    }
    if let Some(panel) = state.ui.menu_restore_panel.take() {
        set_active_panel(state, panel);
    }
}

fn validate_create_or_rename(input: &str, label: &str) -> Result<(), String> {
    match validate_path_name(input) {
        ValidationResult::Valid => Ok(()),
        ValidationResult::EmptyInput => Err(format!("{label} cannot be empty")),
        ValidationResult::InvalidPath(p) => Err(p),
    }
}

fn apply_text_edit(state: &mut AppState, key: KeyCode) {
    apply_dialog_text_edit(&mut state.input.dialog_input, key);
}

fn handle_input_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    running_job: &mut Option<RunningJob>,
    action: &InputAction,
    key: KeyCode,
    terminal_height: u16,
) {
    match key {
        KeyCode::Enter => {
            handle_input_action(state, viewer_state, running_job, action, terminal_height);
        }
        KeyCode::Esc => {
            if *action == InputAction::ViewerSearch {
                state.mode = AppMode::Viewing;
            } else {
                state.mode = AppMode::Normal;
            }
            state.input.dialog_input.clear();
            if let Some(panel) = state.ui.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
        }
        _ => {
            apply_text_edit(state, key);
        }
    }
}

fn handle_error_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        reset_dialog_state(state);
    }
}

fn handle_progress_dialog(state: &mut AppState, running_job: &Option<RunningJob>, key: KeyCode) {
    // Only a cancellable progress dialog reacts to Esc; a non-cancellable one
    // (the renderer hides the "Cancel" hint for it) must ignore the request.
    let cancellable = matches!(
        &state.mode,
        AppMode::Dialog(DialogKind::Progress {
            cancellable: true,
            ..
        })
    );
    if !cancellable || key != KeyCode::Esc {
        return;
    }
    if let Some(job) = running_job.as_ref() {
        // A batch/search job: request cooperative cancellation.
        job.cancel.store(true, Ordering::Relaxed);
        state.ui.status_message = Some("Cancel requested".to_string());
    } else {
        // A background loading dialog (archive listing / tree build) has no job
        // here: dismiss it. The main loop then drops the in-flight loader,
        // which cancels and detaches it.
        dismiss_dialog(state);
    }
}

fn handle_properties_dialog(state: &mut AppState, key: KeyCode) {
    if matches!(key, KeyCode::Enter | KeyCode::Esc) {
        reset_dialog_state(state);
    }
}

fn archive_format_from_path(path: &std::path::Path) -> Option<ArchiveFormat> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("zip") => return Some(ArchiveFormat::Zip),
        Some("tar") => return Some(ArchiveFormat::Tar),
        Some("7z") => return Some(ArchiveFormat::SevenZ),
        Some("tgz") => return Some(ArchiveFormat::TarGz),
        Some("tbz2") => return Some(ArchiveFormat::TarBz2),
        Some("txz") => return Some(ArchiveFormat::TarXz),
        Some("tzst") => return Some(ArchiveFormat::TarZst),
        _ => {}
    }
    let name = path.file_name().and_then(|n| n.to_str())?;
    let lower = name.to_ascii_lowercase();
    [
        (".tar.gz", ArchiveFormat::TarGz),
        (".tar.bz2", ArchiveFormat::TarBz2),
        (".tar.xz", ArchiveFormat::TarXz),
        (".tar.zst", ArchiveFormat::TarZst),
    ]
    .iter()
    .find(|(suffix, _)| lower.ends_with(suffix))
    .map(|(_, fmt)| *fmt)
}

fn apply_dialog_text_edit(dest_input: &mut TextInput, key: KeyCode) {
    match key {
        KeyCode::Backspace => {
            dest_input.backspace();
        }
        KeyCode::Delete => {
            dest_input.delete_forward();
        }
        KeyCode::Char(c) => {
            if dest_input.text().len() + c.len_utf8() > MAX_DIALOG_INPUT_BYTES {
                return;
            }
            dest_input.insert_char(c);
        }
        KeyCode::Left => dest_input.cursor_left(),
        KeyCode::Right => dest_input.cursor_right(),
        KeyCode::Home => dest_input.cursor_start(),
        KeyCode::End => dest_input.cursor_end(),
        _ => {}
    }
}

/// Navigation outcome shared by the archive extract/create dialogs, which use an
/// identical OK/Cancel button layout and text-input field.
enum ArchiveNav {
    /// User confirmed (OK): build and dispatch the pending action.
    Commit,
    /// User cancelled (Esc, Cancel button) — dialog already dismissed.
    Dismissed,
    /// Key fully handled (e.g. OK/Cancel toggle) — do NOT fall through to text
    /// edit. Prevents Left/Right from both toggling the button selection and
    /// moving the destination text cursor (double action).
    Handled,
    /// A non-committing key (no-op for nav) was not consumed; fall through to
    /// text edit (Char/Backspace/Delete/Home/End).
    Continue,
}

/// Handle the OK/Cancel button navigation common to both archive dialogs.
fn archive_dialog_nav(state: &mut AppState, key: KeyCode) -> ArchiveNav {
    match key {
        KeyCode::Esc => {
            dismiss_dialog(state);
            ArchiveNav::Dismissed
        }
        KeyCode::Left | KeyCode::Right => {
            toggle_dialog_selection(state);
            ArchiveNav::Handled
        }
        KeyCode::Enter if state.input.dialog_selection == 1 => {
            dismiss_dialog(state);
            ArchiveNav::Dismissed
        }
        KeyCode::Enter => ArchiveNav::Commit,
        _ => ArchiveNav::Continue,
    }
}

/// Build the `ExtractArchive` pending action from the active extract dialog and
/// dispatch it. Returns early (without dispatch) on empty/invalid input.
fn commit_archive_extract(state: &mut AppState, running_job: &mut Option<RunningJob>) {
    let AppMode::Dialog(DialogKind::ArchiveExtract(details)) = &state.mode else {
        return;
    };
    let source = details.source.clone();
    let dest_text = details.dest_input.text().trim().to_owned();
    if dest_text.is_empty() {
        state.ui.status_message = Some("Destination path cannot be empty".to_string());
        return;
    }
    let dest = fs::path::resolve_user_path(state.active_panel().path(), &dest_text);
    state.ui.pending_action = Some(PendingAction::ExtractArchive {
        source,
        dest,
        overwrite: false,
    });
    dispatch_with_overwrite_check(state, running_job);
}

/// Build the `CreateArchive` pending action from the active create dialog and
/// dispatch it. Returns early (without dispatch) on empty input or unknown format.
fn commit_archive_create(state: &mut AppState, running_job: &mut Option<RunningJob>) {
    let AppMode::Dialog(DialogKind::ArchiveCreate(details)) = &state.mode else {
        return;
    };
    let sources = details.sources.clone();
    let dest_text = details.dest_input.text().trim().to_owned();
    if dest_text.is_empty() {
        state.ui.status_message = Some("Archive path cannot be empty".to_string());
        return;
    }
    let dest = fs::path::resolve_user_path(state.active_panel().path(), &dest_text);
    let Some(format) = archive_format_from_path(&dest) else {
        state.ui.status_message = Some(
            "Unsupported archive format. Use: zip, tar, tar.gz, tar.bz2, tar.xz, tar.zst, 7z"
                .to_string(),
        );
        return;
    };
    state.ui.pending_action = Some(PendingAction::CreateArchive {
        sources,
        dest,
        format,
        overwrite: false,
    });
    dispatch_with_overwrite_check(state, running_job);
}

/// Mouse entry point: activate the OK button of whichever archive dialog is
/// active, committing the extract/create action. Mirrors the keyboard
/// `ArchiveNav::Commit` path.
pub(crate) fn commit_archive_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    is_extract: bool,
) {
    if is_extract {
        commit_archive_extract(state, running_job);
    } else {
        commit_archive_create(state, running_job);
    }
}

/// Borrow the destination text input of whichever archive dialog is active.
fn active_archive_dest_input(state: &mut AppState) -> Option<&mut TextInput> {
    match &mut state.mode {
        AppMode::Dialog(DialogKind::ArchiveExtract(details)) => Some(&mut details.dest_input),
        AppMode::Dialog(DialogKind::ArchiveCreate(details)) => Some(&mut details.dest_input),
        _ => None,
    }
}

/// Shared handler for the archive extract/create dialogs. They share identical
/// OK/Cancel navigation and a single text field; only the committed pending
/// action differs (selected by `is_extract`).
fn handle_archive_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
    is_extract: bool,
) {
    match archive_dialog_nav(state, key) {
        ArchiveNav::Dismissed => return,
        ArchiveNav::Commit => {
            if is_extract {
                commit_archive_extract(state, running_job);
            } else {
                commit_archive_create(state, running_job);
            }
            return;
        }
        // Key already consumed by nav (OK/Cancel toggle); must NOT also run text
        // edit, otherwise Left/Right would move the dest-path cursor too.
        ArchiveNav::Handled => return,
        ArchiveNav::Continue => {}
    }
    if let Some(dest_input) = active_archive_dest_input(state) {
        apply_dialog_text_edit(dest_input, key);
    }
}

fn handle_copymove_dialog(
    state: &mut AppState,
    running_job: &mut Option<RunningJob>,
    key: KeyCode,
) {
    let Some(confirmed) = confirm_dialog_key(state, key) else {
        return;
    };

    if confirmed {
        let action = if let AppMode::Dialog(DialogKind::CopyMove(details)) = &state.mode {
            let transfer = TransferAction {
                sources: details.source.clone(),
                dest: details.dest.clone(),
                overwrite: false,
            };
            if details.kind.is_move() {
                PendingAction::Move(transfer)
            } else {
                PendingAction::Copy(transfer)
            }
        } else {
            return;
        };
        state.ui.pending_action = Some(action);
        dispatch_with_overwrite_check(state, running_job);
    } else {
        dismiss_dialog(state);
    }
}

pub(crate) fn handle_dialog(ctx: &mut EventContext, key: KeyCode, modifiers: KeyModifiers) {
    // A Char with Ctrl/Alt/Super held is not literal text for any dialog;
    // swallow it so it never lands in a text field as a stray letter. Bare or
    // Shift-ed chars, and all non-Char keys, fall through to the handlers.
    if matches!(key, KeyCode::Char(_)) && !modifiers.difference(KeyModifiers::SHIFT).is_empty() {
        return;
    }
    let terminal_size = ctx.term_size;
    let state = &mut *ctx.state;
    let viewer_state = &mut *ctx.viewer_state;
    let running_job = &mut *ctx.running_job;
    if let AppMode::Dialog(DialogKind::Help {
        message,
        scroll_offset,
    }) = &mut state.mode
    {
        let term_rect = Rect::new(0, 0, terminal_size.width, terminal_size.height);
        let geo = dialogs::help_dialog_geometry(term_rect);
        let max_lines = geo.height;
        let msg_width = geo.width;
        let total_lines = dialogs::wrapped_line_count(message, msg_width);
        let should_exit = match key {
            KeyCode::Up | KeyCode::Char('k') => {
                *scroll_offset = scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + 1).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::PageUp => {
                *scroll_offset = scroll_offset.saturating_sub(max_lines);
                false
            }
            KeyCode::PageDown => {
                if total_lines > max_lines {
                    *scroll_offset = (*scroll_offset + max_lines).min(total_lines - max_lines);
                }
                false
            }
            KeyCode::Home => {
                *scroll_offset = 0;
                false
            }
            KeyCode::End => {
                if total_lines > max_lines {
                    *scroll_offset = total_lines - max_lines;
                }
                false
            }
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => true,
            _ => false,
        };
        if should_exit {
            reset_dialog_state(state);
        }
        return;
    }

    // Match on a reference and dispatch by variant. Each handler re-borrows
    // `state.mode` for the data it needs, so cloning the whole `DialogKind`
    // (`Box`/`Vec<PathBuf>`/`TextInput`) on every key press is avoided. Only the
    // `Copy` `InputAction` is lifted out here before the borrow is released.
    let AppMode::Dialog(dk) = &state.mode else {
        return;
    };

    match dk {
        DialogKind::Confirm(_) => {
            handle_confirm_dialog(state, running_job, key);
        }
        DialogKind::Input { action, .. } => {
            let action = *action;
            handle_input_dialog(
                state,
                viewer_state,
                running_job,
                &action,
                key,
                terminal_size.height,
            );
        }
        DialogKind::Error(_) => {
            handle_error_dialog(state, key);
        }
        DialogKind::Progress { .. } => {
            handle_progress_dialog(state, running_job, key);
        }
        DialogKind::Properties(..) => {
            handle_properties_dialog(state, key);
        }
        DialogKind::CopyMove(..) => {
            handle_copymove_dialog(state, running_job, key);
        }
        DialogKind::OverwriteConfirm(..) => {
            handle_overwrite_dialog(state, running_job, key);
        }
        DialogKind::ArchiveExtract(..) => {
            handle_archive_dialog(state, running_job, key, true);
        }
        DialogKind::ArchiveCreate(..) => {
            handle_archive_dialog(state, running_job, key, false);
        }
        // unreachable: Help handled above; arm kept for match exhaustiveness
        DialogKind::Help { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input_state(text: &str, cursor: usize) -> AppState {
        let mut dialog_input = TextInput::new();
        dialog_input.set_text(text.to_string());
        dialog_input.set_cursor(cursor);
        AppState {
            input: InputState {
                dialog_input,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn text_edit_insert_char() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Char('!'));
        assert_eq!(state.input.dialog_input.text(), "hello!");
        assert_eq!(state.input.dialog_input.cursor(), 6);
    }

    #[test]
    fn text_edit_insert_middle() {
        let mut state = make_input_state("helo", 2);
        apply_text_edit(&mut state, KeyCode::Char('l'));
        assert_eq!(state.input.dialog_input.text(), "hello");
        assert_eq!(state.input.dialog_input.cursor(), 3);
    }

    #[test]
    fn text_edit_backspace() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.input.dialog_input.text(), "hell");
        assert_eq!(state.input.dialog_input.cursor(), 4);
    }

    #[test]
    fn text_edit_backspace_at_start() {
        let mut state = make_input_state("hello", 0);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.input.dialog_input.text(), "hello");
        assert_eq!(state.input.dialog_input.cursor(), 0);
    }

    #[test]
    fn text_edit_delete() {
        let mut state = make_input_state("hello", 0);
        apply_text_edit(&mut state, KeyCode::Delete);
        assert_eq!(state.input.dialog_input.text(), "ello");
        assert_eq!(state.input.dialog_input.cursor(), 0);
    }

    #[test]
    fn text_edit_delete_at_end() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Delete);
        assert_eq!(state.input.dialog_input.text(), "hello");
        assert_eq!(state.input.dialog_input.cursor(), 5);
    }

    #[test]
    fn text_edit_left_right() {
        let mut state = make_input_state("hello", 3);
        apply_text_edit(&mut state, KeyCode::Left);
        assert_eq!(state.input.dialog_input.cursor(), 2);
        apply_text_edit(&mut state, KeyCode::Right);
        assert_eq!(state.input.dialog_input.cursor(), 3);
    }

    #[test]
    fn text_edit_home_end() {
        let mut state = make_input_state("hello", 3);
        apply_text_edit(&mut state, KeyCode::Home);
        assert_eq!(state.input.dialog_input.cursor(), 0);
        apply_text_edit(&mut state, KeyCode::End);
        assert_eq!(state.input.dialog_input.cursor(), 5);
    }

    #[test]
    fn text_edit_multibyte_insert() {
        let mut state = make_input_state("hello", 5);
        apply_text_edit(&mut state, KeyCode::Char('ą'));
        assert_eq!(state.input.dialog_input.text(), "helloą");
        assert_eq!(state.input.dialog_input.cursor(), 6);
    }

    #[test]
    fn text_edit_multibyte_backspace() {
        let mut state = make_input_state("helloą", 6);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.input.dialog_input.text(), "hello");
        assert_eq!(state.input.dialog_input.cursor(), 5);
    }

    #[test]
    fn text_edit_emoji_insert() {
        let mut state = make_input_state("test", 4);
        apply_text_edit(&mut state, KeyCode::Char('🎉'));
        assert_eq!(state.input.dialog_input.text(), "test🎉");
        assert_eq!(state.input.dialog_input.cursor(), 5);
    }

    #[test]
    fn text_edit_rejects_multibyte_char_past_byte_limit() {
        let mut state = make_input_state(&"a".repeat(MAX_DIALOG_INPUT_BYTES - 1), 4095);
        apply_text_edit(&mut state, KeyCode::Char('ą'));
        assert_eq!(
            state.input.dialog_input.text().len(),
            MAX_DIALOG_INPUT_BYTES - 1
        );
        assert_eq!(state.input.dialog_input.cursor(), 4095);
    }

    #[test]
    fn text_edit_allows_char_at_exact_byte_limit() {
        let mut state = make_input_state(&"a".repeat(MAX_DIALOG_INPUT_BYTES - 1), 4095);
        apply_text_edit(&mut state, KeyCode::Char('!'));
        assert_eq!(
            state.input.dialog_input.text().len(),
            MAX_DIALOG_INPUT_BYTES
        );
        assert_eq!(state.input.dialog_input.cursor(), 4096);
    }

    #[test]
    fn text_edit_emoji_backspace() {
        let mut state = make_input_state("test🎉", 5);
        apply_text_edit(&mut state, KeyCode::Backspace);
        assert_eq!(state.input.dialog_input.text(), "test");
        assert_eq!(state.input.dialog_input.cursor(), 4);
    }
}

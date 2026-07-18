use std::io;
use std::process::{Command, Stdio};
use std::time::Duration;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::app::types::AppState;
use crate::debug_log;

const EVENT_POLL_TIMEOUT_MS: u64 = 100;
pub const MAX_HISTORY: usize = 100;

/// User-facing prompts shown while the TUI is suspended for an external command.
const MSG_COMMAND_SUCCEEDED: &str = "\n[Command succeeded. Press Enter to return]";
const PRESS_ENTER_TO_RETURN: &str = "Press Enter to return]";
const MSG_EXTERNAL_VIEW_ACTIVE: &str =
    "External view active. Press Enter/Esc/Ctrl+O/Ctrl+C to return to Libre Commander.";

/// Reads a shell path from environment variable `var`, falling back to
/// `default` when the variable is unset or empty.
fn get_shell_from_env(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn enter_tui_stdout() -> io::Result<()> {
    enable_raw_mode()?;
    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide) {
        let _ = disable_raw_mode();
        return Err(err);
    }
    Ok(())
}

fn leave_tui_stdout() -> io::Result<()> {
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
    match (raw_result, screen_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(raw_err), Err(screen_err)) => {
            debug_log!("disable_raw_mode error suppressed: {raw_err}");
            Err(screen_err)
        }
    }
}

struct TerminalRestoreGuard {
    already_restored: bool,
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if !self.already_restored
            && let Err(err) = enter_tui_stdout()
        {
            debug_log!("Terminal restore failed: {err}");
        }
    }
}

/// Returns `(shell_path, flag)` for spawning a shell command.
///
/// On Windows the `for_menu` parameter is ignored — `COMSPEC` (falling back
/// to `cmd.exe`) is always used with `/C`, because menu commands are also
/// executed through `cmd`. Shell spawns are rare, so env is read each time.
#[cfg(windows)]
fn get_shell(_for_menu: bool) -> (String, &'static str) {
    (get_shell_from_env("COMSPEC", "cmd.exe"), "/C")
}

/// Returns `(shell_path, flag)` for spawning a shell command.
///
/// `for_menu` selects between `sh -c` (user menu) and `$SHELL -c`
/// (interactive commands). Shell spawns are rare, so env is read each time.
#[cfg(not(windows))]
fn get_shell(for_menu: bool) -> (String, &'static str) {
    if for_menu {
        ("sh".to_string(), "-c")
    } else {
        (get_shell_from_env("SHELL", "sh"), "-c")
    }
}

pub fn push_history(state: &mut AppState, cmd: &str) {
    if cmd.trim().is_empty() {
        return;
    }
    // O(n) dedup scan; acceptable because MAX_HISTORY == 100
    state.input.command_history.retain(|entry| entry != cmd);
    state.input.command_history.push_back(cmd.to_string());
    if state.input.command_history.len() > MAX_HISTORY {
        state.input.command_history.pop_front();
    }
    state.rebuild_history_cache();
}

/// Runs `cmd` through a shell (`$SHELL -c` for interactive commands, `sh -c`
/// for menu commands) with the active panel directory as cwd.
///
/// # Threat model (shell injection is by design)
///
/// `cmd` is handed verbatim to `sh -c` / `$SHELL -c`, so the shell performs
/// full word-splitting, globbing and command substitution. This is intentional:
/// this is a shell-command runner, exactly like the command line and the user
/// menu in `mc`.
///
/// Sources of `cmd`:
/// * Interactive command line (`for_menu == false`): typed by the user — no
///   trust boundary, the user is executing their own commands.
/// * Global user menu (`for_menu == true`, `MenuSource::Global`): read from the
///   user's own config — trusted to the same degree as their dotfiles.
/// * Local directory menu (`for_menu == true`, `MenuSource::Local`):
///   ATTACKER-CONTROLLED. The menu file ships inside the browsed directory, so a
///   hostile archive/repo can plant arbitrary commands. The only defense is the
///   "Trust Local Menu?" confirm dialog raised in `input/pickers.rs` before this
///   function is ever reached. There is no sandboxing beyond that prompt; if the
///   user confirms, the command runs with their full privileges. This matches
///   the accepted threat model for a file manager, but the confirm gate MUST
///   remain the sole entry point for local-menu execution.
pub fn run_shell_command(
    state: &mut AppState,
    cmd: &str,
    for_menu: bool,
    mut refresh_active: impl FnMut(&mut AppState),
) {
    if cmd.trim().is_empty() {
        return;
    }

    if leave_tui_stdout().is_err() {
        state.ui.status_message = Some("Terminal suspend failed".into());
        return;
    }

    push_history(state, cmd);
    let mut restore_guard = TerminalRestoreGuard {
        already_restored: false,
    };
    let (shell, flag) = get_shell(for_menu);
    let status = Command::new(&shell)
        .arg(flag)
        .arg(cmd)
        .current_dir(state.active_panel().path())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    // Intentional stdout: TUI is suspended, user must see the prompt.
    #[allow(clippy::print_stdout)]
    match status {
        Ok(s) if s.success() => println!("{MSG_COMMAND_SUCCEEDED}"),
        Ok(s) => println!("\n[Command exited with status: {s}. {PRESS_ENTER_TO_RETURN}"),
        Err(e) => println!("\n[Command failed: {e}. {PRESS_ENTER_TO_RETURN}"),
    }
    let mut buf = String::new();
    // Intentionally ignoring read_line error: if stdin is unavailable there's nothing to wait for.
    let _ = io::stdin().read_line(&mut buf);
    match enter_tui_stdout() {
        Ok(()) => restore_guard.already_restored = true,
        Err(e) => {
            state.ui.status_message = Some(format!("Terminal restore failed: {e}"));
        }
    }
    refresh_active(state);
}

/// Blocks until the user leaves the external view by pressing Enter, Esc, or
/// Ctrl+O / Ctrl+C. Assumes raw mode is already enabled by the caller.
fn wait_for_external_view_exit() -> io::Result<()> {
    loop {
        if event::poll(Duration::from_millis(EVENT_POLL_TIMEOUT_MS))?
            && let Event::Key(key) = event::read()?
        {
            match (key.code, key.modifiers) {
                (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => break,
                (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => break,
                (KeyCode::Enter, _) => break,
                (KeyCode::Esc, _) => break,
                _ => {}
            }
        }
    }
    Ok(())
}

/// Toggle external panel view (Ctrl+O / Ctrl+C) - hide panels to see terminal output.
#[allow(clippy::print_stdout)]
pub fn toggle_external_view(
    state: &mut AppState,
    mut refresh_both: impl FnMut(&mut AppState),
) -> io::Result<()> {
    leave_tui_stdout()?;

    let mut restore_guard = TerminalRestoreGuard {
        already_restored: false,
    };

    // Show message to user.
    println!("{MSG_EXTERNAL_VIEW_ACTIVE}");

    // Wait for Ctrl+O or any key.
    enable_raw_mode()?;
    let wait_result = wait_for_external_view_exit();
    let raw_result = disable_raw_mode();

    let resume_result = enter_tui_stdout();
    if resume_result.is_ok() {
        restore_guard.already_restored = true;
    }

    let err = resume_result
        .err()
        .or(raw_result.err())
        .or(wait_result.err());
    if let Some(e) = err {
        return Err(e);
    }

    refresh_both(state);

    Ok(())
}

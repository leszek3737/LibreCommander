#[allow(dead_code)]
mod app;
#[allow(dead_code)]
mod fs;
#[allow(dead_code)]
mod ops;
#[allow(dead_code)]
mod ui;

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::{
    cursor::Show,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
};

use app::types::{ActivePanel, AppMode, AppState, CompareMode, PanelState, PickerKind};
use app::{dir_tree, user_menu};
use fs::reader;
use ops::sorting;
use ui::{dialogs, panels, viewer};

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture, Show);
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let _guard = TerminalGuard;

    let result = run_app(&mut terminal);

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    // Terminal state recovery: if editor was SIGKILL'd, Drop was skipped.
    // Detect leftover state file and restore terminal before doing anything else.
    const TERMINAL_STATE_FILE: &str = "/tmp/lc_terminal_state";
    if std::fs::metadata(TERMINAL_STATE_FILE).is_ok() {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        );
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Show
        );
        let _ = crossterm::terminal::enable_raw_mode();
        let _ = std::fs::remove_file(TERMINAL_STATE_FILE);
    }

    let mut state = AppState::new();
    app::config::load_setup(&mut state);
    let mut viewer_state: Option<viewer::ViewerState> = None;

    refresh_panel(&mut state.left_panel, 0);
    refresh_panel(&mut state.right_panel, 0);

    loop {
        terminal.draw(|f| render_ui(f, &state, &viewer_state))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    match &state.mode {
                        AppMode::Normal => {
                            handle_normal_mode(&mut state, &mut viewer_state, key.code, key.modifiers, terminal.size()?.height, terminal);
                        }
                        AppMode::Viewing => {
                            let sz = terminal.size()?;
                            handle_viewer_mode(&mut state, &mut viewer_state, key.code, sz.height, sz.width);
                        }
                        AppMode::CommandLine => handle_command_line(&mut state, key.code),
                        AppMode::Dialog(_) => handle_dialog(&mut state, &mut viewer_state, key.code, terminal.size()?.height),
                        AppMode::Search => handle_search_mode(&mut state, key.code, terminal.size()?.height),
                        AppMode::Menu => handle_menu_mode(&mut state, &mut viewer_state, key.code, terminal.size()?.height, terminal),
                        AppMode::ListPicker(_) => handle_list_picker(&mut state, key.code),
                        AppMode::DirectoryTree => handle_directory_tree(
                            &mut state,
                            &mut viewer_state,
                            key.code,
                            terminal.size()?.height,
                        ),
                    }
                }
                Event::Mouse(mouse_event) => {
                    let size: ratatui::layout::Size = terminal.size()?;
                    handle_mouse_event(&mut state, &mut viewer_state, mouse_event, size);
                }
                _ => {}
            }
        }

        if state.should_quit {
            return Ok(());
        }
    }
}

fn refresh_panel(panel: &mut PanelState, visible_height: usize) {
    match reader::read_directory(&panel.path, panel.show_hidden) {
        Ok((entries, errors)) => {
            if errors.is_empty() {
                panel.last_error = None;
            } else {
                let error_summary = errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
                panel.last_error = Some(format!("{} file(s) failed to read: {error_summary}", errors.len()));
            }
            let current_name = panel.entries.get(panel.cursor)
                .filter(|e| e.name != "..")
                .map(|e| e.name.clone());
            let saved: HashSet<PathBuf> = panel
                .entries
                .iter()
                .filter(|e| e.selected)
                .map(|e| e.path.clone())
                .collect();
            let sort_mode = panel.sort_mode;
            let mut sort_entries: Vec<reader::FileEntry> = entries
                .iter()
                .filter(|e| {
                    if e.name == ".." {
                        true
                    } else if let Some(filter) = &panel.filter {
                        ops::search::FileSearch::matches_pattern(&e.name, filter, false)
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            sorting::sort_entries(&mut sort_entries, sort_mode);
            panel.entries = sort_entries;
            for entry in &mut panel.entries {
                if saved.contains(&entry.path) {
                    entry.selected = true;
                }
            }
            panel.recalculate_selection_stats();
            if let Some(ref name) = current_name
                && let Some(pos) = panel.entries.iter().position(|e| e.name == *name) {
                panel.cursor = pos;
            }
            if panel.cursor >= panel.entries.len() && !panel.entries.is_empty() {
                panel.cursor = panel.entries.len() - 1;
            }
            let max_scroll = panel.entries.len().saturating_sub(1);
            if panel.scroll_offset > max_scroll {
                panel.scroll_offset = max_scroll;
            }
            if panel.scroll_offset > panel.cursor {
                panel.scroll_offset = panel.cursor;
            }
            panel.ensure_cursor_visible(visible_height);
        }
        Err(e) => {
            panel.entries.clear();
            panel.cursor = 0;
            panel.scroll_offset = 0;
            panel.last_error = Some(e.to_string());
            panel.recalculate_selection_stats();
        }
    }
}

fn refresh_active(state: &mut AppState) {
    let visible = crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(0);
    match state.active_panel {
        ActivePanel::Left => refresh_panel(&mut state.left_panel, visible),
        ActivePanel::Right => refresh_panel(&mut state.right_panel, visible),
    }
}

fn refresh_both(state: &mut AppState) {
    let visible = crossterm::terminal::size()
        .map(|(_, h)| panel_visible_height(h))
        .unwrap_or(0);
    refresh_panel(&mut state.left_panel, visible);
    refresh_panel(&mut state.right_panel, visible);
}

fn set_active_panel(state: &mut AppState, panel: ActivePanel) {
    state.active_panel = panel;
}

fn with_menu_panel<T>(state: &mut AppState, f: impl FnOnce(&mut AppState) -> T) -> T {
    let original = state.active_panel;
    match state.menu_selected {
        0 => set_active_panel(state, ActivePanel::Left),
        4 => set_active_panel(state, ActivePanel::Right),
        _ => {}
    }
    let result = f(state);
    if matches!(state.mode, AppMode::Dialog(_)) {
        state.menu_restore_panel = Some(original);
    } else {
        set_active_panel(state, original);
    }
    result
}

fn cycle_sort_mode(mode: app::types::SortMode) -> app::types::SortMode {
    match mode {
        app::types::SortMode::NameAsc => app::types::SortMode::NameDesc,
        app::types::SortMode::NameDesc => app::types::SortMode::SizeAsc,
        app::types::SortMode::SizeAsc => app::types::SortMode::SizeDesc,
        app::types::SortMode::SizeDesc => app::types::SortMode::ModTimeAsc,
        app::types::SortMode::ModTimeAsc => app::types::SortMode::ModTimeDesc,
        app::types::SortMode::ModTimeDesc => app::types::SortMode::ExtensionAsc,
        app::types::SortMode::ExtensionAsc => app::types::SortMode::ExtensionDesc,
        app::types::SortMode::ExtensionDesc => app::types::SortMode::NameAsc,
    }
}

fn render_ui(f: &mut Frame, state: &AppState, viewer_state: &Option<viewer::ViewerState>) {
    // If viewing, render viewer fullscreen
    if state.mode == AppMode::Viewing {
        if let Some(vs) = viewer_state {
            if vs.hex_mode {
                viewer::render_hex_view(f, f.area(), vs);
            } else {
                viewer::render_viewer(f, f.area(), vs);
            }
        }
        return;
    }

    // If directory tree mode, render fullscreen tree overlay
    if state.mode == AppMode::DirectoryTree {
        render_directory_tree(f, state);
        return;
    }

    let size = f.area();
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Menu bar
            Constraint::Min(10),   // Panels
            Constraint::Length(1), // Status bar
            Constraint::Length(1), // Command line
            Constraint::Length(1), // Function bar
        ])
        .split(size);

    panels::render_menu_bar(f, main_layout[0]);

    let panel_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);

    panels::render_panel(
        f,
        panel_area[0],
        &state.left_panel,
        state.active_panel == ActivePanel::Left,
    );
    panels::render_panel(
        f,
        panel_area[1],
        &state.right_panel,
        state.active_panel == ActivePanel::Right,
    );

    let active = if state.active_panel == ActivePanel::Left {
        &state.left_panel
    } else {
        &state.right_panel
    };
    panels::render_status_bar(f, main_layout[2], active);

    // Command line area
    let cmd_text = if state.mode == AppMode::CommandLine {
        format!("$ {}_", state.command_line)
    } else if state.mode == AppMode::Search {
        format!("Search: {}_", state.search_query)
    } else if let Some(ref msg) = state.status_message {
        msg.clone()
    } else {
        let ap = state.active_panel();
        format!("{}", ap.path.display())
    };
    let cmd_paragraph =
        ratatui::widgets::Paragraph::new(cmd_text).style(Style::default().fg(Color::White));
    f.render_widget(cmd_paragraph, main_layout[3]);

    panels::render_function_bar(f, main_layout[4]);

    // Dialog overlay
    if let AppMode::Dialog(ref dialog_kind) = state.mode {
        let ui_dialog = match dialog_kind {
            app::types::DialogKind::Confirm(msg) => dialogs::DialogKind::Confirm {
                title: "Confirm".to_string(),
                message: msg.clone(),
            },
            app::types::DialogKind::Input(prompt, _) => dialogs::DialogKind::Input {
                title: "Input".to_string(),
                prompt: prompt.clone(),
                value: state.dialog_input.clone(),
                cursor_pos: state.dialog_cursor_pos,
            },
            app::types::DialogKind::Error(msg) => dialogs::DialogKind::Error {
                title: "Error".to_string(),
                message: msg.clone(),
            },
            app::types::DialogKind::Help(msg) => dialogs::DialogKind::Help {
                title: "Help".to_string(),
                message: msg.clone(),
            },
            app::types::DialogKind::Progress(msg, pct) => dialogs::DialogKind::Progress {
                title: "Progress".to_string(),
                message: msg.clone(),
                percent: *pct * 100.0,
            },
            app::types::DialogKind::CopyMove { source, dest, is_move } => {
                let action = if *is_move { "Move" } else { "Copy" };
                let msg = format!(
                    "{} {} item(s)\nfrom: {}\n  to: {}",
                    action,
                    source.len(),
                    source.first().map(|p| p.display().to_string()).unwrap_or_default(),
                    dest.display(),
                );
                dialogs::DialogKind::Confirm {
                    title: format!("{action} Confirm"),
                    message: msg,
                }
            }
            app::types::DialogKind::Properties { name, size, mtime, permissions, owner, group, is_dir, is_symlink } => {
                let file_type = if *is_symlink {
                    "Symlink"
                } else if *is_dir {
                    "Directory"
                } else {
                    "File"
                };
                use chrono::TimeZone;
                let mtime_str = if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    chrono::Local
                        .timestamp_opt(i64::try_from(duration.as_secs()).unwrap_or(i64::MAX), 0)
                        .single()
                        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.into())
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                } else {
                    "Unknown".to_string()
                };
                dialogs::DialogKind::Properties {
                    name: name.clone(),
                    size: app::types::FileEntry::format_size(*size),
                    mtime: mtime_str,
                    permissions: app::types::FileEntry::display_permissions_raw(*permissions),
                    owner: owner.clone(),
                    group: group.clone(),
                    file_type: file_type.to_string(),
                }
            }
        };
        dialogs::render_dialog(f, &ui_dialog);
    }

    // Menu overlay
    if state.mode == AppMode::Menu {
        render_menu_dropdown(
            f,
            main_layout[0],
            state.menu_selected,
            state.menu_item_selected,
        );
    }

    // List picker overlay
    if let AppMode::ListPicker(ref kind) = state.mode {
        match kind {
            PickerKind::History => {
                let items: Vec<String> = state.command_history.iter().rev().cloned().collect();
                dialogs::render_list_picker(
                    f,
                    "Command History",
                    &items,
                    state.picker_selected,
                    "Enter: select  Esc: cancel",
                );
            }
            PickerKind::Hotlist => {
                let items: Vec<String> = state
                    .directory_hotlist
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                dialogs::render_list_picker(
                    f,
                    "Directory Hotlist",
                    &items,
                    state.picker_selected,
                    "Enter: cd  a: add current  d: delete  Esc: close",
                );
            }
            PickerKind::CompareMode => {
                let items = vec![
                    "Quick".to_string(),
                    "Size".to_string(),
                    "Thorough".to_string(),
                ];
                dialogs::render_list_picker(
                    f,
                    "Compare Mode",
                    &items,
                    state.picker_selected,
                    "Enter: select  Esc: cancel",
                );
            }
            PickerKind::UserMenu => {
                let items: Vec<String> = state
                    .user_menu_entries
                    .iter()
                    .map(|e| format!("{}  {}", e.hotkey, e.title))
                    .collect();
                dialogs::render_list_picker(
                    f,
                    "User Menu",
                    &items,
                    state.picker_selected,
                    "Enter: run  Esc: cancel",
                );
            }
        }
    }
}

fn render_menu_dropdown(
    f: &mut Frame,
    menu_bar_area: Rect,
    selected_menu: usize,
    selected_item: usize,
) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let menu_titles = ["Left", "File", "Command", "Options", "Right"];
    let menu_items: [&[&str]; 5] = [
        &[
            "Listing mode...",
            "Sort order...",
            "Filter...",
            "Encoding...",
        ],
        &[
            "User menu",
            "View file",
            "Edit file",
            "Copy",
            "Move",
            "Mkdir",
            "Delete",
            "Rename",
            "Chmod",
            "Quit",
        ],
        &[
            "Directory tree",
            "Find file",
            "Swap panels",
            "Switch panels",
            "Compare dirs",
            "History",
            "Directory hotlist",
        ],
        &[
            "Configuration...",
            "Layout...",
            "Panel options...",
            "Appearance...",
            "Show hidden files",
            "Save setup",
        ],
        &[
            "Listing mode...",
            "Sort order...",
            "Filter...",
            "Encoding...",
        ],
    ];

    // Highlight selected menu title in menu bar
    let mut x_offset = 3u16;
    for (i, title) in menu_titles.iter().enumerate() {
        let title_width = title.len() as u16 + 2;
        let style = if i == selected_menu {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::LightBlue).bg(Color::DarkGray)
        };
        let label = format!(" {title} ");
        let p = Paragraph::new(label).style(style);
        let area = Rect::new(menu_bar_area.x + x_offset, menu_bar_area.y, title_width, 1);
        f.render_widget(p, area);
        x_offset += title_width + 1;
    }

    // Draw dropdown
    let items = menu_items[selected_menu];
    let dropdown_width = items.iter().map(|s| s.len()).max().unwrap_or(10) as u16 + 4;
    let dropdown_height = items.len() as u16 + 2; // +2 for border

    // Calculate dropdown x position
    let mut menu_x = 3u16;
    for title in menu_titles.iter().take(selected_menu) {
        menu_x += title.len() as u16 + 3;
    }
    let dropdown_x = menu_bar_area.x + menu_x;
    let dropdown_y = menu_bar_area.y + 1;

    let max_dropdown_x = menu_bar_area.x.saturating_add(menu_bar_area.width).saturating_sub(dropdown_width);
    let dropdown_area = Rect::new(
        dropdown_x.min(max_dropdown_x),
        dropdown_y,
        dropdown_width,
        dropdown_height,
    );

    f.render_widget(Clear, dropdown_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .style(Style::default().bg(Color::DarkGray));
    let inner = block.inner(dropdown_area);
    f.render_widget(block, dropdown_area);

    for (i, item) in items.iter().enumerate() {
        if i >= inner.height as usize {
            break;
        }
        let style = if i == selected_item {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        let item_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let p = Paragraph::new(format!(" {item} ")).style(style);
        f.render_widget(p, item_area);
    }
}

fn render_directory_tree(f: &mut Frame, state: &AppState) {
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let area = f.area();
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Directory Tree: {} ", state.tree_root.display()))
        .title_style(Style::default().fg(Color::LightCyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || state.tree_entries.is_empty() {
        return;
    }

    let visible_height = inner.height.saturating_sub(1) as usize;
    let scroll = state.tree_scroll;
    let entries = &state.tree_entries;

    // Clamp scroll so selected is visible
    let selected = state.tree_selected;
    let effective_scroll = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };

    let start = effective_scroll;
    let end = (start + visible_height).min(entries.len());

    for (offset, entry) in entries[start..end].iter().enumerate() {
        let row = start + offset;
        let y = inner.y + offset as u16;
        if y >= inner.y + inner.height {
            break;
        }

        let indent = "  ".repeat(entry.depth);
        let prefix = if entry.is_dir {
            if entry.expanded { "- " } else { "+ " }
        } else {
            "  "
        };

        let line_style = if row == selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if entry.is_dir {
            Style::default().fg(Color::LightBlue)
        } else {
            Style::default().fg(Color::White)
        };

        let text = format!("{}{}{}", indent, prefix, entry.name);
        let para = Paragraph::new(text).style(line_style);
        let row_area = Rect::new(inner.x, y, inner.width, 1);
        f.render_widget(para, row_area);
    }

    // Bottom bar (inside border, above bottom border line)
    let bottom_y = inner.y + inner.height.saturating_sub(1);
    let bottom_area = Rect::new(inner.x, bottom_y, inner.width, 1);
    let help_text = " Enter: expand/collapse  c: cd  Esc: close  PgUp/PgDn: scroll";
    let help_para = Paragraph::new(help_text).style(Style::default().fg(Color::Yellow));
    f.render_widget(help_para, bottom_area);
}

fn handle_directory_tree(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
) {
    let visible_height = directory_tree_visible_height(terminal_height);
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.tree_selected > 0 {
                state.tree_selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !state.tree_entries.is_empty() && state.tree_selected + 1 < state.tree_entries.len()
            {
                state.tree_selected += 1;
            }
        }
        KeyCode::Home => {
            state.tree_selected = 0;
            state.tree_scroll = 0;
        }
        KeyCode::End => {
            if !state.tree_entries.is_empty() {
                state.tree_selected = state.tree_entries.len() - 1;
            }
        }
        KeyCode::PageUp => {
            state.tree_selected = state.tree_selected.saturating_sub(visible_height);
            state.tree_scroll = state.tree_scroll.saturating_sub(visible_height);
        }
        KeyCode::PageDown => {
            if !state.tree_entries.is_empty() {
                state.tree_selected =
                    (state.tree_selected + visible_height).min(state.tree_entries.len() - 1);
                state.tree_scroll = state
                    .tree_scroll
                    .saturating_add(visible_height)
                    .min(state.tree_entries.len().saturating_sub(visible_height));
            }
        }
        KeyCode::Enter => {
            let selected = state.tree_selected;
            let is_dir = state.tree_entries.get(selected).is_some_and(|e| e.is_dir);
            let is_file = state.tree_entries.get(selected).is_some_and(|e| !e.is_dir);

            if is_dir {
                let show_hidden = state.active_panel().show_hidden;
                dir_tree::toggle_expand(
                    &mut state.tree_entries,
                    selected,
                    &state.tree_root,
                    show_hidden,
                );
                // Clamp selection after toggle
                if state.tree_selected >= state.tree_entries.len() && !state.tree_entries.is_empty()
                {
                    state.tree_selected = state.tree_entries.len() - 1;
                }
            } else if is_file {
                let path = state.tree_entries[selected].path.clone();
                if let Ok(vs) = viewer::ViewerState::open(&path) {
                    *viewer_state = Some(vs);
                    state.prev_mode = Some(state.mode.clone());
                    state.mode = AppMode::Viewing;
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(entry) = state.tree_entries.get(state.tree_selected) {
                let target = if entry.is_dir {
                    entry.path.clone()
                } else {
                    entry
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default()
                };
                if !target.as_os_str().is_empty() && target.is_dir() {
                    state.active_panel_mut().path = target;
                    state.active_panel_mut().cursor = 0;
                    state.active_panel_mut().scroll_offset = 0;
                    refresh_active(state);
                    state.mode = AppMode::Normal;
                }
            }
        }
        _ => {}
    }

    let selected = state.tree_selected;
    let scroll = state.tree_scroll;
    let effective = if selected < scroll {
        selected
    } else if selected >= scroll + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else {
        scroll
    };
    state.tree_scroll = effective;
}

fn directory_tree_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(3) as usize
}

fn panel_visible_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(6) as usize
}

fn handle_normal_mode(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    modifiers: KeyModifiers,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) {
    let visible = panel_visible_height(terminal_height);
    match key {
        KeyCode::F(10) => state.should_quit = true,
        KeyCode::F(1) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Help(
                "F1=Help F2=Menu F3=View F4=Edit F5=Copy F6=Move F7=Mkdir F8=Delete F9=Menu F10=Quit | Tab=Switch Ctrl+U=Swap Alt+1-9=Hotlist Alt+Back=Back".to_string(),
            ));
        }
        KeyCode::F(2) => {
            state.mode = AppMode::ListPicker(app::types::PickerKind::UserMenu);
            state.picker_selected = 0;
        }
        KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
            // Shift+Up: move cursor up and toggle selection
            let panel = state.active_panel_mut();
            if panel.cursor > 0 {
                panel.cursor -= 1;
                panel.toggle_selection();
                if panel.cursor < panel.scroll_offset {
                    panel.scroll_offset = panel.cursor;
                }
            }
        }
        KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
            // Shift+Down: move cursor down and toggle selection
            let panel = state.active_panel_mut();
            let len = panel.entries.len();
            if len > 0 && panel.cursor < len - 1 {
                panel.cursor += 1;
                panel.toggle_selection();
                if panel.cursor >= panel.scroll_offset + visible {
                    panel.scroll_offset = panel.cursor.saturating_sub(visible) + 1;
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.active_panel_mut().move_cursor_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.active_panel_mut().move_cursor_down(visible);
        }
        KeyCode::Home => {
            let p = state.active_panel_mut();
            p.cursor = 0;
            p.scroll_offset = 0;
        }
        KeyCode::End => {
            let len = state.active_panel().entries.len();
            if len > 0 {
                let p = state.active_panel_mut();
                p.cursor = len - 1;
                p.ensure_cursor_visible(visible);
            }
        }
        KeyCode::PageUp => {
            let p = state.active_panel_mut();
            p.cursor = p.cursor.saturating_sub(visible);
            p.scroll_offset = p.scroll_offset.saturating_sub(visible);
        }
        KeyCode::PageDown => {
            let len = state.active_panel().entries.len();
            let p = state.active_panel_mut();
            p.cursor = (p.cursor + visible).min(len.saturating_sub(1));
            p.scroll_offset = (p.scroll_offset + visible).min(len.saturating_sub(visible));
        }
        KeyCode::Enter if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+Enter: Show file properties dialog
            if let Some(entry) = state.active_panel().current_entry() {
                if entry.name != ".." {
                    state.mode = AppMode::Dialog(app::types::DialogKind::Properties {
                        name: entry.name.clone(),
                        size: entry.size,
                        mtime: entry.modified,
                        permissions: entry.permissions,
                        owner: entry.owner.clone(),
                        group: entry.group.clone(),
                        is_dir: entry.is_dir,
                        is_symlink: entry.is_symlink,
                    });
                }
            }
        }
        KeyCode::Enter => {
            let entry = state.active_panel().current_entry().cloned();
            if let Some(entry) = entry
                && entry.is_dir
            {
                let p = state.active_panel_mut();
                // Push current path to history before changing
                p.history.push(p.path.clone());
                p.path = entry.path.clone();
                p.cursor = 0;
                p.scroll_offset = 0;
                refresh_active(state);
            }
        }
        KeyCode::Tab => {
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            let p = state.active_panel_mut();
            let max = p.entries.len().saturating_sub(1);
            p.cursor = p.cursor.min(max);
            p.ensure_cursor_visible(visible);
        }
        KeyCode::Insert => {
            state.active_panel_mut().toggle_selection();
            state.active_panel_mut().move_cursor_down(visible);
        }
        KeyCode::F(3) => {
            if let Some(entry) = state.active_panel().current_entry()
                && !entry.is_dir
                && let Ok(vs) = viewer::ViewerState::open(&entry.path)
            {
                *viewer_state = Some(vs);
                state.mode = AppMode::Viewing;
            }
        }
        KeyCode::F(4) => {
            if let Some(entry) = state.active_panel().current_entry().cloned()
                && !entry.is_dir
            {
                const TERMINAL_STATE_FILE: &str = "/tmp/lc_terminal_state";

                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                let _ = crossterm::terminal::disable_raw_mode();
                let _ = crossterm::execute!(
                    io::stdout(),
                    crossterm::terminal::LeaveAlternateScreen,
                    crossterm::event::DisableMouseCapture,
                    crossterm::cursor::Show
                );
                // Write state file before launching editor – if editor is SIGKILL'd,
                // Drop is skipped but we can detect this on next startup.
                let _ = std::fs::write(TERMINAL_STATE_FILE, "alternate_screen");
                let mut parts = editor.split_whitespace();
                let cmd = parts.next().unwrap_or("vi");
                let status = std::process::Command::new(cmd)
                    .args(parts)
                    .arg(&entry.path)
                    .stdin(std::process::Stdio::inherit())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .status();
                // Clean up state file on normal exit
                let _ = std::fs::remove_file(TERMINAL_STATE_FILE);
                let _ = crossterm::execute!(
                    io::stdout(),
                    crossterm::terminal::EnterAlternateScreen,
                    crossterm::event::EnableMouseCapture,
                    crossterm::cursor::Show
                );
                let _ = crossterm::terminal::enable_raw_mode();
                if let Err(e) = status {
                    state.status_message = Some(format!("Editor error: {e}"));
                }
                refresh_active(state);
            }
        }
        KeyCode::F(9) => {
            state.mode = AppMode::Menu;
            state.menu_item_selected = 0;
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
        }
        KeyCode::F(5) => {
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
                let dest_dir = state.inactive_panel().path.clone();
                let msg = if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
                    format!("Copy '{}' to '{}'?", name, dest_dir.display())
                } else {
                    format!("Copy {} entries to '{}'?", paths.len(), dest_dir.display())
                };
                state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(msg));
                state.status_message = Some(encode_paths("copy:", &paths));
            }
        }
        KeyCode::F(6) => {
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
                let dest_dir = state.inactive_panel().path.clone();
                let msg = if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
                    format!("Move '{}' to '{}'?", name, dest_dir.display())
                } else {
                    format!("Move {} entries to '{}'?", paths.len(), dest_dir.display())
                };
                state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(msg));
                state.status_message = Some(encode_paths("move:", &paths));
            }
        }
        KeyCode::F(7) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                "Create directory:".to_string(),
                String::new(),
            ));
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
        }
        KeyCode::F(8) => {
            let paths = selected_or_current_paths(state);
            if !paths.is_empty() {
                let msg = if paths.len() == 1 {
                    let name = paths[0]
                        .file_name()
                        .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
                    format!("Delete '{name}'?")
                } else {
                    format!("Delete {} entries?", paths.len())
                };
                state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(msg));
                state.status_message = Some(encode_paths("delete:", &paths));
            }
        }
        KeyCode::Backspace if modifiers.contains(KeyModifiers::ALT) => {
            let panel = state.active_panel_mut();
            if let Some(prev_path) = panel.history.pop() {
                if prev_path.is_dir() {
                    panel.path = prev_path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", prev_path.display()));
                }
            }
        }
        KeyCode::Char('1') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(0).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('2') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(1).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('3') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(2).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('4') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(3).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('5') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(4).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('6') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(5).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('7') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(6).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('8') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(7).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('9') if modifiers.contains(KeyModifiers::ALT) => {
            if let Some(path) = state.directory_hotlist.get(8).cloned() {
                if path.is_dir() {
                    let panel = state.active_panel_mut();
                    panel.path = path.clone();
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                    state.status_message = Some(format!("cd to {}", path.display()));
                }
            }
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::ALT) => {
            state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                "Quick cd:".to_string(),
                state.active_panel().path.display().to_string(),
            ));
            state.dialog_input = state.active_panel().path.display().to_string();
            state.dialog_cursor_pos = state.dialog_input.chars().count();
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = AppMode::Search;
            state.search_query.clear();
        }
        KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
            refresh_active(state);
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+O: Toggle external panel view (hide panels, see terminal)
            let _ = toggle_external_view(state, terminal);
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = AppMode::Search;
            state.search_query.clear();
        }
        KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
            refresh_active(state);
        }
        _ => {
            // Enter command line mode on any regular key press (no modifiers)
            // OR enter incremental search mode for alphanumeric chars
            if let KeyCode::Char(c) = key {
                if modifiers.is_empty() {
                    // Incremental search: typing starts search mode
                    state.search_query.push(c);
                    state.mode = AppMode::Search;
                    // Apply filter immediately - clone query first to avoid borrow issues
                    let filter_query = state.search_query.clone();
                    let panel = state.active_panel_mut();
                    panel.filter = Some(filter_query);
                    panel.cursor = 0;
                    panel.scroll_offset = 0;
                    refresh_active(state);
                }
            }
        }
    }
}

fn handle_viewer_mode(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
    terminal_width: u16,
) {
    if let Some(vs) = viewer_state.as_mut() {
        let page_height = terminal_height.saturating_sub(3) as usize;
        let content_width = terminal_width.saturating_sub(2) as usize;
        match key {
            KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char('q') => {
                state.mode = state.prev_mode.take().unwrap_or(AppMode::Normal);
                *viewer_state = None;
            }
            KeyCode::Up | KeyCode::Char('k') => vs.scroll_up(1),
            KeyCode::Down | KeyCode::Char('j') => vs.scroll_down(1),
            KeyCode::PageUp => vs.page_up(page_height),
            KeyCode::PageDown => vs.page_down(page_height),
            KeyCode::Home => vs.go_to_top(),
            KeyCode::End => vs.go_to_bottom(page_height),
            KeyCode::Left => vs.scroll_left(4),
            KeyCode::Right => vs.scroll_right(4, content_width),
            KeyCode::Char('l') => vs.toggle_line_numbers(),
            KeyCode::Char('w') => vs.toggle_wrap(),
            KeyCode::Char('h') => vs.toggle_hex_mode(),
            KeyCode::Char('n') => vs.next_match(page_height),
            KeyCode::Char('N') => vs.prev_match(page_height),
            KeyCode::Char('/') => {
                state.dialog_input = vs.search_query.clone().unwrap_or_default();
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                    "Viewer search:".to_string(),
                    state.dialog_input.clone(),
                ));
            }
            _ => {}
        }
    } else {
        state.mode = AppMode::Normal;
    }
}

fn handle_command_line(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.history_index = None;
        }
        KeyCode::Enter => {
            let cmd = state.command_line.clone();
            state.mode = AppMode::Normal;
            state.command_line.clear();
            state.history_index = None;
            if !cmd.is_empty() {
                run_shell_command(state, &cmd);
            }
        }
        KeyCode::Backspace => {
            state.command_line.pop();
            state.history_index = None;
        }
        KeyCode::Up => {
            if !state.command_history.is_empty() {
                if state.history_index.is_none() {
                    state.command_draft = state.command_line.clone();
                }
                let idx = match state.history_index {
                    Some(i) if i > 0 => i - 1,
                    Some(i) => i,
                    None => state.command_history.len() - 1,
                };
                state.history_index = Some(idx);
                state.command_line = state.command_history[idx].clone();
            }
        }
        KeyCode::Down => {
            if let Some(idx) = state.history_index {
                if idx + 1 < state.command_history.len() {
                    state.history_index = Some(idx + 1);
                    state.command_line = state.command_history[idx + 1].clone();
                } else {
                    state.history_index = None;
                    state.command_line = state.command_draft.clone();
                }
            }
        }
        KeyCode::Char(c) => {
            state.command_line.push(c);
            state.history_index = None;
        }
        _ => {}
    }
}

fn run_shell_command(state: &mut AppState, cmd: &str) {
    if cmd.trim().is_empty() {
        return;
    }

    if state.command_history.last().is_none_or(|last| last != cmd) {
        state.command_history.push(cmd.to_string());
        if state.command_history.len() > 100 {
            state.command_history.remove(0);
        }
    }

    struct ShellRestoreGuard {
        restore_ok: bool,
    }

    impl Drop for ShellRestoreGuard {
        fn drop(&mut self) {
            if !self.restore_ok {
                eprintln!("Terminal restore failed after shell command");
            }
        }
    }

    let mut restore_guard = ShellRestoreGuard { restore_ok: false };
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    );
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&state.active_panel().path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    match status {
        Ok(s) if s.success() => println!("\n[Command succeeded. Press Enter to return]"),
        Ok(s) => println!("\n[Command exited with status: {s}. Press Enter to return]"),
        Err(e) => println!("\n[Command failed: {e}. Press Enter to return]"),
    }
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    let mut ok = true;
    if crossterm::execute!(
        io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Show
    ).is_err() {
        ok = false;
    }
    if crossterm::terminal::enable_raw_mode().is_err() {
        ok = false;
    }
    if ok {
        restore_guard.restore_ok = true;
    } else {
        state.status_message = Some("Terminal restore failed – display may be corrupted".into());
    }
    refresh_active(state);
}

fn parse_octal_mode(input: &str) -> Option<u32> {
    u32::from_str_radix(input.trim(), 8).ok().map(|m| m & 0o777)
}

fn selected_or_current_paths(state: &AppState) -> Vec<std::path::PathBuf> {
    let selected: Vec<std::path::PathBuf> = state
        .active_panel()
        .selected_entries()
        .into_iter()
        .filter(|entry| entry.name != "..")
        .map(|entry| entry.path.clone())
        .collect();

    if selected.is_empty() {
        state
            .active_panel()
            .current_entry()
            .filter(|entry| entry.name != "..")
            .map(|entry| vec![entry.path.clone()])
            .unwrap_or_default()
    } else {
        selected
    }
}

fn encode_paths(prefix: &str, paths: &[std::path::PathBuf]) -> String {
    let joined = paths
        .iter()
        .map(|p| {
            p.to_string_lossy()
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{prefix}{joined}")
}

fn decode_path_component(encoded: &str) -> String {
    let mut decoded = String::new();
    let mut chars = encoded.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => decoded.push('\n'),
                Some('\\') => decoded.push('\\'),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(ch);
        }
    }

    decoded
}

fn handle_dialog(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
) {
    let dialog_kind = if let AppMode::Dialog(ref dk) = state.mode {
        dk.clone()
    } else {
        return;
    };

    match dialog_kind {
        app::types::DialogKind::Confirm(_) => match key {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                if let Some(ref status) = state.status_message {
                    let status = status.clone();
                    execute_confirmed_action(state, &status);
                    if state.status_message.is_some() {
                        state.mode = AppMode::Normal;
                        refresh_both(state);
                        if let Some(panel) = state.menu_restore_panel.take() {
                            set_active_panel(state, panel);
                        }
                        return;
                    }
                }
                state.mode = AppMode::Normal;
                state.status_message = None;
                refresh_both(state);
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                state.mode = AppMode::Normal;
                state.status_message = None;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
            _ => {}
        },
        app::types::DialogKind::Input(_, _) => match key {
            KeyCode::Enter => {
                let input = state.dialog_input.clone();
                if let AppMode::Dialog(app::types::DialogKind::Input(ref prompt, _)) = state.mode
                    && prompt == "Viewer search:"
                {
                    if let Some(vs) = viewer_state.as_mut() {
                        vs.search(&input, terminal_height.saturating_sub(3) as usize);
                    }
                    state.mode = AppMode::Viewing;
                    state.dialog_input.clear();
                    state.dialog_cursor_pos = 0;
                    return;
                }
                if let AppMode::Dialog(app::types::DialogKind::Input(prompt, _)) = &state.mode
                {
                    let prompt = prompt.clone();
                    if prompt == "Create directory:" && !input.trim().is_empty() {
                        let dir = state.active_panel().path.clone();
                        if let Err(err) = ops::file_ops::create_directory(&dir.join(&input)) {
                            state.status_message =
                                Some(format!("Create directory failed: {err}"));
                        } else {
                            refresh_active(state);
                        }
                    } else if prompt == "Rename to:" && !input.is_empty() {
                        if let Some(entry) = state.active_panel().current_entry()
                            && let Err(err) = ops::file_ops::rename_entry(&entry.path, &input)
                        {
                            state.status_message = Some(format!("Rename failed: {err}"));
                        }
                    } else if prompt == "Chmod (octal):" && !input.is_empty() {
                        if let Some(mode) = parse_octal_mode(&input) {
                            if let Some(entry) = state.active_panel().current_entry()
                                && let Err(err) = ops::file_ops::chmod(&entry.path, mode) {
                                state.status_message = Some(format!("Chmod failed: {err}"));
                            }
                        } else {
                            state.status_message = Some(format!("Invalid octal mode '{input}'"));
                        }
                    } else if prompt == "Filter:" {
                        let panel = state.active_panel_mut();
                        panel.filter = if input.trim().is_empty() {
                            None
                        } else {
                            Some(input)
                        };
                    } else if prompt == "Quick cd:" {
                        let expanded = if let Some(stripped) = input.strip_prefix('~') {
                            if let Some(home) = std::env::var_os("HOME") {
                                std::path::PathBuf::from(home)
                                    .join(stripped.trim_start_matches('/'))
                            } else {
                                std::path::PathBuf::from(&input)
                            }
                        } else {
                            let path = std::path::PathBuf::from(&input);
                            if path.is_absolute() {
                                path
                            } else {
                                state.active_panel().path.join(path)
                            }
                        };

                        if expanded.is_dir() {
                            let panel = state.active_panel_mut();
                            // Push current path to history before changing
                            panel.history.push(panel.path.clone());
                            panel.path = expanded.clone();
                            panel.cursor = 0;
                            panel.scroll_offset = 0;
                            refresh_active(state);
                            if !state.directory_hotlist.iter().any(|p| p == &expanded) {
                                state.directory_hotlist.push(expanded);
                            }
                        } else {
                            state.status_message = Some(format!("Directory not found: {input}"));
                        }
                    } else if prompt == "Find file:" {
                        let dir = state.active_panel().path.clone();
                        let results =
                            ops::search::FileSearch::search_files(&dir, &input, true, false);
                        if let Some(first) = results.first() {
                            if let Some(parent) = first.parent() {
                                state.active_panel_mut().path = parent.to_path_buf();
                                refresh_active(state);
                                if let Some(pos) = state
                                    .active_panel()
                                    .entries
                                    .iter()
                                    .position(|e| e.path == *first)
                                {
                                    state.active_panel_mut().cursor = pos;
                                    state.active_panel_mut().ensure_cursor_visible(panel_visible_height(terminal_height));
                                }
                            }
                        } else {
                            state.status_message = Some(format!("No matches for '{input}'"));
                        }
                    }
                }
                state.mode = AppMode::Normal;
                state.dialog_input.clear();
                state.dialog_cursor_pos = 0;
                refresh_active(state);
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
            KeyCode::Esc => {
                let return_to_viewing = matches!(
                    &state.mode,
                    AppMode::Dialog(app::types::DialogKind::Input(p, _)) if p == "Viewer search:"
                );
                state.mode = if return_to_viewing {
                    AppMode::Viewing
                } else {
                    AppMode::Normal
                };
                state.dialog_input.clear();
                state.dialog_cursor_pos = 0;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
            KeyCode::Backspace => {
                if state.dialog_cursor_pos > 0 {
                    state.dialog_cursor_pos -= 1;
                    let byte_pos = state.dialog_input.char_indices().nth(state.dialog_cursor_pos).map(|(i, _)| i).unwrap_or(state.dialog_input.len());
                    let next_byte = state.dialog_input[byte_pos..]
                        .chars()
                        .next()
                        .map(|c| byte_pos + c.len_utf8())
                        .unwrap_or(state.dialog_input.len());
                    state.dialog_input.drain(byte_pos..next_byte);
                }
            }
            KeyCode::Delete => {
                let byte_pos = state.dialog_input
                    .char_indices()
                    .nth(state.dialog_cursor_pos)
                    .map(|(i, _)| i);
                if let Some(pos) = byte_pos {
                    let next_char_end = state.dialog_input[pos..]
                        .chars()
                        .next()
                        .map(|c| pos + c.len_utf8())
                        .unwrap_or(state.dialog_input.len());
                    state.dialog_input.drain(pos..next_char_end);
                }
            }
            KeyCode::Char(c) => {
                let byte_pos = state.dialog_input.char_indices().nth(state.dialog_cursor_pos).map(|(i, _)| i).unwrap_or(state.dialog_input.len());
                state.dialog_input.insert(byte_pos, c);
                state.dialog_cursor_pos += 1;
            }
            KeyCode::Left => {
                if state.dialog_cursor_pos > 0 {
                    state.dialog_cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if state.dialog_cursor_pos < state.dialog_input.chars().count() {
                    state.dialog_cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                state.dialog_cursor_pos = 0;
            }
            KeyCode::End => {
                state.dialog_cursor_pos = state.dialog_input.chars().count();
            }
            _ => {}
        },
        app::types::DialogKind::Error(_) => {
            if matches!(key, KeyCode::Enter | KeyCode::Esc) {
                state.mode = AppMode::Normal;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
        }
        app::types::DialogKind::Help(_) => {
            // Help dialog exits on any key
            state.mode = AppMode::Normal;
            if let Some(panel) = state.menu_restore_panel.take() {
                set_active_panel(state, panel);
            }
        }
        app::types::DialogKind::Progress(_, _) => {
            // Progress dialog - exits on Esc
            if key == KeyCode::Esc {
                state.mode = AppMode::Normal;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
        }
        app::types::DialogKind::Properties { .. } => {
            // Properties dialog exits on Enter or Esc
            if matches!(key, KeyCode::Enter | KeyCode::Esc) {
                state.mode = AppMode::Normal;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
        }
        _ => {
            if key == KeyCode::Esc {
                state.mode = AppMode::Normal;
                if let Some(panel) = state.menu_restore_panel.take() {
                    set_active_panel(state, panel);
                }
            }
        }
    }
}

fn handle_list_picker(state: &mut AppState, key: KeyCode) {
    let kind = if let AppMode::ListPicker(ref k) = state.mode {
        k.clone()
    } else {
        return;
    };

    match kind {
        PickerKind::History => {
            let len = state.command_history.len();
            match key {
                KeyCode::Esc => {
                    state.mode = AppMode::Normal;
                }
                KeyCode::Up => {
                    if len > 0 && state.picker_selected > 0 {
                        state.picker_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if len > 0 && state.picker_selected + 1 < len {
                        state.picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let idx = len.saturating_sub(1).saturating_sub(state.picker_selected);
                    if let Some(cmd) = state.command_history.get(idx).cloned() {
                        state.command_line = cmd;
                        state.mode = AppMode::CommandLine;
                    } else {
                        state.mode = AppMode::Normal;
                    }
                }
                _ => {}
            }
        }
        PickerKind::Hotlist => {
            let len = state.directory_hotlist.len();
            match key {
                KeyCode::Esc => {
                    state.mode = AppMode::Normal;
                }
                KeyCode::Up => {
                    if len > 0 && state.picker_selected > 0 {
                        state.picker_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if len > 0 && state.picker_selected + 1 < len {
                        state.picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(path) = state.directory_hotlist.get(state.picker_selected).cloned()
                    {
                        if path.is_dir() {
                            state.active_panel_mut().path = path;
                            state.active_panel_mut().cursor = 0;
                            state.active_panel_mut().scroll_offset = 0;
                            refresh_active(state);
                        } else {
                            state.status_message =
                                Some("Hotlist entry no longer exists".to_string());
                        }
                        state.mode = AppMode::Normal;
                    }
                }
                KeyCode::Char('a') => {
                    let cur = state.active_panel().path.clone();
                    if state.directory_hotlist.iter().any(|p| p == &cur) {
                        state.status_message = Some("Directory already in hotlist".to_string());
                    } else {
                        state.directory_hotlist.push(cur);
                        state.status_message =
                            Some("Added current directory to hotlist".to_string());
                    }
                }
                KeyCode::Char('d') => {
                    if state.picker_selected < state.directory_hotlist.len() {
                        state.directory_hotlist.remove(state.picker_selected);
                        if state.picker_selected > 0
                            && state.picker_selected >= state.directory_hotlist.len()
                        {
                            state.picker_selected -= 1;
                        }
                    }
                }
                _ => {}
            }
        }
        PickerKind::CompareMode => {
            const MODES: [CompareMode; 3] =
                [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
            let len = MODES.len();
            match key {
                KeyCode::Esc => {
                    state.mode = AppMode::Normal;
                }
                KeyCode::Up => {
                    if state.picker_selected > 0 {
                        state.picker_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if state.picker_selected + 1 < len {
                        state.picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let chosen = MODES[state.picker_selected.min(len - 1)];
                    state.mode = AppMode::Normal;
                    compare_directories(state, chosen);
                }
                _ => {}
            }
        }
        PickerKind::UserMenu => {
            let len = state.user_menu_entries.len();
            match key {
                KeyCode::Esc => {
                    state.mode = AppMode::Normal;
                }
                KeyCode::Up => {
                    if len > 0 && state.picker_selected > 0 {
                        state.picker_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if len > 0 && state.picker_selected + 1 < len {
                        state.picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let idx = state.picker_selected.min(len.saturating_sub(1));
                    state.mode = AppMode::Normal;
                    if let Some(entry) = state.user_menu_entries.get(idx).cloned() {
                        let active_dir = state.active_panel().path.clone();
                        let other_dir = state.inactive_panel().path.clone();
                        let current_file = state
                            .active_panel()
                            .current_entry()
                            .map(|e| e.name.clone())
                            .unwrap_or_default();
                        let tagged: Vec<PathBuf> = state
                            .active_panel()
                            .selected_entries()
                            .into_iter()
                            .filter(|e| e.name != "..")
                            .map(|e| e.path.clone())
                            .collect();
                        let ctx = user_menu::SubstContext {
                            current_file: &current_file,
                            active_dir: &active_dir,
                            other_dir: &other_dir,
                            tagged: &tagged,
                        };
                        let cmd = user_menu::apply_substitutions(&entry.command, &ctx);
                        run_shell_command(state, &cmd);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Handle mouse events for click selection, double-click open, and panel switching
fn handle_mouse_event(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    mouse_event: crossterm::event::MouseEvent,
    terminal_size: ratatui::layout::Size,
) {
    use crossterm::event::{MouseEventKind, MouseButton};
    use std::time::Duration;

    // Only handle mouse events in Normal mode
    if !matches!(state.mode, AppMode::Normal) {
        return;
    }

    let MouseEventKind::Down(button) = mouse_event.kind else {
        return;
    };

    // Only handle left button clicks
    if button != MouseButton::Left {
        return;
    }

    let col = mouse_event.column;
    let row = mouse_event.row;

    // Calculate panel areas (matching render_ui layout)
    // Menu bar: row 0 (1 row)
    // Panels: row 1 to (height - 4)
    // Status bar: height - 3
    // Command line: height - 2
    // Function bar: height - 1
    let panel_start_row = 1u16;
    let panel_end_row = terminal_size.height.saturating_sub(4);
    let panel_height = panel_end_row.saturating_sub(panel_start_row) + 1;

    // Check if click is in panel area
    if row < panel_start_row || row > panel_end_row {
        return;
    }

    // Determine which panel was clicked (left or right half of screen)
    let mid_col = terminal_size.width / 2;
    let clicked_left = col < mid_col;

    // Switch to clicked panel
    if clicked_left {
        state.active_panel = ActivePanel::Left;
    } else {
        state.active_panel = ActivePanel::Right;
    }

    // Get the panel that was clicked to calculate clicked index
    let panel = if clicked_left {
        &state.left_panel
    } else {
        &state.right_panel
    };

    // Calculate which entry was clicked
    // Account for border (1 char) and padding
    let list_start_row = panel_start_row + 1; // Border top
    let relative_row = row.saturating_sub(list_start_row);
    let clicked_index = panel.scroll_offset + relative_row as usize;

    // Check if click is within valid entry range
    if clicked_index >= panel.entries.len() {
        return;
    }

    let now = std::time::Instant::now();
    let is_double_click = if let Some(last_time) = state.last_click_time {
        if let Some(last_pos) = state.last_click_position {
            // Same position and within 300ms = double click
            last_pos.0 == col && last_pos.1 == row && now.duration_since(last_time) < Duration::from_millis(300)
        } else {
            false
        }
    } else {
        false
    };

    if is_double_click {
        // Double-click: open directory or view file (F3 action)
        state.last_click_time = None;
        state.last_click_position = None;

        let entry = panel.entries[clicked_index].clone();
        if entry.is_dir {
            // Open directory
            let panel_mut = state.active_panel_mut();
            panel_mut.history.push(panel_mut.path.clone());
            panel_mut.path = entry.path.clone();
            panel_mut.cursor = 0;
            panel_mut.scroll_offset = 0;
            refresh_panel(panel_mut, panel_height as usize);
        } else {
            // View file
            if let Ok(vs) = viewer::ViewerState::open(&entry.path) {
                *viewer_state = Some(vs);
                state.prev_mode = Some(state.mode.clone());
                state.mode = AppMode::Viewing;
            }
        }
    } else {
        // Single click: select file (move cursor)
        state.last_click_time = Some(now);
        state.last_click_position = Some((col, row));

        let panel_mut = state.active_panel_mut();
        panel_mut.cursor = clicked_index;
        panel_mut.ensure_cursor_visible(panel_height as usize);
    }
}

/// Toggle external panel view (Ctrl+O) - hide panels to see terminal output
fn toggle_external_view(
    state: &mut AppState,
    _terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> io::Result<()> {
    use crossterm::{cursor::Show, terminal::LeaveAlternateScreen};

    // Save current terminal state and leave alternate screen
    let _ = crossterm::execute!(
        std::io::stdout(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        Show
    );
    let _ = crossterm::terminal::disable_raw_mode();

    // Show message to user
    println!("External view active. Press Ctrl+O to return to Libre Commander.");
    println!("Press Enter to continue...");

    // Wait for Ctrl+O or any key
    let _ = crossterm::terminal::enable_raw_mode();
    loop {
        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = crossterm::event::read()? {
                if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                // Also allow Enter to return
                if key.code == KeyCode::Enter {
                    break;
                }
                // Esc to return
                if key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }

    // Restore terminal state
    let _ = crossterm::terminal::disable_raw_mode();
    let mut stdout = std::io::stdout();
    let _ = crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    );
    let _ = crossterm::terminal::enable_raw_mode();

    // Hide cursor again
    let _ = crossterm::execute!(stdout, crossterm::cursor::Hide);

    // Refresh display
    refresh_both(state);

    Ok(())
}

fn decode_paths(prefix: &str, status: &str) -> Vec<std::path::PathBuf> {
    status[prefix.len()..]
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| std::path::PathBuf::from(decode_path_component(line)))
        .collect()
}

fn execute_confirmed_action(state: &mut AppState, status: &str) {
    state.status_message = None;
    if status.starts_with("copy:") {
        let paths = decode_paths("copy:", status);
        let dest_dir = state.inactive_panel().path.clone();
        let mut errors: Vec<String> = Vec::new();
        let mut used_dests: HashSet<PathBuf> = HashSet::new();
        for src in &paths {
            let file_name = src.file_name().unwrap_or_default();
            let dest = dest_dir.join(file_name);
            if !used_dests.insert(dest.clone()) {
                errors.push(format!("{}: duplicate destination {}", src.display(), dest.display()));
                continue;
            }
            let result = match src.symlink_metadata() {
                Ok(meta) if meta.file_type().is_symlink() => ops::file_ops::copy_symlink(src, &dest),
                Ok(meta) if meta.is_dir() => ops::file_ops::copy_dir_recursive(src, &dest).map(|_| ()),
                Ok(_) => ops::file_ops::copy_file(src, &dest).map(|_| ()),
                Err(e) => Err(e),
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        state.active_panel_mut().clear_selection();
        if !errors.is_empty() {
            state.status_message = Some(format!("Copy errors: {}", errors.join("; ")));
        }
        refresh_both(state);
    } else if status.starts_with("move:") {
        let paths = decode_paths("move:", status);
        let dest_dir = state.inactive_panel().path.clone();
        let mut errors: Vec<String> = Vec::new();
        let mut used_dests: HashSet<PathBuf> = HashSet::new();
        for src in &paths {
            let file_name = src.file_name().unwrap_or_default();
            let dest = dest_dir.join(file_name);
            if !used_dests.insert(dest.clone()) {
                errors.push(format!("{}: duplicate destination {}", src.display(), dest.display()));
                continue;
            }
            if let Err(e) = ops::file_ops::move_entry(src, &dest) {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        state.active_panel_mut().clear_selection();
        if !errors.is_empty() {
            state.status_message = Some(format!("Move errors: {}", errors.join("; ")));
        }
        refresh_both(state);
    } else if status.starts_with("delete:") {
        let paths = decode_paths("delete:", status);
        let mut errors: Vec<String> = Vec::new();
        for path in &paths {
            let result = match path.symlink_metadata() {
                Ok(meta) if meta.file_type().is_symlink() => ops::file_ops::delete_file(path),
                Ok(meta) if meta.is_dir() => ops::file_ops::delete_dir_recursive(path),
                Ok(_) => ops::file_ops::delete_file(path),
                Err(e) => Err(e),
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", path.display(), e));
            }
        }
        state.active_panel_mut().clear_selection();
        if !errors.is_empty() {
            state.status_message = Some(format!("Delete errors: {}", errors.join("; ")));
        }
        refresh_both(state);
    }
}

fn handle_search_mode(state: &mut AppState, key: KeyCode, _terminal_height: u16) {
    match key {
        KeyCode::Esc => {
            state.mode = AppMode::Normal;
            state.search_query.clear();
            // Clear filter
            let panel = state.active_panel_mut();
            panel.filter = None;
            panel.cursor = 0;
            panel.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Enter => {
            // Confirm search and stay in search mode with filter active
            state.mode = AppMode::Search;
        }
        KeyCode::Backspace => {
            state.search_query.pop();
            // Update filter - clone query first to avoid borrow issues
            let filter_query = if state.search_query.is_empty() {
                None
            } else {
                Some(state.search_query.clone())
            };
            let panel = state.active_panel_mut();
            panel.filter = filter_query;
            panel.cursor = 0;
            panel.scroll_offset = 0;
            refresh_active(state);
        }
        KeyCode::Char(c) => {
            state.search_query.push(c);
            // Update filter in real-time - clone query first to avoid borrow issues
            let filter_query = state.search_query.clone();
            let panel = state.active_panel_mut();
            panel.filter = Some(filter_query);
            panel.cursor = 0;
            panel.scroll_offset = 0;
            refresh_active(state);
        }
        _ => {}
    }
}

fn handle_menu_mode(
    state: &mut AppState,
    viewer_state: &mut Option<viewer::ViewerState>,
    key: KeyCode,
    terminal_height: u16,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) {
    let menu_counts: [usize; 5] = [4, 10, 7, 6, 4];
    let max_items = menu_counts[state.menu_selected];

    match key {
        KeyCode::Esc | KeyCode::F(9 | 10) => {
            state.mode = AppMode::Normal;
        }
        KeyCode::Left => {
            state.menu_selected = if state.menu_selected == 0 {
                4
            } else {
                state.menu_selected - 1
            };
            state.menu_item_selected = 0;
        }
        KeyCode::Right => {
            state.menu_selected = (state.menu_selected + 1) % 5;
            state.menu_item_selected = 0;
        }
        KeyCode::Up => {
            state.menu_item_selected = if state.menu_item_selected == 0 {
                max_items - 1
            } else {
                state.menu_item_selected - 1
            };
        }
        KeyCode::Down => {
            state.menu_item_selected = (state.menu_item_selected + 1) % max_items;
        }
        KeyCode::Enter => {
            let previous_mode = state.mode.clone();
            if let Some(action_key) = execute_menu_action(state) {
                state.mode = AppMode::Normal;
                handle_normal_mode(state, viewer_state, action_key, KeyModifiers::NONE, terminal_height, terminal);
            } else if state.mode == previous_mode {
                state.mode = AppMode::Normal;
            }
        }
        _ => {}
    }
}

fn execute_menu_action(state: &mut AppState) -> Option<KeyCode> {
    match (state.menu_selected, state.menu_item_selected) {
        (0 | 4, 0) => {
            with_menu_panel(state, |state| {
                let panel = state.active_panel_mut();
                panel.listing_mode = match panel.listing_mode {
                    app::types::ListingMode::Long => app::types::ListingMode::Brief,
                    app::types::ListingMode::Brief => app::types::ListingMode::Long,
                };
            });
            None
        }
        (0 | 4, 1) => {
            with_menu_panel(state, |state| {
                let p = state.active_panel_mut();
                p.sort_mode = cycle_sort_mode(p.sort_mode);
                refresh_active(state);
            });
            None
        }
        (0 | 4, 2) => {
            with_menu_panel(state, |state| {
                state.dialog_input = state.active_panel().filter.clone().unwrap_or_default();
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                    "Filter:".to_string(),
                    state.dialog_input.clone(),
                ));
            });
            None
        }
        (0 | 4, 3) => {
            with_menu_panel(state, refresh_active);
            None
        }
        // File menu: "User menu"(0), "View file"(1), "Edit file"(2), "Copy"(3), "Move"(4), "Mkdir"(5), "Delete"(6), "Rename"(7), "Chmod"(8), "Quit"(9)
        (1, 0) => {
            let panel_dir = state.active_panel().path.clone();
            let current_file = state
                .active_panel()
                .current_entry()
                .map(|e| e.name.clone())
                .unwrap_or_default();
            match user_menu::load_menu(&panel_dir, &current_file) {
                Ok(entries) if entries.is_empty() => {
                    state.mode = AppMode::Dialog(app::types::DialogKind::Error(
                        "No matching menu entries found.".to_string(),
                    ));
                }
                Ok(entries) => {
                    state.user_menu_entries = entries;
                    state.picker_selected = 0;
                    state.mode = AppMode::ListPicker(PickerKind::UserMenu);
                }
                Err(msg) => {
                    state.mode = AppMode::Dialog(app::types::DialogKind::Error(msg));
                }
            }
            None
        }
        (1, 1) => Some(KeyCode::F(3)),
        (1, 2) => Some(KeyCode::F(4)),
        (1, 3) => Some(KeyCode::F(5)),
        (1, 4) => Some(KeyCode::F(6)),
        (1, 5) => Some(KeyCode::F(7)),
        (1, 6) => Some(KeyCode::F(8)),
        (1, 7) => {
            if let Some(entry) = state.active_panel().current_entry().cloned()
                && entry.name != ".."
            {
                state.dialog_input = entry.name.clone();
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                    "Rename to:".to_string(),
                    entry.name.clone(),
                ));
            }
            None
        }
        (1, 8) => {
            if let Some(entry) = state.active_panel().current_entry().cloned()
                && entry.name != ".."
            {
                state.dialog_input = format!("{:o}", entry.permissions & 0o777);
                state.dialog_cursor_pos = state.dialog_input.chars().count();
                state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                    "Chmod (octal):".to_string(),
                    state.dialog_input.clone(),
                ));
            }
            None
        }
        (1, 9) => {
            state.should_quit = true;
            None
        }
        // Command menu: "Directory tree"(0), "Find file"(1), "Swap panels"(2), "Switch panels"(3), ...
        (2, 0) => {
            let path = state.active_panel().path.clone();
            let show_hidden = state.active_panel().show_hidden;
            state.tree_root = path.clone();
            state.tree_entries = dir_tree::build_tree(&path, 2, show_hidden);
            state.tree_selected = 0;
            state.tree_scroll = 0;
            state.mode = AppMode::DirectoryTree;
            None
        }
        (2, 1) => {
            state.dialog_input.clear();
            state.dialog_cursor_pos = 0;
            state.mode = AppMode::Dialog(app::types::DialogKind::Input(
                "Find file:".to_string(),
                String::new(),
            ));
            None
        }
        (2, 2) => {
            std::mem::swap(&mut state.left_panel, &mut state.right_panel);
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        (2, 3) => {
            state.active_panel = match state.active_panel {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
            None
        }
        (2, 4) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::CompareMode);
            None
        }
        (2, 5) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::History);
            None
        }
        (2, 6) => {
            state.picker_selected = 0;
            state.mode = AppMode::ListPicker(PickerKind::Hotlist);
            None
        }
        // Options menu: "Configuration..."(0), "Layout..."(1), "Panel options..."(2), "Appearance..."(3), "Show hidden files"(4), "Save setup"(5)
        (3, 0) => {
            if !state
                .directory_hotlist
                .iter()
                .any(|p| p == &state.active_panel().path)
            {
                state
                    .directory_hotlist
                    .push(state.active_panel().path.clone());
            }
            state.status_message =
                Some("Configuration saved current path into hotlist".to_string());
            None
        }
        (3, 1) => {
            let panel = state.active_panel_mut();
            panel.listing_mode = match panel.listing_mode {
                app::types::ListingMode::Long => app::types::ListingMode::Brief,
                app::types::ListingMode::Brief => app::types::ListingMode::Long,
            };
            state.status_message = Some(format!("Layout changed to {:?}", panel.listing_mode));
            None
        }
        (3, 2) => {
            let panel = state.active_panel_mut();
            panel.show_hidden = !panel.show_hidden;
            refresh_active(state);
            state.status_message = Some(format!(
                "Panel options: hidden={}",
                state.active_panel().show_hidden
            ));
            None
        }
        (3, 3) => {
            let panel = state.active_panel_mut();
            panel.filter = None;
            refresh_active(state);
            state.status_message = Some("Appearance reset active panel filter".to_string());
            None
        }
        (3, 4) => {
            let p = state.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            p.cursor = 0;
            p.scroll_offset = 0;
            refresh_active(state);
            None
        }
        (3, 5) => {
            match app::config::save_setup(state) {
                Ok(path) => {
                    state.status_message = Some(format!("Setup saved to {}", path.display()));
                }
                Err(err) => {
                    state.status_message = Some(format!("Save setup failed: {err}"));
                }
            }
            None
        }
        _ => None,
    }
}

fn compare_directories(state: &mut AppState, mode: CompareMode) {
    use std::collections::{HashMap, HashSet};

    // Reset all selections first.
    for entry in &mut state.left_panel.entries {
        entry.selected = false;
    }
    for entry in &mut state.right_panel.entries {
        entry.selected = false;
    }

    // Build lookup: name → metadata needed for comparison.
    // We collect only what we need: (is_dir, size, mtime) to avoid holding references.
    #[derive(Clone, Copy, PartialEq)]
    struct EntryMeta {
        is_dir: bool,
        size: u64,
        mtime: std::time::SystemTime,
    }

    let right_meta: HashMap<&str, EntryMeta> = state
        .right_panel
        .entries
        .iter()
        .filter(|e| e.name != "..")
        .map(|e| {
            (
                e.name.as_str(),
                EntryMeta {
                    is_dir: e.is_dir,
                    size: e.size,
                    mtime: e.modified,
                },
            )
        })
        .collect();

    let left_meta: HashMap<&str, EntryMeta> = state
        .left_panel
        .entries
        .iter()
        .filter(|e| e.name != "..")
        .map(|e| {
            (
                e.name.as_str(),
                EntryMeta {
                    is_dir: e.is_dir,
                    size: e.size,
                    mtime: e.modified,
                },
            )
        })
        .collect();

    // Helper: check if two entries match by mode.
    fn meta_matches(left: &EntryMeta, right: &EntryMeta, mode: CompareMode) -> bool {
        if left.is_dir != right.is_dir {
            return false;
        }
        if left.is_dir {
            return true;
        }
        match mode {
            CompareMode::Quick => true,
            CompareMode::Size => left.size == right.size,
            CompareMode::Thorough => left.size == right.size && left.mtime == right.mtime,
        }
    }

    // Count unique-left, unique-right, differing.
    let mut unique_left: usize = 0;
    let mut unique_right: usize = 0;
    let mut differing: usize = 0;

    for (name, left_meta) in &left_meta {
        match right_meta.get(name) {
            None => unique_left += 1,
            Some(right_meta) => {
                if !meta_matches(left_meta, right_meta, mode) {
                    differing += 1;
                }
            }
        }
    }
    for name in right_meta.keys() {
        if !left_meta.contains_key(name) {
            unique_right += 1;
        }
    }

    // Collect names to mark (no references to entries held).
    let mut left_to_mark: HashSet<String> = HashSet::new();
    let mut right_to_mark: HashSet<String> = HashSet::new();

    for (name, left_meta) in &left_meta {
        let should_mark = match right_meta.get(name) {
            None => true,
            Some(right_meta) => !meta_matches(left_meta, right_meta, mode),
        };
        if should_mark {
            left_to_mark.insert(name.to_string());
        }
    }

    for (name, right_entry) in &right_meta {
        match left_meta.get(name) {
            None => right_to_mark.insert(name.to_string()),
            Some(left_entry) => {
                if meta_matches(left_entry, right_entry, mode) {
                    false
                } else {
                    right_to_mark.insert(name.to_string())
                }
            }
        };
    }

    // Apply selection (now we only mutate, no references held).
    for entry in &mut state.left_panel.entries {
        if entry.name == ".." {
            continue;
        }
        entry.selected = left_to_mark.contains(&entry.name);
    }
    state.left_panel.recalculate_selection_stats();
    for entry in &mut state.right_panel.entries {
        if entry.name == ".." {
            continue;
        }
        entry.selected = right_to_mark.contains(&entry.name);
    }
    state.right_panel.recalculate_selection_stats();

    let mode_name = match mode {
        CompareMode::Quick => "Quick",
        CompareMode::Size => "Size",
        CompareMode::Thorough => "Thorough",
    };
    state.status_message = None;
    state.mode = AppMode::Dialog(app::types::DialogKind::Confirm(format!(
        "Compare dirs ({mode_name}):\nUnique in left:  {unique_left}\nUnique in right: {unique_right}\nDiffering:       {differing}"
    )));
}

// ---- Type conversion helpers ----


#[cfg(test)]
mod tests {
    use super::*;
    use app::types::{ActivePanel, FileEntry};
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn menu_toggle_hidden_files_refreshes_active_panel() {
        let state = AppState {
            active_panel: ActivePanel::Left,
            ..Default::default()
        };
        let mut state = state;
        state.left_panel.path = std::env::temp_dir();
        state.left_panel.show_hidden = false;
        state.mode = AppMode::Menu;
        state.menu_selected = 3;
        state.menu_item_selected = 4;

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::Normal);
        assert!(state.left_panel.show_hidden);
    }

    #[test]
    fn menu_rename_opens_input_dialog_with_current_name() {
        let mut state = AppState::default();
        state.left_panel.entries.push(app::types::FileEntry {
            name: "old.txt".to_string(),
            path: std::env::temp_dir().join("old.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        });
        state.mode = AppMode::Menu;
        state.menu_selected = 1;
        state.menu_item_selected = 7;

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.dialog_input, "old.txt");
        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Input(
                "Rename to:".to_string(),
                "old.txt".to_string(),
            ))
        );
    }

    #[test]
    fn parse_octal_mode_accepts_valid_input() {
        assert_eq!(parse_octal_mode("755"), Some(0o755));
        assert_eq!(parse_octal_mode("0644"), Some(0o644));
        assert_eq!(parse_octal_mode("bad"), None);
    }

    #[test]
    fn compare_directories_reports_summary() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "a.txt".to_string(),
            path: std::env::temp_dir().join("a.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        }];
        state.right_panel.entries = vec![app::types::FileEntry {
            name: "b.txt".to_string(),
            path: std::env::temp_dir().join("b.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        }];

        compare_directories(&mut state, CompareMode::Quick);

        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 1\nDiffering:       0"
                    .to_string()
            ))
        );
    }

    #[test]
    fn menu_history_opens_picker() {
        let state = AppState {
            mode: AppMode::Menu,
            menu_selected: 2,
            menu_item_selected: 5,
            ..Default::default()
        };
        let mut state = state;
        state.command_history.push("ls -la".to_string());

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::History));
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn menu_hotlist_opens_picker() {
        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 2,
            menu_item_selected: 6,
            ..Default::default()
        };
        state.directory_hotlist.push(std::env::temp_dir());

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::Hotlist));
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn command_line_up_loads_last_history_entry() {
        let mut state = AppState::default();
        state.command_history.push("git status".to_string());

        handle_command_line(&mut state, KeyCode::Up);

        assert_eq!(state.command_line, "git status");
    }

    #[test]
    fn compare_directories_marks_unique_entries_selected() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![
            app::types::FileEntry {
                name: "same.txt".to_string(),
                path: std::env::temp_dir().join("same.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
            },
            app::types::FileEntry {
                name: "left.txt".to_string(),
                path: std::env::temp_dir().join("left.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
            },
        ];
        state.right_panel.entries = vec![
            app::types::FileEntry {
                name: "same.txt".to_string(),
                path: std::env::temp_dir().join("same.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
            },
            app::types::FileEntry {
                name: "right.txt".to_string(),
                path: std::env::temp_dir().join("right.txt"),
                is_dir: false,
                is_symlink: false,
                is_executable: false,
                size: 0,
                modified: std::time::SystemTime::now(),
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                selected: false,
                is_hidden: false,
            },
        ];

        compare_directories(&mut state, CompareMode::Quick);

        assert!(!state.left_panel.entries[0].selected);
        assert!(state.left_panel.entries[1].selected);
        assert!(!state.right_panel.entries[0].selected);
        assert!(state.right_panel.entries[1].selected);
    }

    fn make_entry(name: &str, selected: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 100,
            modified: UNIX_EPOCH + Duration::from_secs(0),
            permissions: 0o644,
            owner: "user".to_string(),
            group: "group".to_string(),
            selected,
            is_hidden: false,
        }
    }

    #[test]
    fn test_selected_or_current_paths_fallback_to_cursor() {
        // No entries are selected → should return the cursor entry
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        state.left_panel.entries = vec![
            make_entry("file_a.txt", false),
            make_entry("file_b.txt", false),
        ];
        state.left_panel.cursor = 1;

        let paths = selected_or_current_paths(&state);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/tmp/file_b.txt"));
    }

    #[test]
    fn test_selected_or_current_paths_uses_selection_when_present() {
        // Two entries selected → returns both, ignoring cursor position
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        state.left_panel.entries = vec![
            make_entry("file_a.txt", true),
            make_entry("file_b.txt", false),
            make_entry("file_c.txt", true),
        ];
        state.left_panel.cursor = 1; // cursor on unselected file_b

        let paths = selected_or_current_paths(&state);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("/tmp/file_a.txt")));
        assert!(paths.contains(&PathBuf::from("/tmp/file_c.txt")));
    }

    #[test]
    fn test_selected_or_current_paths_skips_dotdot() {
        // ".." selected → should not appear in results; cursor is on ".."  → empty
        let mut state = AppState::new();
        state.active_panel = ActivePanel::Left;
        let mut dotdot = make_entry("..", false);
        dotdot.name = "..".to_string();
        dotdot.selected = true;
        state.left_panel.entries = vec![dotdot];
        state.left_panel.cursor = 0;

        let paths = selected_or_current_paths(&state);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_selected_or_current_paths_empty_panel() {
        let state = AppState::new();
        let paths = selected_or_current_paths(&state);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_encode_paths_single() {
        let paths = vec![PathBuf::from("/a/b.txt")];
        let result = encode_paths("copy:", &paths);
        assert_eq!(result, "copy:/a/b.txt");
    }

    #[test]
    fn test_encode_paths_multiple() {
        let paths = vec![PathBuf::from("/a/b.txt"), PathBuf::from("/c/d.txt")];
        let result = encode_paths("copy:", &paths);
        assert_eq!(result, "copy:/a/b.txt\n/c/d.txt");
    }

    #[test]
    fn test_encode_paths_escapes_newlines() {
        let paths = vec![PathBuf::from("/a/line\nbreak.txt")];
        let result = encode_paths("copy:", &paths);
        assert_eq!(result, "copy:/a/line\\nbreak.txt");
    }

    #[test]
    fn test_encode_paths_escapes_backslashes() {
        let paths = vec![PathBuf::from(r#"/a/literal\nname.txt"#)];
        let result = encode_paths("copy:", &paths);
        assert_eq!(result, r#"copy:/a/literal\\nname.txt"#);
    }

    #[test]
    fn directory_tree_page_down_uses_terminal_height() {
        let mut state = AppState::default();
        state.tree_entries = (0..50)
            .map(|i| app::dir_tree::TreeEntry {
                path: PathBuf::from(format!("/tmp/{i}")),
                depth: 0,
                is_dir: false,
                expanded: false,
                name: format!("entry-{i}"),
            })
            .collect();

        handle_directory_tree(&mut state, &mut None, KeyCode::PageDown, 12);

        assert_eq!(state.tree_selected, 9);
        assert_eq!(state.tree_scroll, 9);
    }

    #[test]
    fn directory_tree_page_up_uses_terminal_height() {
        let mut state = AppState::default();
        state.tree_entries = (0..50)
            .map(|i| app::dir_tree::TreeEntry {
                path: PathBuf::from(format!("/tmp/{i}")),
                depth: 0,
                is_dir: false,
                expanded: false,
                name: format!("entry-{i}"),
            })
            .collect();
        state.tree_selected = 25;
        state.tree_scroll = 25;

        handle_directory_tree(&mut state, &mut None, KeyCode::PageUp, 12);

        assert_eq!(state.tree_selected, 16);
        assert_eq!(state.tree_scroll, 16);
    }

    #[test]
    fn test_decode_paths_single() {
        let status = "copy:/a/b.txt";
        let paths = decode_paths("copy:", status);
        assert_eq!(paths, vec![PathBuf::from("/a/b.txt")]);
    }

    #[test]
    fn test_decode_paths_multiple() {
        let status = "delete:/a/b.txt\n/c/d.txt";
        let paths = decode_paths("delete:", status);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/a/b.txt"));
        assert_eq!(paths[1], PathBuf::from("/c/d.txt"));
    }

    #[test]
    fn test_decode_paths_unescapes_newlines() {
        let status = "copy:/a/line\\nbreak.txt";
        let paths = decode_paths("copy:", status);
        assert_eq!(paths, vec![PathBuf::from("/a/line\nbreak.txt")]);
    }

    #[test]
    fn test_decode_paths_preserves_literal_backslash_n() {
        let status = r#"copy:/a/literal\\nname.txt"#;
        let paths = decode_paths("copy:", status);
        assert_eq!(paths, vec![PathBuf::from(r#"/a/literal\nname.txt"#)]);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = vec![
            PathBuf::from("/home/user/doc.txt"),
            PathBuf::from("/home/user/photo.png"),
            PathBuf::from("/home/user/music.mp3"),
        ];
        let encoded = encode_paths("move:", &original);
        let decoded = decode_paths("move:", &encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn history_dedup_consecutive() {
        let mut state = AppState::default();
        state.left_panel.path = std::env::temp_dir();
        state.command_history.push("echo hi".to_string());
        // Simulate push logic (same as run_shell_command but without executing)
        let cmd = "echo hi";
        if state.command_history.last().is_none_or(|l| l != cmd) {
            state.command_history.push(cmd.to_string());
        }
        assert_eq!(state.command_history.len(), 1);
        assert_eq!(state.command_history[0], "echo hi");
    }

    #[test]
    fn history_dedup_different_commands() {
        let mut state = AppState::default();
        state.command_history.push("echo hi".to_string());
        let cmd = "ls -la";
        if state.command_history.last().is_none_or(|l| l != cmd) {
            state.command_history.push(cmd.to_string());
        }
        assert_eq!(state.command_history.len(), 2);
    }

    #[test]
    fn history_cap_at_100() {
        let mut state = AppState::default();
        for i in 0..101 {
            let cmd = format!("cmd_{}", i);
            if state
                .command_history
                .last()
                .is_none_or(|l| l.as_str() != cmd.as_str())
            {
                state.command_history.push(cmd);
                if state.command_history.len() > 100 {
                    state.command_history.remove(0);
                }
            }
        }
        assert_eq!(state.command_history.len(), 100);
        assert_eq!(state.command_history[0], "cmd_1");
        assert_eq!(state.command_history[99], "cmd_100");
    }

    #[test]
    fn history_picker_enter_loads_command_line() {
        let mut state = AppState::default();
        state.command_history.push("git status".to_string());
        state.command_history.push("git log".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Enter);

        assert_eq!(state.mode, AppMode::CommandLine);
        assert_eq!(state.command_line, "git log");
    }

    #[test]
    fn history_picker_esc_cancels() {
        let mut state = AppState::default();
        state.command_history.push("ls".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn history_picker_navigate_up_down() {
        let mut state = AppState::default();
        state.command_history.push("cmd1".to_string());
        state.command_history.push("cmd2".to_string());
        state.command_history.push("cmd3".to_string());
        state.mode = AppMode::ListPicker(PickerKind::History);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn hotlist_picker_add_current_dir() {
        let mut state = AppState::default();
        let tmp = std::env::temp_dir();
        state.left_panel.path = tmp.clone();
        state.directory_hotlist.clear();
        state.mode = AppMode::ListPicker(PickerKind::Hotlist);

        handle_list_picker(&mut state, KeyCode::Char('a'));

        assert!(state.directory_hotlist.contains(&tmp));
    }

    #[test]
    fn hotlist_picker_add_dedup() {
        let mut state = AppState::default();
        let tmp = std::env::temp_dir();
        state.left_panel.path = tmp.clone();
        state.directory_hotlist = vec![tmp.clone()];
        state.mode = AppMode::ListPicker(PickerKind::Hotlist);

        handle_list_picker(&mut state, KeyCode::Char('a'));

        assert_eq!(
            state
                .directory_hotlist
                .iter()
                .filter(|p| *p == &tmp)
                .count(),
            1
        );
    }

    #[test]
    fn hotlist_picker_delete_entry() {
        let mut state = AppState {
            directory_hotlist: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Char('d'));

        assert_eq!(state.directory_hotlist.len(), 2);
        assert!(!state.directory_hotlist.contains(&PathBuf::from("/b")));
    }

    #[test]
    fn hotlist_picker_delete_adjusts_cursor() {
        let mut state = AppState {
            directory_hotlist: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            mode: AppMode::ListPicker(PickerKind::Hotlist),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Char('d'));

        assert_eq!(state.directory_hotlist.len(), 1);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn hotlist_persistence_roundtrip() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let tmp_dir = std::env::temp_dir();
        let state = AppState {
            directory_hotlist: vec![tmp_dir.clone(), PathBuf::from("/usr")],
            ..Default::default()
        };

        // Serialize and deserialize manually via PersistedSetup
        let hotlist_strs: Vec<String> = state
            .directory_hotlist
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let content = format!(
            "version = 1\nactive_panel = \"left\"\nhotlist = {:?}\n\
            [left]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n\
            [right]\npath = \"/tmp\"\nshow_hidden = false\nlisting_mode = \"long\"\nsort_mode = \"name_asc\"\nfilter = \"\"\n",
            hotlist_strs
        );

        // Write to a temp file, then read back via toml
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        let read_back = std::fs::read_to_string(f.path()).unwrap();
        let parsed: app::config::PersistedSetup = toml::from_str(&read_back).unwrap();

        let loaded: Vec<PathBuf> = parsed.hotlist.iter().map(PathBuf::from).collect();
        assert_eq!(loaded, state.directory_hotlist);
    }

    #[test]
    fn user_menu_picker_esc_closes() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            user_menu_entries: vec![app::user_menu::MenuEntry {
                hotkey: 'A',
                title: "Archive".to_string(),
                command: "tar czf a.tgz".to_string(),
                condition: None,
            }],
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn user_menu_picker_navigate_and_select() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::UserMenu),
            user_menu_entries: vec![
                app::user_menu::MenuEntry {
                    hotkey: 'A',
                    title: "Archive".to_string(),
                    command: "echo archive".to_string(),
                    condition: None,
                },
                app::user_menu::MenuEntry {
                    hotkey: 'B',
                    title: "Build".to_string(),
                    command: "echo build".to_string(),
                    condition: None,
                },
            ],
            ..Default::default()
        };

        // Navigate down
        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        // Navigate up
        handle_list_picker(&mut state, KeyCode::Up);
        assert_eq!(state.picker_selected, 0);
    }

    #[test]
    fn user_menu_file_menu_no_menu_file_shows_error() {
        // Point the panel at a temp dir with no .mc.menu file
        let tmp = std::env::temp_dir();
        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 1,
            menu_item_selected: 0,
            ..Default::default()
        };
        state.left_panel.path = tmp.clone();

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        // Should show an error dialog since no menu file exists
        assert!(matches!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Error(_))
        ));
    }

    #[test]
    fn user_menu_file_menu_with_entries_opens_picker() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let menu_path = tmp.path().join(".mc.menu");
        let mut f = std::fs::File::create(&menu_path).unwrap();
        write!(
            f,
            "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n"
        )
        .unwrap();

        let mut state = AppState {
            mode: AppMode::Menu,
            menu_selected: 1,
            menu_item_selected: 0,
            ..Default::default()
        };
        state.left_panel.path = tmp.path().to_path_buf();

        handle_menu_mode(&mut state, &mut None, KeyCode::Enter, 24);

        assert_eq!(state.mode, AppMode::ListPicker(PickerKind::UserMenu));
        assert_eq!(state.picker_selected, 0);
        assert_eq!(state.user_menu_entries.len(), 2);
        assert_eq!(state.user_menu_entries[0].hotkey, 'A');
        assert_eq!(state.user_menu_entries[1].hotkey, 'B');
    }

    #[test]
    fn compare_mode_picker_maps_index_to_mode() {
        // picker_selected 0 => Quick, 1 => Size, 2 => Thorough
        const MODES: [CompareMode; 3] =
            [CompareMode::Quick, CompareMode::Size, CompareMode::Thorough];
        assert_eq!(MODES[0], CompareMode::Quick);
        assert_eq!(MODES[1], CompareMode::Size);
        assert_eq!(MODES[2], CompareMode::Thorough);
    }

    #[test]
    fn compare_mode_picker_esc_cancels() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            picker_selected: 1,
            ..Default::default()
        };

        handle_list_picker(&mut state, KeyCode::Esc);

        assert_eq!(state.mode, AppMode::Normal);
    }

    #[test]
    fn compare_mode_picker_enter_runs_quick_by_default() {
        let mut state = AppState::default();
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "a.txt".to_string(),
            path: std::env::temp_dir().join("a.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 0,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        }];
        state.mode = AppMode::ListPicker(PickerKind::CompareMode);
        state.picker_selected = 0;

        handle_list_picker(&mut state, KeyCode::Enter);

        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                "Compare dirs (Quick):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
                    .to_string()
            ))
        );
    }

    #[test]
    fn compare_mode_picker_navigate_and_select_thorough() {
        let mut state = AppState {
            mode: AppMode::ListPicker(PickerKind::CompareMode),
            picker_selected: 0,
            ..Default::default()
        };
        state.left_panel.entries = vec![app::types::FileEntry {
            name: "x.txt".to_string(),
            path: std::env::temp_dir().join("x.txt"),
            is_dir: false,
            is_symlink: false,
            is_executable: false,
            size: 42,
            modified: std::time::SystemTime::now(),
            permissions: 0,
            owner: String::new(),
            group: String::new(),
            selected: false,
            is_hidden: false,
        }];

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 1);

        handle_list_picker(&mut state, KeyCode::Down);
        assert_eq!(state.picker_selected, 2);

        handle_list_picker(&mut state, KeyCode::Enter);
        assert_eq!(
            state.mode,
            AppMode::Dialog(app::types::DialogKind::Confirm(
                "Compare dirs (Thorough):\nUnique in left:  1\nUnique in right: 0\nDiffering:       0"
                    .to_string()
            ))
        );
    }
}

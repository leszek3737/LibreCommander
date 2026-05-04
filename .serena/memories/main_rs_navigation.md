# `src/main.rs` — Section Map

`main.rs` is ~3067 lines. **Do NOT read it linearly.** Use `find_symbol` with
the function name from this map, or `get_symbols_overview` for an up-to-date list.

Approximate line ranges (current at time of writing — verify before relying):

| Line  | Section                                                        |
|-------|----------------------------------------------------------------|
|   21  | `mod input;` declaration                                       |
|   36  | `struct TerminalGuard` + `Drop` impl                           |
|   44  | `install_panic_hook`                                           |
|   52  | `enter_tui_stdout` / `leave_tui_stdout` / suspend/resume       |
|   80  | `terminal_state_file_path`                                     |
|   85  | `fn main`                                                      |
|  104  | **`fn run_app`** — main event loop (longest function)          |
|  265  | `file_names_from_paths`                                        |
|  276  | `sync_watcher_job_state`                                       |
|  325  | `update_panel_read_errors`                                     |
|  341  | `current_panel_entry_name` / `selected_panel_paths`            |
|  358  | `filtered_sorted_entries`                                      |
|  380  | panel selection / cursor / scroll restore + clamp              |
|  428  | `set_active_panel`                                             |
|  448  | **`fn render_ui`** — top-level UI renderer (large)             |
|  608  | `to_ui_dialog` — map domain DialogKind → ui::dialogs::DialogKind |
|  706  | `handle_directory_tree`                                        |
|  808  | `directory_tree_visible_height` / `panel_visible_height`       |
|  837  | `shift_select` — range selection                               |
|  854  | `navigate_to_hotlist`                                          |
| 1242  | `handle_viewer_mode`                                           |
| 1286  | `handle_command_line`                                          |
| 1337  | `parse_octal_mode`                                             |
| 1342  | `selected_or_current_paths`                                    |
| 1364  | `dismiss_dialog_and_restore`                                   |
| 1382  | `handle_confirm_dialog`                                        |
| 1433  | `handle_input_dialog`                                          |
| 1640  | `handle_error_dialog`                                          |
| 1647  | `handle_progress_dialog`                                       |
| 1657  | `handle_properties_dialog`                                     |
| 1664  | `handle_copymove_dialog`                                       |
| 1670  | **`fn handle_dialog`** — dialog event dispatcher               |
| 1776  | `handle_list_picker`                                           |
| 1924  | `apply_search_filter`                                          |
| 1936  | `handle_search_mode`                                           |
| 1977  | `run_selected_menu_action`                                     |
| 1999  | `handle_menu_mode`                                             |
| 2049  | `compare_directories`                                          |
| 2074  | `#[cfg(test)] mod tests`                                       |

## How to navigate

- For one function: `find_symbol({name_path: "run_app", relative_path: "src/main.rs", include_body: true})`
- For shape only: `get_symbols_overview({relative_path: "src/main.rs"})`
- For "who calls X": `find_referencing_symbols({name_path: "X", relative_path: "src/main.rs"})`

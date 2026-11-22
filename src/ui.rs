use crate::app::{App, AppMode, BucketSubmenu, HistoryState, SearchKind, SearchState};
use crate::config::resolve_target_dir;
use crate::search::truncate_path_middle;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use std::path::Path;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    let main_area = chunks[0];
    let status_area = chunks[1];

    match &app.mode {
        AppMode::Home => draw_home(frame, app, main_area),
        AppMode::ContentScopeSelect => draw_content_scope(frame, app, main_area),
        AppMode::Search(st) => draw_search(frame, app, st, main_area),
        AppMode::History(st) => draw_history(frame, st, main_area),
        AppMode::FileNotFoundPrompt { path } => draw_file_not_found(frame, path, main_area),
        AppMode::ActionMenu => draw_action_menu(frame, app, main_area),
        AppMode::CopySubmenu => draw_copy_submenu(frame, app, main_area),
        AppMode::PathSubmenu { from_bucket } => {
            draw_path_submenu(frame, app, *from_bucket, main_area)
        }
        AppMode::OpenSubmenu => draw_open_submenu(frame, app, main_area),
        AppMode::Bucket(sub) => draw_bucket(frame, app, sub, main_area),
    }

    draw_status_bar(frame, app, status_area);
}

fn draw_home(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.active_project {
        Some(proj) => format!(
            " File Scout Context v0.1.0  |  Project: {} ({}) ",
            proj.name, proj.path
        ),
        None => " File Context Scout v0.1.0 ".to_string(),
    };

    let bucket_hint = if app.bucket.is_empty() {
        "Open the persistent Bucket staging workspace".to_string()
    } else {
        format!(
            "Open the persistent Bucket staging workspace ({} staged)",
            app.bucket.len()
        )
    };

    let recents_hint = if app.active_project.is_some() {
        "Search recently modified files in project (newest first)"
    } else {
        "Search recently modified files in search_roots (newest first)"
    };

    let mut lines = vec![
        Line::from(""),
        menu_row("f", "Find File", "Fuzzy search files via fzf / ripgrep"),
        menu_row("d", "Find Directory", "Fuzzy search system directories"),
        menu_row("c", "Find Content", "Fuzzy search file contents via rg + fzf"),
        menu_row("r", "Recents", recents_hint),
        menu_row("h", "History", "Browse persistent file history (1-9)"),
        menu_row("p", "Projects", "Search strictly within configured roots"),
        menu_row("b", "Bucket", &bucket_hint),
    ];

    lines.push(Line::from(""));
    let footer = if app.active_project.is_some() {
        "   [Esc] Clear Project Scope   [q] Quit   [Ctrl+C] Exit"
    } else {
        "   [q] Quit   [Ctrl+C] Exit"
    };
    lines.push(Line::from(Span::styled(
        footer,
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn menu_row<'a>(key: &'a str, label: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::raw("   "),
        Span::styled(
            format!("[{}]", key),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("   {:<18}", label)),
        Span::styled(desc, Style::default().fg(Color::Gray)),
    ])
}

fn draw_content_scope(frame: &mut Frame, app: &App, area: Rect) {
    let roots = app.active_search_roots();
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "   Select directory scope for Find Content (rg + fzf):",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        menu_row("0", "All Directories", "Search across all configured search_roots"),
    ];

    for (idx, root) in roots.iter().take(9).enumerate() {
        let num = (idx + 1).to_string();
        let display = root.display().to_string();
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!("[{}]", num),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(display, Style::default().fg(Color::Cyan)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   [Esc] Cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" Find Content — Scope Selection ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn draw_search(frame: &mut Frame, app: &App, st: &SearchState, area: Rect) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    let input_area = vertical[0];
    let body_area = vertical[1];

    let (title, hint) = match &st.kind {
        SearchKind::FindFile => (
            " Find File (fzf) ".to_string(),
            " [↑/↓] Navigate  [Enter] Select  [Tab] Preview Controls  [Esc] Back ",
        ),
        SearchKind::FindDirectory => (
            " Find Directory (fzf) ".to_string(),
            " [↑/↓] Navigate  [Enter] Select  [Tab] Preview Controls  [Esc] Back ",
        ),
        SearchKind::FindContent { .. } => (
            " Find Content (rg + fzf, min 3 chars) ".to_string(),
            " [↑/↓] Navigate  [Enter] Select  [Tab] Preview Mode ([1]↑ [2]↓ [t]Top [b]Bot) ",
        ),
        SearchKind::Recents => (
            " Recents — Last Modified (Newest First, fzf) ".to_string(),
            " [↑/↓] Navigate  [Enter] Select  [Tab] Preview Controls  [Esc] Back ",
        ),
        SearchKind::Projects => (
            " Projects (projects.json + fzf) ".to_string(),
            " [↑/↓] Navigate  [Enter] Set Scope  [Esc] Back ",
        ),
        SearchKind::StrictDirectoryTarget { intent } => (
            format!(" {} — Strict Directory Target (fzf --exact) ", intent.label()),
            " [Enter] Move/mkdir -p  [Ctrl+O] Override (Cut↔Copy)  [Esc] Back ",
        ),
    };

    let input_border_color = if st.preview_mode {
        Color::DarkGray
    } else {
        Color::Yellow
    };

    let loading_tag = if st.loading { " [searching...]" } else { "" };
    let input_text = format!("> {}{}", st.query, loading_tag);
    let input_widget = Paragraph::new(input_text).block(
        Block::default()
            .title(title)
            .title_bottom(hint)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(input_border_color)),
    );
    frame.render_widget(input_widget, input_area);

    // Split layout: results pane on the left, preview pane on the right
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(body_area);

    let results_area = horizontal[0];
    let preview_area = horizontal[1];

    // Render Results Pane
    let mut items: Vec<ListItem> = Vec::new();
    if matches!(st.kind, SearchKind::FindContent { .. }) && st.query.trim().chars().count() < 3 {
        items.push(ListItem::new(Span::styled(
            "Type at least 3 characters to run ripgrep + fzf...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if st.results.is_empty() {
        if let SearchKind::StrictDirectoryTarget { .. } = &st.kind {
            if !st.query.trim().is_empty() {
                let resolved = resolve_target_dir(&st.query, &app.active_search_roots());
                items.push(ListItem::new(Span::styled(
                    "No exact matching directory found.",
                    Style::default().fg(Color::Yellow),
                )));
                items.push(ListItem::new(Span::styled(
                    format!("Press Enter to mkdir -p: {}", resolved.display()),
                    Style::default().fg(Color::Green),
                )));
            } else if st.loading {
                items.push(ListItem::new(Span::styled(
                    "Loading directories...",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                items.push(ListItem::new(Span::styled(
                    "Type a directory path...",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else if st.loading {
            items.push(ListItem::new(Span::styled(
                "Loading...",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            items.push(ListItem::new(Span::styled(
                "No matches found.",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        // Compute scroll window so selected_idx is always visible
        let visible_height = results_area.height.saturating_sub(2) as usize;
        let offset = if visible_height > 0 && st.selected_idx >= visible_height {
            st.selected_idx - visible_height + 1
        } else {
            0
        };

        for (i, res) in st
            .results
            .iter()
            .enumerate()
            .skip(offset)
            .take(visible_height.max(1))
        {
            let is_selected = i == st.selected_idx;
            let raw_path = res.path.display().to_string();
            let truncated = truncate_path_middle(&raw_path, 40);

            let label = match (&res.project_name, res.line_number, &res.snippet) {
                (Some(proj), _, _) => format!("{}  ({})", proj, truncated),
                (None, Some(ln), Some(snip)) => format!("{}:{}  {}", truncated, ln, snip),
                _ => truncated,
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            items.push(ListItem::new(Span::styled(
                format!("{}{}", prefix, label),
                style,
            )));
        }
    }

    let results_block = Block::default()
        .title(format!(" Results ({}) ", st.results.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let results_list = List::new(items).block(results_block);
    frame.render_widget(results_list, results_area);

    // Render Preview Pane (right side) with syntax highlighting
    let preview_border_color = if st.preview_mode {
        Color::Yellow
    } else {
        Color::Blue
    };

    let preview_title = match &st.preview_path {
        Some(p) => {
            let mode_tag = if st.preview_mode {
                " [PREVIEW ACTIVE: 1↑ 2↓ t:Top b:Bot] "
            } else {
                " [Tab: Preview Controls] "
            };
            format!(" Preview: {} {}", truncate_path_middle(&p.display().to_string(), 32), mode_tag)
        }
        None => " Preview ".to_string(),
    };

    let preview_height = preview_area.height.saturating_sub(2) as usize;
    let ext = st
        .preview_path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let preview_lines: Vec<Line> = if st.preview_lines.is_empty() {
        vec![Line::from(Span::styled(
            "No preview available.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        st.preview_lines
            .iter()
            .enumerate()
            .skip(st.preview_scroll)
            .take(preview_height.max(1))
            .map(|(idx, raw_line)| {
                let line_no = idx + 1;
                let is_match = st.preview_highlight_line == Some(line_no);
                let num_style = if is_match {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let mut spans = vec![Span::styled(format!("{:>4} │ ", line_no), num_style)];
                if is_match {
                    spans.push(Span::styled(
                        raw_line.clone(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.extend(highlight_line(raw_line, ext));
                }
                Line::from(spans)
            })
            .collect()
    };

    let preview_widget = Paragraph::new(preview_lines).block(
        Block::default()
            .title(preview_title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(preview_border_color)),
    );
    frame.render_widget(preview_widget, preview_area);
}

fn draw_history(frame: &mut Frame, st: &HistoryState, area: Rect) {
    let mut items = Vec::new();
    if st.entries.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No history entries yet.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (idx, path) in st.entries.iter().enumerate() {
            let is_cur = idx == st.cursor;
            let exists_tag = if path.exists() { "" } else { " [missing]" };
            let text = format!("  [{}] {}{}", idx + 1, path.display(), exists_tag);
            let style = if is_cur {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if !path.exists() {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            };
            items.push(ListItem::new(Span::styled(text, style)));
        }
    }

    let list = List::new(items).block(
        Block::default()
            .title(" History (Persistent, 1-9) ")
            .title_bottom(" [1-9] Quick Select  [↑/↓ + Enter] Select  [Esc] Home ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(list, area);
}

fn draw_file_not_found(frame: &mut Frame, path: &Path, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "   File does not exist",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("   Path: {}", path.display()),
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "   Press [h] for History, [r] for Recents, or [Esc] / any key to return Home.",
            Style::default().fg(Color::Yellow),
        )),
    ];

    let block = Block::default()
        .title(" Notice ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_action_menu(frame: &mut Frame, app: &App, area: Rect) {
    let ctx_str = app
        .active_context
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<None>".to_string());

    let is_dir = app
        .active_context
        .as_ref()
        .map(|p| p.is_dir())
        .unwrap_or(false);

    let paste_span = if is_dir {
        Span::styled(
            "[p] Paste        ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("                 ")
    };

    let add_indicator = if app.additionally_active {
        Span::styled(
            "  [ADDITIONALLY: ON — context will be preserved]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "  [a] Additionally (chain next action)",
            Style::default().fg(Color::DarkGray),
        )
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " Active File Context: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(ctx_str, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "[m] Move        ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[c] Copy Menu    ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[x] Cut          ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[e] Edit",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "[b] Bucket      ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[o] Open Menu    ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            paste_span,
            Span::styled(
                "[h] Home",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(add_indicator),
    ];

    if let Some(pending) = &app.pending_operation {
        lines.push(Line::from(Span::styled(
            format!(
                "  Pending {}: {}  ([Ctrl+O] Override: swap Cut ↔ Copy)",
                pending.label(),
                pending.path().display()
            ),
            Style::default().fg(Color::Magenta),
        )));
    }

    let block = Block::default()
        .title(" ACTION MENU ")
        .title_bottom(" [Esc] Home ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_copy_submenu(frame: &mut Frame, app: &App, area: Rect) {
    let ctx = app
        .active_context
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let lines = vec![
        Line::from(Span::styled(
            format!("   Target: {}", ctx),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        menu_row("n", "Copy File Name", "Copy only the filename string to clipboard"),
        menu_row("c", "Copy Contents", "Copy plaintext file contents to clipboard"),
        menu_row("f", "Copy File", "Copy native file reference to macOS NSPasteboard"),
        menu_row("d", "Copy Dir Path", "Open Path Submenu (Relative / Absolute)"),
        Line::from(""),
        Line::from(Span::styled(
            "   [a] Toggle Additionally   [Esc] Back to Action Menu",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" COPY SUBMENU ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_path_submenu(frame: &mut Frame, _app: &App, from_bucket: bool, area: Rect) {
    let subtitle = if from_bucket {
        "Bucket Path Submenu"
    } else {
        "Path Submenu"
    };
    let lines = vec![
        Line::from(""),
        menu_row("r", "Relative Path", "Copy path relative to active project root (or $HOME)"),
        menu_row("a", "Absolute Path", "Copy full canonical path (also [f])"),
        Line::from(""),
        Line::from(Span::styled(
            "   [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(format!(" {} ", subtitle))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_open_submenu(frame: &mut Frame, app: &App, area: Rect) {
    let ctx = app
        .active_context
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let lines = vec![
        Line::from(Span::styled(
            format!("   Target: {}", ctx),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        menu_row(
            "f",
            "Open File",
            "Plaintext -> configured editor; binary -> default macOS app",
        ),
        menu_row("d", "Open Directory", "Open containing folder in Finder"),
        menu_row("q", "Quick Look", "Preview file via macOS qlmanage"),
        Line::from(""),
        Line::from(Span::styled(
            "   [Esc] Back to Action Menu",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" OPEN SUBMENU ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_bucket(frame: &mut Frame, app: &App, sub: &BucketSubmenu, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(5)])
        .split(area);

    let list_area = chunks[0];
    let actions_area = chunks[1];

    let mut items = Vec::new();
    if app.bucket.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  Bucket is empty. Select a file and press [b] in Action Menu to stage it here.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (idx, path) in app.bucket.items.iter().enumerate() {
            let is_cur = idx == app.bucket.cursor;
            let is_sel = app.bucket.selected.contains(&idx);
            let check = if is_sel { "[x]" } else { "[ ]" };
            let num_label = if idx < 9 {
                format!("[{}]", idx + 1)
            } else {
                "   ".to_string()
            };
            let text = format!("  {} {} {}", num_label, check, path.display());
            let style = if is_cur {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            items.push(ListItem::new(Span::styled(text, style)));
        }
    }

    let list = List::new(items).block(
        Block::default()
            .title(format!(
                " THE BUCKET ({} staged, {} selected) ",
                app.bucket.len(),
                app.bucket.selected.len()
            ))
            .title_bottom(" [1-9] Jump  [↑/↓] Navigate  [Space] Multi-Select ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(list, list_area);

    let action_lines = match sub {
        BucketSubmenu::Main => vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[c] Copy Menu    ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[o] Open Menu    ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[a] Archive (Clear)    ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[h] Home", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled(
                format!("  Staging dir: {}", app.bucket.staging_dir().display()),
                Style::default().fg(Color::DarkGray),
            )),
        ],
        BucketSubmenu::Copy => vec![
            Line::from(vec![
                Span::raw("  COPY: "),
                Span::styled("[f] Copy File (NSPasteboard)   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[c] Copy Contents   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[d] Copy Dir Path   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[Esc] Back", Style::default().fg(Color::DarkGray)),
            ]),
        ],
        BucketSubmenu::Open => vec![
            Line::from(vec![
                Span::raw("  OPEN: "),
                Span::styled("[f] Finder   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[o] Open File   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[d] Open Bucket Staging Dir   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[Esc] Back", Style::default().fg(Color::DarkGray)),
            ]),
        ],
    };

    let actions_block = Block::default()
        .title(" Bucket Actions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Clear, actions_area);
    frame.render_widget(Paragraph::new(action_lines).block(actions_block), actions_area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut parts: Vec<Span> = Vec::new();

    if let Some(pending) = &app.pending_operation {
        parts.push(Span::styled(
            format!(" {} ", pending.label()),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
        parts.push(Span::raw(" "));
    }

    if app.additionally_active {
        parts.push(Span::styled(
            " +ADDITIONALLY ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        parts.push(Span::raw(" "));
    }

    let status = app
        .status_message
        .as_deref()
        .unwrap_or("Ready — Press a keybinding to begin");
    parts.push(Span::styled(status, Style::default().fg(Color::White)));

    let bar = Paragraph::new(Line::from(parts)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(bar, area);
}

/// Lightweight terminal syntax highlighting for the split-pane preview.
fn highlight_line(line: &str, ext: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    // Comments
    if trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("--")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
    {
        return vec![Span::styled(
            line.to_string(),
            Style::default().fg(Color::DarkGray),
        )];
    }

    let keywords: &[&str] = match ext {
        "rs" => &[
            "fn", "let", "mut", "pub", "struct", "enum", "impl", "use", "mod", "match", "if",
            "else", "for", "while", "loop", "return", "async", "await", "const", "type", "trait",
            "where", "self", "Self", "true", "false",
        ],
        "js" | "jsx" | "ts" | "tsx" => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while", "import",
            "export", "from", "default", "class", "interface", "type", "async", "await", "new",
            "true", "false", "null", "undefined",
        ],
        "py" => &[
            "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while",
            "with", "as", "try", "except", "async", "await", "True", "False", "None", "pass",
        ],
        "json" => &["true", "false", "null"],
        _ => &[
            "if", "else", "for", "while", "return", "fn", "function", "def", "class", "import",
            "true", "false",
        ],
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = line.chars().peekable();
    let mut buf = String::new();

    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let quote = c;
            let mut s = String::from(quote);
            let mut escaped = false;
            for sc in chars.by_ref() {
                s.push(sc);
                if escaped {
                    escaped = false;
                } else if sc == '\\' {
                    escaped = true;
                } else if sc == quote {
                    break;
                }
            }
            spans.push(Span::styled(s, Style::default().fg(Color::Green)));
        } else if c.is_ascii_alphanumeric() || c == '_' {
            if !buf.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut buf)));
            }
            let mut word = String::from(c);
            while let Some(&nc) = chars.peek() {
                if nc.is_ascii_alphanumeric() || nc == '_' {
                    word.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if keywords.contains(&word.as_str()) {
                spans.push(Span::styled(
                    word,
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ));
            } else if word.chars().all(|ch| ch.is_ascii_digit()) {
                spans.push(Span::styled(word, Style::default().fg(Color::Yellow)));
            } else if word
                .chars()
                .next()
                .map(|ch| ch.is_ascii_uppercase())
                .unwrap_or(false)
            {
                spans.push(Span::styled(word, Style::default().fg(Color::Cyan)));
            } else {
                spans.push(Span::raw(word));
            }
        } else {
            buf.push(c);
        }
    }

    if !buf.is_empty() {
        spans.push(Span::raw(buf));
    }

    spans
}

use std::cmp::{max, min};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::config::AppConfig;

const DEFAULT_SCROLLBACK: usize = 2000;
const TAB_WIDTH: usize = 8;

#[derive(Clone, Copy)]
enum ParseState {
    Ground,
    Escape,
    Csi,
    Osc,
    Dcs,
    CharsetG0,
    CharsetG1,
}

#[derive(Clone, Copy)]
enum OscColorTarget {
    Foreground,
    Background,
    Cursor,
}

#[derive(Clone, Copy)]
enum Charset {
    Ascii,
    DecSpecial,
}

#[derive(Clone, Copy)]
struct SavedCursor {
    row: usize,
    col: usize,
    fg: u32,
    bg: u32,
    g0_charset: Charset,
    g1_charset: Charset,
    use_g1_charset: bool,
    origin_mode: bool,
    autowrap: bool,
    insert_mode: bool,
}

pub enum ClipboardCommand {
    Set(String),
    Query,
}

#[derive(Clone)]
pub struct Cell {
    pub span: usize,
    pub wide_continuation: bool,
    pub fg: u32,
    pub bg: u32,
    pub text: String,
}

impl Cell {
    fn blank(fg: u32, bg: u32) -> Self {
        Self {
            span: 1,
            wide_continuation: false,
            fg,
            bg,
            text: " ".to_string(),
        }
    }
}

#[derive(Clone)]
struct ScreenState {
    screen: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    scroll_top: usize,
    scroll_bottom: usize,
    origin_mode: bool,
    autowrap: bool,
    insert_mode: bool,
}

impl ScreenState {
    fn new(cols: usize, rows: usize, fg: u32, bg: u32) -> Self {
        Self {
            screen: blank_screen(cols, rows, fg, bg),
            cursor_row: 0,
            cursor_col: 0,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            origin_mode: false,
            autowrap: true,
            insert_mode: false,
        }
    }
}

pub struct TerminalBuffer {
    title: String,
    banner: Vec<String>,
    main: ScreenState,
    alternate: ScreenState,
    scrollback: Vec<Vec<Cell>>,
    cols: usize,
    rows: usize,
    using_alternate: bool,
    saved_cursor: Option<SavedCursor>,
    parse_state: ParseState,
    csi_buf: String,
    osc_buf: String,
    osc_escape: bool,
    dcs_buf: String,
    dcs_escape: bool,
    current_fg: u32,
    current_bg: u32,
    default_fg: u32,
    default_bg: u32,
    cursor_style: usize,
    cursor_visible: bool,
    application_cursor_keys: bool,
    mouse_reporting: bool,
    mouse_motion: bool,
    mouse_sgr: bool,
    alternate_scroll: bool,
    focus_reporting: bool,
    bracketed_paste: bool,
    g0_charset: Charset,
    g1_charset: Charset,
    use_g1_charset: bool,
    command_mode: bool,
    command_input: String,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    scroll_offset: usize,
    status: String,
    shell_label: String,
    cwd_label: String,
    outbound: Vec<Vec<u8>>,
    clipboard_commands: Vec<ClipboardCommand>,
}

impl TerminalBuffer {
    pub fn new(config: &AppConfig) -> Self {
        let cols = 80;
        let rows = 24;
        let mut terminal = Self {
            title: config.title.clone(),
            banner: config.banner.clone(),
            main: ScreenState::new(cols, rows, config.foreground, config.background),
            alternate: ScreenState::new(cols, rows, config.foreground, config.background),
            scrollback: Vec::new(),
            cols,
            rows,
            using_alternate: false,
            saved_cursor: None,
            parse_state: ParseState::Ground,
            csi_buf: String::new(),
            osc_buf: String::new(),
            osc_escape: false,
            dcs_buf: String::new(),
            dcs_escape: false,
            current_fg: config.foreground,
            current_bg: config.background,
            default_fg: config.foreground,
            default_bg: config.background,
            cursor_style: 1,
            cursor_visible: true,
            application_cursor_keys: false,
            mouse_reporting: false,
            mouse_motion: false,
            mouse_sgr: false,
            alternate_scroll: false,
            focus_reporting: false,
            bracketed_paste: false,
            g0_charset: Charset::Ascii,
            g1_charset: Charset::Ascii,
            use_g1_charset: false,
            command_mode: false,
            command_input: String::new(),
            command_history: Vec::new(),
            command_history_index: None,
            scroll_offset: 0,
            status: String::new(),
            shell_label: String::new(),
            cwd_label: String::new(),
            outbound: Vec::new(),
            clipboard_commands: Vec::new(),
        };
        for line in &config.banner {
            terminal.push_output(line);
        }
        terminal
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = max(cols, 1);
        let rows = max(rows, 1);
        self.main = resize_screen(&self.main, cols, rows, self.default_fg, self.default_bg);
        self.alternate =
            resize_screen(&self.alternate, cols, rows, self.default_fg, self.default_bg);
        self.cols = cols;
        self.rows = rows;
        self.scroll_offset = min(self.scroll_offset, self.scrollback.len());
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn in_command_mode(&self) -> bool {
        self.command_mode
    }

    pub fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
    }

    pub fn mouse_reporting_enabled(&self) -> bool {
        self.mouse_reporting
    }

    pub fn mouse_motion_enabled(&self) -> bool {
        self.mouse_motion
    }

    pub fn mouse_sgr_enabled(&self) -> bool {
        self.mouse_sgr
    }

    pub fn alternate_scroll_enabled(&self) -> bool {
        self.alternate_scroll
    }

    pub fn focus_reporting_enabled(&self) -> bool {
        self.focus_reporting
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste
    }

    pub fn cursor_style(&self) -> usize {
        self.cursor_style
    }

    pub fn take_outbound(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.outbound)
    }

    pub fn take_clipboard_commands(&mut self) -> Vec<ClipboardCommand> {
        std::mem::take(&mut self.clipboard_commands)
    }

    pub fn respond_clipboard_query(&mut self, text: &str) {
        let encoded = BASE64.encode(text.as_bytes());
        self.outbound
            .push(format!("\x1b]52;c;{}\x07", encoded).into_bytes());
    }

    pub fn render_cells(&self) -> Vec<Vec<Cell>> {
        let mut rows = if self.using_alternate {
            self.alternate.screen.clone()
        } else {
            let mut combined = self.scrollback.clone();
            combined.extend(self.main.screen.clone());
            let total_rows = combined.len();
            let end = total_rows.saturating_sub(self.scroll_offset);
            let start = end.saturating_sub(self.rows);
            combined[start..end].to_vec()
        };
        while rows.len() < self.rows {
            rows.insert(0, vec![Cell::blank(self.default_fg, self.default_bg); self.cols]);
        }
        if self.command_mode && !rows.is_empty() {
            let overlay = format!(":{}", self.command_input);
            let last = rows.len() - 1;
            rows[last].fill(Cell::blank(self.default_fg, self.default_bg));
            for (idx, ch) in overlay.chars().take(self.cols).enumerate() {
                rows[last][idx] = Cell {
                    span: 1,
                    text: ch.to_string(),
                    fg: self.default_fg,
                    bg: self.default_bg,
                    wide_continuation: false,
                };
            }
        }
        rows
    }

    pub fn selected_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let rows = self.render_cells();
        if rows.is_empty() {
            return String::new();
        }
        let (start, end) = normalize_selection(start, end);
        let mut out = String::new();
        for row in start.0..=end.0.min(rows.len().saturating_sub(1)) {
            let row_cells = &rows[row];
            let start_col = if row == start.0 { start.1 } else { 0 };
            let end_col = if row == end.0 {
                end.1.min(row_cells.len().saturating_sub(1))
            } else {
                row_cells.len().saturating_sub(1)
            };
            for col in start_col..=end_col {
                if !row_cells[col].wide_continuation {
                    out.push_str(&row_cells[col].text);
                }
            }
            if row != end.0 {
                out.push('\n');
            }
        }
        out
    }

    pub fn selection_cell(&self, row: usize, col: usize, clamp_to_content: bool) -> Option<(usize, usize)> {
        let rows = self.render_cells();
        let row_cells = rows.get(row)?;
        let last_meaningful = row_cells
            .iter()
            .enumerate()
            .rfind(|(_, cell)| !cell.wide_continuation && !cell.text.trim().is_empty())
            .map(|(idx, _)| idx)?;
        if col <= last_meaningful {
            Some((row, col))
        } else if clamp_to_content {
            Some((row, last_meaningful))
        } else {
            None
        }
    }

    pub fn cursor_for_render(&self) -> (usize, usize) {
        let active = self.active_screen();
        if self.scroll_offset > 0 && !self.command_mode {
            return (self.rows.saturating_sub(1), self.cols.saturating_sub(1));
        }
        if self.command_mode {
            let row = self.rows.saturating_sub(1);
            let col = min(self.command_input.chars().count() + 1, self.cols.saturating_sub(1));
            (row, col)
        } else {
            (
                min(active.cursor_row, self.rows.saturating_sub(1)),
                min(active.cursor_col, self.cols.saturating_sub(1)),
            )
        }
    }

    pub fn cursor_visible_for_render(&self) -> bool {
        self.cursor_visible && (self.scroll_offset == 0 || self.command_mode)
    }

    pub fn set_footer_context(&mut self, shell_label: impl Into<String>, cwd_label: impl Into<String>) {
        self.shell_label = shell_label.into();
        self.cwd_label = cwd_label.into();
    }

    pub fn status_line_left(&self) -> String {
        let mut parts = Vec::new();
        if !self.status.is_empty() {
            parts.push(self.status.clone());
        }
        if self.command_mode {
            parts.push("Command mode".to_string());
        }
        if self.scroll_offset > 0 {
            parts.push(format!("Viewing scrollback ({})", self.scroll_offset));
        }
        if self.using_alternate {
            parts.push("Full-screen app".to_string());
        }
        parts.join("  •  ")
    }

    pub fn status_line_right(&self) -> String {
        let mut parts = Vec::new();
        if !self.cwd_label.is_empty() {
            parts.push(self.cwd_label.clone());
        }
        if !self.shell_label.is_empty() {
            parts.push(self.shell_label.clone());
        }
        if self.mouse_reporting {
            parts.push("Mouse".to_string());
        }
        parts.join("  •  ")
    }

    pub fn enter_command_mode(&mut self) {
        self.command_mode = true;
        self.command_input.clear();
        self.command_history_index = None;
    }

    pub fn handle_command_text(&mut self, text: &str) {
        self.command_input.push_str(text);
        self.command_history_index = None;
    }

    pub fn backspace(&mut self) {
        self.command_input.pop();
        self.command_history_index = None;
    }

    pub fn clear(&mut self) {
        self.main = ScreenState::new(self.cols, self.rows, self.default_fg, self.default_bg);
        self.alternate = ScreenState::new(self.cols, self.rows, self.default_fg, self.default_bg);
        self.scrollback.clear();
        self.using_alternate = false;
        self.saved_cursor = None;
        self.parse_state = ParseState::Ground;
        self.csi_buf.clear();
        self.osc_buf.clear();
        self.osc_escape = false;
        self.dcs_buf.clear();
        self.dcs_escape = false;
        self.current_fg = self.default_fg;
        self.current_bg = self.default_bg;
        self.cursor_style = 1;
        self.cursor_visible = true;
        self.application_cursor_keys = false;
        self.mouse_reporting = false;
        self.mouse_motion = false;
        self.mouse_sgr = false;
        self.alternate_scroll = false;
        self.focus_reporting = false;
        self.bracketed_paste = false;
        self.g0_charset = Charset::Ascii;
        self.g1_charset = Charset::Ascii;
        self.use_g1_charset = false;
        for line in self.banner.clone() {
            self.push_output(&line);
        }
        self.command_mode = false;
        self.command_input.clear();
        self.command_history_index = None;
        self.scroll_offset = 0;
        self.outbound.clear();
        self.clipboard_commands.clear();
    }

    pub fn take_command_input(&mut self) -> String {
        let input = std::mem::take(&mut self.command_input);
        if !input.trim().is_empty() {
            self.command_history.push(input.clone());
        }
        self.command_history_index = None;
        self.command_mode = false;
        self.scroll_offset = 0;
        input
    }

    pub fn cancel_command_mode(&mut self) {
        self.command_mode = false;
        self.command_input.clear();
        self.command_history_index = None;
    }

    pub fn push_output(&mut self, line: &str) {
        self.write_plain(line);
        self.newline();
    }

    pub fn append_output_chunk(&mut self, chunk: &str) {
        let pinned_scrollback = self.scroll_offset > 0 && !self.command_mode && !self.using_alternate;
        let previous_scrollback_len = self.scrollback.len();
        for ch in chunk.chars() {
            match self.parse_state {
                ParseState::Ground => self.handle_ground(ch),
                ParseState::Escape => self.handle_escape(ch),
                ParseState::Csi => self.handle_csi(ch),
                ParseState::Osc => self.handle_osc(ch),
                ParseState::Dcs => self.handle_dcs(ch),
                ParseState::CharsetG0 => self.set_charset(0, ch),
                ParseState::CharsetG1 => self.set_charset(1, ch),
            }
        }
        if pinned_scrollback {
            let added_rows = self.scrollback.len().saturating_sub(previous_scrollback_len);
            self.scroll_offset = min(self.scroll_offset.saturating_add(added_rows), self.scrollback.len());
        }
    }

    pub fn set_status(&mut self, status: &str) {
        self.status = status.to_string();
    }

    pub fn scroll_viewport(&mut self, delta: isize) {
        if self.using_alternate {
            return;
        }
        let max_offset = self.scrollback.len();
        if delta.is_negative() {
            let next_offset = self.scroll_offset.saturating_sub(delta.unsigned_abs());
            if next_offset == 0 {
                self.scroll_to_bottom();
            } else {
                self.scroll_offset = next_offset;
            }
        } else {
            self.scroll_offset = min(self.scroll_offset.saturating_add(delta as usize), max_offset);
        }
    }

    pub fn page_up(&mut self) {
        self.scroll_viewport(self.rows as isize);
    }

    pub fn page_down(&mut self) {
        self.scroll_viewport(-(self.rows as isize));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn previous_history(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let next = match self.command_history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(idx) => idx,
            None => self.command_history.len().saturating_sub(1),
        };
        self.command_history_index = Some(next);
        self.command_input = self.command_history[next].clone();
    }

    pub fn next_history(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        match self.command_history_index {
            Some(idx) if idx + 1 < self.command_history.len() => {
                let next = idx + 1;
                self.command_history_index = Some(next);
                self.command_input = self.command_history[next].clone();
            }
            _ => {
                self.command_history_index = None;
                self.command_input.clear();
            }
        }
    }

    pub fn apply_config(&mut self, config: &AppConfig) {
        self.title = config.title.clone();
        self.banner = config.banner.clone();
        self.default_fg = config.foreground;
        self.default_bg = config.background;
        self.current_fg = config.foreground;
        self.current_bg = config.background;
    }

    fn handle_ground(&mut self, ch: char) {
        match ch {
            '\u{1b}' => self.parse_state = ParseState::Escape,
            '\n' => self.newline(),
            '\r' => self.active_screen_mut().cursor_col = 0,
            '\u{08}' => {
                let active = self.active_screen_mut();
                active.cursor_col = active.cursor_col.saturating_sub(1);
            }
            '\u{0e}' => self.use_g1_charset = true,
            '\u{0f}' => self.use_g1_charset = false,
            '\t' => {
                let spaces = TAB_WIDTH - (self.active_screen().cursor_col % TAB_WIDTH);
                for _ in 0..spaces {
                    self.put_char(' ');
                }
            }
            c if !c.is_control() => self.put_char(c),
            _ => {}
        }
    }

    fn handle_escape(&mut self, ch: char) {
        match ch {
            '[' => {
                self.csi_buf.clear();
                self.parse_state = ParseState::Csi;
            }
            ']' => {
                self.osc_buf.clear();
                self.osc_escape = false;
                self.parse_state = ParseState::Osc;
            }
            'P' => {
                self.dcs_buf.clear();
                self.dcs_escape = false;
                self.parse_state = ParseState::Dcs;
            }
            '(' => self.parse_state = ParseState::CharsetG0,
            ')' => self.parse_state = ParseState::CharsetG1,
            '7' => {
                self.save_cursor();
                self.parse_state = ParseState::Ground;
            }
            '8' => {
                self.restore_cursor();
                self.parse_state = ParseState::Ground;
            }
            'D' => {
                self.newline();
                self.parse_state = ParseState::Ground;
            }
            'E' => {
                self.newline();
                self.active_screen_mut().cursor_col = 0;
                self.parse_state = ParseState::Ground;
            }
            'M' => {
                self.reverse_index();
                self.parse_state = ParseState::Ground;
            }
            'c' => {
                self.full_reset();
                self.parse_state = ParseState::Ground;
            }
            _ => self.parse_state = ParseState::Ground,
        }
    }

    fn handle_csi(&mut self, ch: char) {
        if ('@'..='~').contains(&ch) {
            self.execute_csi(ch);
            self.csi_buf.clear();
            self.parse_state = ParseState::Ground;
        } else {
            self.csi_buf.push(ch);
        }
    }

    fn handle_osc(&mut self, ch: char) {
        if self.osc_escape {
            if ch == '\\' {
                self.execute_osc();
                self.osc_buf.clear();
                self.osc_escape = false;
                self.parse_state = ParseState::Ground;
            } else {
                self.osc_buf.push('\u{1b}');
                self.osc_buf.push(ch);
                self.osc_escape = false;
            }
            return;
        }

        match ch {
            '\u{07}' => {
                self.execute_osc();
                self.osc_buf.clear();
                self.parse_state = ParseState::Ground;
            }
            '\u{1b}' => self.osc_escape = true,
            _ => self.osc_buf.push(ch),
        }
    }

    fn handle_dcs(&mut self, ch: char) {
        if self.dcs_escape {
            if ch == '\\' {
                self.execute_dcs();
                self.dcs_buf.clear();
                self.dcs_escape = false;
                self.parse_state = ParseState::Ground;
            } else {
                self.dcs_buf.push('\u{1b}');
                self.dcs_buf.push(ch);
                self.dcs_escape = false;
            }
            return;
        }

        match ch {
            '\u{1b}' => self.dcs_escape = true,
            _ => self.dcs_buf.push(ch),
        }
    }

    fn execute_csi(&mut self, final_byte: char) {
        let private = self.csi_buf.starts_with('?');
        let secondary = self.csi_buf.starts_with('>');
        let mode_query = self.csi_buf.contains("$");
        let params = self
            .csi_buf
            .trim_start_matches('?')
            .trim_start_matches('>')
            .trim_end_matches('$')
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<usize>().unwrap_or(0))
            .collect::<Vec<_>>();

        let p = |idx: usize, default: usize| params.get(idx).copied().filter(|v| *v > 0).unwrap_or(default);

        match final_byte {
            'A' => {
                let active = self.active_screen_mut();
                active.cursor_row = active.cursor_row.saturating_sub(p(0, 1));
            }
            'B' => {
                let max_row = self.rows.saturating_sub(1);
                let active = self.active_screen_mut();
                active.cursor_row = min(active.cursor_row + p(0, 1), max_row);
            }
            'C' => {
                let max_col = self.cols.saturating_sub(1);
                let active = self.active_screen_mut();
                active.cursor_col = min(active.cursor_col + p(0, 1), max_col);
            }
            'D' => {
                let active = self.active_screen_mut();
                active.cursor_col = active.cursor_col.saturating_sub(p(0, 1));
            }
            'E' => {
                let max_row = self.rows.saturating_sub(1);
                let active = self.active_screen_mut();
                active.cursor_row = min(active.cursor_row + p(0, 1), max_row);
                active.cursor_col = 0;
            }
            'F' => {
                let active = self.active_screen_mut();
                active.cursor_row = active.cursor_row.saturating_sub(p(0, 1));
                active.cursor_col = 0;
            }
            'G' => {
                self.active_screen_mut().cursor_col =
                    min(p(0, 1).saturating_sub(1), self.cols.saturating_sub(1));
            }
            'H' | 'f' => {
                let max_row = self.rows.saturating_sub(1);
                let max_col = self.cols.saturating_sub(1);
                let base_row = if self.active_screen().origin_mode {
                    self.active_screen().scroll_top
                } else {
                    0
                };
                let limit_row = if self.active_screen().origin_mode {
                    self.active_screen().scroll_bottom
                } else {
                    max_row
                };
                let active = self.active_screen_mut();
                active.cursor_row = min(base_row + p(0, 1).saturating_sub(1), limit_row);
                active.cursor_col = min(p(1, 1).saturating_sub(1), max_col);
            }
            'd' => {
                let max_row = if self.active_screen().origin_mode {
                    self.active_screen().scroll_bottom
                } else {
                    self.rows.saturating_sub(1)
                };
                let base_row = if self.active_screen().origin_mode {
                    self.active_screen().scroll_top
                } else {
                    0
                };
                self.active_screen_mut().cursor_row =
                    min(base_row + p(0, 1).saturating_sub(1), max_row);
            }
            'J' => {
                match params.first().copied().unwrap_or(0) {
                    0 => self.erase_in_display_from_cursor(),
                    1 => self.erase_in_display_to_cursor(),
                    2 | 3 => {
                        self.clear_active_screen();
                        if !self.using_alternate && params.first().copied().unwrap_or(0) == 3 {
                            self.scrollback.clear();
                        }
                    }
                    _ => {}
                }
            }
            'K' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => self.erase_line_right(),
                    1 => self.erase_line_left(),
                    2 => self.erase_line_all(),
                    _ => {}
                }
            }
            'r' => {
                let top = p(0, 1).saturating_sub(1);
                let bottom = if params.len() > 1 {
                    p(1, self.rows).saturating_sub(1)
                } else {
                    self.rows.saturating_sub(1)
                };
                self.set_scroll_region(top, bottom);
            }
            'L' => self.insert_lines(p(0, 1)),
            'M' => self.delete_lines(p(0, 1)),
            'P' => self.delete_chars(p(0, 1)),
            '@' => self.insert_blank_chars(p(0, 1)),
            'X' => self.erase_chars(p(0, 1)),
            'S' => self.scroll_region_up(p(0, 1)),
            'T' => self.scroll_region_down(p(0, 1)),
            'h' if private => self.apply_private_mode(true, &params),
            'l' if private => self.apply_private_mode(false, &params),
            'h' => self.apply_mode(true, &params),
            'l' => self.apply_mode(false, &params),
            's' => {
                self.save_cursor();
            }
            'u' => self.restore_cursor(),
            'm' => self.apply_sgr(&params),
            'n' => self.respond_to_status_request(&params),
            'c' => self.respond_to_device_attributes(secondary),
            'q' if self.csi_buf.starts_with(' ') => self.set_cursor_style(&params),
            'p' if mode_query => self.respond_to_mode_query(private, &params),
            _ => {}
        }
    }

    fn apply_sgr(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.reset_attributes();
            return;
        }
        let mut idx = 0;
        while idx < params.len() {
            let code = params[idx];
            match code {
                0 => self.reset_attributes(),
                30..=37 => self.current_fg = ansi_palette((code - 30) as u8),
                39 => self.current_fg = self.default_fg,
                40..=47 => self.current_bg = ansi_palette((code - 40) as u8),
                49 => self.current_bg = self.default_bg,
                90..=97 => self.current_fg = ansi_palette((code - 90 + 8) as u8),
                100..=107 => self.current_bg = ansi_palette((code - 100 + 8) as u8),
                38 | 48 => {
                    let color = if code == 38 {
                        &mut self.current_fg
                    } else {
                        &mut self.current_bg
                    };
                    if let Some((advance, rgb)) = parse_extended_color(&params[idx + 1..]) {
                        *color = rgb;
                        idx += advance;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
    }

    fn reset_attributes(&mut self) {
        self.current_fg = self.default_fg;
        self.current_bg = self.default_bg;
    }

    fn execute_osc(&mut self) {
        let osc = self.osc_buf.clone();
        let mut parts = osc.splitn(2, ';');
        let code = parts.next().unwrap_or_default();
        let payload = parts.next().unwrap_or_default();
        match code {
            "0" | "2" => {
                if !payload.is_empty() {
                    self.title = payload.to_string();
                }
            }
            "4" => self.handle_palette_osc(payload),
            "10" => self.handle_color_osc(payload, OscColorTarget::Foreground),
            "11" => self.handle_color_osc(payload, OscColorTarget::Background),
            "12" => self.handle_color_osc(payload, OscColorTarget::Cursor),
            "52" => self.handle_clipboard_osc(payload),
            _ => {}
        }
    }

    fn respond_to_status_request(&mut self, params: &[usize]) {
        match params.first().copied().unwrap_or(0) {
            5 => self.outbound.push(b"\x1b[0n".to_vec()),
            6 => {
                let row = self.active_screen().cursor_row.saturating_add(1);
                let col = self.active_screen().cursor_col.saturating_add(1);
                self.outbound
                    .push(format!("\x1b[{};{}R", row, col).into_bytes());
            }
            _ => {}
        }
    }

    fn respond_to_device_attributes(&mut self, secondary: bool) {
        if secondary {
            self.outbound.push(b"\x1b[>0;10;1c".to_vec());
        } else {
            self.outbound.push(b"\x1b[?1;2c".to_vec());
        }
    }

    fn set_cursor_style(&mut self, params: &[usize]) {
        self.cursor_style = params.first().copied().unwrap_or(1).max(1);
    }

    fn respond_to_mode_query(&mut self, private: bool, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        let enabled = if private {
            match mode {
                1 => self.application_cursor_keys,
                6 => self.active_screen().origin_mode,
                7 => self.active_screen().autowrap,
                25 => self.cursor_visible,
                1000 => self.mouse_reporting,
                1004 => self.focus_reporting,
                1006 => self.mouse_sgr,
                1007 => self.alternate_scroll,
                2004 => self.bracketed_paste,
                _ => false,
            }
        } else {
            match mode {
                4 => self.active_screen().insert_mode,
                _ => false,
            }
        };
        let report = if private {
            format!("\x1b[?{};{}$y", mode, if enabled { 1 } else { 2 })
        } else {
            format!("\x1b[{};{}$y", mode, if enabled { 1 } else { 2 })
        };
        self.outbound.push(report.into_bytes());
    }

    fn handle_palette_osc(&mut self, payload: &str) {
        let mut parts = payload.split(';');
        while let (Some(index), Some(spec)) = (parts.next(), parts.next()) {
            let Ok(index) = index.parse::<u8>() else {
                continue;
            };
            if spec == "?" {
                let color = ansi_256_palette(index);
                self.outbound.push(
                    format!("\x1b]4;{};{}\x07", index, rgb_spec(color)).into_bytes(),
                );
            }
        }
    }

    fn handle_color_osc(&mut self, payload: &str, target: OscColorTarget) {
        if payload == "?" {
            let color = match target {
                OscColorTarget::Foreground => self.default_fg,
                OscColorTarget::Background => self.default_bg,
                OscColorTarget::Cursor => self.current_fg,
            };
            let code = match target {
                OscColorTarget::Foreground => 10,
                OscColorTarget::Background => 11,
                OscColorTarget::Cursor => 12,
            };
            self.outbound
                .push(format!("\x1b]{};{}\x07", code, rgb_spec(color)).into_bytes());
            return;
        }

        if let Some(color) = parse_rgb_spec(payload) {
            match target {
                OscColorTarget::Foreground => {
                    self.default_fg = color;
                    self.current_fg = color;
                }
                OscColorTarget::Background => {
                    self.default_bg = color;
                    self.current_bg = color;
                }
                OscColorTarget::Cursor => {
                    self.current_fg = color;
                }
            }
        }
    }

    fn handle_clipboard_osc(&mut self, payload: &str) {
        let mut parts = payload.splitn(2, ';');
        let _selection = parts.next().unwrap_or_default();
        let data = parts.next().unwrap_or_default();
        if data == "?" {
            self.clipboard_commands.push(ClipboardCommand::Query);
            return;
        }
        if data.is_empty() {
            self.clipboard_commands.push(ClipboardCommand::Set(String::new()));
            return;
        }
        if let Ok(bytes) = BASE64.decode(data) {
            if let Ok(text) = String::from_utf8(bytes) {
                self.clipboard_commands.push(ClipboardCommand::Set(text));
            }
        }
    }

    fn execute_dcs(&mut self) {
        let dcs = self.dcs_buf.clone();
        if let Some(query) = dcs.strip_prefix("$q") {
            self.respond_to_decrqss(query);
        }
    }

    fn respond_to_decrqss(&mut self, query: &str) {
        let reply = match query {
            "\"q" => format!("\x1bP1$r{} q\x1b\\", self.cursor_style),
            "r" => {
                let (top, bottom) = self.scroll_region_bounds();
                format!("\x1bP1$r{};{}r\x1b\\", top + 1, bottom + 1)
            }
            "m" => "\x1bP1$r0m\x1b\\".to_string(),
            _ => "\x1bP0$r\x1b\\".to_string(),
        };
        self.outbound.push(reply.into_bytes());
    }

    fn write_plain(&mut self, text: &str) {
        for ch in text.chars() {
            self.put_char(ch);
        }
    }

    fn put_char(&mut self, ch: char) {
        let ch = self.translate_charset(ch);
        if self.should_append_to_previous_cluster(ch) {
            self.append_to_previous_cluster(ch);
            return;
        }
        let width = UnicodeWidthChar::width(ch)
            .unwrap_or_else(|| if is_combining_mark(ch) { 0 } else { 1 });
        if width == 0 {
            self.append_combining_mark(ch);
            return;
        }
        if width > 1 && self.active_screen().cursor_col + width > self.cols {
            if self.active_screen().autowrap {
                self.newline();
            } else {
                return;
            }
        }
        if self.active_screen().cursor_col >= self.cols {
            if self.active_screen().autowrap {
                self.newline();
            } else {
                self.active_screen_mut().cursor_col = self.cols.saturating_sub(1);
            }
        }
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        if self.active_screen().insert_mode {
            let blank = Cell::blank(self.current_fg, self.current_bg);
            let line = &mut self.active_screen_mut().screen[row];
            for _ in 0..width {
                line.insert(col, blank.clone());
                line.pop();
            }
        }
        self.active_screen_mut().screen[row][col] = Cell {
            span: width,
            text: normalize_cluster(&ch.to_string()),
            fg: self.current_fg,
            bg: self.current_bg,
            wide_continuation: false,
        };
        for offset in 1..width {
            if col + offset >= self.cols {
                break;
            }
            self.active_screen_mut().screen[row][col + offset] = Cell {
                span: 0,
                text: String::new(),
                fg: self.current_fg,
                bg: self.current_bg,
                wide_continuation: true,
            };
        }
        self.active_screen_mut().cursor_col += width;
        if self.active_screen().cursor_col >= self.cols {
            if self.active_screen().autowrap {
                self.newline();
            } else {
                self.active_screen_mut().cursor_col = self.cols.saturating_sub(1);
            }
        }
    }

    fn append_combining_mark(&mut self, ch: char) {
        let Some((row, target_col)) = self.previous_visible_cell_position() else {
            return;
        };
        self.active_screen_mut().screen[row][target_col].text.push(ch);
        let normalized = normalize_cluster(&self.active_screen().screen[row][target_col].text);
        self.active_screen_mut().screen[row][target_col].text = normalized;
    }

    fn append_to_previous_cluster(&mut self, ch: char) {
        let Some((row, col)) = self.previous_visible_cell_position() else {
            self.put_char(ch);
            return;
        };
        self.active_screen_mut().screen[row][col].text.push(ch);
        let normalized = normalize_cluster(&self.active_screen().screen[row][col].text);
        self.active_screen_mut().screen[row][col].text = normalized;
        let span = cluster_display_width(&self.active_screen().screen[row][col].text);
        self.set_cell_span(row, col, span.max(1));
    }

    fn newline(&mut self) {
        self.active_screen_mut().cursor_col = 0;
        if self.active_screen().cursor_row >= self.active_screen().scroll_bottom {
            self.scroll_up_in_region(1);
        } else {
            self.active_screen_mut().cursor_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.active_screen().cursor_row <= self.active_screen().scroll_top {
            self.scroll_down_in_region(1);
        } else {
            self.active_screen_mut().cursor_row -= 1;
        }
    }

    fn full_reset(&mut self) {
        self.clear();
        self.set_status("terminal reset");
    }

    fn active_screen(&self) -> &ScreenState {
        if self.using_alternate {
            &self.alternate
        } else {
            &self.main
        }
    }

    fn active_screen_mut(&mut self) -> &mut ScreenState {
        if self.using_alternate {
            &mut self.alternate
        } else {
            &mut self.main
        }
    }

    fn clear_active_screen(&mut self) {
        let cols = self.cols;
        let rows = self.rows;
        let default_fg = self.default_fg;
        let default_bg = self.default_bg;
        let active = self.active_screen_mut();
        active.screen = blank_screen(cols, rows, default_fg, default_bg);
        active.cursor_row = active.scroll_top.min(rows.saturating_sub(1));
        active.cursor_col = 0;
    }

    fn erase_line_right(&mut self) {
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        for idx in col..self.cols {
            self.active_screen_mut().screen[row][idx] = blank.clone();
        }
    }

    fn erase_line_left(&mut self) {
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        for idx in 0..=col.min(self.cols.saturating_sub(1)) {
            self.active_screen_mut().screen[row][idx] = blank.clone();
        }
    }

    fn erase_line_all(&mut self) {
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let row = self.active_screen().cursor_row;
        self.active_screen_mut().screen[row].fill(blank);
    }

    fn erase_in_display_from_cursor(&mut self) {
        self.erase_line_right();
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        let row = self.active_screen().cursor_row;
        for idx in row + 1..self.rows {
            self.active_screen_mut().screen[idx] = blank.clone();
        }
    }

    fn erase_in_display_to_cursor(&mut self) {
        self.erase_line_left();
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        let row = self.active_screen().cursor_row;
        for idx in 0..row {
            self.active_screen_mut().screen[idx] = blank.clone();
        }
    }

    fn insert_lines(&mut self, count: usize) {
        let row = self.active_screen().cursor_row;
        let (_, bottom) = self.scroll_region_bounds();
        if row > bottom {
            return;
        }
        let count = count.min(bottom.saturating_sub(row).saturating_add(1));
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        for _ in 0..count {
            self.active_screen_mut().screen.insert(row, blank.clone());
            self.active_screen_mut().screen.remove(bottom + 1);
        }
    }

    fn delete_lines(&mut self, count: usize) {
        let row = self.active_screen().cursor_row;
        let (_, bottom) = self.scroll_region_bounds();
        if row > bottom {
            return;
        }
        let count = count.min(bottom.saturating_sub(row).saturating_add(1));
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        for _ in 0..count {
            self.active_screen_mut().screen.remove(row);
            self.active_screen_mut().screen.insert(bottom, blank.clone());
        }
    }

    fn insert_blank_chars(&mut self, count: usize) {
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        let cols = self.cols;
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let line = &mut self.active_screen_mut().screen[row];
        for _ in 0..count.min(cols.saturating_sub(col)) {
            line.insert(col, blank.clone());
            line.pop();
        }
    }

    fn delete_chars(&mut self, count: usize) {
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        let cols = self.cols;
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let line = &mut self.active_screen_mut().screen[row];
        for _ in 0..count.min(cols.saturating_sub(col)) {
            line.remove(col);
            line.push(blank.clone());
        }
    }

    fn erase_chars(&mut self, count: usize) {
        let row = self.active_screen().cursor_row;
        let col = self.active_screen().cursor_col;
        let blank = Cell::blank(self.current_fg, self.current_bg);
        let end = min(col + count, self.cols);
        for idx in col..end {
            self.active_screen_mut().screen[row][idx] = blank.clone();
        }
    }

    fn scroll_region_up(&mut self, count: usize) {
        self.scroll_up_in_region(count);
    }

    fn scroll_region_down(&mut self, count: usize) {
        self.scroll_down_in_region(count);
    }

    fn restore_cursor(&mut self) {
        if let Some(saved) = self.saved_cursor {
            let max_row = self.rows.saturating_sub(1);
            let max_col = self.cols.saturating_sub(1);
            self.current_fg = saved.fg;
            self.current_bg = saved.bg;
            self.g0_charset = saved.g0_charset;
            self.g1_charset = saved.g1_charset;
            self.use_g1_charset = saved.use_g1_charset;
            let active = self.active_screen_mut();
            active.cursor_row = min(saved.row, max_row);
            active.cursor_col = min(saved.col, max_col);
            active.origin_mode = saved.origin_mode;
            active.autowrap = saved.autowrap;
            active.insert_mode = saved.insert_mode;
        }
    }

    fn save_cursor(&mut self) {
        let active = self.active_screen();
        self.saved_cursor = Some(SavedCursor {
            row: active.cursor_row,
            col: active.cursor_col,
            fg: self.current_fg,
            bg: self.current_bg,
            g0_charset: self.g0_charset,
            g1_charset: self.g1_charset,
            use_g1_charset: self.use_g1_charset,
            origin_mode: active.origin_mode,
            autowrap: active.autowrap,
            insert_mode: active.insert_mode,
        });
    }

    fn apply_private_mode(&mut self, enable: bool, params: &[usize]) {
        for mode in params {
            match *mode {
                1 => self.application_cursor_keys = enable,
                6 => self.active_screen_mut().origin_mode = enable,
                7 => self.active_screen_mut().autowrap = enable,
                25 => self.cursor_visible = enable,
                1000 => {
                    self.mouse_reporting = enable;
                    if !enable {
                        self.mouse_motion = false;
                    }
                }
                1002 | 1003 => {
                    self.mouse_reporting = enable;
                    self.mouse_motion = enable;
                }
                1004 => self.focus_reporting = enable,
                1006 => self.mouse_sgr = enable,
                1007 => self.alternate_scroll = enable,
                47 | 1047 | 1049 => self.set_alternate_screen(enable),
                1048 => {
                    if enable {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                2004 => self.bracketed_paste = enable,
                _ => {}
            }
        }
        if params.iter().any(|mode| *mode == 6) {
            let row = if self.active_screen().origin_mode {
                self.active_screen().scroll_top
            } else {
                0
            };
            let active = self.active_screen_mut();
            active.cursor_row = row;
            active.cursor_col = 0;
        }
    }

    fn set_alternate_screen(&mut self, enable: bool) {
        if self.using_alternate == enable {
            return;
        }
        if enable {
            self.save_cursor();
            self.alternate = ScreenState::new(self.cols, self.rows, self.default_fg, self.default_bg);
            self.using_alternate = true;
            self.scroll_offset = 0;
        } else {
            self.using_alternate = false;
            self.restore_cursor();
            self.scroll_to_bottom();
        }
    }

    fn scroll_up_in_region(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region_bounds();
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        for _ in 0..count {
            if top >= bottom || bottom >= self.rows {
                break;
            }
            if !self.using_alternate && top == 0 {
                self.scrollback.push(self.main.screen.remove(top));
                if self.scrollback.len() > DEFAULT_SCROLLBACK {
                    let overflow = self.scrollback.len() - DEFAULT_SCROLLBACK;
                    self.scrollback.drain(0..overflow);
                }
                self.main.screen.insert(bottom, blank.clone());
            } else {
                let screen = &mut self.active_screen_mut().screen;
                for row in top..bottom {
                    screen[row] = screen[row + 1].clone();
                }
                screen[bottom] = blank.clone();
            }
        }
        self.active_screen_mut().cursor_row = bottom.min(self.rows.saturating_sub(1));
    }

    fn scroll_down_in_region(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region_bounds();
        let blank = vec![Cell::blank(self.default_fg, self.default_bg); self.cols];
        for _ in 0..count {
            if top >= bottom || bottom >= self.rows {
                break;
            }
            let screen = &mut self.active_screen_mut().screen;
            for row in (top + 1..=bottom).rev() {
                screen[row] = screen[row - 1].clone();
            }
            screen[top] = blank.clone();
        }
    }

    fn scroll_region_bounds(&self) -> (usize, usize) {
        let active = self.active_screen();
        (
            active.scroll_top.min(self.rows.saturating_sub(1)),
            active.scroll_bottom.min(self.rows.saturating_sub(1)),
        )
    }

    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let max_row = self.rows.saturating_sub(1);
        let top = top.min(max_row);
        let bottom = bottom.min(max_row);
        if top >= bottom {
            return;
        }
        let origin_mode = self.active_screen().origin_mode;
        let active = self.active_screen_mut();
        active.scroll_top = top;
        active.scroll_bottom = bottom;
        active.cursor_row = if origin_mode { top } else { 0 };
        active.cursor_col = 0;
    }

    fn apply_mode(&mut self, enable: bool, params: &[usize]) {
        for mode in params {
            match *mode {
                4 => self.active_screen_mut().insert_mode = enable,
                20 => {}
                _ => {}
            }
        }
    }

    fn set_charset(&mut self, bank: usize, selector: char) {
        let charset = match selector {
            '0' => Charset::DecSpecial,
            'B' => Charset::Ascii,
            _ => Charset::Ascii,
        };
        match bank {
            0 => self.g0_charset = charset,
            _ => self.g1_charset = charset,
        }
        self.parse_state = ParseState::Ground;
    }

    fn translate_charset(&self, ch: char) -> char {
        match self.active_charset() {
            Charset::Ascii => ch,
            Charset::DecSpecial => dec_special_graphics(ch),
        }
    }

    fn active_charset(&self) -> Charset {
        if self.use_g1_charset {
            self.g1_charset
        } else {
            self.g0_charset
        }
    }

    fn should_append_to_previous_cluster(&self, ch: char) -> bool {
        is_combining_mark(ch)
            || is_variation_selector(ch)
            || is_zero_width_joiner(ch)
            || is_emoji_modifier(ch)
            || self.previous_cluster_expects_join()
            || self.previous_cluster_expects_flag_pair(ch)
    }

    fn previous_cluster_expects_join(&self) -> bool {
        let Some((row, col)) = self.previous_visible_cell_position() else {
            return false;
        };
        let text = &self.active_screen().screen[row][col].text;
        text.chars()
            .last()
            .map(|ch| is_zero_width_joiner(ch) || is_variation_selector(ch))
            .unwrap_or(false)
    }

    fn previous_cluster_expects_flag_pair(&self, ch: char) -> bool {
        if !is_regional_indicator(ch) {
            return false;
        }
        let Some((row, col)) = self.previous_visible_cell_position() else {
            return false;
        };
        let text = &self.active_screen().screen[row][col].text;
        let grapheme = text.graphemes(true).last().unwrap_or("");
        let mut scalars = grapheme.chars();
        match (scalars.next(), scalars.next(), scalars.next()) {
            (Some(a), None, None) => is_regional_indicator(a),
            _ => false,
        }
    }

    fn previous_visible_cell_position(&self) -> Option<(usize, usize)> {
        if self.cols == 0 {
            return None;
        }
        let row = self.active_screen().cursor_row;
        let mut col = self.active_screen().cursor_col.saturating_sub(1);
        loop {
            let cell = &self.active_screen().screen[row][col];
            if !cell.wide_continuation {
                return Some((row, col));
            }
            if col == 0 {
                return Some((row, 0));
            }
            col -= 1;
        }
    }

    fn set_cell_span(&mut self, row: usize, col: usize, span: usize) {
        let span = span.min(self.cols.saturating_sub(col)).max(1);
        let fg = self.active_screen().screen[row][col].fg;
        let bg = self.active_screen().screen[row][col].bg;
        self.active_screen_mut().screen[row][col].span = span;
        self.active_screen_mut().screen[row][col].wide_continuation = false;
        for offset in 1..self.cols.saturating_sub(col) {
            let is_cont = offset < span;
            self.active_screen_mut().screen[row][col + offset] = if is_cont {
                Cell {
                    span: 0,
                    text: String::new(),
                    fg,
                    bg,
                    wide_continuation: true,
                }
            } else {
                let cell = &self.active_screen().screen[row][col + offset];
                if cell.wide_continuation {
                    Cell::blank(fg, bg)
                } else {
                    cell.clone()
                }
            };
        }
    }
}

fn blank_screen(cols: usize, rows: usize, fg: u32, bg: u32) -> Vec<Vec<Cell>> {
    vec![vec![Cell::blank(fg, bg); cols]; rows]
}

fn resize_screen(state: &ScreenState, cols: usize, rows: usize, fg: u32, bg: u32) -> ScreenState {
    let mut next = blank_screen(cols, rows, fg, bg);
    let old_rows = state.screen.len();
    let copy_rows = min(rows, state.screen.len());
    let copy_cols = min(cols, state.screen.first().map(Vec::len).unwrap_or(0));
    for row in 0..copy_rows {
        for col in 0..copy_cols {
            next[row][col] = state.screen[row][col].clone();
        }
    }
    let full_height_scroll_region = old_rows > 0
        && state.scroll_top == 0
        && state.scroll_bottom >= old_rows.saturating_sub(1);
    ScreenState {
        screen: next,
        cursor_row: min(state.cursor_row, rows.saturating_sub(1)),
        cursor_col: min(state.cursor_col, cols.saturating_sub(1)),
        scroll_top: state.scroll_top.min(rows.saturating_sub(1)),
        scroll_bottom: if full_height_scroll_region {
            rows.saturating_sub(1)
        } else {
            state.scroll_bottom.min(rows.saturating_sub(1))
        },
        origin_mode: state.origin_mode,
        autowrap: state.autowrap,
        insert_mode: state.insert_mode,
    }
}

fn ansi_palette(index: u8) -> u32 {
    match index {
        0 => 0x1d2021,
        1 => 0xcc241d,
        2 => 0x98971a,
        3 => 0xd79921,
        4 => 0x458588,
        5 => 0xb16286,
        6 => 0x689d6a,
        7 => 0xebdbb2,
        8 => 0x928374,
        9 => 0xfb4934,
        10 => 0xb8bb26,
        11 => 0xfabd2f,
        12 => 0x83a598,
        13 => 0xd3869b,
        14 => 0x8ec07c,
        15 => 0xfbf1c7,
        _ => 0xf3efe0,
    }
}

fn dec_special_graphics(ch: char) -> char {
    match ch {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        '`' => '◆',
        'a' => '▒',
        'f' => '°',
        'g' => '±',
        '~' => '·',
        ',' => '←',
        '+' => '→',
        '.' => '↓',
        '-' => '↑',
        'h' => '␤',
        'i' => '␋',
        '0' => '█',
        _ => ch,
    }
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
    )
}

fn is_zero_width_joiner(ch: char) -> bool {
    ch == '\u{200d}'
}

fn is_variation_selector(ch: char) -> bool {
    matches!(ch as u32, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}

fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch as u32, 0x1F3FB..=0x1F3FF)
}

fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

fn cluster_display_width(text: &str) -> usize {
    let width = UnicodeWidthStr::width(text);
    let regional_indicators = text.chars().filter(|&ch| is_regional_indicator(ch)).count();
    if text.chars().any(|ch| is_zero_width_joiner(ch) || is_variation_selector(ch))
        || regional_indicators >= 2
    {
        width.max(2)
    } else {
        width
    }
}

fn normalize_cluster(text: &str) -> String {
    text.nfc().collect()
}

fn parse_rgb_spec(spec: &str) -> Option<u32> {
    let value = spec.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let digits = hex.len();
        return match digits {
            6 => u32::from_str_radix(hex, 16).ok(),
            3 => {
                let expanded = hex
                    .chars()
                    .flat_map(|ch| [ch, ch])
                    .collect::<String>();
                u32::from_str_radix(&expanded, 16).ok()
            }
            _ => None,
        };
    }
    if let Some(rgb) = value.strip_prefix("rgb:") {
        let mut parts = rgb.split('/');
        let r = parse_rgb_component(parts.next()?)?;
        let g = parse_rgb_component(parts.next()?)?;
        let b = parse_rgb_component(parts.next()?)?;
        return Some((r << 16) | (g << 8) | b);
    }
    None
}

fn parse_rgb_component(part: &str) -> Option<u32> {
    let part = part.trim();
    if part.is_empty() {
        return None;
    }
    let digits = part.len().min(4);
    let value = u32::from_str_radix(&part[..digits], 16).ok()?;
    Some(match digits {
        1 => value * 17,
        2 => value,
        3 => value >> 4,
        _ => value >> 8,
    })
}

fn rgb_spec(color: u32) -> String {
    let r = (color >> 16) & 0xff;
    let g = (color >> 8) & 0xff;
    let b = color & 0xff;
    format!("rgb:{r:02x}/{g:02x}/{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        AppConfig {
            title: "VTerm".to_string(),
            width: 800,
            height: 600,
            cell_width: 16,
            cell_height: 16,
            padding: 20,
            background: 0x111111,
            foreground: 0xf5f2e8,
            accent: 0xe07a5f,
            banner: Vec::new(),
            shortcuts: Vec::new(),
        }
    }

    #[test]
    fn osc_title_updates_title() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1b]2;Alt Title\x07");
        assert_eq!(terminal.title(), "Alt Title");
    }

    #[test]
    fn osc_52_generates_clipboard_set() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1b]52;c;aGVsbG8=\x07");
        let commands = terminal.take_clipboard_commands();
        match commands.as_slice() {
            [ClipboardCommand::Set(text)] => assert_eq!(text, "hello"),
            _ => panic!("expected clipboard set"),
        }
    }

    #[test]
    fn dec_special_graphics_maps_line_drawing() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1b(0q");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "─");
    }

    #[test]
    fn mode_query_replies_for_private_mode() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1b[?25$p");
        let outbound = terminal.take_outbound();
        let first = String::from_utf8(outbound[0].clone()).unwrap();
        assert_eq!(first, "\x1b[?25;1$y");
    }

    #[test]
    fn dcs_decrqss_replies_with_cursor_style() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1bP$q\"q\x1b\\");
        let outbound = terminal.take_outbound();
        let first = String::from_utf8(outbound[0].clone()).unwrap();
        assert_eq!(first, "\x1bP1$r1 q\x1b\\");
    }

    #[test]
    fn clipboard_query_generates_osc52_reply() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.respond_clipboard_query("clip");
        let outbound = terminal.take_outbound();
        let first = String::from_utf8(outbound[0].clone()).unwrap();
        assert_eq!(first, "\x1b]52;c;Y2xpcA==\x07");
    }

    #[test]
    fn wide_character_marks_continuation_cell() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("界");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "界");
        assert!(rows[0][1].wide_continuation);
    }

    #[test]
    fn combining_mark_attaches_to_previous_cell() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("e\u{301}");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "é");
    }

    #[test]
    fn zwj_sequence_stays_in_single_lead_cell() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("👩\u{200d}🚀");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "👩\u{200d}🚀");
        assert_eq!(rows[0][0].span, 2);
        assert!(rows[0][1].wide_continuation);
    }

    #[test]
    fn flag_pair_stays_in_single_lead_cell() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("🇺🇸");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "🇺🇸");
        assert_eq!(rows[0][0].span, 2);
        assert!(rows[0][1].wide_continuation);
    }

    #[test]
    fn emoji_modifier_stays_in_single_lead_cell() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("👍🏽");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "👍🏽");
        assert_eq!(rows[0][0].span, 2);
        assert!(rows[0][1].wide_continuation);
    }

    #[test]
    fn decsc_decrc_restore_attributes_and_charset_state() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("\x1b[31m\x1b(0\x1b7\x1b[32mqq\x1b(B\x1b8q");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "─");
        assert_eq!(rows[0][0].fg, 0xcc241d);
        assert_eq!(rows[0][1].text, "─");
        assert_eq!(rows[0][1].fg, 0x98971a);
    }

    #[test]
    fn csi_save_restore_restores_insert_mode() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.append_output_chunk("ab\x1b[s\x1b[4h\x1b[1GZ\x1b[u");
        assert!(!terminal.active_screen().insert_mode);
        terminal.append_output_chunk("X");
        let rows = terminal.render_cells();
        assert_eq!(rows[0][0].text, "Z");
        assert_eq!(rows[0][1].text, "a");
        assert_eq!(rows[0][2].text, "X");
    }

    #[test]
    fn delete_lines_at_bottom_region_does_not_drop_past_buffer_end() {
        let mut terminal = TerminalBuffer::new(&config());
        terminal.resize(4, 3);
        terminal.append_output_chunk("1\r\n2\r\n3");
        terminal.append_output_chunk("\x1b[2;3r\x1b[2;1H\x1b[M");
        let rows = terminal.render_cells();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0].text, "1");
        assert_eq!(rows[1][0].text, "3");
        assert_eq!(rows[2][0].text, " ");
    }
}

fn parse_extended_color(params: &[usize]) -> Option<(usize, u32)> {
    match params.first().copied()? {
        5 => {
            let index = *params.get(1)?;
            Some((2, ansi_256_palette(index as u8)))
        }
        2 => {
            let r = *params.get(1)? as u32;
            let g = *params.get(2)? as u32;
            let b = *params.get(3)? as u32;
            Some((4, (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)))
        }
        _ => None,
    }
}

fn ansi_256_palette(index: u8) -> u32 {
    match index {
        0..=15 => ansi_palette(index),
        16..=231 => {
            let n = index - 16;
            let r = n / 36;
            let g = (n % 36) / 6;
            let b = n % 6;
            let scale = |v: u8| if v == 0 { 0 } else { v as u32 * 40 + 55 };
            (scale(r) << 16) | (scale(g) << 8) | scale(b)
        }
        232..=255 => {
            let level = 8 + (index as u32 - 232) * 10;
            (level << 16) | (level << 8) | level
        }
    }
}

fn normalize_selection(
    a: (usize, usize),
    b: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1) {
        (a, b)
    } else {
        (b, a)
    }
}

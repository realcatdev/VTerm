mod config;
mod lua_runtime;
mod pty;
mod render;
mod terminal;

use std::path::PathBuf;

use arboard::Clipboard;
use anyhow::{Context, Result};
use config::{AppConfig, Shortcut};
use lua_runtime::LuaRuntime;
use pty::PtySession;
use render::Renderer;
use terminal::{ClipboardCommand, TerminalBuffer};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(target_os = "macos")]
use objc2::sel;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApp, NSCommandKeyMask, NSEventModifierFlags, NSMenu, NSMenuItem,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{ns_string, MainThreadMarker};

#[cfg(target_os = "macos")]
fn install_macos_menu_bar() {
    fn add_menu(
        mtm: MainThreadMarker,
        menu_bar: &NSMenu,
        title: &objc2_foundation::NSString,
        submenu: &NSMenu,
    ) -> objc2::rc::Retained<NSMenuItem> {
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc::<NSMenuItem>(),
                title,
                None,
                ns_string!(""),
            );
            item.setSubmenu(Some(submenu));
            menu_bar.addItem(&item);
            item
        }
    }

    fn add_item(
        menu: &NSMenu,
        title: &objc2_foundation::NSString,
        action: Option<objc2::runtime::Sel>,
        key: &objc2_foundation::NSString,
        modifiers: Option<NSEventModifierFlags>,
    ) {
        unsafe {
            let item = menu.addItemWithTitle_action_keyEquivalent(title, action, key);
            if let Some(modifiers) = modifiers {
                item.setKeyEquivalentModifierMask(modifiers);
            }
        }
    }

    let mtm = MainThreadMarker::new().expect("AppKit menu install requires main thread");
    let app = NSApp(mtm);

    let main_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("MainMenu")) };

    let app_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("VTerm")) };
    add_menu(mtm, &main_menu, ns_string!("VTerm"), &app_menu);
    add_item(
        &app_menu,
        ns_string!("About VTerm"),
        Some(sel!(orderFrontStandardAboutPanel:)),
        ns_string!(""),
        None,
    );
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    let services_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc::<NSMenuItem>(),
            ns_string!("Services"),
            None,
            ns_string!(""),
        )
    };
    let services_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("Services")) };
    services_item.setSubmenu(Some(&services_menu));
    app_menu.addItem(&services_item);
    unsafe {
        app.setServicesMenu(Some(&services_menu));
    }
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        &app_menu,
        ns_string!("Hide VTerm"),
        Some(sel!(hide:)),
        ns_string!("h"),
        None,
    );
    add_item(
        &app_menu,
        ns_string!("Hide Others"),
        Some(sel!(hideOtherApplications:)),
        ns_string!("h"),
        Some(NSCommandKeyMask | objc2_app_kit::NSAlternateKeyMask),
    );
    add_item(
        &app_menu,
        ns_string!("Show All"),
        Some(sel!(unhideAllApplications:)),
        ns_string!(""),
        None,
    );
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        &app_menu,
        ns_string!("Quit VTerm"),
        Some(sel!(terminate:)),
        ns_string!("q"),
        None,
    );

    let edit_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("Edit")) };
    add_menu(mtm, &main_menu, ns_string!("Edit"), &edit_menu);
    add_item(&edit_menu, ns_string!("Undo"), Some(sel!(undo:)), ns_string!("z"), None);
    add_item(
        &edit_menu,
        ns_string!("Redo"),
        Some(sel!(redo:)),
        ns_string!("Z"),
        Some(NSCommandKeyMask | objc2_app_kit::NSShiftKeyMask),
    );
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(&edit_menu, ns_string!("Cut"), Some(sel!(cut:)), ns_string!("x"), None);
    add_item(&edit_menu, ns_string!("Copy"), Some(sel!(copy:)), ns_string!("c"), None);
    add_item(&edit_menu, ns_string!("Paste"), Some(sel!(paste:)), ns_string!("v"), None);
    add_item(
        &edit_menu,
        ns_string!("Select All"),
        Some(sel!(selectAll:)),
        ns_string!("a"),
        None,
    );

    let view_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("View")) };
    add_menu(mtm, &main_menu, ns_string!("View"), &view_menu);
    add_item(
        &view_menu,
        ns_string!("Enter Full Screen"),
        Some(sel!(toggleFullScreen:)),
        ns_string!("f"),
        Some(NSCommandKeyMask | objc2_app_kit::NSControlKeyMask),
    );

    let window_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("Window")) };
    add_menu(mtm, &main_menu, ns_string!("Window"), &window_menu);
    add_item(
        &window_menu,
        ns_string!("Minimize"),
        Some(sel!(performMiniaturize:)),
        ns_string!("m"),
        None,
    );
    add_item(&window_menu, ns_string!("Zoom"), Some(sel!(zoom:)), ns_string!(""), None);
    window_menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        &window_menu,
        ns_string!("Bring All to Front"),
        Some(sel!(arrangeInFront:)),
        ns_string!(""),
        None,
    );
    unsafe {
        app.setWindowsMenu(Some(&window_menu));
    }

    let help_menu = unsafe { NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!("Help")) };
    add_menu(mtm, &main_menu, ns_string!("Help"), &help_menu);
    add_item(
        &help_menu,
        ns_string!("VTerm Help"),
        Some(sel!(showHelp:)),
        ns_string!("?"),
        Some(NSCommandKeyMask | objc2_app_kit::NSShiftKeyMask),
    );
    unsafe {
        app.setHelpMenu(Some(&help_menu));
    }

    app.setMainMenu(Some(&main_menu));
}

struct VTermApp {
    config: AppConfig,
    base_cell_width: u32,
    base_cell_height: u32,
    zoom_percent: u32,
    lua: LuaRuntime,
    pty: PtySession,
    terminal: TerminalBuffer,
    window: Option<&'static Window>,
    renderer: Option<Renderer>,
    modifiers: ModifiersState,
    clipboard: Option<Clipboard>,
    last_cursor_pos: Option<(f64, f64)>,
    selection_anchor: Option<(usize, usize)>,
    selection_focus: Option<(usize, usize)>,
    mouse_left_down: bool,
}

impl VTermApp {
    fn new(
        config: AppConfig,
        lua: LuaRuntime,
        pty: PtySession,
    ) -> Self {
        let mut terminal = TerminalBuffer::new(&config);
        terminal.set_footer_context(pty.shell_label(), pty.cwd_label());
        Self {
            base_cell_width: config.cell_width,
            base_cell_height: config.cell_height,
            zoom_percent: 100,
            config,
            lua,
            pty,
            terminal,
            window: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            clipboard: Clipboard::new().ok(),
            last_cursor_pos: None,
            selection_anchor: None,
            selection_focus: None,
            mouse_left_down: false,
        }
    }

    fn update_grid(&mut self, width: u32, height: u32) {
        let cols = (width.saturating_sub(self.config.padding * 2) / self.config.cell_width).max(1) as usize;
        let usable_height = height
            .saturating_sub(self.config.padding * 2)
            .saturating_sub(self.config.cell_height * 2);
        let rows = (usable_height / self.config.cell_height).max(1) as usize;
        self.terminal.resize(cols, rows);
        let _ = self
            .pty
            .resize(cols.min(u16::MAX as usize) as u16, rows.min(u16::MAX as usize) as u16);
    }

    fn redraw(&mut self) {
        if let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) {
            let title = format!("{}  [{}]", self.terminal.title(), self.lua.version());
            window.set_title(&title);
            let selection = self
                .selection_anchor
                .zip(self.selection_focus)
                .filter(|(a, b)| a != b);
            if let Err(err) = renderer.render_with_selection(&self.config, &self.terminal, selection) {
                self.terminal.set_status(&format!("render error: {err}"));
            }
        }
    }

    fn apply_zoom_percent(&mut self, zoom_percent: u32, status: &str) {
        self.zoom_percent = zoom_percent.clamp(50, 400);
        self.config.cell_width = ((self.base_cell_width * self.zoom_percent) / 100).max(8);
        self.config.cell_height = ((self.base_cell_height * self.zoom_percent) / 100).max(12);
        if let Some(size) = self.window.as_ref().map(|window| window.inner_size()) {
            self.update_grid(size.width, size.height);
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        self.terminal.set_status(status);
    }

    fn handle_shortcut(&mut self, action: &str, event_loop: &ActiveEventLoop) -> bool {
        match action {
            "quit" => {
                event_loop.exit();
                true
            }
            "clear" => {
                self.terminal.clear();
                self.terminal.set_status("screen cleared locally");
                true
            }
            "demo" => {
                self.terminal.push_output("Lua and Rust are both live.");
                self.terminal.push_output("Type a command and press enter.");
                self.terminal.set_status("demo injected");
                true
            }
            "reload" => {
                match self.lua.reload() {
                    Ok((lua, config)) => {
                        self.lua = lua;
                        self.config = config;
                        self.base_cell_width = self.config.cell_width;
                        self.base_cell_height = self.config.cell_height;
                        self.zoom_percent = 100;
                        self.terminal.apply_config(&self.config);
                        self.terminal
                            .set_footer_context(self.pty.shell_label(), self.pty.cwd_label());
                        if let Some(window) = self.window.as_ref() {
                            let size = window.inner_size();
                            self.update_grid(size.width, size.height);
                        }
                        self.terminal.set_status(&format!(
                            "reloaded {}",
                            self.lua.config_label()
                        ));
                    }
                    Err(err) => self.terminal.set_status(&format!("reload failed: {err}")),
                }
                true
            }
            "zoom_in" => {
                self.apply_zoom_percent(self.zoom_percent.saturating_add(10), "zoomed in");
                true
            }
            "zoom_out" => {
                self.apply_zoom_percent(self.zoom_percent.saturating_sub(10), "zoomed out");
                true
            }
            "zoom_reset" => {
                self.apply_zoom_percent(100, "zoom reset");
                true
            }
            "command_mode" => {
                self.terminal.enter_command_mode();
                self.terminal.set_status("command mode");
                true
            }
            _ => false,
        }
    }

    fn drain_pty(&mut self) -> bool {
        let chunks = self.pty.try_read();
        let had_output = !chunks.is_empty();
        for chunk in chunks {
            self.terminal.append_output_chunk(&chunk);
        }
        let outbound = self.terminal.take_outbound();
        for reply in outbound {
            if let Err(err) = self.pty.write_bytes(&reply) {
                self.terminal.set_status(&format!("pty reply failed: {err}"));
            }
        }
        let clipboard_commands = self.terminal.take_clipboard_commands();
        for command in clipboard_commands {
            match command {
                ClipboardCommand::Set(text) => {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        if let Err(err) = clipboard.set_text(text) {
                            self.terminal
                                .set_status(&format!("clipboard write failed: {err}"));
                        }
                    }
                }
                ClipboardCommand::Query => {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        match clipboard.get_text() {
                            Ok(text) => self.terminal.respond_clipboard_query(&text),
                            Err(err) => self
                                .terminal
                                .set_status(&format!("clipboard read failed: {err}")),
                        }
                    }
                }
            }
        }
        had_output
    }

    fn write_pty_bytes(&mut self, bytes: &[u8], status: &str) {
        if let Err(err) = self.pty.write_bytes(bytes) {
            self.terminal.set_status(&format!("pty write failed: {err}"));
        } else {
            self.terminal.set_status(status);
        }
    }

    fn write_pty_bytes_quiet(&mut self, bytes: &[u8]) {
        if let Err(err) = self.pty.write_bytes(bytes) {
            self.terminal.set_status(&format!("pty write failed: {err}"));
        }
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    fn pointer_to_cell(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let px = x - self.config.padding as f64;
        let py = y - self.config.padding as f64;
        if px < 0.0 || py < 0.0 {
            return None;
        }
        let col = (px / self.config.cell_width as f64).floor() as usize;
        let row = (py / self.config.cell_height as f64).floor() as usize;
        Some((row, col))
    }

    fn selection_cell_from_pointer(&self, x: f64, y: f64, clamp_to_content: bool) -> Option<(usize, usize)> {
        let (row, col) = self.pointer_to_cell(x, y)?;
        self.terminal.selection_cell(row, col, clamp_to_content)
    }

    fn copy_selection(&mut self) {
        if let (Some(start), Some(end), Some(clipboard)) = (
            self.selection_anchor,
            self.selection_focus,
            self.clipboard.as_mut(),
        ) {
            let text = self.terminal.selected_text(start, end);
            if !text.is_empty() && clipboard.set_text(text).is_ok() {
                self.terminal.set_status("selection copied");
            }
        }
    }

    fn paste_clipboard(&mut self) {
        if let Some(clipboard) = self.clipboard.as_mut() {
            match clipboard.get_text() {
                Ok(text) => {
                    if self.terminal.in_command_mode() {
                        self.terminal.handle_command_text(&text);
                        self.terminal.set_status("pasted into lua command mode");
                    } else {
                        if self.terminal.bracketed_paste_enabled() {
                            let wrapped = format!("\x1b[200~{}\x1b[201~", text);
                            self.write_pty_bytes(wrapped.as_bytes(), "bracketed paste");
                        } else {
                            self.write_pty_bytes(text.as_bytes(), "pasted into shell");
                        }
                    }
                }
                Err(err) => self.terminal.set_status(&format!("clipboard read failed: {err}")),
            }
        } else {
            self.terminal.set_status("clipboard unavailable");
        }
    }

    fn handle_text_input(&mut self, text: &str) {
        if text.is_empty() || text.chars().all(char::is_control) {
            return;
        }
        if self.modifiers.super_key() || self.modifiers.control_key() {
            return;
        }
        self.clear_selection();
        if self.terminal.in_command_mode() {
            self.terminal.handle_command_text(text);
        } else if self.modifiers.alt_key() {
            let mut bytes = vec![0x1b];
            bytes.extend_from_slice(text.as_bytes());
            self.write_pty_bytes_quiet(&bytes);
        } else {
            self.write_pty_bytes_quiet(text.as_bytes());
        }
    }

    fn matches_shortcut(shortcut: &Shortcut, modifiers: ModifiersState, key: &Key) -> bool {
        let shortcut_key = shortcut.key.to_lowercase();
        let key_matches = match key {
            Key::Character(text) => text.to_lowercase() == shortcut_key,
            Key::Named(NamedKey::Escape) => shortcut_key == "escape",
            _ => false,
        };
        if !key_matches {
            return false;
        }

        shortcut.modifiers.iter().all(|modifier| match modifier.as_str() {
            "SUPER" => modifiers.super_key(),
            "SHIFT" => modifiers.shift_key(),
            "ALT" => modifiers.alt_key(),
            "CTRL" => modifiers.control_key(),
            _ => false,
        })
    }

    fn encode_mouse_report(&self, button: u8, row: usize, col: usize, release: bool) -> Vec<u8> {
        let x = col.saturating_add(1);
        let y = row.saturating_add(1);
        if self.terminal.mouse_sgr_enabled() {
            let suffix = if release { 'm' } else { 'M' };
            format!("\x1b[<{};{};{}{}", button, x, y, suffix).into_bytes()
        } else {
            let cb = (32 + button as usize).min(255) as u8;
            let cx = (32 + x).min(255) as u8;
            let cy = (32 + y).min(255) as u8;
            vec![0x1b, b'[', b'M', cb, cx, cy]
        }
    }

    fn arrow_sequence<'a>(&self, normal: &'a [u8], application: &'a [u8]) -> &'a [u8] {
        if self.terminal.application_cursor_keys() {
            application
        } else {
            normal
        }
    }

    fn modifier_param(&self) -> Option<u8> {
        let mut value = 1u8;
        if self.modifiers.shift_key() {
            value += 1;
        }
        if self.modifiers.alt_key() {
            value += 2;
        }
        if self.modifiers.control_key() {
            value += 4;
        }
        (value > 1).then_some(value)
    }

    fn csi_modified(&self, suffix: &str) -> Vec<u8> {
        if let Some(mods) = self.modifier_param() {
            format!("\x1b[1;{}{}", mods, suffix).into_bytes()
        } else {
            format!("\x1b[{}", suffix).into_bytes()
        }
    }

    fn ss3_or_csi(&self, ss3: u8, csi_suffix: &str) -> Vec<u8> {
        if self.terminal.application_cursor_keys() && self.modifier_param().is_none() {
            vec![0x1b, b'O', ss3]
        } else {
            self.csi_modified(csi_suffix)
        }
    }

    fn modified_tilde(&self, code: u8) -> Vec<u8> {
        if let Some(mods) = self.modifier_param() {
            format!("\x1b[{};{}~", code, mods).into_bytes()
        } else {
            format!("\x1b[{}~", code).into_bytes()
        }
    }
}

impl ApplicationHandler for VTermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        #[cfg(target_os = "macos")]
        install_macos_menu_bar();

        let attrs = WindowAttributes::default()
            .with_title(self.terminal.title().to_string())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.width as f64,
                self.config.height as f64,
            ));

        let window = event_loop
            .create_window(attrs)
            .expect("failed to create macOS window");
        let boxed = Box::new(window);
        let window_ref: &'static Window = Box::leak(boxed);
        window_ref.set_ime_allowed(true);
        self.update_grid(self.config.width, self.config.height);
        let mut renderer = Renderer::new(window_ref).expect("failed to create renderer");
        renderer
            .resize(self.config.width, self.config.height)
            .expect("failed to size renderer");
        self.renderer = Some(renderer);
        self.window = Some(window_ref);
        window_ref.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.drain_pty() {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(focused) => {
                if self.terminal.focus_reporting_enabled() {
                    let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
                    self.write_pty_bytes_quiet(seq);
                }
            }
            WindowEvent::Resized(size) => {
                self.update_grid(size.width, size.height);
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(err) = renderer.resize(size.width, size.height) {
                        self.terminal.set_status(&format!("resize error: {err}"));
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_cursor_pos = Some((position.x, position.y));
                if self.terminal.mouse_reporting_enabled() {
                    if self.terminal.mouse_motion_enabled() && self.mouse_left_down {
                        if let Some((row, col)) = self.pointer_to_cell(position.x, position.y) {
                            let report = self.encode_mouse_report(32, row, col, false);
                            self.write_pty_bytes_quiet(&report);
                        }
                    }
                } else if self.mouse_left_down && self.selection_anchor.is_some() {
                    self.selection_focus = self.selection_cell_from_pointer(position.x, position.y, true);
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    self.mouse_left_down = state == ElementState::Pressed;
                    if self.terminal.mouse_reporting_enabled() {
                        if let Some((x, y)) = self.last_cursor_pos {
                            if let Some((row, col)) = self.pointer_to_cell(x, y) {
                                let button = if state == ElementState::Pressed { 0 } else { 3 };
                                let release = state == ElementState::Released;
                                let report = self.encode_mouse_report(button, row, col, release);
                                self.write_pty_bytes_quiet(&report);
                            }
                        }
                    } else {
                        match state {
                            ElementState::Pressed => {
                                if let Some((x, y)) = self.last_cursor_pos {
                                    let cell = self.selection_cell_from_pointer(x, y, false);
                                    if cell.is_some() {
                                        self.selection_anchor = cell;
                                        self.selection_focus = cell;
                                    } else {
                                        self.clear_selection();
                                    }
                                }
                            }
                            ElementState::Released => {
                                self.copy_selection();
                                self.mouse_left_down = false;
                            }
                        }
                    }
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.handle_text_input(&text);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if let Some(text) = event.text.as_deref() {
                        self.handle_text_input(text);
                    }
                    let shortcut_action = self
                        .config
                        .shortcuts
                        .iter()
                        .find(|shortcut| {
                            Self::matches_shortcut(shortcut, self.modifiers, &event.logical_key)
                        })
                        .map(|shortcut| shortcut.action.clone());
                    if let Some(action) = shortcut_action {
                        if self.handle_shortcut(&action, event_loop) {
                            if let Some(window) = self.window.as_ref() {
                                window.request_redraw();
                            }
                            return;
                        }
                    }

                    match &event.logical_key {
                        Key::Named(NamedKey::Enter) => {
                            self.clear_selection();
                            if self.terminal.in_command_mode() {
                                let input = self.terminal.take_command_input();
                                self.terminal.push_output(&format!(":{input}"));
                                match self.lua.run_command(&input) {
                                    Ok(lines) => {
                                        for line in lines {
                                            self.terminal.push_output(&line);
                                        }
                                        self.terminal.set_status("lua command executed");
                                    }
                                    Err(err) => self.terminal.set_status(&format!("lua error: {err}")),
                                }
                            } else {
                                self.write_pty_bytes(b"\n", "sent newline to shell");
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            self.clear_selection();
                            if self.terminal.in_command_mode() {
                                self.terminal.backspace();
                            } else if self.modifiers.super_key() {
                                self.write_pty_bytes(&[0x15], "deleted to start of line");
                            } else if self.modifiers.alt_key() {
                                self.write_pty_bytes(b"\x1b\x7f", "deleted previous word");
                            } else {
                                self.write_pty_bytes(&[0x7f], "sent backspace to shell");
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            if self.terminal.in_command_mode() {
                                self.terminal.previous_history();
                            } else if self.modifiers.super_key() {
                                self.terminal.scroll_viewport(3);
                                self.terminal.set_status("scrolled viewport up");
                            } else {
                                let seq = self.ss3_or_csi(b'A', "A");
                                self.write_pty_bytes(&seq, "sent up arrow to shell");
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            if self.terminal.in_command_mode() {
                                self.terminal.next_history();
                            } else if self.modifiers.super_key() {
                                self.terminal.scroll_viewport(-3);
                                self.terminal.set_status("scrolled viewport down");
                            } else {
                                let seq = self.ss3_or_csi(b'B', "B");
                                self.write_pty_bytes(&seq, "sent down arrow to shell");
                            }
                        }
                        Key::Named(NamedKey::PageUp) => {
                            self.clear_selection();
                            if self.modifiers.super_key() {
                                self.terminal.page_up();
                                self.terminal.set_status("page up");
                            } else {
                                let seq = self.modified_tilde(5);
                                self.write_pty_bytes(&seq, "sent page up to shell");
                            }
                        }
                        Key::Named(NamedKey::PageDown) => {
                            self.clear_selection();
                            if self.modifiers.super_key() {
                                self.terminal.page_down();
                                self.terminal.set_status("page down");
                            } else {
                                let seq = self.modified_tilde(6);
                                self.write_pty_bytes(&seq, "sent page down to shell");
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if self.modifiers.super_key() {
                                let big = self.config.height as isize;
                                self.terminal.scroll_viewport(big);
                                self.terminal.set_status("jumped toward top of scrollback");
                            } else {
                                let seq = self.ss3_or_csi(b'H', "H");
                                self.write_pty_bytes(&seq, "sent home to shell");
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if self.modifiers.super_key() {
                                self.terminal.scroll_to_bottom();
                                self.terminal.set_status("returned to live output");
                            } else {
                                let seq = self.ss3_or_csi(b'F', "F");
                                self.write_pty_bytes(&seq, "sent end to shell");
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            let seq = self.ss3_or_csi(b'D', "D");
                            self.write_pty_bytes(&seq, "sent left arrow to shell");
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            let seq = self.ss3_or_csi(b'C', "C");
                            self.write_pty_bytes(&seq, "sent right arrow to shell");
                        }
                        Key::Named(NamedKey::Tab) => {
                            if self.modifiers.shift_key() {
                                self.write_pty_bytes(b"\x1b[Z", "sent shift-tab to shell");
                            } else {
                                self.write_pty_bytes(b"\t", "sent tab to shell");
                            }
                        }
                        Key::Named(NamedKey::Delete) => {
                            if self.modifiers.super_key() {
                                self.write_pty_bytes(&[0x0b], "deleted to end of line");
                            } else if self.modifiers.alt_key() {
                                self.write_pty_bytes(b"\x1bd", "deleted next word");
                            } else {
                                let seq = self.modified_tilde(3);
                                self.write_pty_bytes(&seq, "sent delete to shell");
                            }
                        }
                        Key::Named(NamedKey::Insert) => {
                            let seq = self.modified_tilde(2);
                            self.write_pty_bytes(&seq, "sent insert to shell");
                        }
                        Key::Named(NamedKey::F1) => {
                            let seq = if self.modifier_param().is_some() {
                                self.modified_tilde(11)
                            } else {
                                b"\x1bOP".to_vec()
                            };
                            self.write_pty_bytes(&seq, "sent f1 to shell");
                        }
                        Key::Named(NamedKey::F2) => {
                            let seq = if self.modifier_param().is_some() {
                                self.modified_tilde(12)
                            } else {
                                b"\x1bOQ".to_vec()
                            };
                            self.write_pty_bytes(&seq, "sent f2 to shell");
                        }
                        Key::Named(NamedKey::F3) => {
                            let seq = if self.modifier_param().is_some() {
                                self.modified_tilde(13)
                            } else {
                                b"\x1bOR".to_vec()
                            };
                            self.write_pty_bytes(&seq, "sent f3 to shell");
                        }
                        Key::Named(NamedKey::F4) => {
                            let seq = if self.modifier_param().is_some() {
                                self.modified_tilde(14)
                            } else {
                                b"\x1bOS".to_vec()
                            };
                            self.write_pty_bytes(&seq, "sent f4 to shell");
                        }
                        Key::Named(NamedKey::F5) => {
                            let seq = self.modified_tilde(15);
                            self.write_pty_bytes(&seq, "sent f5 to shell");
                        }
                        Key::Named(NamedKey::F6) => {
                            let seq = self.modified_tilde(17);
                            self.write_pty_bytes(&seq, "sent f6 to shell");
                        }
                        Key::Named(NamedKey::F7) => {
                            let seq = self.modified_tilde(18);
                            self.write_pty_bytes(&seq, "sent f7 to shell");
                        }
                        Key::Named(NamedKey::F8) => {
                            let seq = self.modified_tilde(19);
                            self.write_pty_bytes(&seq, "sent f8 to shell");
                        }
                        Key::Named(NamedKey::F9) => {
                            let seq = self.modified_tilde(20);
                            self.write_pty_bytes(&seq, "sent f9 to shell");
                        }
                        Key::Named(NamedKey::F10) => {
                            let seq = self.modified_tilde(21);
                            self.write_pty_bytes(&seq, "sent f10 to shell");
                        }
                        Key::Named(NamedKey::F11) => {
                            let seq = self.modified_tilde(23);
                            self.write_pty_bytes(&seq, "sent f11 to shell");
                        }
                        Key::Named(NamedKey::F12) => {
                            let seq = self.modified_tilde(24);
                            self.write_pty_bytes(&seq, "sent f12 to shell");
                        }
                        Key::Named(NamedKey::Escape) => {
                            if self.selection_anchor.is_some() || self.selection_focus.is_some() {
                                self.clear_selection();
                                self.terminal.set_status("selection cleared");
                            } else if self.terminal.in_command_mode() {
                                self.terminal.cancel_command_mode();
                                self.terminal.set_status("left lua command mode");
                            } else {
                                self.write_pty_bytes(&[0x1b], "sent escape to shell");
                            }
                        }
                        _ => {}
                    }

                    if self.modifiers.control_key() {
                        match &event.logical_key {
                            Key::Character(ch) if ch.eq_ignore_ascii_case("c") => {
                                self.write_pty_bytes(&[0x03], "sent ctrl-c to shell");
                            }
                            Key::Character(ch) if ch.eq_ignore_ascii_case("d") => {
                                self.write_pty_bytes(&[0x04], "sent ctrl-d to shell");
                            }
                            Key::Character(ch) if ch.len() == 1 => {
                                if let Some(byte) = ctrl_byte(ch.chars().next().unwrap()) {
                                    self.write_pty_bytes(&[byte], "sent ctrl-key to shell");
                                }
                            }
                            _ => {}
                        }
                    }

                    if self.modifiers.super_key() {
                        match &event.logical_key {
                            Key::Character(ch) if ch.eq_ignore_ascii_case("c") => self.copy_selection(),
                            Key::Character(ch) if ch.eq_ignore_ascii_case("v") => self.paste_clipboard(),
                            _ => {}
                        }
                    }

                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.round() as isize,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 24.0).round() as isize,
                };
                if amount != 0 {
                    if self.terminal.mouse_reporting_enabled() {
                        if let Some((x, y)) = self.last_cursor_pos {
                            if let Some((row, col)) = self.pointer_to_cell(x, y) {
                                let button = if amount > 0 { 64 } else { 65 };
                                let report =
                                    self.encode_mouse_report(button, row, col, false);
                                self.write_pty_bytes_quiet(&report);
                            }
                        }
                    } else if self.terminal.alternate_scroll_enabled() {
                        let steps = amount.unsigned_abs().max(1);
                        let seq = if amount > 0 {
                            self.arrow_sequence(b"\x1b[A", b"\x1bOA").to_vec()
                        } else {
                            self.arrow_sequence(b"\x1b[B", b"\x1bOB").to_vec()
                        };
                        for _ in 0..steps {
                            self.write_pty_bytes_quiet(&seq);
                        }
                    } else {
                        self.terminal.scroll_viewport(amount.saturating_mul(3));
                        self.terminal.set_status("mouse wheel scroll");
                    }
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn bundled_bootstrap_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let mut candidates = Vec::new();

    if let Some(exe_dir) = exe.parent() {
        candidates.push(exe_dir.join("../Resources/bootstrap.lua"));
        candidates.push(exe_dir.join("../../lua/bootstrap.lua"));
        candidates.push(exe_dir.join("../lua/bootstrap.lua"));
    }

    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    candidates.push(cwd.join("../lua/bootstrap.lua"));
    candidates.push(cwd.join("lua/bootstrap.lua"));

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    Err(anyhow::anyhow!(
        "failed to locate bootstrap.lua relative to executable or current directory"
    ))
}

fn user_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let candidates = [
        config_home.join("vterm/config.lua"),
        home.join("Library/Application Support/VTerm/config.lua"),
    ];
    candidates.into_iter().find(|path| path.exists()).or_else(|| {
        Some(config_home.join("vterm/config.lua"))
    })
}

fn ctrl_byte(ch: char) -> Option<u8> {
    match ch {
        '@' | ' ' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        c if c.is_ascii_alphabetic() => Some((c.to_ascii_uppercase() as u8) - b'@'),
        _ => None,
    }
}

fn main() -> Result<()> {
    let bundled_bootstrap = bundled_bootstrap_path()?;
    let user_config = user_config_path();
    let (lua, config) = LuaRuntime::new(&bundled_bootstrap, user_config.clone())
        .map_err(|err| anyhow::anyhow!("failed to load bootstrap config: {err}"))?;
    let cols = ((config.width.saturating_sub(config.padding * 2) / config.cell_width).max(1))
        .min(u16::MAX as u32) as u16;
    let usable_height = config
        .height
        .saturating_sub(config.padding * 2)
        .saturating_sub(config.cell_height * 2);
    let rows = ((usable_height / config.cell_height).max(1))
        .min(u16::MAX as u32) as u16;
    let pty = PtySession::new(cols, rows)?;

    let event_loop = EventLoop::new()?;
    let mut app = VTermApp::new(config, lua, pty);
    event_loop.run_app(&mut app)?;
    Ok(())
}

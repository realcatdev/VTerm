use std::collections::HashMap;
use std::fs;
use std::num::NonZeroU32;

use anyhow::{Context, Result};
use fontdue::{Font, FontSettings, Metrics};
use softbuffer::{Context as SoftContext, Surface};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use winit::window::Window;

use crate::config::AppConfig;
use crate::terminal::{Cell, TerminalBuffer};

const PRIMARY_FONT_PATH: &str = "/System/Library/Fonts/SFNSMono.ttf";
const FALLBACK_FONT_PATHS: &[&str] = &["/System/Library/Fonts/Monaco.ttf"];

pub struct Renderer {
    context: SoftContext<&'static Window>,
    surface: Surface<&'static Window, &'static Window>,
    width: u32,
    height: u32,
    primary_font: Font,
    fallback_fonts: Vec<Font>,
    glyph_cache: HashMap<(usize, char, u32), (Metrics, Vec<u8>)>,
}

impl Renderer {
    pub fn new(window: &'static Window) -> Result<Self> {
        let context = SoftContext::new(window)
            .map_err(|err| anyhow::anyhow!("failed to create softbuffer context: {err}"))?;
        let surface = Surface::new(&context, window)
            .map_err(|err| anyhow::anyhow!("failed to create surface: {err}"))?;
        let primary_font = load_font(PRIMARY_FONT_PATH)
            .with_context(|| format!("failed to load primary font at {PRIMARY_FONT_PATH}"))?;
        let fallback_fonts = FALLBACK_FONT_PATHS
            .iter()
            .filter_map(|path| load_font(path).ok())
            .collect::<Vec<_>>();
        Ok(Self {
            context,
            surface,
            width: 1,
            height: 1,
            primary_font,
            fallback_fonts,
            glyph_cache: HashMap::new(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.width = width.max(1);
        self.height = height.max(1);
        let _ = &self.context;
        self.surface
            .resize(
                NonZeroU32::new(self.width).unwrap(),
                NonZeroU32::new(self.height).unwrap(),
            )
            .map_err(|err| anyhow::anyhow!("failed to resize surface: {err}"))
    }

    pub fn render_with_selection(
        &mut self,
        config: &AppConfig,
        terminal: &TerminalBuffer,
        selection: Option<((usize, usize), (usize, usize))>,
    ) -> Result<()> {
        let width = self.width;
        let height = self.height;
        let mut frame = vec![0u32; (width as usize).saturating_mul(height as usize)];
        let pixels = frame.as_mut_slice();
        fill_background(pixels, width, height, config.background);

        let cols = (width.saturating_sub(config.padding * 2) / config.cell_width) as usize;

        let rows_to_draw = terminal.render_cells();

        for (row, line) in rows_to_draw.iter().enumerate() {
            self.draw_row(
                pixels,
                width,
                config,
                line,
                row,
                selection,
                config.padding,
                config.padding + row as u32 * config.cell_height,
            );
        }

        let status_y = footer_y(height, config);
        let status_left = truncate(&terminal.status_line_left(), cols);
        let status_right = truncate(&terminal.status_line_right(), cols / 2);
        self.draw_text_line(
            pixels,
            width,
            config.padding,
            status_y,
            config.cell_width,
            cols as u32 * config.cell_width,
            config.cell_height.saturating_sub(2),
            &status_left,
            mix(config.foreground, config.background, 0.25),
            false,
        );
        if !status_right.is_empty() {
            self.draw_text_line(
                pixels,
                width,
                config.padding,
                status_y,
                config.cell_width,
                cols as u32 * config.cell_width,
                config.cell_height.saturating_sub(2),
                &status_right,
                mix(config.foreground, config.background, 0.25),
                true,
            );
        }

        let (cursor_row, cursor_col) = terminal.cursor_for_render();
        if selection.is_none() && terminal.cursor_visible_for_render() {
            let cursor_x = config.padding + cursor_col as u32 * config.cell_width;
            let cursor_y = config.padding + cursor_row as u32 * config.cell_height;
            let cursor_color = mix(config.accent, 0xffffff, 0.2);
            match terminal.cursor_style() {
                3 | 4 => draw_rect(
                    pixels,
                    width,
                    cursor_x,
                    cursor_y + config.cell_height / 2,
                    config.cell_width.saturating_sub(1),
                    2,
                    cursor_color,
                ),
                5 | 6 => draw_rect(
                    pixels,
                    width,
                    cursor_x,
                    cursor_y,
                    2,
                    config.cell_height,
                    cursor_color,
                ),
                _ => draw_rect(
                    pixels,
                    width,
                    cursor_x,
                    cursor_y,
                    config.cell_width.saturating_sub(1),
                    config.cell_height,
                    cursor_color,
                ),
            }
            if let Some(line) = rows_to_draw.get(cursor_row) {
                if let Some(cell) = line.get(cursor_col) {
                    if !matches!(terminal.cursor_style(), 3 | 4 | 5 | 6) {
                        self.draw_cluster(
                            pixels,
                            width,
                            cursor_x,
                            cursor_y,
                            config.cell_width,
                            config.cell_height,
                            &cell.text,
                            config.background,
                        );
                    }
                }
            }
        }

        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|err| anyhow::anyhow!("failed to map surface buffer: {err}"))?;
        buffer.as_mut().copy_from_slice(&frame);
        buffer
            .present()
            .map_err(|err| anyhow::anyhow!("failed to present frame: {err}"))
    }

    fn draw_row(
        &mut self,
        pixels: &mut [u32],
        width: u32,
        config: &AppConfig,
        row: &[Cell],
        row_index: usize,
        selection: Option<((usize, usize), (usize, usize))>,
        x: u32,
        y: u32,
    ) {
        let mut idx = 0usize;
        while idx < row.len() {
            let cell = &row[idx];
            let cell_x = x + idx as u32 * config.cell_width;
            let selected = selection
                .map(|(start, end)| selection_contains(start, end, row_index, idx))
                .unwrap_or(false);
            let bg = if selected {
                mix(cell.bg, 0x214d7a, 0.9)
            } else {
                cell.bg
            };
            let fg = if selected { 0xf5f9ff } else { cell.fg };
            let span = cell.span.max(1);
            for offset in 0..span {
                draw_rect(
                    pixels,
                    width,
                    x + (idx + offset) as u32 * config.cell_width,
                    y,
                    config.cell_width,
                    config.cell_height,
                    bg,
                );
            }
            if !cell.wide_continuation && !cell.text.trim().is_empty() {
                self.draw_cluster(
                    pixels,
                    width,
                    cell_x,
                    y,
                    config.cell_width * span as u32,
                    config.cell_height,
                    &cell.text,
                    fg,
                );
                idx += span;
            } else {
                idx += 1;
            }
        }
    }

    fn draw_text_line(
        &mut self,
        pixels: &mut [u32],
        width: u32,
        x: u32,
        y: u32,
        cell_width: u32,
        line_width: u32,
        line_height: u32,
        text: &str,
        color: u32,
        right_align: bool,
    ) {
        if text.trim().is_empty() {
            return;
        }
        let total_width = grapheme_display_width(text).max(1) as u32 * cell_width.max(1);
        let origin_x = if right_align {
            x + line_width.saturating_sub(total_width)
        } else {
            x
        };
        let mut col = 0u32;
        let slot_width = cell_width.max(1);
        for grapheme in text.graphemes(true) {
            let width_cells = grapheme_display_width(grapheme).max(1) as u32;
            self.draw_cluster(
                pixels,
                width,
                origin_x + col * slot_width.max(1),
                y,
                (slot_width * width_cells).max(cell_width),
                line_height.saturating_sub(1),
                grapheme,
                color,
            );
            col += width_cells;
        }
    }

    fn draw_cluster(
        &mut self,
        pixels: &mut [u32],
        width: u32,
        x: u32,
        y: u32,
        cell_width: u32,
        cell_height: u32,
        text: &str,
        color: u32,
    ) {
        if text.is_empty() || text.trim().is_empty() {
            return;
        }
        let font_px = ((cell_height as f32) * 0.74)
            .min((cell_width as f32) * 1.2)
            .max(9.0)
            .round() as u32;
        let bottom_padding = ((cell_height as f32) * 0.12).round();
        let primary_line = self
            .primary_font
            .horizontal_line_metrics(font_px as f32);
        let baseline_y = primary_line
            .map(|line| y as f32 + cell_height as f32 - bottom_padding + line.descent)
            .unwrap_or_else(|| y as f32 + cell_height as f32 - bottom_padding);

        let mut glyphs = Vec::new();
        let mut run_width = 0f32;
        for ch in text.chars() {
            let font_index = self.choose_font_index(ch);
            let (metrics, bitmap) = self.cached_glyph(font_index, ch, font_px);
            let advance = if metrics.advance_width > 0.0 {
                metrics.advance_width
            } else {
                metrics.width as f32
            };
            glyphs.push((metrics, bitmap, advance));
            run_width += advance;
        }
        if glyphs.is_empty() {
            return;
        }

        let cluster_width = cell_width as f32;
        let mut pen_x = x as f32 + ((cluster_width - run_width).max(0.0) * 0.5);
        let min_x = x as f32 + ((cell_width as f32) * 0.04);
        if pen_x < min_x {
            pen_x = min_x;
        }

        for (metrics, bitmap, advance) in glyphs {
            if metrics.width > 0 && metrics.height > 0 {
                let draw_x = (pen_x + metrics.xmin as f32).round() as i32;
                let draw_y = baseline_y.round() as i32 - metrics.height as i32 - metrics.ymin;
                for gy in 0..metrics.height {
                    for gx in 0..metrics.width {
                        let alpha = bitmap[gy * metrics.width + gx];
                        if alpha == 0 {
                            continue;
                        }
                        blend_pixel(
                            pixels,
                            width,
                            draw_x + gx as i32,
                            draw_y + gy as i32,
                            color,
                            alpha,
                        );
                    }
                }
            }
            pen_x += advance;
        }
    }

    fn font_by_index(&self, index: usize) -> &Font {
        if index == 0 {
            &self.primary_font
        } else {
            &self.fallback_fonts[index - 1]
        }
    }

    fn choose_font_index(&self, ch: char) -> usize {
        if self.primary_font.lookup_glyph_index(ch) != 0 {
            return 0;
        }
        self.fallback_fonts
            .iter()
            .position(|font| font.lookup_glyph_index(ch) != 0)
            .map(|idx| idx + 1)
            .unwrap_or(0)
    }

    fn cached_glyph(
        &mut self,
        font_index: usize,
        ch: char,
        font_px: u32,
    ) -> (Metrics, Vec<u8>) {
        let key = (font_index, ch, font_px);
        if let Some((metrics, bitmap)) = self.glyph_cache.get(&key) {
            return (*metrics, bitmap.clone());
        }
        let font = self.font_by_index(font_index);
        let glyph = if font.lookup_glyph_index(ch) != 0 { ch } else { '?' };
        let rasterized = font.rasterize(glyph, font_px as f32);
        self.glyph_cache.insert(key, rasterized.clone());
        rasterized
    }
}

fn load_font(path: &str) -> Result<Font> {
    let bytes = fs::read(path)?;
    Font::from_bytes(bytes, FontSettings::default())
        .map_err(|err| anyhow::anyhow!("failed to parse font {path}: {err}"))
}

fn truncate(input: &str, width: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    for grapheme in input.graphemes(true) {
        let w = grapheme_display_width(grapheme);
        if used + w > width {
            break;
        }
        used += w;
        out.push_str(grapheme);
    }
    out
}

fn argb(rgb: u32) -> u32 {
    0xff00_0000 | rgb
}

fn footer_y(height: u32, config: &AppConfig) -> u32 {
    height
        .saturating_sub(config.padding)
        .saturating_sub(config.cell_height)
}

fn fill_background(pixels: &mut [u32], width: u32, height: u32, background: u32) {
    if width == 0 || height == 0 {
        return;
    }
    let color = argb(background);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            pixels[idx] = color;
        }
    }
}

fn selection_contains(
    start: (usize, usize),
    end: (usize, usize),
    row: usize,
    col: usize,
) -> bool {
    let (start, end) = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
        (start, end)
    } else {
        (end, start)
    };
    if row < start.0 || row > end.0 {
        return false;
    }
    if start.0 == end.0 {
        return row == start.0 && col >= start.1 && col <= end.1;
    }
    if row == start.0 {
        return col >= start.1;
    }
    if row == end.0 {
        return col <= end.1;
    }
    true
}

fn grapheme_display_width(grapheme: &str) -> usize {
    UnicodeWidthStr::width(grapheme).max(1)
}

fn draw_rect(
    pixels: &mut [u32],
    width: u32,
    x: u32,
    y: u32,
    rect_w: u32,
    rect_h: u32,
    color: u32,
) {
    if width == 0 {
        return;
    }
    let height = pixels.len() as u32 / width;
    let x_end = x.saturating_add(rect_w).min(width);
    let y_end = y.saturating_add(rect_h).min(height);
    for py in y..y_end {
        for px in x..x_end {
            let idx = (py * width + px) as usize;
            if let Some(pixel) = pixels.get_mut(idx) {
                *pixel = argb(color);
            }
        }
    }
}

fn blend_pixel(pixels: &mut [u32], width: u32, x: i32, y: i32, color: u32, alpha: u8) {
    if x < 0 || y < 0 || width == 0 {
        return;
    }
    let x = x as u32;
    let y = y as u32;
    let height = pixels.len() as u32 / width;
    if x >= width || y >= height {
        return;
    }
    let idx = (y * width + x) as usize;
    let dst = pixels[idx] & 0x00ff_ffff;
    pixels[idx] = argb(blend(dst, color, alpha as f32 / 255.0));
}

fn blend(dst: u32, src: u32, alpha: f32) -> u32 {
    let a = alpha.clamp(0.0, 1.0);
    let dr = ((dst >> 16) & 0xff) as f32;
    let dg = ((dst >> 8) & 0xff) as f32;
    let db = (dst & 0xff) as f32;
    let sr = ((src >> 16) & 0xff) as f32;
    let sg = ((src >> 8) & 0xff) as f32;
    let sb = (src & 0xff) as f32;
    (((dr + (sr - dr) * a) as u32) << 16)
        | (((dg + (sg - dg) * a) as u32) << 8)
        | ((db + (sb - db) * a) as u32)
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let clamp = t.clamp(0.0, 1.0);
    let ar = ((a >> 16) & 0xff) as f32;
    let ag = ((a >> 8) & 0xff) as f32;
    let ab = (a & 0xff) as f32;
    let br = ((b >> 16) & 0xff) as f32;
    let bg = ((b >> 8) & 0xff) as f32;
    let bb = (b & 0xff) as f32;
    let r = ar + (br - ar) * clamp;
    let g = ag + (bg - ag) * clamp;
    let blue = ab + (bb - ab) * clamp;
    ((r as u32) << 16) | ((g as u32) << 8) | blue as u32
}

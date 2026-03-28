use anyhow::{Context, Result};
use mlua::Table;

#[derive(Debug, Clone)]
pub struct Shortcut {
    pub key: String,
    pub modifiers: Vec<String>,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding: u32,
    pub background: u32,
    pub foreground: u32,
    pub accent: u32,
    pub banner: Vec<String>,
    pub shortcuts: Vec<Shortcut>,
}

fn parse_color(value: String) -> Result<u32> {
    let raw = value.trim().trim_start_matches('#');
    let parsed = u32::from_str_radix(raw, 16)
        .with_context(|| format!("invalid color value {value}"))?;
    Ok(parsed)
}

impl AppConfig {
    pub fn from_table(app: &Table) -> Result<Self> {
        let shortcuts = match app.get::<Table>("shortcuts") {
            Ok(items) => items
                .sequence_values::<Table>()
                .map(|value| {
                    let item = value?;
                    let modifiers = match item.get::<Table>("modifiers") {
                        Ok(mods) => mods
                            .sequence_values::<String>()
                            .collect::<mlua::Result<Vec<_>>>()?,
                        Err(_) => Vec::new(),
                    };
                    Ok(Shortcut {
                        key: item.get("key")?,
                        modifiers,
                        action: item.get("action")?,
                    })
                })
                .collect::<mlua::Result<Vec<_>>>()
                .map_err(|err| anyhow::anyhow!("invalid shortcuts table: {err}"))?,
            Err(_) => Vec::new(),
        };

        let banner = match app.get::<Table>("banner") {
            Ok(lines) => lines
                .sequence_values::<String>()
                .collect::<mlua::Result<Vec<_>>>()
                .map_err(|err| anyhow::anyhow!("invalid banner table: {err}"))?,
            Err(_) => Vec::new(),
        };

        Ok(Self {
            title: app
                .get("title")
                .map_err(|err| anyhow::anyhow!("missing app.title: {err}"))?,
            width: app
                .get("width")
                .map_err(|err| anyhow::anyhow!("missing app.width: {err}"))?,
            height: app
                .get("height")
                .map_err(|err| anyhow::anyhow!("missing app.height: {err}"))?,
            cell_width: app.get("cell_width").unwrap_or(16),
            cell_height: app.get("cell_height").unwrap_or(16),
            padding: app.get("padding").unwrap_or(20),
            background: parse_color(app.get("background").unwrap_or_else(|_| "111111".to_string()))?,
            foreground: parse_color(app.get("foreground").unwrap_or_else(|_| "f5f2e8".to_string()))?,
            accent: parse_color(app.get("accent").unwrap_or_else(|_| "e07a5f".to_string()))?,
            banner,
            shortcuts,
        })
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mlua::{Function, Lua, Table, Value};

use crate::config::AppConfig;

pub struct LuaRuntime {
    bundled_script_path: PathBuf,
    user_script_path: Option<PathBuf>,
    lua: Lua,
    on_command: Function,
}

impl LuaRuntime {
    pub fn new(bundled_script_path: &Path, user_script_path: Option<PathBuf>) -> Result<(Self, AppConfig)> {
        let lua = Lua::new();

        let bundled_exports = load_exports(&lua, bundled_script_path, "bundled bootstrap")?;
        let bundled_setup: Function = bundled_exports
            .get("setup")
            .map_err(|err| anyhow::anyhow!("bootstrap.lua must export setup(): {err}"))?;
        let merged_app: Table = bundled_setup
            .call(())
            .map_err(|err| anyhow::anyhow!("bootstrap setup() failed: {err}"))?;

        let mut on_command: Function = bundled_exports
            .get("on_command")
            .map_err(|err| anyhow::anyhow!("bootstrap.lua must export on_command(): {err}"))?;

        let active_user_path = user_script_path.filter(|path| path.exists());
        if let Some(path) = active_user_path.as_ref() {
            let user_exports = load_exports(&lua, path, "user config")?;
            if let Ok(setup) = user_exports.get::<Function>("setup") {
                let user_app: Table = setup
                    .call(())
                    .map_err(|err| anyhow::anyhow!("user config setup() failed: {err}"))?;
                merge_tables(&merged_app, &user_app)?;
            }
            if let Ok(user_on_command) = user_exports.get::<Function>("on_command") {
                on_command = user_on_command;
            }
        }

        let config = AppConfig::from_table(&merged_app)?;
        Ok((
            Self {
                bundled_script_path: bundled_script_path.to_path_buf(),
                user_script_path: active_user_path,
                lua,
                on_command,
            },
            config,
        ))
    }

    pub fn run_command(&self, input: &str) -> Result<Vec<String>> {
        let result: Table = self
            .on_command
            .call(input)
            .map_err(|err| anyhow::anyhow!("Lua on_command failed: {err}"))?;
        result
            .sequence_values::<String>()
            .collect::<mlua::Result<Vec<_>>>()
            .map_err(|err| anyhow::anyhow!("Lua on_command must return an array of strings: {err}"))
    }

    pub fn version(&self) -> &'static str {
        let _ = &self.lua;
        "LuaJIT"
    }

    pub fn config_label(&self) -> String {
        self.user_script_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.bundled_script_path.display().to_string())
    }

    pub fn reload(&self) -> Result<(Self, AppConfig)> {
        Self::new(&self.bundled_script_path, self.user_script_path.clone())
    }
}

fn load_exports(lua: &Lua, script_path: &Path, label: &str) -> Result<Table> {
    let source = fs::read_to_string(script_path)
        .with_context(|| format!("failed to read {label} at {}", script_path.display()))?;
    let chunk = lua.load(&source).set_name(script_path.to_string_lossy().as_ref());
    chunk
        .eval()
        .map_err(|err| anyhow::anyhow!("failed to evaluate {label}: {err}"))
}

fn merge_tables(base: &Table, overlay: &Table) -> Result<()> {
    for pair in overlay.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|err| anyhow::anyhow!("failed to iterate config table: {err}"))?;
        let existing = base
            .get::<Value>(key.clone())
            .map_err(|err| anyhow::anyhow!("failed to read config key during merge: {err}"))?;
        match (&existing, &value) {
            (Value::Table(base_table), Value::Table(overlay_table))
                if !is_sequence(base_table) && !is_sequence(overlay_table) =>
            {
                merge_tables(base_table, overlay_table)?;
            }
            _ => {
                base.set(key, value)
                    .map_err(|err| anyhow::anyhow!("failed to write config key during merge: {err}"))?;
            }
        }
    }
    Ok(())
}

fn is_sequence(table: &Table) -> bool {
    table.raw_len() > 0
}

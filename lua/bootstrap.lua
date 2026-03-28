local M = {}

function M.setup()
  return {
    title = "VTerm",
    width = 1280,
    height = 800,
    cell_width = 23,
    cell_height = 45,
    padding = 20,
    background = "1c1c1e",
    foreground = "f2f2f7",
    accent = "0a84ff",
    banner = {},
    shortcuts = {
      { key = ";", modifiers = { "SUPER" }, action = "command_mode" },
      { key = "=", modifiers = { "SUPER" }, action = "zoom_in" },
      { key = "-", modifiers = { "SUPER" }, action = "zoom_out" },
      { key = "0", modifiers = { "SUPER" }, action = "zoom_reset" },
      { key = "q", modifiers = { "SUPER" }, action = "quit" },
      { key = "k", modifiers = { "SUPER" }, action = "clear" },
      { key = "l", modifiers = { "SUPER" }, action = "demo" },
      { key = "r", modifiers = { "SUPER" }, action = "reload" },
    },
  }
end

function M.on_command(input)
  local trimmed = input:gsub("^%s+", ""):gsub("%s+$", "")
  if trimmed == "" then
    return { "" }
  end

  if trimmed == "help" then
    return {
      "Available commands:",
      "  help    show built-in commands",
      "  about   describe the rewrite scaffold",
      "  clear   handled by cmd+k",
      "  echo X  print text back into the buffer",
    }
  end

  if trimmed == "about" then
    return {
      "This shell is the first Rust/Lua slice of the VTerm rewrite.",
      "Rust owns the macOS app, event loop, buffer model, and renderer.",
      "Lua owns command behavior and can grow into config/plugins.",
    }
  end

  if trimmed == "reload" then
    return {
      "Use cmd+R to reload Lua config and commands without recompiling.",
    }
  end

  local echoed = trimmed:match("^echo%s+(.+)$")
  if echoed then
    return { echoed }
  end

  return {
    "unknown command: " .. trimmed,
    "try: help",
  }
end

return M

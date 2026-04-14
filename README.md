# VTerm

VTerm is a fast terminal for macOS written in Rust.

It uses Lua for configuration and command extensions, but the core app, renderer, PTY integration, and terminal model are all built around a Rust codebase.

Status: `beta`

Release bar: see [RELEASE_CHECKLIST.md](https://github.com/realcatdev/VTerm/blob/master/RELEASE_CHECKLIST.md)

## What Exists Now

- native macOS window via `winit`
- PTY-backed real shell session
- Rust terminal grid, parser, and scrollback model
- practical ANSI/VT, DEC, OSC, and DCS support for common shell and TUI workflows
- Lua bootstrap and user config surface
- local selection, clipboard, mouse reporting, and zoom
- packaged `VTerm.app` and portable alpha build output

## Interesting Implementation Details

- Unicode-aware rendering (grapheme clusters, emoji modifiers, wide characters)
- ANSI/VT parser with support for CSI, OSC, and DCS sequences
- Separate main/alternate screen buffers with scrollback
- Clipboard integration via OSC 52

## What Still Needs Work

- broader terminal compatibility validation beyond the current shell, `nvim`, `tmux`, and `less` passes
- more mature text shaping and layout behavior
- packaging/signing/notarization for a real macOS release
- longer stability and QA coverage

## Run From Source

```bash
cd app
cargo run
```

## Build The macOS App

```bash
./scripts/package_macos_app.sh
```

This creates:

- `dist/VTerm.app`
- `dist/VTerm-alpha/`

The app resolves `bootstrap.lua` relative to the executable, so both the bundle and the portable layout run outside the source checkout.

## User Config

User config lives at:

```bash
~/.config/vterm/config.lua
```

The bundled defaults live in [lua/bootstrap.lua](https://github.com/realcatdev/VTerm/blob/master/lua/bootstrap.lua), and user config overrides them on reload.

## Current Shortcuts

- `cmd+Q`: quit
- `cmd+R`: reload config
- `cmd+C`: copy selection
- `cmd+V`: paste
- `cmd+=`: zoom in
- `cmd+-`: zoom out
- `cmd+0`: reset zoom
- `cmd+;`: enter Lua command mode
- `opt+backspace`: delete previous word
- `cmd+backspace`: delete to start of line
- `opt+delete`: delete next word
- `cmd+delete`: delete to end of line

## Release Guidance

The current bar is “usable beta,” not `1.0`.

Before calling it `1.0`, the remaining work is:

- prove longer-term daily-driver stability
- harden rendering and zoom behavior
- expand compatibility coverage across more TUIs and terminal edge cases
- finish macOS release packaging

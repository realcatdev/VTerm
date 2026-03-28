# VTerm 1.0 Checklist

This is the bar for calling the Rust/Lua macOS rewrite `1.0`.

Current status: `beta`

## 1. Terminal Compatibility

- [ ] Validate interactive shell use across `zsh`, `bash`, and `fish`
- [ ] Validate full-screen TUIs: `vim` or `nvim`, `tmux`, `less`, `lazygit`, `btop`
- [ ] Close the major VT/DEC behavior gaps still handled only heuristically
- [ ] Improve terminal query/reply coverage beyond the current minimal CSI/OSC/DCS subset
- [ ] Confirm mouse, focus, alternate screen, resize, and clipboard behavior against real apps

Exit criteria:

- No obvious breakage in the core app matrix above
- No major cursor, redraw, or input corruption during ordinary TUI use

## 2. Text And Rendering

- [ ] Replace the current heuristic text path with a real shaping/layout engine
- [ ] Improve grapheme handling beyond the current best-effort cluster attachment
- [ ] Validate emoji, ZWJ sequences, variation selectors, combining marks, and CJK width behavior
- [ ] Harden font fallback and glyph selection for non-ASCII text
- [ ] Measure and improve render performance under scrollback and full-screen TUI load

Exit criteria:

- Text rendering is not based on ad hoc per-codepoint placement
- Unicode-heavy prompts and sample files render without obvious corruption

## 3. macOS Productization

- [ ] Make the `.app` bundle launch cleanly through LaunchServices
- [ ] Add a proper app icon and stable bundle metadata
- [ ] Add signing and notarization steps
- [ ] Make release packaging repeatable from one documented command path
- [ ] Decide the supported macOS baseline and verify it

Exit criteria:

- `VTerm.app` is the primary release artifact, not just the portable alpha layout
- A release build can be reproduced without manual patching

## 4. Quality And QA

- [ ] Keep unit coverage for parser and text model behavior growing with new features
- [ ] Add integration or smoke tests around PTY startup, resize, clipboard, and mouse reporting
- [ ] Create a manual QA pass for shell, TUI, Unicode, packaging, and crash behavior
- [ ] Run sustained manual sessions instead of only short launch probes
- [ ] Track known limitations explicitly before beta

Exit criteria:

- Regressions are caught by tests or a repeatable QA checklist before release
- There is a known-good manual validation pass for release candidates

## Suggested Milestones

### Alpha

- Current state
- Safe description: experimental macOS terminal rewrite

### Beta

- Core terminal matrix behaves reliably
- `.app` launch path is fixed
- Unicode/text behavior is materially stronger
- QA checklist exists and is run

### 1.0

- Comfortable daily-driver use on macOS
- Core TUI matrix passes without obvious breakage
- Text/layout path is no longer heuristic-driven
- Packaging, signing, docs, and release process are complete

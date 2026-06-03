# Input Spectrum TUI

`inputspectrum` turns keyboard and mouse input inside the focused terminal into a live spectrum wall. Each key press, mouse click, drag, movement, and wheel event injects energy into different bands; faster input produces taller and faster pulses.

Only the focused TUI terminal is monitored. The app enables terminal focus-change events and ignores keyboard/mouse events after focus is lost.

## Run

```bash
cargo run -- --fps 60 --bars 96 --theme cyber
```

## Controls

- `q` / `Esc`: quit
- `space`: pause or resume decay/render updates
- `tab`: switch mode (`bars`, `wave`, `peaks`)
- `1`, `2`, `3`: switch theme (`cyber`, `mono`, `amber`)
- `+` / `-`: adjust sensitivity
- mouse click: pulse at the clicked horizontal position
- mouse drag/move: low amplitude motion wave
- mouse wheel: directional sweep pulse

## Scope

This v1 captures input only while the TUI terminal is focused. It does not use global OS-level keyboard or mouse hooks.

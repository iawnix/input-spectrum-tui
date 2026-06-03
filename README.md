# Input Spectrum TUI

`inputspectrum` turns keyboard and mouse input into a live spectrum wall. Each key press, mouse click, drag, movement, and wheel event injects energy into different bands; faster input produces taller and faster pulses.

On Linux it reads global input from `/dev/input/event*`, so keyboard and mouse activity can animate the wall even when the TUI terminal is not focused. Access usually requires running as a user in the `input` group or using `sudo`.

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

The visual surface is intentionally clean: no title, status bar, or on-screen control hints. Terminal-local controls still work when the TUI is focused so the process can be closed safely.

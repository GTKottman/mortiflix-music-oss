# Mortiflix Studio app icon

**The Mortiflix mark** (owner, 2026-10-18: "get away from the FilmCraft" owl): the Mortiflix logo's 3D "M"
on its dark violet ground, in a rounded tile.

## Source

The mark is a 3D render, `brand/mortiflix/logo-3d/mortiflix-mark-dark.png` in the private GTKottman/mortiflix repo
(Blender source `mortiflix-logo.blend` beside it). `packaging/icon_from_mark.py ROOT MARK.png SCRATCH` crops it
square around the mark (about 72 % of the width), rounds the corners and writes every file below; then
`cargo run -p xtask -- ico assets/app-icon/mortiflix-studio.ico SCRATCH/ico-*.png` packs the Windows icon.

## Geometry

- A 512-unit tile, rounded square `rx=112`, the render full bleed (no frame or border).
- macOS files (`mortiflix-studio.icns`, `mortiflix-studio-macos-512.png`) put the tile in Apple's grid (824 of
  1024 px, transparent margin). Windows, Linux and the web use the full-bleed tile.
- The SVGs embed the render (it is not a vector drawing), so `packaging/icons.sh` can still render from
  `mortiflix-studio.svg`.

Licence: [LICENSE.txt](LICENSE.txt) (MIT OR Apache-2.0 for the files; the Mortiflix name and logo stay the owner's
brand).

## Files

| File | What |
|---|---|
| `mortiflix-studio.svg` | canonical tile (embeds a 1024 px render) |
| `mortiflix-studio-small.svg` | the same at 256 px, for when size matters (also `hicolor/scalable`) |
| `mortiflix-studio-1024.png` | 1024 px tile |
| `mortiflix-studio-macos-512.png` | runtime Dock icon on macOS (embedded by `apps/mortiflix-studio/src/main.rs`) |
| `mortiflix-studio.icns` | macOS bundle icon (`CFBundleIconFile`) |
| `mortiflix-studio.ico` | Windows icon, 16–256 px (embedded in `mortiflix-studio.exe` by `apps/mortiflix-studio/build.rs`) |
| `hicolor/` | Linux icon theme sizes 16–512 px + scalable |

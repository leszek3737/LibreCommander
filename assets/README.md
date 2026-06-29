# Assets

This directory holds images, GIFs, and other media referenced by the top-level
[`README.md`](../README.md).

## Adding a real screenshot / demo

The README currently ships an ASCII mockup of the dual-panel UI so it renders
everywhere with no binary assets. To upgrade it to a real capture:

1. **Record a demo.** A short GIF or asciinema cast works best for a TUI:

   ```bash
   # Option A: animated GIF via asciinema + agg
   asciinema rec lc-demo.cast --command "./target/release/lc"
   agg lc-demo.cast assets/lc-demo.gif    # https://github.com/asciinema/agg

   # Option B: static screenshot of a representative panel
   # (use your terminal's built-in screenshot, or `teiler`, `grim`, etc.)
   ```

2. **Save the file here**, e.g. `assets/lc-demo.gif` or `assets/lc-demo.png`.

3. **Reference it in the README.** Replace the ASCII mockup block with:

   ```html
   <p align="center">
     <img src="assets/lc-demo.gif" alt="Libre Commander in action" width="720">
   </p>
   ```

4. Keep the file **small** (≤ ~2 MB) so the README stays fast. Prefer a cropped,
   representative shot over a long recording.

## Logo

`assets/logo.png` (optional) — a project logo for the README hero and social
preview. If added, reference it in the `<div align="center">` hero block.

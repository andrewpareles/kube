# Cursors

`draw_cursors.mjs` draws `arrow.png`, `pointer.png` and `crosshair.png` at 8 pixels per point. The desktop client builds them
in as the `"default"`, `"pointer"` and `"crosshair"` mouse cursors (`crates/client/src/play_runner/mouse_cursor.rs`).

| Cursor | Size (points) | Point that clicks |
| --- | --- | --- |
| arrow | 21.875 x 29 | 0.1421, 0.0119 |
| pointer | 24.875 x 27.875 | 0.3934, 0.0123 |
| crosshair | 39.875 x 39.875 | 0.5, 0.5 |

All three are drawn like the kube.new logo.

The arrow and the pointing hand are made from Font Awesome Free 7.3.1's `arrow-pointer` and `hand-pointer` icons
by Fonticons, Inc. (https://fontawesome.com), licensed under CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/). They are recolored, and the arrow's stem is wider.

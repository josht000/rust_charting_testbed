# DigiTrader — Slint/Rust port (initial skeleton)

This directory is the beginning of a Slint front end for the existing DigiTrader
application. The legacy Qt application remains the behavior reference. The
current UI is a static-data prototype: it deliberately has no live broker
connection and the order ticket is preview-only.

## Design direction

- Keep chart data, indicators, drawings, and interaction state independent of
  the rendering backend so a bitmap renderer can later be replaced by wgpu.
- Use visible-range/pixel-aware drawing and batched updates, following the
  lessons in `../3_slint`.
- Treat drawings as time/price-anchored annotations shared by the viewer's
  charts for the same normalized symbol, regardless of timeframe or link group.
- Separate app color themes from chart-style presets; make settings serializable.
- Keep order validation and hotkey parsing testable without a broker or GUI.

## Run and test

```powershell
cargo test
cargo run --release --bin digitrader-desktop

# WebAssembly (requires wasm-pack and the wasm32-unknown-unknown target)
.\build-web.ps1
.\serve-web.ps1
# open http://localhost:5174
```

Fixtures in `fixtures/` are deterministic, offline-only examples. No Alpaca
credentials are read, no network requests are made, and no real orders can be
sent by this prototype.

The native binary and browser build share the same Slint UI and Rust models.
Rendering is kept behind a chart image boundary so a future native wgpu or
browser GPU renderer can replace the software path without changing order,
indicator, or drawing semantics.

The chart currently supports cursor pan, wheel zoom, and interactive placement
of horizontal/vertical lines, trend lines, rays, crosshairs, and measurement
boxes. Drawings are held in a viewer-scoped store and rendered on charts by
normalized symbol. The eraser removes the nearest unlocked visible drawing.
Quote/account display and order previews use embedded copies of the JSON
fixtures, so the same flow works in desktop and WebAssembly builds. The chart
bars remain a deterministic sample series; changing the quote symbol does
not yet load symbol-specific bars. Each chart has price and UTC time axes
derived from its visible sample bars, so chart prices are not fixture quotes.
Price levels and UTC time intervals move with the data as the chart is panned;
their spacing adjusts when zooming or resizing. Price grid lines follow the
same levels as the price labels.
An orange dashed line and right-axis tag show the newest sample bar's close,
not the separate fixture quote. They follow the chart's price scale and are
hidden when that price is outside the visible range; compact pop-outs share
the same marker. Live market updates are not yet connected.

Use **+ CHART** to add up to eight charts to the workspace grid. The first
two charts share a row, the third starts a second row, and the fourth fills
that row; further charts continue in pairs. Adding or removing a chart resets
the grid to equal row heights and equal widths within each pair. Drag the
vertical and horizontal dividers to resize adjacent charts; each pair has an
independent width divider. Select a chart by clicking its symbol field or plot.
Edit a chart's symbol in its header and press Enter to apply it; Escape cancels
an edit. Entries are checked
against the offline quote fixtures; unavailable symbols produce a status message
and leave the chart unchanged. Each chart retains its own pan/zoom state. **LINK**
shares symbol changes with the other linked charts; **SOLO** keeps a chart
independent. Joining the linked group adopts its current symbol. Drawings
remain shared by symbol even when charts are unlinked. The trade panel scrolls
independently.

**POP** opens a compact, read-only view in a separate native window or browser
pop-up. It stays in sync with the chart's symbol, viewport, and drawings
while the workspace is open. Closing the chart's grid tile leaves an open
pop-out running independently; close the pop-out itself when finished. Linked
symbols and same-symbol drawings continue to update it. At least one chart
must remain in the grid. Browser pop-ups must be allowed.
Native windows have custom minimize, maximize/restore, and close controls. Drag
the title area to move them and drag their edges or corners to resize. In the
browser, the browser and operating system own window chrome, movement,
minimizing, maximizing, and resizing; these cannot be removed or replaced by a
web page. Browser pop-outs also have an in-content close button.
The browser workspace follows the viewport; below its 800 by 520 logical-pixel
minimum, scroll the page to reach the rest of the interface.

The order ticket is preview-only. Changing the symbol, side, order type,
quantity, or limit price invalidates the displayed preview; review the order
again to calculate a new one. Changing symbols resets the limit price to the
new fixture quote.

Hotkey parsing, indicator calculations, app themes, and chart-style presets
have tested Rust models, but their full editing screens and persistence flows
are not part of this initial UI skeleton. Timeframe and date-range selection
are also not implemented; the interface does not present inactive controls for
them.

`cargo test --test chart_outline --test ui_buttons` runs Slint rendering,
pointer, and keyboard events against
the actual UI. It checks every application button, chart drawing pixels,
symbol linking, grid geometry and divider dragging, order previews,
validation, selected-chart borders, and scrolling at the minimum
window size. The headless test checks the pop-out action's status; verify
native windows and browser pop-ups separately on their respective platforms.

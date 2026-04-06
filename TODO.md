# Turbo Language — TODO

## Reference counting (Priority: Critical, v0.6)
The runtime currently does not reference-count heap allocations: `rt_release`
is a no-op. This produces a ~2.5 KB/request leak on the example HTTP server
and makes long-running services impractical. Real ARC is planned for v0.6
and is tracked as the top P1 follow-up from the v0.5.1 hardening sprint.

Scope:
- Per-allocation refcount header (already allocated in `turbo_alloc`)
- `rt_retain` / `rt_release` that atomically inc/dec the count
- Codegen insertion of retains on assignment and releases at scope exit
- Deep release for array/hashmap element types
- Re-enable the leak test that was disabled while this is a no-op

## Visual Library (Priority: High)
Turbo needs a visual output library so programs can produce graphics, charts, and UI.

### Approach: Server-rendered SVG/Canvas via HTTP
- Add `svg_*` and `canvas_*` builtins to the runtime (turbo_rt.c + codegen)
- Programs generate SVG/HTML strings and serve them via the HTTP server
- Browser renders the visuals — no native GUI toolkit needed for v1
- Enables: charts, data visualization, dashboards, generative art

### Builtins to add
- `svg_rect(x, y, w, h, fill)` → SVG `<rect>` string
- `svg_circle(cx, cy, r, fill)` → SVG `<circle>` string
- `svg_line(x1, y1, x2, y2, stroke)` → SVG `<line>` string
- `svg_text(x, y, content, size)` → SVG `<text>` string
- `svg_path(d, fill, stroke)` → SVG `<path>` string
- `svg_group(children)` → SVG `<g>` wrapper
- `svg_document(w, h, children)` → full `<svg>` document string
- Bar chart, line chart, pie chart helpers built on top

### Future: Native GUI (v1.3 roadmap)
- Turbo/ui declarative framework compiling to native widgets
- Cross-platform: macOS (AppKit/SwiftUI), Windows, Linux
- See `design/ROADMAP.md` v1.3 section

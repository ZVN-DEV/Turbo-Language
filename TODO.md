# Turbo Language — TODO

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

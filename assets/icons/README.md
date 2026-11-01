# Toolbar icons

All icons are 24 × 24 SVGs in the style of Fluent UI System Icons (regular, 1.5 px lines). They
are meant for GPUI's `svg()` element, which uses only their shape and paints them in the current
text color, so light and dark themes and disabled buttons need no extra files.

## `fluent/`

Unchanged copies of [Fluent UI System Icons](https://github.com/microsoft/fluentui-system-icons)
by Microsoft, MIT license (`fluent/LICENSE`). The license text must ship with UmlCraft once the
icons are embedded.

| Use | File |
|---|---|
| Select tool | `ic_fluent_cursor_24_regular.svg` |
| Save | `ic_fluent_save_24_regular.svg` |
| Undo, redo | `ic_fluent_arrow_undo_24_regular.svg`, `ic_fluent_arrow_redo_24_regular.svg` |
| Zoom in, out, to fit | `ic_fluent_zoom_in_…`, `ic_fluent_zoom_out_…`, `ic_fluent_zoom_fit_…` |
| Align left, center, right | `ic_fluent_align_left_…`, `ic_fluent_align_center_horizontal_…`, `ic_fluent_align_right_…` |
| Align top, middle, bottom | `ic_fluent_align_top_…`, `ic_fluent_align_center_vertical_…`, `ic_fluent_align_bottom_…` |
| Distribute horizontally, vertically | `ic_fluent_align_space_evenly_horizontal_…`, `ic_fluent_align_space_evenly_vertical_…` |
| Theme | `ic_fluent_dark_theme_24_regular.svg` |
| Language | `ic_fluent_translate_24_regular.svg` |
| Text, rectangle, ellipse, line, freehand, image | `ic_fluent_text_t_…`, `ic_fluent_rectangle_landscape_…`, `ic_fluent_oval_…`, `ic_fluent_line_…`, `ic_fluent_pen_…`, `ic_fluent_image_…` |
| Pin (keep tool selected) | `ic_fluent_pin_24_regular.svg` |
| Paste, cut, copy | `ic_fluent_clipboard_paste_…`, `ic_fluent_cut_…`, `ic_fluent_copy_…` |
| Delete, remove from diagram, select all | `ic_fluent_delete_…`, `ic_fluent_dismiss_…`, `ic_fluent_select_all_on_…` |
| Line style, wrap in fragment | `ic_fluent_line_style_…`, `ic_fluent_group_…` |
| Arrange automatically, route lines | `ic_fluent_flowchart_…`, `ic_fluent_arrow_routing_…` |
| 100 %, light, dark, language, settings | `ic_fluent_ratio_one_to_one_…`, `ic_fluent_weather_sunny_…`, `ic_fluent_weather_moon_…`, `ic_fluent_local_language_…`, `ic_fluent_settings_…` |
| Source import, Astah import and export, image export, image to clipboard | `ic_fluent_code_…`, `ic_fluent_arrow_import_…`, `ic_fluent_arrow_export_ltr_…`, `ic_fluent_image_arrow_forward_…`, `ic_fluent_image_copy_…` |
| New diagram (no diagram open) | `ic_fluent_document_add_24_regular.svg` |
| Hover handles on the canvas: new member, draw a line or message | `ic_fluent_add_24_regular.svg`, `ic_fluent_arrow_right_24_regular.svg` |
## `uml/`

UmlCraft's own icons for the diagram tools, named after the palette's tool keys
(`Tool::key()`, e.g. `useCase.svg`, `asyncMessage.svg`), except the line tool, whose key is
`line` and whose icon is `freeLine.svg`; select and text have none here. A few have no tool yet:
`navigableAssociation`, `aggregation`, `composition`, `associationClass`, `containment` and
`requiredInterface`. `structogram.svg` stands for structograms in the model tree and on their
tabs. `messageNumbers.svg` and `flatMessageNumbers.svg` are the sequence diagram's switches for
message numbers and for numbering them 1, 2, 3.

`make_uml_icons.py` writes them; change the shapes there rather than in the SVGs. The shapes
are laid out on a 20 px grid and mapped onto Fluent's 24 px grid.

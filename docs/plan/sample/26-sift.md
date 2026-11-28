# Sample 26 — sift

A desktop application, not a game: an image library that opens a folder, lists
and previews what is in it, and lets the user sort through it. It is built only
on what `crcbl-ui`, `crcbl-shell`, `crcbl-image` and `crcbl-store` already
offer. Planned 2026-11-28 at the owner's request. Nothing is built.

**Why it exists.** `hud` (04) is the UI system's gallery and stylesheet fixture,
and `apps/editor` is a desktop app, but the editor is a tool for the engine's
own content and not a sample. No sample uses the widget tree the way a desktop
application does: a window the user resizes, panes that keep their size, a list
of thousands of things, a keyboard path through all of it, and settings that
survive a restart. Rule 13 says a feature nobody uses is a feature nobody has
checked. The widgets sift would be the first sample to use are:

- `Ui::list` and `Ui::selectable_list`
- `Ui::collapsing` and `Ui::tree_node`
- `Ui::tabs` and `Ui::split`
- `Ui::scrollbar`, `Ui::checkbox` and the tree's `Ui::slider`
- `Ui::paragraph`, `Ui::open_popup` and `Ui::enabled`

Beyond the widgets, it would also be the first sample to use:

- **Parsed and system fonts:** the `parsed-font` and `system-fonts` features.
- **Icons:** SVG icons through `crcbl_image::theme::IconThemes`, and
  `ImageAtlas::register_evictable`.
- **Accessibility:** AccessKit, through the `accesskit` feature.
- **Desktop integration:** IME preedit, `DroppedFile`, more than one window, and
  `crcbl-store`'s `SettingsStack` used for window and pane state.

Images are the subject because `crcbl-image` decodes every format a library
holds (BMP, GIF, ICO, JPEG, PNG, SVG, WebP, XPM), reads sizes without decoding
(`dimensions` per format), applies EXIF orientation and resizes. A library of a
few thousand files is the load that makes a virtualised list and evictable
thumbnails necessary rather than decorative.

## Proves

- **A desktop layout that holds up under resizing:** a folder tree, a file list
  and a preview, each in a `Ui::split` pane whose ratio the user drags and the
  app restores. Layout is flex only, so a thumbnail grid is a `flex-wrap: wrap`
  block, which the CSS subset has.
- **Scale in the list:** a folder of thousands of files scrolls through
  `Ui::list` at a steady frame time. Rows read sizes through each format's
  `dimensions` and never decode for the list. The count and the frame time go in
  the headless summary line.
- **Off-thread decoding:** previews and thumbnails decode on `crcbl-jobs`'s
  pool, resized to the size shown. They land in `ImageAtlas` with
  `register_evictable`, so scrolling a large folder cannot fill the atlas page.
  A file that refuses to decode shows its `ImageError` in the preview pane, not
  a blank one.
- **Keyboard-only use:** every action is reachable without the pointer:
  - `NavInput` moves between panes;
  - arrows and type-to-find work in the list;
  - Enter opens the selection, and the context-menu key opens the context menu.

  A focus-path e2e walks it.

- **Accessibility:** the tree, list, tabs and preview report roles and names
  through `Ui::accessibility_update`. This is checked against the `TreeUpdate`
  it returns, as `crcbl-ui`'s own accessibility tests check theirs.
- **Persistence:** the last library, pane ratios, sort order, view (list or
  grid), thumbnail size, theme and window size are saved through `SettingsStack`
  and come back on the next launch. That is the options sample's round trip,
  applied to an application's own state rather than a game's settings.
- **Theming by sheet swap:** a light and a dark stylesheet, switched at run time
  with `replace_stylesheet`, plus `--styles DIR` hot reload as in hud. The theme
  switcher hud's plan wants is built here first, on a real app.
- **The platform seam as a desktop app sees it:**
  - Dropping files or a folder from the OS onto the window adds them to a
    collection (`DroppedFile`).
  - The copy-path action goes through `clipboard_offer`.
  - Cursor shapes follow the hovered widget.
  - Opening an image in its own window uses `MULTI_WINDOW`.
  - Each of these is degraded honestly where a backend lacks the capability (the
    web has no clipboard, drop or second window), never silently absent.

## Scope

1. **Library pane:**
   - A folder tree under the root given on the command line or remembered from
     the last run (`Ui::collapsing`/`tree_node`), lazily listed.
   - A "show hidden files" checkbox.
   - Collections: named sets of dropped files, kept in settings.
2. **File list:**
   - List and grid views behind `Ui::tabs`, selection through
     `Ui::selectable_list`.
   - Sort by name, size, dimensions or modified time with `Ui::select`, and a
     search box filtering by name (`Ui::text_input`, with IME and clipboard).
   - A thumbnail-size `Ui::slider`.
   - A context menu (`Ui::context_menu`): copy path, open in new window, add to
     collection, reveal the containing folder in the tree.
   - Tooltips showing the full path.
3. **Preview and info:**
   - A preview fitted to its pane, upright per EXIF.
   - An info tab rendered as a `Ui::paragraph` with styled runs: format, stored
     and upright size, orientation, file size and modified time.
   - A per-channel histogram drawn as rects.
4. **A second window:** the selected image alone, at fit or at actual size, with
   pan.
5. **Chrome:** a toolbar row of icon buttons with tooltips, standing in for the
   menu bar the toolkit does not have; a theme switch; the debug panel (rule 4)
   with a `sift` section (files listed, thumbnails resident and evicted, decode
   queue depth, decode time p50/p95).

## Non-goals (hard cap)

- No image editing, writing or format conversion. Files are opened read-only.
- No file operations: no delete, rename, move or copy of the files themselves.
  "Collection" is a list kept in settings, not a folder on disk.
- No multi-line text anywhere. The toolkit has no multi-line editor
  (`edit::LineEdit` strips newlines), and a captions or notes feature would be
  the sample driving a widget, which hud's rule against speculative widgets
  forbids. If one is wanted it is engine work first.
- No network, no cloud library, no thumbnails cache written to disk in the first
  cut.
- No native menu bar, file dialog or tray. None exists in `crcbl-shell` (the
  backlog records file dialogs as a known gap), and sift does not grow them. The
  root folder comes from the command line, the remembered setting, a path line
  like the editor's `panel/path_line.rs`, or a dropped folder.

## Rule exemptions, argued

- **Rule 2 (server-authoritative):** taken. sift has no simulation and no
  authoritative state; its model is a folder on the user's disk. bracket (16)
  set the precedent of a sample with no transport at all. Running a library
  index as a server system over `InMemoryTransport` would prove only that the
  shape can be imposed on something that does not need it.
- **Rule 3 (scenes from files):** not applicable; there is no scene.
- **Rule 7 (web demo):** kept, as a reduced slice. The browser build opens a
  bundled library fetched through `crcbl-store`'s `web::FetchSource`. The web
  shell has no clipboard, drop or second window, so those controls are disabled
  with the reason shown on hover (`Ui::enabled` and a tooltip). That makes the
  page itself the test that the degraded states render.
- **Rule 8 (sound):** taken. A desktop application that makes sounds on clicks
  is worse for it, and the cue grammar has nothing positional to say here.
- **Rules 9 and 10 (physics, module API):** not applicable; there is no
  gameplay.
- **Rule 11 (pixel art):** taken, as hud and viewer take it. The subject is the
  user's own images, and the icons are SVG from the icon theme, which is the
  path a desktop app actually uses.
- **Rule 12 (paths):** the summary line names the backend, shell and scale
  factor, and `--headless` runs on the lesser UI path CI selects.

## Milestones

1. **Skeleton (native):**
   - A new sample crate, sift, laid out like `apps/hud`. `--library DIR` opens a
     folder.
   - The tree, list and preview in two `Ui::split`s; the preview decodes on the
     UI thread at first.
   - The headless summary line
     (`sift: N files listed, …, on the <shell> shell at WxH`) and the debug
     panel.
   - CI runs it headless over a committed fixture library under the crate's own
     `tests/library/`: small files, every format, one EXIF-rotated JPEG, one
     damaged file. A golden frame comes with it.
2. **Scale:**
   - Decoding moves to `crcbl-jobs`, with evictable thumbnails and the grid
     view.
   - A generated library of thousands of files, made at test time and never
     committed, measured for frame time and resident thumbnails while scrolling,
     and recorded in this doc.
3. **Desktop behaviour:**
   - Keyboard paths and the focus-path e2e, the context menu, search, and
     sorting.
   - `SettingsStack` persistence with a round-trip test (launch, change, quit,
     relaunch, same state).
   - Light and dark themes with a golden frame each.
4. **Platform:**
   - OS drop into collections, the second window, copy path, cursor shapes.
   - AccessKit roles and names checked against `Ui::accessibility_update`.
   - Parsed and system fonts with IME in search.
   - Each capability exercised on every backend that offers it, and refused
     visibly on the ones that do not.
5. **Web slice:** the bundled library on the Pages site, a `web/build.sh`
   `DEMOS` row, and the browser gate.

## Engine gaps it will meet

Recorded now so the plan does not pretend otherwise. Each one is engine work
filed when sift reaches it, not built inside the sample:

- No table or grid widget: the list view's columns are flex rows of fixed
  widths, and the grid is `flex-wrap`.
- No menu bar, native file dialog or tray.
- No CSS transitions or `prefers-color-scheme`: the theme is a setting, not
  followed from the OS.
- No drag source or drop target inside the widget tree: dragging a file from the
  list onto a collection needs one. `grid_drag` is built on the old
  `widget::UiState`, not the tree.
- No undo in `Ui::text_input` (it matters for the search box only).
- `ImageAtlas` is one fixed page: the grid's resident thumbnails are bounded by
  it, which milestone 2 measures.
- `open_url` (for "open in the default viewer") works on Win32 only.

## Exit criteria

- The fixture library renders pixel-stable in both themes (golden frames in CI).
- The generated large library scrolls with frame-time p95 recorded, and resident
  thumbnails stay under the atlas page, asserted rather than observed.
- The focus-path e2e reaches every control without the pointer.
- Settings round-trip green.
- Every capability-gated control is shown working on a backend with the
  capability and disabled with its reason on one without.
- Published on the Pages site.

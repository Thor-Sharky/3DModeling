# Shapeshop architecture

## Crates (lower layers never depend on higher ones)

| Crate | What it owns | May depend on |
|---|---|---|
| `ss_math` | Transform, Ray, Aabb, Plane, OrbitCamera, Viewport, units, snapping | glam, serde |
| `ss_mesh` | `PolyMesh` (stable `VertId`/`FaceId`), primitives, topology, health, the one triangulation, BVH, `RenderMesh` | ss_math |
| `ss_csg` | Booleans behind `BooleanEngine` (Manifold arrives in M5) | ss_mesh |
| `ss_doc` | `Document`, `Object`, `Shape`, `Material`, `Tx`/`DocDelta`/`History`, `Selection`, `Evaluator`, `RenderScene`, picking | ss_mesh, ss_csg |
| `ss_ops` | `AppAction`, `ToolDef` (from `assets/guide/tools.ron`), `Operator`s, `Registry`, `run_tool` | ss_doc |
| `ss_guide` | Goal search, quests, glossary, jargon lint | ss_ops, ss_doc |
| `ss_io` | `.shapes` project files, schema/migrations, atomic save, autosave, recovery, import | ss_doc |
| `ss_export` | glTF/GLB, OBJ+MTL, STL, presets, pre-flight checks, report | ss_doc |
| `ss_render` | OpenGL 3.3 renderer (only via `eframe::glow`) | ss_doc |
| `shapeshop` (`crates/ss_app`) | The egui app: layout, viewport input, gizmos, dialogs, dispatch, scenario runner | everything |

`cargo xtask check-layers` enforces: `ss_math`..`ss_export` never depend on egui, eframe, winit, glow or rfd;
only `shapeshop` uses rfd and transform-gizmo-egui; only `ss_csg` links manifold-csg.

## Coordinates and units

- Right-handed, **Y up**, ground = XZ plane. The model's **front faces −Z** (Roblox `LookVector`).
- 1 unit = 1 stud in Roblox projects (`Units::Studs`), 1 m otherwise.
- Transforms are rigid (translation + rotation). **Size is never a scale**: sizing a primitive edits its
  `size`; sizing a mesh moves its points around the pivot (mirroring flips winding — `PolyMesh::transform` does it).
  Minimum size per axis: `ss_mesh::primitives::MIN_SIZE` (0.05).
- `Shape.pivot` is a point in the source mesh's own coordinates that sits at the object's origin.
  Primitives are built centered, so pivot `ZERO` = center; "Pivot: bottom" sets `pivot = (0, -h/2, 0)`
  **and** moves the object's translation so nothing moves on screen. Every consumer gets the
  pivot-applied mesh via `Evaluator::shape_mesh` (or `Shape::base_mesh` / `Document::mesh_of`).
  Element edits through `Tx::mesh_mut` work in **source** coordinates: object-space point = source − pivot.
- Primitive face ids are fixed (see `ss_mesh::primitives` docs): Block 0 +X, 1 −X, 2 top, 3 bottom, 4 back(+Z), 5 front(−Z).

## Changing the document

- `Document` fields are private. The only way to change it: `let mut tx = doc.begin("Label"); ...; tx.commit()`.
- `commit()` returns `None` when nothing changed (record nothing). Dropping a `Tx` rolls back.
- Every committed change becomes one `HistoryEntry { label, invocation, delta, sel_before, sel_after, coalesce }`
  pushed to `History`. Slider drags pass the same `coalesce` key so they merge into one step.
- Interactive previews: clone the `Document` (cheap: shared `Arc`s), run the operator on the clone, draw the clone;
  on release run it for real once; on Esc drop the clone.
- Ids are never reused (objects, materials, mesh elements).

## Tools

- Every user-visible action is an `ss_ops::AppAction` (plain serializable data). Buttons, shortcuts, search,
  quests, fix buttons and scripts all go through the app's single `dispatch`.
- Every operator has a `ToolDef` in `assets/guide/tools.ron` with friendly `name`, optional `term` (the only
  place jargon is allowed), `tip` (≤ 140 chars), `hint`, ≥ 3 `keywords`, `reasons` (text + optional fix) for
  each way it can be unavailable, and `params` with defaults. Registry tests enforce this.
- The app draws every tool button with one helper (`tool_button`), which adds the icon, label, hover tip,
  shortcut, disabled reason and accessibility name.

## Files

- Project = one `.shapes` zip: `manifest.json` (`format`, `format_version`, `min_reader_version`, `app_version`),
  `document.json` (a separate versioned DTO schema, never the in-memory types), `thumbnail.png`.
- Save = write `.tmp` + fsync, old file → `.bak`, rename into place. Load = migrate raw JSON → DTO → validate & repair.
- Autosave on a background thread from `Document` snapshots; crash recovery offered on next launch.

## Exports

One shared `prepare()` makes the export scene (per-object nodes at their pivots, the shared triangulation,
normals per Look, sRGB → linear colors, box-projected UVs in 0..1, unit scale, front = −Z, stable safe names
that never end in `_Att`/`_OuterCage`/`_InnerCage`). Writers: glTF/GLB, OBJ+MTL, STL. The Roblox pre-flight
check flags > 20,000 triangles per part, open shapes, inside-out faces, tiny/huge parts and openings
(CollisionFidelity tip).

## Rendering

`ss_doc::RenderScene` is plain data; `ss_render::GlRenderer` draws it into its own framebuffer (sky, grid,
lit shapes, cutters, overlays, selection outline) and the app shows the texture with egui. Shaders write
sRGB-encoded color because egui composites in gamma space.

## Verifying without a GPU

`scripts/headless.sh` runs the app under Xvfb with Mesa llvmpipe (GLX). `--screenshot PNG --after-frames N`
saves the window; `--script FILE.ron` drives the real app with injected input and assertions.

# Crabby Navmesh Builder

A multi-threaded navigation mesh generator for Source engine maps, written in
Rust. It reads a compiled map (`.bsp`) and writes the `.nav` file the engine's
bots load, without launching the game.

Two tools ship together:

- **`navgen`** builds `MAP.nav` from `MAP.bsp`.
- **`navinfo`** analyzes any `.nav` (yours or Valve's): counts, connectivity,
  components, and findings worth acting on.

Both print machine-readable reports on stdout: [TOON](https://toonformat.dev)
by default, which is compact for LLM agents, or JSON with `--json`.

## Install

Download a binary for Linux, macOS, or Windows from the
[releases page](https://github.com/moonexpr/crabby-navmesh-builder/releases),
or build from source with Rust 1.88 or later:

```sh
cargo install --git https://github.com/moonexpr/crabby-navmesh-builder
```

## Use

```sh
navgen maps/cp_dustbowl.bsp            # writes maps/cp_dustbowl.nav
navgen maps/ctf_2fort.bsp -o out.nav --json
navinfo maps/cp_dustbowl.nav --bsp maps/cp_dustbowl.bsp
navinfo out.nav --areas --area 12      # every area as a table row, plus area 12 in full
```

`navgen` options:

| Option | Meaning |
|---|---|
| `-o, --output PATH` | Where to write the `.nav`; defaults to the `.bsp` path with `.nav` |
| `--seed x,y,z` | Extra flood-fill start points (repeatable), used alongside spawn points |
| `-j, --threads N` | Worker threads; defaults to one per core |
| `--cell-size N` | Sampling grid spacing in units (default 25, as `nav_generate`) |
| `--merge-tolerance N` | How far a corner may stray from its merged area's surface (default 6) |
| `--json` | Report in JSON instead of TOON |

A `navgen` report looks like this (abridged):

```
status: ok
nav:
  path: maps/cp_dustbowl.nav
  bytes: 168518
  areas: 1375
mesh:
  columns: 256530
  samples: 330241
  reachable_samples: 32335
  cells: 27369
  areas: 1375
threads: 10
elapsed_ms: 753
next_steps[2]: navinfo maps/cp_dustbowl.nav,"in game: nav_analyze, to add hiding spots, encounter paths, and visibility"
```

Failures are reports too, with `status: error`, a `kind`, the message, and its
causes. The exit code says whose move it is: `0` success, `2` bad input (fix
the file or arguments), `1` an internal error (please report it).

After generating, run `nav_analyze` in game to add hiding spots, encounter
paths, and visibility, which need the running engine.

## How it works

1. **Parse.** BSP versions 19 to 21, including LZMA-compressed lumps. Brushes
   that block players (`MASK_PLAYERSOLID_BRUSHONLY`) come from each model's BSP
   tree; displacements are rebuilt the way `CCoreDispInfo` does and become thin
   triangle prisms. Brush entities that stay put (`func_brush`, `func_wall`)
   are solid; doors, and anything the map's logic can toggle by name, are not.
2. **Sample** (parallel). Every 25-unit grid column is cast through the world
   to find each floor where a 32-unit-wide player hull fits, standing or
   crouching, on slopes up to `nav_slope_limit`.
3. **Link** (parallel). Each floor sample finds where it can walk (18-unit
   steps), drop (up to 200), or jump (up to 58) in its four neighbor columns,
   each checked with a hull sweep.
4. **Reach.** A flood fill from the spawn points keeps what players can reach.
5. **Merge.** Grid cells merge greedily into the largest near-planar
   rectangles, which become nav areas.
6. **Connect** (parallel). Areas connect along shared or linked edges, one-way
   where the link is (drops).

Output does not depend on the thread count. On a 10-core machine, TF2's
`cp_dustbowl` (22 MB, 589 displacements) takes about 0.75 s.

## Limitations

- **Static props are not collided.** Their physics models live in the game's
  VPKs, not the `.bsp`, so areas can pass under or through `prop_static`s.
- No ladders, places, or hiding spots; `nav_analyze` adds the latter in game.
- A passage narrower than about two grid cells plus the hull width can hold
  walkable samples but no area, which can leave a small island disconnected.
  `navinfo` reports these as components and isolated areas.
- The output targets TF2's format (nav version 16, sub-version 2).

## Develop

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The `.nav` and `.bsp` layouts follow Valve's Source SDK 2013
(`nav_file.cpp`, `tf_nav_area.cpp`, `bspfile.h`, `builddisp.cpp`).

## License

MIT; see [LICENSE](LICENSE).

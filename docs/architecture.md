# Architecture

Layers, each depending only on those below it:

```
bin/navgen, bin/navinfo     composition roots: arguments in, report out
          │
         app                use cases (generate_nav, inspect_nav), whose fault a failure is, reports
          │
         spec               what each game expects: implements format's NavDialect and core's WorldPolicy
          │
        format              bytes ⇄ domain values; defines the NavDialect hooks
          │
         core               geometry, solids, collision, world, mesh, generation, analysis
```

`core` and `format` define the interfaces (`WorldPolicy`, `NavDialect`,
`Collision`); `spec` supplies a game's implementations, so neither lower
layer names a game.

## The seams

| Seam | Where | Pattern | Job |
|---|---|---|---|
| Core logic and math | `core` | Port (`Collision`), pipeline (`generate`) | What holds whatever the game and file: geometry, solids, collision, the nav mesh as a value, generation, analysis. No I/O, no byte layouts |
| Parser | `format::bsp::parse`, `format::nav::parse` | Factory | Bytes become domain values or are refused, once, at the edge; nothing past it re-checks |
| Compiler | `format::nav::compile` | Template method | Writes `CNavMesh`'s base layout and calls the dialect's hooks where a game adds its own data |
| Specification | `spec` | Strategy, factory | `Build` names a game; `Build::spec()` gives its `GameSpec` or refuses it as unsupported. A spec owns its nav dialect and its game policy: spawn classes, which brush entities stay solid, the agent's size |
| World assembly | `core::world` | Builder | The parser reports what a map holds; assembly decides, under a spec's policy, what blocks a player. New solid sources (static props) join here |

## Adding a game

Write a `GameSpec` with its `NavDialect` in `spec/`, and return it from
`Build::spec()`. Nothing else changes.

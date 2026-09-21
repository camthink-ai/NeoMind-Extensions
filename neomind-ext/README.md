# neomind-ext — Extension Scaffold CLI

A minimal scaffolding helper for NeoMind extensions. The heavy lifting
(build / package / install) lives in the repo's `build.sh` — this CLI
only creates new extension skeletons and runs quick checks.

## Commands (all that exist today)

| Command | What it does |
|---|---|
| `neomind-ext new <name> [--with-frontend]` | Scaffold `extensions/<name>/` from `templates/basic/` (Cargo.toml + src/lib.rs) and register it in the workspace `members` |
| `neomind-ext build <path>` | `cargo check` the crate at `<path>` |
| `neomind-ext package <name>` | Delegates to `./build.sh --single <name>` (single source of truth) |
| `neomind-ext validate [--path X]` | Validate a `.nep` (via scripts/test_nep.py) or `cargo check` a crate dir |
| `neomind-ext test` | `cargo test` |

## Notes

- The generated `Cargo.toml` uses `neomind-extension-sdk = "0.6"` from
  crates.io — no local NeoMind checkout is needed.
- After scaffolding you still need a `metadata.json` (copy from a sibling
  extension and regenerate via `scripts/update-versions.sh`) and, for UI,
  a `frontend/` workspace — see `EXTENSION_GUIDE.md`.
- Build/install/publish flows: see the root `README.md` → Build System.

## Layout

```
neomind-ext/
├── src/main.rs        # single-file clap CLI
└── templates/basic/   # the one scaffold template
```

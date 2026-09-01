# NeoMind Extension Build Scripts Guide

## Script Overview

There are 3 build/release scripts available, each designed for different use cases:

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `build.sh` | Build all extensions + create .nep packages | CI/CD, full release builds |
| `release.sh` | Build + GitHub release preparation | Official releases |
| `build.sh --dev` | Build single extension for development | Daily development |
| `build.sh --single` | Package single extension as .nep | Testing individual extensions |

---

## Script Details

### 1. build.sh (Main Build Script)

**Purpose:** Build all V2 extensions and create .nep packages

**Features:**
- Builds all extensions in batch
- Optional frontend building
- Creates .nep packages for all extensions
- Can auto-install to NeoMind

**Usage:**
```bash
# Build all extensions with default options
./build.sh

# Build without installation
./build.sh --skip-install

# Build in debug mode
./build.sh --debug

# Skip frontend builds
./build.sh --skip-frontend

# Auto-confirm prompts
./build.sh --yes
```

**Output:**
- Binaries: `target/release/`
- Packages: `dist/*.nep`
- Optional install: Configurable

---

### 2. release.sh (Release Script)

**Purpose:** Prepare extensions for GitHub release

**Features:**
- Clean build from scratch
- Builds all extensions
- Creates .nep packages
- Prepares for GitHub release

**Usage:**
```bash
# Create release packages
./release.sh
```

**Output:**
- Clean `dist/` directory
- All `.nep` packages ready for release

---

### 3. build.sh --dev (Development Script) ⭐ RECOMMENDED

**Purpose:** Quick development iteration for single extension

**Features:**
- Builds single extension
- Deploys directly to `NeoMind/data/extensions/`
- Fast iteration cycle
- Unified deployment path

**Usage:**
```bash
# Build and deploy yolo-video-v2
./build.sh --dev yolo-video-v2

# Specify custom NeoMind path
./build.sh --dev yolo-video-v2 /path/to/NeoMind
```

**Output:**
- `NeoMind/data/extensions/<extension-name>/`
- Includes: binary, models, frontend, manifest

**When to use:**
- ✅ Daily development
- ✅ Testing code changes
- ✅ Quick iteration

---

### 4. build.sh --single (Package Script)

**Purpose:** Package single extension as .nep file

**Features:**
- Builds single extension
- Creates .nep package
- Good for testing before release

**Usage:**
```bash
# Package yolo-video-v2 to ./dist/
./build.sh --single yolo-video-v2

# Package to custom output directory
./build.sh --single yolo-video-v2 ./my-output
```

**Output:**
- `dist/<extension-name>.nep`

**When to use:**
- ✅ Testing .nep installation
- ✅ Sharing single extension
- ✅ Pre-release testing

---

## Workflow Recommendations

### Development Workflow (Recommended)

```bash
# 1. Make code changes
# Edit files in extensions/<name>/src/

# 2. Build and deploy to NeoMind
./build.sh --dev <extension-name>

# 3. Restart NeoMind or reload extension
# Test your changes

# 4. Repeat steps 1-3 for iteration
```

### Release Workflow

```bash
# 1. Build all extensions and create packages
./build.sh --skip-install

# 2. Or use release script for clean build
./release.sh

# 3. Upload .nep files via frontend
# Or distribute via GitHub releases
```

### Testing Single Extension

```bash
# 1. Build and package single extension
./build.sh --single <extension-name>

# 2. Upload .nep via frontend
# Test installation

# 3. If issues found, use build.sh --dev for iteration
./build.sh --dev <extension-name>
```

---

## Path Conflicts Explained

### Potential Conflicts

| Scenario | Problem | Solution |
|----------|---------|-----------|
| Using `build.sh` then `build.sh --dev` | Different output locations | Use one workflow consistently |
| Using `build.sh --dev` then frontend upload | Same target directory | Uninstall first via frontend |
| Multiple scripts simultaneously | Race conditions | Don't run scripts in parallel |

### Unified Path (build.sh --dev)

`build.sh --dev` uses the unified path:
```
NeoMind/data/extensions/<extension-name>/
```

This is the **same path** used by frontend uploads, ensuring consistency.

---

## Quick Reference

| Task | Command |
|------|---------|
| Develop extension | `./build.sh --dev <name>` |
| Package for testing | `./build.sh --single <name>` |
| Full build (all extensions) | `./build.sh --skip-install` |
| Release preparation | `./release.sh` |
| Clean build artifacts | `cargo clean && rm -rf dist/` |

---

## Best Practices

1. **Use `build.sh --dev` for daily development** - Fast, unified path
2. **Use `build.sh` for CI/CD** - Builds everything
3. **Use `release.sh` for releases** - Clean, reproducible
4. **Don't mix workflows** - Stick to one script per session
5. **Always uninstall before reinstalling** - Via frontend API

---

## Troubleshooting

### Q: Which script should I use?

**A:** For development, use `build.sh --dev`. For releases, use `build.sh` or `release.sh`.

### Q: Can I run multiple scripts at once?

**A:** No, this can cause race conditions. Run scripts sequentially.

### Q: My extension won't load after using build.sh --dev

**A:** Restart NeoMind to reload extensions from the updated directory.

### Q: Frontend upload conflicts with build.sh --dev output

**A:** They use the same directory. Uninstall via frontend first, then use one method consistently.

## `scripts/update-versions.sh` (AUTHORITATIVE generator)

Syncs `VERSION` → per-extension `Cargo.toml` → `metadata.json` →
`extensions/index.json`, preserving `env_hints` and variant build entries.
This is the canonical JSON generator; `release.sh`'s inline generation is
a legacy fallback. `--check` validates the chain, `--bump-versions`
updates Cargo.toml files.

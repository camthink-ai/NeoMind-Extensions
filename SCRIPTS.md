# NeoMind Extension Build Scripts Guide

## Script Overview

There are 4 build scripts available, each designed for different use cases:

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `build.sh` | Build all extensions + create .nep packages | CI/CD, full release builds |
| `release.sh` | Build + GitHub release preparation | Official releases |
| `build-dev.sh` | Build single extension for development | Daily development |
| `build-package.sh` | Package single extension as .nep | Testing individual extensions |

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

### 3. build-dev.sh (Development Script) ⭐ RECOMMENDED

**Purpose:** Quick development iteration for single extension

**Features:**
- Builds single extension
- Deploys directly to `NeoMind/data/extensions/`
- Fast iteration cycle
- Unified deployment path

**Usage:**
```bash
# Build and deploy yolo-video-v2
./build-dev.sh yolo-video-v2

# Specify custom NeoMind path
./build-dev.sh yolo-video-v2 /path/to/NeoMind
```

**Output:**
- `NeoMind/data/extensions/<extension-name>/`
- Includes: binary, models, frontend, manifest

**When to use:**
- ✅ Daily development
- ✅ Testing code changes
- ✅ Quick iteration

---

### 4. build-package.sh (Package Script)

**Purpose:** Package single extension as .nep file

**Features:**
- Builds single extension
- Creates .nep package
- Good for testing before release

**Usage:**
```bash
# Package yolo-video-v2 to ./dist/
./build-package.sh yolo-video-v2

# Package to custom output directory
./build-package.sh yolo-video-v2 ./my-output
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
./build-dev.sh <extension-name>

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
./build-package.sh <extension-name>

# 2. Upload .nep via frontend
# Test installation

# 3. If issues found, use build-dev.sh for iteration
./build-dev.sh <extension-name>
```

---

## Path Conflicts Explained

### Potential Conflicts

| Scenario | Problem | Solution |
|----------|---------|-----------|
| Using `build.sh` then `build-dev.sh` | Different output locations | Use one workflow consistently |
| Using `build-dev.sh` then frontend upload | Same target directory | Uninstall first via frontend |
| Multiple scripts simultaneously | Race conditions | Don't run scripts in parallel |

### Unified Path (build-dev.sh)

`build-dev.sh` uses the unified path:
```
NeoMind/data/extensions/<extension-name>/
```

This is the **same path** used by frontend uploads, ensuring consistency.

---

## Quick Reference

| Task | Command |
|------|---------|
| Develop extension | `./build-dev.sh <name>` |
| Package for testing | `./build-package.sh <name>` |
| Full build (all extensions) | `./build.sh --skip-install` |
| Release preparation | `./release.sh` |
| Clean build artifacts | `cargo clean && rm -rf dist/` |

---

## Best Practices

1. **Use `build-dev.sh` for daily development** - Fast, unified path
2. **Use `build.sh` for CI/CD** - Builds everything
3. **Use `release.sh` for releases** - Clean, reproducible
4. **Don't mix workflows** - Stick to one script per session
5. **Always uninstall before reinstalling** - Via frontend API

---

## Troubleshooting

### Q: Which script should I use?

**A:** For development, use `build-dev.sh`. For releases, use `build.sh` or `release.sh`.

### Q: Can I run multiple scripts at once?

**A:** No, this can cause race conditions. Run scripts sequentially.

### Q: My extension won't load after using build-dev.sh

**A:** Restart NeoMind to reload extensions from the updated directory.

### Q: Frontend upload conflicts with build-dev.sh output

**A:** They use the same directory. Uninstall via frontend first, then use one method consistently.

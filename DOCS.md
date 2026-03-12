# NeoMind Extension Documentation Index

This file provides an overview of all documentation available in this repository.

---

## Quick Reference

| Document | Purpose | Audience |
|----------|---------|----------|
| [README.md](README.md) | Getting started, overview | Everyone |
| [README.zh.md](README.zh.md) | 快速入门，概述 | 中文用户 |
| [SCRIPTS.md](SCRIPTS.md) | Build scripts guide | Developers |
| [DEPLOYMENT.md](DEPLOYMENT.md) | Development & deployment workflow | Developers |
| [skill/README.md](skill/README.md) | Claude Code skill | AI-assisted developers |

---

## Core Documentation

### README.md / README.zh.md

**Purpose**: Main repository documentation

**Contents**:
- Quick start guide
- Build scripts overview
- Available extensions
- Installation instructions
- Developer guide
- SDK usage examples

**When to use**:
- First time setup
- General reference
- Installation guidance

---

### SCRIPTS.md

**Purpose**: Complete build scripts documentation

**Contents**:
- Detailed explanation of all 4 scripts:
  - `build-dev.sh` - Development builds
  - `build-package.sh` - Single extension packaging
  - `build.sh` - Full batch builds
  - `release.sh` - Release preparation
- Workflow recommendations
- Path conflict explanations
- Troubleshooting guide

**When to use**:
- Choosing the right script
- Understanding script differences
- Resolving build conflicts

---

### DEPLOYMENT.md

**Purpose**: Development and deployment workflow guide

**Contents**:
- Directory structure explanation
- Core principles (unified path)
- Development workflow
- Production deployment workflow
- Extension update workflow
- Model file path explanation
- FAQ

**When to use**:
- Setting up development environment
- Understanding deployment流程
- Troubleshooting path issues

---

## Skill Documentation

### skill/README.md

**Purpose**: Claude Code skill for AI-assisted development

**Contents**:
- Skill installation
- Usage examples
- Feature overview
- Integration with NeoMind development

**When to use**:
- Installing the skill
- Learning AI-assisted development

---

## Build Scripts

| Script | Description | Output |
|--------|-------------|--------|
| `build-dev.sh` | Development build for single extension | `NeoMind/data/extensions/` |
| `build-package.sh` | Package single extension as .nep | `dist/*.nep` |
| `build.sh` | Build all extensions | `target/release/` + `dist/*.nep` |
| `release.sh` | Clean build for releases | `dist/*.nep` |

### Usage Examples

```bash
# Development (recommended)
./build-dev.sh yolo-video-v2

# Package for testing
./build-package.sh yolo-video-v2

# Full build
./build.sh --skip-install

# Release preparation
./release.sh
```

---

## Extension Guides

### For New Developers

1. **Start with README.md** - Get overview
2. **Read DEPLOYMENT.md** - Understand workflow
3. **Use build-dev.sh** - Daily development
4. **Refer to SCRIPTS.md** - When confused about scripts

### For Release Managers

1. **Use build.sh or release.sh** - Full builds
2. **Test with build-package.sh** - Individual packages
3. **Upload via frontend** - Distribution

---

## External Resources

- **NeoMind Extension SDK**: `../../NeoMind/crates/neomind-extension-sdk`
- **NeoMind Main Project**: `../../NeoMind/`
- **SDK Documentation**: See NeoMind repository

---

## Document Update History

| Date | Document | Changes |
|------|----------|---------|
| 2026-03-12 | All docs | Unified workflow, script documentation |
| 2026-03-12 | SCRIPTS.md | Created - script reference |
| 2026-03-12 | DEPLOYMENT.md | Created - workflow guide |
| 2026-03-12 | README.md | Updated - added script section |
| 2026-03-12 | README.zh.md | Updated - Chinese translation |

---

## Getting Help

1. **Check FAQ** in respective documents
2. **Use Claude Code skill** for AI assistance
3. **Review extension examples** in `extensions/` directory
4. **Consult SDK documentation** for API reference

# Claude Code Skill: NeoMind Extension Development

A comprehensive Claude Code skill for developing extensions for the NeoMind Edge AI Platform.

## Overview

This skill provides AI-assisted guidance for creating NeoMind extensions for the isolated NeoMind runtime. When you ask Claude about NeoMind extension development, this skill automatically activates to provide step-by-step instructions, code templates, and best practices.

## Features

### 🎯 Automatic AI Guidance
- **Contextual Help**: Claude automatically uses this skill when you ask about NeoMind extension development
- **Code Generation**: Generate boilerplate code for extensions, commands, metrics, and frontend components
- **Best Practices**: Built-in knowledge of NeoMind extension architecture and patterns
- **Troubleshooting**: AI-assisted debugging and problem solving

### 📚 Comprehensive Documentation
- **Quick Start**: Fast-track from zero to working extension
- **Architecture Guide**: Understand process isolation and IPC communication
- **SDK Reference**: Complete API documentation with examples
- **Frontend Development**: React component integration guide
- **Working Examples**: Full implementation of a simple counter extension

### 🛠️ Development Support
- **Project Setup**: Template-based extension creation
- **Code Templates**: Ready-to-use Rust and React code
- **Build Scripts**: Compilation and deployment commands
- **Testing Guide**: How to test and verify extensions

## Installation

### Quick Install

From the NeoMind-Extension repository root:

```bash
./install-skill.sh
```

The script will:
- ✅ Create `~/.claude/skills/` directory if needed
- ✅ Copy the skill files to your personal Claude skills directory
- ✅ Verify installation
- ✅ Display usage instructions

### Manual Install

```bash
# Create skills directory
mkdir -p ~/.claude/skills

# Copy skill
cp -r .claude/skills/neomind-extension ~/.claude/skills/

# Verify
ls ~/.claude/skills/neomind-extension/SKILL.md
```

### Verification

After installation, start a new Claude Code session and ask:
```
How do I create a NeoMind extension?
```

If the skill is installed correctly, Claude will provide detailed guidance using the skill's knowledge.

## Usage

### Automatic Invocation

Claude automatically uses this skill when you ask questions like:

- "Create a temperature sensor extension for NeoMind"
- "How do I implement a command in a NeoMind extension?"
- "Show me the structure of a NeoMind extension"
- "How do I add a frontend component?"
- "What's the difference between execute_command and produce_metrics?"

### Manual Invocation

You can also invoke the skill directly:

```bash
/neomind-extension                    # General guidance
/neomind-extension weather-sensor     # Create weather sensor extension
/neomind-extension my-extension-v2    # Create custom extension
```

## What This Skill Provides

### 1. Complete Development Workflow

```
1. Create Extension Project
   ↓
2. Configure Cargo.toml
   ↓
3. Implement Extension Trait
   ↓
4. Build & Test
   ↓
5. Add Frontend Component (Optional)
   ↓
6. Deploy
```

### 2. Code Templates

**Backend (Rust)**:
- Extension struct implementation
- Command handlers
- Metric producers
- Error handling patterns
- Async/sync best practices

**Frontend (React)**:
- Component templates with TypeScript
- API integration helpers
- CSS theming with variables
- Build configuration

### 3. Architecture Knowledge

- **Process Isolation**: How extensions run in isolated processes
- **IPC Protocol**: Communication between main process and extensions
- **Runtime Protocol v3**: FFI interface requirements
- **Safety Requirements**: Panic handling and async rules

### 4. Reference Documentation

| File | Content |
|------|---------|
| `SKILL.md` | Main guide with workflow and templates |
| `reference/architecture.md` | Process isolation architecture details |
| `reference/sdk-api.md` | Complete SDK API reference |
| `reference/frontend.md` | React component development guide |
| `examples/simple-counter.md` | Complete working example |

## Examples

### Example 1: Create a New Extension

**You ask:**
```
Create a temperature monitoring extension for NeoMind
```

**Claude provides:**
1. Project setup commands
2. Complete Cargo.toml configuration
3. Rust implementation with temperature_celsius metric
4. get_temperature command implementation
5. Build and installation instructions

### Example 2: Add Frontend Component

**You ask:**
```
Add a frontend component to display temperature data
```

**Claude provides:**
1. React component template
2. API integration code
3. CSS theming
4. Build configuration
5. Installation steps

### Example 3: Troubleshooting

**You ask:**
```
My extension is crashing, how do I debug it?
```

**Claude provides:**
1. Check panic configuration (must be "unwind")
2. Review error handling patterns
3. Verify async/sync rules
4. Check process isolation logs
5. Common pitfalls and solutions

## Skill Structure

```
.claude/skills/neomind-extension/
├── SKILL.md                    # Main skill instructions (12KB)
│                               # - Quick commands
│                               # - Step-by-step workflow
│                               # - Code templates
│                               # - Best practices
│
├── README.md                   # Usage documentation
│
├── reference/                  # Detailed reference docs
│   ├── architecture.md         # Process isolation & IPC (6KB)
│   ├── sdk-api.md              # Complete SDK API (8KB)
│   └── frontend.md             # React component guide (9KB)
│
└── examples/                   # Working examples
    └── simple-counter.md       # Complete counter extension (7KB)
```

## Key Concepts Covered

### Extension Basics
- **Extension ID Convention**: `{category}-{name}-v{major}`
- **ABI Version**: Must return 3 for V2 extensions
- **Process Isolation**: All extensions run in isolated processes
- **IPC Communication**: JSON messages over stdin/stdout

### Safety Requirements
- **Panic Configuration**: Always use `panic = "unwind"`
- **Async Rules**:
  - `execute_command()`: async, can use `.await`
  - `produce_metrics()`: sync, NO `.await`
- **Error Handling**: Use `?` operator, avoid `unwrap()`

### Best Practices
- Use atomic types for metrics (AtomicI64, etc.)
- Cache data for synchronous metric production
- Validate command arguments
- Handle errors gracefully
- Follow naming conventions

## Development Standards

This skill follows official Claude Code skill authoring best practices:

✅ **Concise**: Essential information only, no fluff
✅ **Progressive Disclosure**: Main guide points to detailed references
✅ **Workflow-Oriented**: Clear step-by-step processes
✅ **Example-Driven**: Working code samples included
✅ **Well-Structured**: Organized by topic and complexity

## Integration with NeoMind

### File Locations

The skill includes references to standard NeoMind project locations:

- **NeoMind repo**: `~/NeoMindProject/NeoMind`
- **Extension SDK**: `~/NeoMindProject/NeoMind/crates/neomind-extension-sdk`
- **Extensions repo**: `~/NeoMindProject/NeoMind-Extension`
- **Extension templates**: `~/NeoMindProject/NeoMind-Extension/extensions/`
- **Install location**: `~/.neomind/extensions/`

### Works With Existing Tools

The skill complements existing NeoMind documentation:

- **Extension Guide**: `EXTENSION_GUIDE.md` - Detailed SDK documentation
- **Quick Start**: `QUICKSTART.md` - Getting started with development
- **User Guide**: `USER_GUIDE.md` - End-user documentation
- **Template Extensions**: `extensions/weather-forecast-v2`, etc.

## Platform Support

The skill covers development for all supported platforms:

| Platform | Architecture | Binary Extension |
|----------|--------------|------------------|
| macOS    | ARM64 (M1/M2) | `*.dylib`        |
| macOS    | x86_64        | `*.dylib`        |
| Linux    | x86_64        | `*.so`           |
| Linux    | ARM64         | `*.so`           |
| Windows  | x86_64        | `*.dll`          |
| Universal| Any           | `*.wasm`         |

## Updating the Skill

The skill files are located in this repository at `.claude/skills/neomind-extension/`.

To update the skill for all developers:

```bash
# Make changes to skill files
vim .claude/skills/neomind-extension/SKILL.md

# Commit changes
git add .claude/skills/neomind-extension/
git commit -m "Update neomind-extension skill"
git push

# Other developers can update their installation
./install-skill.sh  # Overwrites existing installation
```

Changes take effect immediately in Claude Code (no restart needed).

## Troubleshooting

### Skill Not Loading

**Problem**: Claude doesn't seem to use the skill.

**Solutions**:
1. Verify installation: `ls ~/.claude/skills/neomind-extension/SKILL.md`
2. Check SKILL.md has valid YAML frontmatter
3. Restart Claude Code
4. Try manual invocation: `/neomind-extension`

### Skill Not Found

**Problem**: `/neomind-extension` command not recognized.

**Solutions**:
1. Reinstall: `./install-skill.sh`
2. Check file permissions: `chmod -R 644 ~/.claude/skills/neomind-extension/`
3. Verify skill name in SKILL.md frontmatter matches directory name

### Outdated Information

**Problem**: Skill provides outdated instructions.

**Solutions**:
1. Pull latest changes: `git pull`
2. Reinstall: `./install-skill.sh`
3. Check skill version in README.md

## Contributing

To improve this skill:

1. **Edit skill files** in `.claude/skills/neomind-extension/`
2. **Test changes** by reinstalling: `./install-skill.sh`
3. **Verify** with Claude Code
4. **Submit PR** with clear description of changes

### Guidelines

- Keep SKILL.md under 500 lines (use reference files for details)
- Follow progressive disclosure pattern
- Include working code examples
- Update version in README.md
- Test with actual extension development

## Version History

### v1.0.0 (2026-03-03)
- ✨ Initial release
- 📚 Complete development workflow
- 🏗️ Architecture reference
- 📖 SDK API documentation
- 🎨 Frontend component guide
- 💡 Simple counter example
- 🚀 Installation script

## Resources

### Official Documentation
- [Claude Code Skills](https://code.claude.com/docs/en/skills)
- [Skill Authoring Best Practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices)

### NeoMind Documentation
- [Extension Development Guide](./EXTENSION_GUIDE.md)
- [Quick Start Guide](./QUICKSTART.md)
- [User Guide](./USER_GUIDE.md)
- [Repository README](./README.md)

## License

MIT License

---

**Created**: March 3, 2026
**Status**: Production Ready
**Maintainer**: NeoMind Team

For issues or suggestions, please open an issue in the NeoMind-Extension repository.

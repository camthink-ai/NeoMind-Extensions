# NeoMind Extension Development Skill

A comprehensive Claude Code skill for creating extensions for the NeoMind Edge AI Platform.

## Overview

This skill provides step-by-step guidance, templates, and reference documentation for developing NeoMind extensions for the isolated NeoMind runtime.

## Features

- 📚 Complete extension development workflow
- 🔧 Ready-to-use code templates
- 📖 Comprehensive API reference
- 🎨 Frontend component guide
- 💡 Working examples
- 🚀 Quick start commands

## Installation

The skill is already installed at: `~/.claude/skills/neomind-extension/`

## Usage

### Invoke the Skill

You can use this skill in two ways:

1. **Automatic invocation**: Just ask Claude about NeoMind extension development
   - "How do I create a NeoMind extension?"
   - "Help me build a weather extension for NeoMind"
   - "What's the structure of a NeoMind extension?"

2. **Manual invocation**: Use the `/neomind-extension` command
   ```
   /neomind-extension weather-sensor
   ```

### Quick Start

1. **Create a new extension:**
   ```bash
   cp -r extensions/weather-forecast-v2 extensions/my-extension-v2
   cd extensions/my-extension-v2
   ```

2. **Update configuration:**
   - Edit `Cargo.toml` with your extension details
   - Implement extension in `src/lib.rs`

3. **Build and install:**
   ```bash
   cargo build --release
   cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/
   ```

## Skill Structure

```
neomind-extension/
├── SKILL.md                    # Main skill instructions
├── reference/
│   ├── architecture.md         # Process isolation & IPC details
│   ├── sdk-api.md              # Complete SDK API reference
│   └── frontend.md             # React component development
├── examples/
│   └── simple-counter.md       # Complete working example
└── README.md                   # This file
```

## What This Skill Provides

### 1. Core Development Workflow
- Extension project setup
- Cargo.toml configuration
- Rust implementation patterns
- Build and deployment commands

### 2. Architecture Understanding
- Process isolation model
- IPC communication protocol
- runtime protocol v3 interface
- Native vs WASM comparison

### 3. SDK Reference
- Extension trait implementation
- Data structures and types
- Error handling patterns
- Common dependencies

### 4. Frontend Development
- React component templates
- API integration
- CSS theming with variables
- Build configuration

### 5. Complete Examples
- Simple counter extension
- Working code samples
- Testing procedures

## Key Concepts Covered

### Extension Basics
- **Extension ID Convention**: `{category}-{name}-v{major}`
- **ABI Version**: Must return 3 for V2 extensions
- **Process Isolation**: All extensions run in isolated processes

### Safety Requirements
- **Panic Mode**: Always use `panic = "unwind"`
- **Async Rules**:
  - `execute_command()`: async, can use `.await`
  - `produce_metrics()`: sync, NO `.await`

### Best Practices
- Use `?` operator for error propagation
- Use `unwrap_or()` for default values
- Avoid direct `unwrap()` calls
- Use atomic types for metrics
- Cache data for synchronous metric production

## Development Standards

Based on official Claude Code skill authoring best practices:

✅ **Concise**: Focused, essential information only
✅ **Progressive Disclosure**: Main instructions in SKILL.md, details in reference files
✅ **Workflow-Oriented**: Clear step-by-step processes
✅ **Examples-Driven**: Working code samples
✅ **Well-Structured**: Organized by topic

## Related Resources

- **Official Docs**: [Claude Code Skills](https://code.claude.com/docs/en/skills)
- **Best Practices**: [Skill Authoring Guide](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices)
- **NeoMind Extension Guide**: `NeoMind-Extension/EXTENSION_GUIDE.md`
- **NeoMind SDK**: `NeoMind/crates/neomind-extension-sdk`

## File Locations

- **NeoMind repo**: `~/NeoMindProject/NeoMind`
- **Extension SDK**: `~/NeoMindProject/NeoMind/crates/neomind-extension-sdk`
- **Extensions repo**: `~/NeoMindProject/NeoMind-Extension`
- **Extension templates**: `~/NeoMindProject/NeoMind-Extension/extensions/`
- **Install location**: `~/.neomind/extensions/`

## Testing the Skill

Try these prompts with Claude:

1. "Create a new temperature sensor extension for NeoMind"
2. "Show me how to implement a command in a NeoMind extension"
3. "How do I add a frontend component to my NeoMind extension?"
4. "What's the difference between execute_command and produce_metrics?"

## Updates and Maintenance

To update the skill:

1. Edit files in `~/.claude/skills/neomind-extension/`
2. Changes take effect immediately (no restart needed)
3. Test with Claude to verify changes

## License

MIT License

## Created

March 3, 2026

Built following official Claude Code skill development standards and the NeoMind isolated runtime specifications.

# NeoMind Extension Development - Claude Code Skill

AI-powered development assistance for creating NeoMind Edge AI Platform extensions.

[中文文档](README.zh.md) | [English Documentation](README.md)

## 📚 Quick Links

| Document | Description |
|----------|-------------|
| **[Installation](#installation)** | How to install the skill |
| **[GUIDE.md](GUIDE.md)** | Complete skill documentation |
| **[QUICK_START.md](QUICK_START.md)** | Hands-on examples and tutorials |
| **[neomind-extension/](neomind-extension/)** | Skill package (instructions, references, examples) |

---

## 🎯 What This Provides

This Claude Code skill gives you AI-powered assistance for developing NeoMind extensions:

### ✨ Features

- 🤖 **Automatic Guidance**: Claude automatically helps when you discuss extension development
- 💻 **Code Generation**: Generate complete Rust backend and React frontend code
- 📚 **On-Demand Documentation**: Progressive disclosure of architecture, SDK, and patterns
- 🔧 **Troubleshooting**: AI-assisted debugging and problem solving
- 💡 **Best Practices**: Built-in knowledge of NeoMind patterns and standards

### 📦 What's Included

```
skill/
├── install.sh                      # Installation script
├── README.md                       # This file
├── GUIDE.md                        # Complete skill documentation
├── QUICK_START.md                  # Quick start examples
└── neomind-extension/              # Skill package
    ├── SKILL.md                    # Main skill instructions
    ├── reference/                  # Detailed references
    │   ├── architecture.md         # Process isolation & IPC
    │   ├── sdk-api.md              # Complete SDK API
    │   └── frontend.md             # React components
    └── examples/                   # Working examples
        └── simple-counter.md       # Complete counter extension
```

---

## 🚀 Installation

### One-Step Install

From the repository root:

```bash
cd NeoMind-Extension
./skill/install.sh
```

The script will:
- ✅ Copy skill to `~/.claude/skills/neomind-extension/`
- ✅ Verify installation
- ✅ Display usage instructions

### Manual Install

```bash
# Create skills directory
mkdir -p ~/.claude/skills

# Copy skill package
cp -r skill/neomind-extension ~/.claude/skills/

# Verify
ls ~/.claude/skills/neomind-extension/SKILL.md
```

---

## 💡 Usage

### Method 1: Natural Conversation

Ask Claude naturally about extension development:

```
"How do I create a NeoMind extension?"
"Create a temperature monitoring extension"
"Add a frontend component with a gauge"
"What's the difference between execute_command and produce_metrics?"
```

Claude automatically uses the skill to provide guidance.

### Method 2: Direct Invocation

Use the slash command:

```bash
/neomind-extension                    # General guidance
/neomind-extension weather-sensor     # Create specific extension
```

### Method 3: Specific Help

Ask targeted questions:

```
"Show me the Extension trait structure"
"How do I implement async commands?"
"Generate a React component template"
```

---

## 📖 Documentation

### [GUIDE.md](GUIDE.md) - Complete Documentation

Comprehensive guide covering:
- Installation and setup
- All features and capabilities
- Usage examples
- Architecture overview
- Troubleshooting
- Team onboarding

**Start here for full understanding.**

### [QUICK_START.md](QUICK_START.md) - Hands-On Examples

Interactive examples showing:
- Creating extensions (with code)
- Adding frontend components
- Debugging and troubleshooting
- Real-world workflows
- Common patterns

**Start here to learn by example.**

### [neomind-extension/](neomind-extension/) - Skill Package

The actual skill files used by Claude:
- `SKILL.md` - Main instructions
- `reference/` - Deep-dive documentation
- `examples/` - Working code samples

**This is what Claude reads.**

---

## 🎓 Quick Examples

### Example 1: Create Extension

**You:**
```
Create a CPU temperature monitoring extension
```

**Claude provides:**
- Complete project setup
- Rust implementation with sysinfo crate
- Command and metric definitions
- Build and install instructions

### Example 2: Add Frontend

**You:**
```
Add a gauge component to display temperature
```

**Claude provides:**
- React component with circular gauge
- Color-coded visualization
- API integration code
- Build configuration

### Example 3: Debug Issue

**You:**
```
My extension crashes with panic error
```

**Claude provides:**
- Diagnosis steps
- Common fixes (panic = "unwind", error handling)
- Testing commands
- Prevention tips

---

## 🏗️ What You Can Build

With this skill, you can create:

### Sensor Extensions
- Temperature monitors
- System metrics
- Environmental sensors
- Custom hardware integrations

### AI Extensions
- Image analysis
- Video processing
- Natural language processing
- Custom ML models

### Integration Extensions
- API clients
- Database connectors
- External service integrations
- Protocol implementations

### Utility Extensions
- Data processors
- Format converters
- Aggregators
- Automation tools

---

## 📊 Skill Contents

### Main Skill (neomind-extension/)

**SKILL.md** (12KB)
- Quick commands
- Step-by-step workflow
- Code templates
- Common patterns
- Safety requirements

**reference/** (20KB)
- `architecture.md` - Process isolation, IPC protocol
- `sdk-api.md` - Complete SDK reference
- `frontend.md` - React component guide

**examples/** (7KB)
- `simple-counter.md` - Complete working extension

### Documentation (30KB)

**GUIDE.md** (11KB)
- Complete skill documentation
- Installation guide
- Usage examples
- Troubleshooting

**QUICK_START.md** (8KB)
- Interactive examples
- Real-world workflows
- Common patterns
- Tips and tricks

---

## 🔄 Updates

### Updating the Skill

After pulling latest changes:

```bash
# Reinstall
./skill/install.sh

# Or update manually
cp -r skill/neomind-extension ~/.claude/skills/
```

Changes take effect immediately in Claude Code (no restart needed).

### Contributing Improvements

To improve the skill:

1. Edit files in `skill/neomind-extension/`
2. Test with `./skill/install.sh`
3. Verify in Claude Code
4. Submit PR

---

## 🆘 Support

### Documentation
- **Complete Guide**: [GUIDE.md](GUIDE.md)
- **Quick Start**: [QUICK_START.md](QUICK_START.md)
- **Skill Package**: [neomind-extension/](neomind-extension/)

### Installation Issues

**Skill not found:**
```bash
# Check installation
ls ~/.claude/skills/neomind-extension/SKILL.md

# Reinstall
./skill/install.sh
```

**Skill not activating:**
```bash
# Try manual invocation
/neomind-extension

# Check YAML frontmatter
head -10 ~/.claude/skills/neomind-extension/SKILL.md
```

### Getting Help

1. Check [GUIDE.md](GUIDE.md) for common issues
2. Review [QUICK_START.md](QUICK_START.md) for examples
3. Ask Claude: "I'm having trouble with the neomind-extension skill"

---

## 📈 Benefits

### For Individual Developers
- ⚡ **Faster Development**: AI generates boilerplate code
- 📚 **Always Learning**: Interactive documentation on-demand
- 🛠️ **Fewer Errors**: Built-in best practices
- 🎯 **Focused Work**: Less time on docs, more on features

### For Teams
- 🤝 **Consistent Code**: Shared standards and patterns
- 📖 **Easy Onboarding**: New developers self-serve
- 🚀 **Higher Productivity**: Reduce questions and blockers
- 🏆 **Better Quality**: Automatic best practices

---

## 📋 System Requirements

- **Claude Code**: Any version supporting skills
- **NeoMind**: isolated extension runtime
- **Platform**: macOS, Linux, Windows

---

## 📄 License

MIT License - Same as NeoMind-Extension repository

---

## 🎉 Get Started

1. **Install**: `./skill/install.sh`
2. **Read**: [QUICK_START.md](QUICK_START.md)
3. **Build**: Ask Claude to create your first extension!

---

**Version**: 1.0.0
**Created**: March 3, 2026
**Maintained by**: NeoMind Team

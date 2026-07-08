#!/bin/bash
# Install NeoMind Extension Development Skill for Claude Code
#
# This script installs the neomind-extension skill to your personal Claude skills directory.
# The skill provides comprehensive guidance for developing NeoMind extensions.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Skill information
SKILL_NAME="neomind-extension"
SOURCE_DIR="skill/${SKILL_NAME}"
TARGET_DIR="${HOME}/.claude/skills/${SKILL_NAME}"

echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   NeoMind Extension Development Skill Installer           ║${NC}"
echo -e "${BLUE}║   For Claude Code                                          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo

# Check if we're in the right directory
if [ ! -d "${SOURCE_DIR}" ]; then
    echo -e "${RED}Error: Skill source not found!${NC}"
    echo "Please run this script from the NeoMind-Extension repository root."
    exit 1
fi

echo -e "${YELLOW}📦 Installing skill: ${SKILL_NAME}${NC}"
echo

# Create target directory
echo -e "${BLUE}→${NC} Creating skill directory..."
mkdir -p "${HOME}/.claude/skills"

# Check if skill already exists
if [ -d "${TARGET_DIR}" ]; then
    echo -e "${YELLOW}⚠️  Skill already exists at: ${TARGET_DIR}${NC}"
    read -p "Do you want to overwrite it? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo -e "${YELLOW}Installation cancelled.${NC}"
        exit 0
    fi
    echo -e "${BLUE}→${NC} Removing existing skill..."
    rm -rf "${TARGET_DIR}"
fi

# Copy skill files
echo -e "${BLUE}→${NC} Copying skill files..."
cp -r "${SOURCE_DIR}" "${TARGET_DIR}"

# Verify installation
if [ -f "${TARGET_DIR}/SKILL.md" ]; then
    echo -e "${GREEN}✓${NC} Skill installed successfully!"
    echo
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║   Installation Complete!                                   ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
    echo
    echo -e "Skill location: ${BLUE}${TARGET_DIR}${NC}"
    echo
    echo -e "${YELLOW}📚 Usage:${NC}"
    echo "  1. Ask Claude about NeoMind extension development"
    echo "     Example: 'How do I create a NeoMind extension?'"
    echo
    echo "  2. Invoke the skill directly:"
    echo "     ${BLUE}/neomind-extension [extension-name]${NC}"
    echo
    echo -e "${YELLOW}📖 Documentation:${NC}"
    echo "  • Main guide:     ${TARGET_DIR}/SKILL.md"
    echo "  • Architecture:   ${TARGET_DIR}/reference/architecture.md"
    echo "  • SDK API:        ${TARGET_DIR}/reference/sdk-api.md"
    echo "  • Events:         ${TARGET_DIR}/reference/event-subscription.md"
    echo "  • Frontend:       ${TARGET_DIR}/reference/frontend.md"
    echo "  • Example:        ${TARGET_DIR}/examples/simple-counter.md"
    echo
    echo -e "${GREEN}🚀 Ready to start developing NeoMind extensions!${NC}"
    echo
else
    echo -e "${RED}✗ Installation failed!${NC}"
    echo "Skill file not found at expected location."
    exit 1
fi

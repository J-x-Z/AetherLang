#!/bin/bash
# AetherLang Installer Script
# Usage: curl -sSL https://raw.githubusercontent.com/J-x-Z/AetherLang/main/install.sh | bash

set -e

echo "🚀 Installing AetherLang..."

# Detect OS and Architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    darwin) OS="macos" ;;
    linux) OS="linux" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Set install directory
INSTALL_DIR="${HOME}/.aetherlang"
BIN_DIR="${INSTALL_DIR}/bin"

# Create directories
mkdir -p "$BIN_DIR"
mkdir -p "${INSTALL_DIR}/stdlib"

# Download latest release
VERSION="0.1.0-alpha"
RELEASE_URL="https://github.com/J-x-Z/AetherLang/releases/download/v${VERSION}"
BINARY_NAME="aethc-${OS}-${ARCH}"

echo "📦 Downloading AetherLang ${VERSION} for ${OS}-${ARCH}..."

# Download compiler
curl -sSL "${RELEASE_URL}/${BINARY_NAME}" -o "${BIN_DIR}/aethc"
chmod +x "${BIN_DIR}/aethc"

# Download jxz package manager
curl -sSL "${RELEASE_URL}/jxz-${OS}-${ARCH}" -o "${BIN_DIR}/jxz"
chmod +x "${BIN_DIR}/jxz"

# Download stdlib
echo "📚 Installing standard library..."
curl -sSL "${RELEASE_URL}/stdlib.tar.gz" -o "/tmp/stdlib.tar.gz"
tar -xzf "/tmp/stdlib.tar.gz" -C "${INSTALL_DIR}/stdlib" --strip-components=1
rm /tmp/stdlib.tar.gz

# Add to PATH
SHELL_CONFIG=""
if [ -f "${HOME}/.zshrc" ]; then
    SHELL_CONFIG="${HOME}/.zshrc"
elif [ -f "${HOME}/.bashrc" ]; then
    SHELL_CONFIG="${HOME}/.bashrc"
fi

if [ -n "$SHELL_CONFIG" ]; then
    if ! grep -q "\.aetherlang/bin" "$SHELL_CONFIG"; then
        echo "" >> "$SHELL_CONFIG"
        echo "# AetherLang" >> "$SHELL_CONFIG"
        echo 'export PATH="$HOME/.aetherlang/bin:$PATH"' >> "$SHELL_CONFIG"
        echo "✅ Added AetherLang to PATH in $SHELL_CONFIG"
    fi
fi

echo ""
echo "✨ AetherLang installed successfully!"
echo ""
echo "To get started, run:"
echo "  source ${SHELL_CONFIG}"
echo "  aethc --version"
echo "  jxz --version"
echo ""
echo "Quick start:"
echo "  echo 'fn main() -> i32 { return 42 }' > hello.aeth"
echo "  aethc --emit-c hello.aeth && cc hello.c -o hello && ./hello"
echo ""

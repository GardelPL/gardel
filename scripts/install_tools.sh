#!/usr/bin/env bash
#
# Downloads and sets up optional verification tools (Dafny, Z3, Boogie) for Gardel.
# Supports Linux and macOS.
#

set -e

echo "=== Setting up Gardel Verification Tools ==="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="${SCRIPT_DIR}/../tools/dafny"
DAFNY_VERSION="4.9.1"

OS="$(uname -s)"
ARCH="$(uname -m)"

# 1. macOS with Homebrew option
if [ "$OS" = "Darwin" ] && command -v brew >/dev/null 2>&1; then
    echo "[*] Detected macOS with Homebrew. Installing tools..."
    brew install dafny z3 boogie || true
    exit 0
fi

# 2. Linux / manual Dafny setup
mkdir -p "${SCRIPT_DIR}/../tools"

if [ -f "${TARGET_DIR}/dafny" ]; then
    echo "[✓] Dafny already installed at ${TARGET_DIR}"
else
    if [ "$OS" = "Darwin" ]; then
        if [ "$ARCH" = "arm64" ]; then
            DAFNY_ZIP="dafny-${DAFNY_VERSION}-arm64-macos.zip"
        else
            DAFNY_ZIP="dafny-${DAFNY_VERSION}-x64-macos.zip"
        fi
    else
        DAFNY_ZIP="dafny-${DAFNY_VERSION}-x64-ubuntu-20.04.zip"
    fi

    URL="https://github.com/dafny-lang/dafny/releases/download/v${DAFNY_VERSION}/${DAFNY_ZIP}"
    echo "[*] Downloading Dafny from ${URL}..."
    TMP_ZIP="/tmp/dafny-${DAFNY_VERSION}.zip"
    curl -sSL "$URL" -o "$TMP_ZIP"
    
    echo "[*] Extracting to ${TARGET_DIR}..."
    unzip -q -o "$TMP_ZIP" -d "${SCRIPT_DIR}/../tools"
    rm -f "$TMP_ZIP"
    echo "[✓] Dafny installed to ${TARGET_DIR}"
fi

# 3. Boogie via dotnet
if command -v dotnet >/dev/null 2>&1; then
    echo "[*] Installing/updating Boogie via dotnet..."
    dotnet tool install --global Boogie || dotnet tool update --global Boogie || true
else
    echo "[!] .NET SDK not detected. To install Boogie, install .NET SDK (https://dotnet.microsoft.com/download) and run: dotnet tool install -g Boogie"
fi

echo ""
echo "=== Verification Summary ==="
if command -v dafny >/dev/null 2>&1; then
    echo "[✓] Dafny: $(dafny --version)"
fi
if command -v boogie >/dev/null 2>&1; then
    echo "[✓] Boogie: available"
fi
echo "Ready to run: cargo run -- --stdlib --run"

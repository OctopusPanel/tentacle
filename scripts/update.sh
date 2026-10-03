#!/usr/bin/env bash
# ==============================================================================
#  🐙 Tentacle CLI Fallback Updater
#  Specification: Concept/UPDATE_ORCHESTRATION.md
# ==============================================================================

set -eo pipefail
export LC_ALL=C.UTF-8

BINARY_PATH="/usr/local/bin/tentacle"
GITHUB_REPO="OctopusPanel/tentacle"
TARGET_VERSION=""
DOWNLOAD_URL=""
EXPECTED_SHA256=""

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    -v|--version)
      TARGET_VERSION="$2"
      shift 2
      ;;
    -u|--url)
      DOWNLOAD_URL="$2"
      shift 2
      ;;
    -s|--sha256)
      EXPECTED_SHA256="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: $0 [--version <version>] [--url <download_url>] [--sha256 <checksum>]"
      exit 1
      ;;
  esac
done

echo "🐙 Tentacle Updater Starting..."

# Ensure root
if [[ $EUID -ne 0 ]]; then
  echo "❌ Error: This script must be run as root (or with sudo)."
  exit 1
fi

# Detect architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)
    RELEASE_ARCH="x86_64"
    ;;
  aarch64|arm64)
    RELEASE_ARCH="aarch64"
    ;;
  *)
    echo "❌ Error: Unsupported architecture: ${ARCH}"
    exit 1
    ;;
esac

# Resolve version and URL if not provided
if [[ -z "${DOWNLOAD_URL}" ]]; then
  if [[ -z "${TARGET_VERSION}" ]]; then
    echo "🔍 Querying latest release from GitHub (${GITHUB_REPO})..."
    TARGET_VERSION="$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)"
    if [[ -z "${TARGET_VERSION}" ]]; then
      echo "❌ Error: Could not determine latest version. Please provide --version <tag> explicitly."
      exit 1
    fi
  fi
  DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/download/${TARGET_VERSION}/tentacle-linux-${RELEASE_ARCH}.tar.gz"
fi

echo "📦 Target Version: ${TARGET_VERSION:-custom}"
echo "🌐 Download URL:   ${DOWNLOAD_URL}"

# Create temp dir
TMP_DIR="$(mktemp -d -t tentacle-update-XXXXXX)"
cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

PKG_PATH="${TMP_DIR}/package"
echo "⬇️  Downloading update package..."
curl -fsSL --progress-bar "${DOWNLOAD_URL}" -o "${PKG_PATH}"

# Verify SHA256 if provided
if [[ -n "${EXPECTED_SHA256}" ]]; then
  echo "🔒 Validating SHA256 checksum..."
  ACTUAL_SHA256="$(sha256sum "${PKG_PATH}" | awk '{print $1}')"
  if [[ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]]; then
    echo "❌ Error: Checksum mismatch!"
    echo "   Expected: ${EXPECTED_SHA256}"
    echo "   Got:      ${ACTUAL_SHA256}"
    exit 1
  fi
  echo "✅ Checksum verified."
fi

# Extract binary
BIN_SRC=""
if file "${PKG_PATH}" | grep -q "gzip compressed"; then
  tar -xzf "${PKG_PATH}" -C "${TMP_DIR}"
  if [[ -f "${TMP_DIR}/tentacle" ]]; then
    BIN_SRC="${TMP_DIR}/tentacle"
  fi
else
  BIN_SRC="${PKG_PATH}"
fi

if [[ ! -f "${BIN_SRC}" ]]; then
  echo "❌ Error: Could not find 'tentacle' binary in downloaded package."
  exit 1
fi

chmod +x "${BIN_SRC}"

# Atomic replacement
echo "🔄 Atomically updating ${BINARY_PATH}..."
install -m 755 "${BIN_SRC}" "${BINARY_PATH}"

# Restart daemon
if command -v systemctl &>/dev/null && systemctl is-active --quiet tentacle; then
  echo "🔁 Restarting Tentacle daemon (Docker containers will stay online)..."
  systemctl restart tentacle
fi

echo "✅ Tentacle updated successfully to $("${BINARY_PATH}" --version 2>/dev/null || echo "${TARGET_VERSION}")."

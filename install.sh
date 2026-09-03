#!/usr/bin/env bash
set -e

REPO="agungprasastia/graphia"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux*)  TARGET_OS="linux" ;;
  darwin*) TARGET_OS="darwin" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) TARGET_ARCH="x64" ;;
  arm64|aarch64) TARGET_ARCH="arm64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${TARGET_OS}-${TARGET_ARCH}"
RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"

echo "Detected platform: ${TARGET}"
echo "Fetching latest release from GitHub..."

ARCHIVE_NAME="graphia-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ARCHIVE_NAME}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "Downloading ${DOWNLOAD_URL}..."
curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}"

echo "Extracting binary..."
tar -xzf "${TMP_DIR}/${ARCHIVE_NAME}" -C "${TMP_DIR}"

INSTALL_DIR="/usr/local/bin"
if [ ! -w "${INSTALL_DIR}" ]; then
  INSTALL_DIR="${HOME}/.local/bin"
  mkdir -p "${INSTALL_DIR}"
  if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo "Notice: Add ${INSTALL_DIR} to your PATH to run graphia from anywhere:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  fi
fi

mv "${TMP_DIR}/graphia" "${INSTALL_DIR}/graphia"
chmod +x "${INSTALL_DIR}/graphia"

echo "Successfully installed graphia to ${INSTALL_DIR}/graphia"
"${INSTALL_DIR}/graphia" --version || true

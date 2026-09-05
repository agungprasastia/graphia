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
if [ -n "${GRAPHIA_ARCHIVE_PATH:-}" ]; then
  cp "${GRAPHIA_ARCHIVE_PATH}" "${TMP_DIR}/${ARCHIVE_NAME}"
else
  curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}"
fi

echo "Extracting binary..."
tar -xzf "${TMP_DIR}/${ARCHIVE_NAME}" -C "${TMP_DIR}"

AGENT_HOME="${GRAPHIA_INSTALL_HOME:-${HOME}}"
if [ -n "${GRAPHIA_INSTALL_HOME:-}" ]; then
  INSTALL_DIR="${AGENT_HOME}/.local/bin"
elif [ -w "/usr/local/bin" ]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="${AGENT_HOME}/.local/bin"
fi
if [ ! -d "${INSTALL_DIR}" ]; then
  mkdir -p "${INSTALL_DIR}"
fi
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
  if [ "${INSTALL_DIR}" != "/usr/local/bin" ]; then
    echo "Notice: Add ${INSTALL_DIR} to your PATH to run graphia from anywhere:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  fi
fi

mv "${TMP_DIR}/graphia" "${INSTALL_DIR}/graphia"
chmod +x "${INSTALL_DIR}/graphia"

echo "Successfully installed graphia to ${INSTALL_DIR}/graphia"
"${INSTALL_DIR}/graphia" --version || true

SKILL_SOURCE="${TMP_DIR}/skills/graphia"
if [ -f "${SKILL_SOURCE}/SKILL.md" ]; then
  for SKILL_TARGET in \
    "${AGENT_HOME}/.codex/skills/graphia" \
    "${AGENT_HOME}/.claude/skills/graphia" \
    "${AGENT_HOME}/.agents/skills/graphia" \
    "${AGENT_HOME}/.copilot/skills/graphia" \
    "${AGENT_HOME}/.config/opencode/skills/graphia"
  do
    mkdir -p "${SKILL_TARGET}"
    cp -R "${SKILL_SOURCE}/." "${SKILL_TARGET}/"
  done
  echo "Installed Graphia skill for Codex, Claude Code, Copilot, OpenCode, and Agent Skills clients."
else
  echo "Warning: release does not contain skills/graphia; binary installation remains usable." >&2
fi

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

APP_NAME="OTP Tools"
EXEC_NAME="otp-tools-gui"
BUNDLE_ID="com.otp-tools.gui"
ICON_SOURCE="${REPO_ROOT}/assets/AppIcon.icns"

BINARY_PATH="${1:-${REPO_ROOT}/target/release/${EXEC_NAME}}"
RAW_OUTPUT_DIR="${2:-${REPO_ROOT}/dist}"
OUTPUT_DIR="${RAW_OUTPUT_DIR%/}"
if [[ -z "${OUTPUT_DIR}" ]]; then
  OUTPUT_DIR="/"
fi

if [[ -n "${3:-}" ]]; then
  VERSION="${3}"
else
  VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "${REPO_ROOT}/Cargo.toml")"
fi

if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "Erro: binário GUI não encontrado em '${BINARY_PATH}'." >&2
  echo "Execute 'cargo build --release --bin ${EXEC_NAME}' antes de empacotar." >&2
  exit 1
fi

if [[ ! -f "${ICON_SOURCE}" ]]; then
  echo "Erro: ícone macOS não encontrado em '${ICON_SOURCE}'." >&2
  exit 1
fi

if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "Erro: versão inválida '${VERSION}'." >&2
  exit 1
fi

APP_BUNDLE="${OUTPUT_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${APP_BUNDLE}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

rm -rf -- "${APP_BUNDLE}"
mkdir -p -- "${MACOS_DIR}" "${RESOURCES_DIR}"

cp -- "${BINARY_PATH}" "${MACOS_DIR}/${EXEC_NAME}"
chmod -- 755 "${MACOS_DIR}/${EXEC_NAME}"

cp -- "${ICON_SOURCE}" "${RESOURCES_DIR}/AppIcon.icns"
chmod -- 644 "${RESOURCES_DIR}/AppIcon.icns"

cat > "${RESOURCES_DIR}/Credits.rtf" <<'EOF'
{\rtf1\ansi\ansicpg1252\cocoartf2761
{\fonttbl\f0\fnil\fcharset0 HelveticaNeue;\f1\fnil\fcharset0 HelveticaNeue-Bold;}
\pard\tx560\tx1120\tx1680\tx2240\tx2800\tx3360\tx3920\tx4480\tx5040\tx5600\tx6160\tx6720\pardirnatural\qc\partightenfactor0

\f1\b\fs24 OTP Tools\
\f0\b0\fs22 Leitor de QR Code & Decoder OTP (100% Offline)\
\
\f1\b Criado por:\f0\b0  Glauco G. Cetara\
\f1\b E-mail:\f0\b0  {\field{\*\fldinst{HYPERLINK "mailto:glaucogcetara@gmail.com"}}{\fldrslt glaucogcetara@gmail.com}}\
\f1\b GitHub:\f0\b0  {\field{\*\fldinst{HYPERLINK "https://github.com/c3t4r4/otp-tools.git"}}{\fldrslt https://github.com/c3t4r4/otp-tools.git}}\
\f1\b Licen\'e7a:\f0\b0  MIT License\
}
EOF
chmod -- 644 "${RESOURCES_DIR}/Credits.rtf"

cat > "${CONTENTS_DIR}/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleExecutable</key>
  <string>${EXEC_NAME}</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.utilities</string>
  <key>NSCameraUsageDescription</key>
  <string>O OTP Tools utiliza a câmera localmente para ler QR Codes de autenticação (otpauth:// e otpauth-migration://) diretamente da tela do seu celular, sem enviar nenhuma imagem pela rede.</string>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright © 2026 Glauco G. Cetara (glaucogcetara@gmail.com) • MIT License</string>
</dict>
</plist>
EOF

if command -v plutil >/dev/null 2>&1; then
  plutil -lint -- "${CONTENTS_DIR}/Info.plist" >/dev/null
fi

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - -- "${APP_BUNDLE}"
  codesign --verify --deep --strict -- "${APP_BUNDLE}"
fi

# Atualiza o mtime do bundle para sinalizar ao Finder/LaunchServices a atualização do ícone
touch -- "${APP_BUNDLE}"

echo "Bundle macOS criado com sucesso em: ${APP_BUNDLE}"

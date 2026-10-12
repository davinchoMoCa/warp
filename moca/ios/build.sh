#!/bin/bash
#
# Compila la app de iPad y la instala en el iPad conectado (cable o Wi‑Fi), firmada con el
# equipo de desarrollador. Requiere xcodegen (brew install xcodegen).
#
# Uso: moca/ios/build.sh                 # compila, instala y abre en el primer iPad emparejado
#      moca/ios/build.sh <id-dispositivo> # en un dispositivo concreto (xcrun devicectl list devices)

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
CONFIG="$HOME/.config/moca/remote-tty.env"
BUNDLE_ID="gt.moca.MocaWarp.ipad"

if [ ! -f "$CONFIG" ]; then
    echo "Falta $CONFIG (ver moca/pty-proxy/README.md)." >&2
    exit 1
fi
set -a
# shellcheck source=/dev/null
. "$CONFIG"
set +a

echo "==> Configuración (no va al repo: .gitignore)"
plutil -create xml1 MocaWarp/Config.generated.plist
plutil -insert url -string "$MOCA_REMOTE_TTY_URL" MocaWarp/Config.generated.plist
plutil -insert token -string "$MOCA_REMOTE_TTY_TOKEN" MocaWarp/Config.generated.plist

echo "==> Proyecto Xcode"
xcodegen generate --quiet

DEVICE="${1:-}"
if [ -z "$DEVICE" ]; then
    DEVICE=$(xcrun devicectl list devices 2>/dev/null | awk '/iPad/ && /(paired|connected)/ {print $3; exit}')
fi
if [ -z "$DEVICE" ]; then
    echo "No encontré un iPad emparejado (xcrun devicectl list devices)." >&2
    exit 1
fi

# La UDID (no el identificador de CoreDevice) es la que Xcode registra en la cuenta de
# desarrollador; compilar contra ese dispositivo concreto lo agrega al perfil de firma.
UDID=$(xcrun devicectl device info details --device "$DEVICE" 2>/dev/null | awk -F': ' '/udid:/ {print $2; exit}')

echo "==> Compilando para el dispositivo $DEVICE"
xcodebuild -project MocaWarp.xcodeproj -scheme MocaWarp -configuration Debug \
    -destination "platform=iOS,id=$UDID" -derivedDataPath build \
    -allowProvisioningUpdates -allowProvisioningDeviceRegistration -skipPackagePluginValidation -skipMacroValidation -quiet build

APP=$(find build/Build/Products/Debug-iphoneos -maxdepth 1 -name '*.app' | head -1)
echo "==> Instalando $(basename "$APP")"
xcrun devicectl device install app --device "$DEVICE" "$APP"
xcrun devicectl device process launch --device "$DEVICE" "$BUNDLE_ID" || \
    echo "Instalada. Si no abre, confía en el desarrollador: Ajustes → General → VPN y gestión de dispositivos."
echo "Listo."

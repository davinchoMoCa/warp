#!/bin/bash
#
# Moca Warp β para iPad: compila el motor de Warp (Rust) para iOS, lo empaqueta como .app,
# lo firma con el perfil de equipo y lo instala en el iPad. Convive con la v0.1 (SwiftTerm):
# otro bundle id, otro ícono en la pantalla de inicio.
#
# Uso: moca/ios/build-warp.sh [--release] [<id-dispositivo>]
#
# La terminal remota usa ~/.config/moca/remote-tty.env de la Mac: se copia dentro del .app
# (remote-tty.env junto al ejecutable) y también se pasa como variables de entorno al abrir la app
# desde aquí (devicectl). Nunca va al repo.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. && pwd)"
cd "$REPO_ROOT"

PROFILE_DIR="${CARGO_PROFILE:-debug}"
CARGO_FLAGS=()
if [ "${1:-}" = "--release" ]; then
    PROFILE_DIR=release
    CARGO_FLAGS+=(--release)
    shift
fi

TARGET=aarch64-apple-ios
BUNDLE_ID="gt.moca.MocaWarp.beta"
TEAM="DSXB7C9XQA"
IDENTITY="${MOCA_SIGNING_CERT:-CEFD043726E5D14EE1681BBA71704B1567C4523F}"
CONFIG="$HOME/.config/moca/remote-tty.env"
OUT="moca/ios/build/warp"
APP="$OUT/Moca Warp β.app"

# Misma versión mínima para el código en C (cc-rs) y para el enlace de Rust: si no, libgit2 pide
# ___chkstk_darwin, que no existe en el mínimo por defecto de rustc (iOS 10).
export IPHONEOS_DEPLOYMENT_TARGET=17.0

echo "==> Compilando Warp para iOS ($PROFILE_DIR)"
[ -f ~/.cargo/env ] && source ~/.cargo/env
cargo build -p warp --bin warp-oss --target "$TARGET" ${CARGO_FLAGS[@]+"${CARGO_FLAGS[@]}"} \
    --features gui,remote_tty

echo "==> Empaquetando $APP"
rm -rf "$APP"
mkdir -p "$APP"
cp "target/$TARGET/$PROFILE_DIR/warp-oss" "$APP/warp-oss"
# URL y token de la XPS dentro del .app (firmado, solo va al iPad): así conecta aunque se abra
# desde la pantalla de inicio. El repo es público: el archivo sale de la Mac, nunca de git.
if [ -f "$CONFIG" ]; then
    grep -E '^MOCA_REMOTE_TTY_(URL|TOKEN)=' "$CONFIG" > "$APP/remote-tty.env"
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)
cat > "$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key><string>warp-oss</string>
    <key>CFBundleName</key><string>Moca Warp β</string>
    <key>CFBundleDisplayName</key><string>Moca Warp β</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$(date +%Y%m%d%H%M)</string>
    <key>CFBundleSupportedPlatforms</key><array><string>iPhoneOS</string></array>
    <key>MinimumOSVersion</key><string>$IPHONEOS_DEPLOYMENT_TARGET</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>UIRequiredDeviceCapabilities</key><array><string>arm64</string></array>
    <key>UILaunchScreen</key><dict/>
    <key>UIApplicationSupportsIndirectInputEvents</key><true/>
    <key>UIFileSharingEnabled</key><true/>
    <key>LSSupportsOpeningDocumentsInPlace</key><true/>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>NSAppTransportSecurity</key><dict><key>NSAllowsArbitraryLoads</key><true/></dict>
</dict>
</plist>
EOF

# Claves de compilación que Xcode pone en toda app de iOS; sin ellas el iPad trata el perfil
# como "beta" y rechaza la instalación (MIInstallerErrorDomain 13, 0xe800801f).
SDK_VERSION=$(xcrun --sdk iphoneos --show-sdk-version)
SDK_BUILD=$(xcrun --sdk iphoneos --show-sdk-build-version)
XCODE_VERSION=$(xcodebuild -version | awk 'NR==1 {print $2}')
XCODE_BUILD=$(xcodebuild -version | awk 'NR==2 {print $3}')
XCODE_DT=$(printf '%s' "$XCODE_VERSION" | awk -F. '{printf "%02d%d%d", $1, $2, ($3 == "" ? 0 : $3)}')
for kv in "DTPlatformName:iphoneos" "DTPlatformVersion:$SDK_VERSION" "DTPlatformBuild:$SDK_BUILD" \
          "DTSDKName:iphoneos$SDK_VERSION" "DTSDKBuild:$SDK_BUILD" "DTXcode:$XCODE_DT" \
          "DTXcodeBuild:$XCODE_BUILD" "DTCompiler:com.apple.compilers.llvm.clang.1_0" \
          "BuildMachineOSBuild:$(sw_vers -buildVersion)"; do
    plutil -insert "${kv%%:*}" -string "${kv#*:}" "$APP/Info.plist"
done

echo "==> Firmando"
PROFILE=""
for f in "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"/*.mobileprovision \
         "$HOME/Library/MobileDevice/Provisioning Profiles"/*.mobileprovision; do
    [ -f "$f" ] || continue
    # Perfil de desarrollo comodín: application-identifier exactamente "EQUIPO.*" y con
    # get-task-allow (los de App Store/Ad Hoc también mencionan EQUIPO.* en otros campos).
    decoded=$(security cms -D -i "$f" 2>/dev/null) || continue
    appid=$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:application-identifier' /dev/stdin <<< "$decoded" 2>/dev/null)
    debug=$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:get-task-allow' /dev/stdin <<< "$decoded" 2>/dev/null)
    if [ "$appid" = "$TEAM.*" ] && [ "$debug" = "true" ]; then
        PROFILE="$f"
        break
    fi
done
if [ -z "$PROFILE" ]; then
    echo "No hay perfil comodín del equipo $TEAM: corre moca/ios/build.sh una vez (lo crea Xcode)." >&2
    exit 1
fi
cp "$PROFILE" "$APP/embedded.mobileprovision"
cat > "$OUT/entitlements.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>application-identifier</key><string>$TEAM.$BUNDLE_ID</string>
    <key>com.apple.developer.team-identifier</key><string>$TEAM</string>
    <key>get-task-allow</key><true/>
</dict>
</plist>
EOF
codesign --force --sign "$IDENTITY" --entitlements "$OUT/entitlements.plist" "$APP"
codesign --verify --strict "$APP"

DEVICE="${1:-}"
if [ -z "$DEVICE" ]; then
    DEVICE=$(xcrun devicectl list devices 2>/dev/null | awk '/iPad/ && /(paired|connected)/ {print $3; exit}')
fi
echo "==> Instalando en el iPad $DEVICE"
xcrun devicectl device install app --device "$DEVICE" "$APP"

ENV_JSON="{}"
if [ -f "$CONFIG" ]; then
    set -a
    # shellcheck source=/dev/null
    . "$CONFIG"
    set +a
    ENV_JSON=$(printf '{"MOCA_REMOTE_TTY_URL":"%s","MOCA_REMOTE_TTY_TOKEN":"%s","RUST_BACKTRACE":"1"}' \
        "$MOCA_REMOTE_TTY_URL" "$MOCA_REMOTE_TTY_TOKEN")
fi
echo "==> Abriendo"
xcrun devicectl device process launch --device "$DEVICE" --terminate-existing \
    --environment-variables "$ENV_JSON" "$BUNDLE_ID"
echo "Listo: Moca Warp β en el iPad."

#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
if [ -x bin/relay ]; then
    relay_binary=bin/relay
else
    if command -v devbox >/dev/null 2>&1; then
        devbox --project "$PWD" cargo build --release --locked --target-dir target/podman
        relay_binary=target/podman/release/relay
    else
        cargo build --release --locked
        relay_binary=target/release/relay
    fi
fi
install -Dm755 "$relay_binary" "$HOME/.local/opt/relay/relay"
# One binary; compatibility names are symlinks. No Python, zenity or wl-paste
# helper scripts are installed.
mkdir -p "$HOME/.local/libexec/relay"
for helper in relay-engine relay-strata relay-codex relay-skills relay-web \
        relay-clipboard relay-sudo relay-askpass relay-local-model; do
    ln -sfn "$HOME/.local/opt/relay/relay" "$HOME/.local/libexec/relay/$helper"
done
rm -f "$HOME/.local/libexec/relay/sudo-session.py" "$HOME/.local/libexec/relay/askpass"
install -Dm644 assets/relay.png "$HOME/.local/share/icons/relay.png"
install -Dm644 assets/relay.png "$HOME/.local/share/icons/relay-mark.png"
install -Dm644 assets/relay.png "$HOME/.local/share/icons/relay-rounded.png"
install -Dm644 assets/relay.png "$HOME/.local/share/icons/relay-symbol.png"
install -Dm644 assets/relay-small.png "$HOME/.local/share/icons/relay-small-rounded.png"
install -Dm644 assets/relay-tray.png "$HOME/.local/share/icons/relay-tray.png"
install -Dm644 assets/relay-tray.png "$HOME/.local/share/icons/relay-tray-rounded.png"
install -Dm644 assets/relay-tray.png "$HOME/.local/share/icons/relay-symbol-tray.png"
install -Dm644 assets/relay-tray.svg "$HOME/.local/share/icons/relay-tray.svg"
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
ln -sfn "$HOME/.local/opt/relay/relay" "$HOME/.local/bin/relay"
cat > "$HOME/.local/share/applications/relay.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Relay
Comment=Local AI agent with files and tools
Exec=$HOME/.local/bin/relay
Icon=$HOME/.local/share/icons/relay-small-rounded.png
Terminal=false
StartupNotify=true
StartupWMClass=relay
Categories=Development;Utility;
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" || true
printf '%s\n' 'Installed Relay 0.1. Launch Relay and configure the Strata endpoint in Settings.'

#!/usr/bin/env bash
set -e

if ! pkg-config --exists gtk4; then
    echo "Install GTK4 dev packages first:"
    echo "  Fedora: sudo dnf install gtk4-devel gcc pkg-config"
    echo "  Ubuntu/Debian: sudo apt install libgtk-4-dev build-essential pkg-config"
    echo "  Arch: sudo pacman -S gtk4 base-devel"
    exit 1
fi

pkill -x gnome-carousel 2>/dev/null || true

cargo build --release
mkdir -p "$HOME/.local/bin"
cp target/release/gnome-carousel "$HOME/.local/bin/gnome-carousel"

EXT_DIR="$HOME/.local/share/gnome-shell/extensions/gnome-carousel-overlay@local"
mkdir -p "$EXT_DIR"
cp shell-overlay/* "$EXT_DIR/"

# Autostart background daemon on login
AUTOSTART_DIR="$HOME/.config/autostart"
mkdir -p "$AUTOSTART_DIR"
cat > "$AUTOSTART_DIR/gnome-carousel.desktop" << EOF
[Desktop Entry]
Type=Application
Name=GNOME Wallpaper Carousel Daemon
Exec=$HOME/.local/bin/gnome-carousel --daemon
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF

# Register Super + W without touching existing shortcuts
BASE="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings"
SCHEMA="org.gnome.settings-daemon.plugins.media-keys"
CURRENT=$(gsettings get $SCHEMA custom-keybindings)

TARGET_PATH=""
if [ "$CURRENT" != "@as []" ]; then
    for path in $(echo "$CURRENT" | tr -d "[]'" | tr ',' ' '); do
        name=$(gsettings get "$SCHEMA.custom-keybinding:$path" name 2>/dev/null || echo "")
        if [ "$name" = "'Wallpaper Carousel'" ]; then
            TARGET_PATH="$path"
            break
        fi
    done
fi

if [ -z "$TARGET_PATH" ]; then
    i=0
    while echo "$CURRENT" | grep -q "$BASE/custom$i/"; do
        i=$((i + 1))
    done
    TARGET_PATH="$BASE/custom$i/"

    if [ "$CURRENT" = "@as []" ]; then
        NEW_LIST="['$TARGET_PATH']"
    else
        NEW_LIST="${CURRENT%]*}, '$TARGET_PATH']"
    fi
    gsettings set $SCHEMA custom-keybindings "$NEW_LIST"
fi

gsettings set "$SCHEMA.custom-keybinding:$TARGET_PATH" name "Wallpaper Carousel"
gsettings set "$SCHEMA.custom-keybinding:$TARGET_PATH" command "$HOME/.local/bin/gnome-carousel"
gsettings set "$SCHEMA.custom-keybinding:$TARGET_PATH" binding "<Super>w"

# Start the background daemon right now
nohup "$HOME/.local/bin/gnome-carousel" --daemon >/dev/null 2>&1 &

echo "Done! Background daemon is running and enabled at startup."
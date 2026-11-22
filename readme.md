# GNOME Wallpaper Carousel (`gnome-carousel`)

An instant-opening, borderless, slanted-parallelogram wallpaper switcher for GNOME (49, 50, and 51+) written in Rust and GTK4. It runs quietly in the background (~40 MB RAM) and toggles instantaneously on `Super + W` as a floating hover overlay without appearing in your dock or Overview.

---

## Controls

| Input | Action |
| :--- | :--- |
| `Super + W` | Toggle carousel open / closed |
| `←` / `→` or `A` / `D` or `H` / `L` | Slide left / right through wallpapers |
| Mouse Wheel / Trackpad Scroll | Slide through wallpapers |
| Click Side Card | Center that wallpaper |
| `Enter` or Click Center Card | Apply wallpaper and hide overlay |
| `Esc` or `Q` | Cancel and hide overlay |

---

## Installation

### 1. Install Dependencies
* **Fedora:**
  ```bash
  sudo dnf install gtk4-devel gcc pkg-config cargo rust
  ```
* **Ubuntu / Debian:**
  ```bash
  sudo apt install libgtk-4-dev build-essential pkg-config cargo rustc
  ```
* **Arch Linux:**
  ```bash
  sudo pacman -S gtk4 base-devel rust
  ```

### 2. Clone & Run the Installer
1. Place your wallpapers (`.jpg`, `.jpeg`, `.png`, or `.webp`) inside `~/Pictures/Wallpapers`.
2. Clone this repository and run `install.sh`:
   ```bash
   git clone <YOUR_REPO_URL> gnome-carousel
   cd gnome-carousel
   chmod +x install.sh
   ./install.sh
   ```
   This builds the binary into `~/.local/bin/gnome-carousel`, installs the GNOME Shell overlay extension, configures login autostart (`~/.config/autostart/gnome-carousel.desktop`), binds `Super + W` without overwriting existing shortcuts, and starts the background daemon.

### 3. Enable the Floating Overlay Extension
1. **Log out and log back in once** so GNOME Shell detects the new extension on Wayland.
2. Enable the extension:
   ```bash
   gnome-extensions enable gnome-carousel-overlay@local
   ```
3. Press **`Super + W`** anywhere on your desktop to open or close the carousel.

---

## How Thumbnail Loading Works

Thumbnails are cached automatically—you do not need to load or rebuild them manually:

* **On Login:** The background daemon scans `~/Pictures/Wallpapers` and decodes `640×360` thumbnails in parallel across all CPU cores into `~/.cache/gnome-carousel/`.
* **Adding or Modifying Wallpapers:** Each time you press `Super + W`, the daemon checks `~/Pictures/Wallpapers` for new files or changed modification timestamps (`mtime`) and caches only the new/updated images automatically.
* **Force Cache Rebuild (Optional):**
  ```bash
  rm -rf ~/.cache/gnome-carousel
  ```
  Fresh thumbnails will be generated automatically the next time you press `Super + W`.

---

## Troubleshooting

* **`Super + W` does nothing / Daemon stopped running:**
  ```bash
  pkill -x gnome-carousel || true
  nohup ~/.local/bin/gnome-carousel --daemon >/dev/null 2>&1 &
  ```
* **Carousel is not centered or appears in the dock / Overview:**
  Log out and back in once if you haven't already, then enable and verify the extension:
  ```bash
  gnome-extensions enable gnome-carousel-overlay@local
  gnome-extensions info gnome-carousel-overlay@local
  ```
* **Missing shortcut or moved binary:**
  Re-run `./install.sh` from the repository directory to restore the `Super + W` keybinding and autostart entry.

---

## Uninstall

```bash
pkill -x gnome-carousel || true
gnome-extensions disable gnome-carousel-overlay@local || true
rm -rf ~/.local/bin/gnome-carousel \
       ~/.local/share/gnome-shell/extensions/gnome-carousel-overlay@local \
       ~/.config/autostart/gnome-carousel.desktop \
       ~/.cache/gnome-carousel
```
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

export default class CarouselOverlayExtension {
    _positionAndFloat(win) {
        win.make_above();
        win.stick();

        const monitor = Main.layoutManager.primaryMonitor;
        const rect = win.get_frame_rect();
        const x = monitor.x + Math.round((monitor.width - rect.width) / 2);
        const y = monitor.y + Math.round((monitor.height - rect.height) / 2);
        win.move_frame(false, x, y);
    }

    enable() {
        this._winCreatedId = global.display.connect('window-created', (_display, win) => {
            const wmClass = (win.get_wm_class() || '').toLowerCase();
            const title = (win.get_title() || '').toLowerCase();
            const appId = (win.get_gtk_application_id() || '').toLowerCase();

            if (wmClass.includes('gnome-carousel') || title.includes('gnome-carousel') || appId.includes('gnome_carousel')) {
                this._positionAndFloat(win);

                const shownId = win.connect('shown', () => {
                    this._positionAndFloat(win);
                });

                const unmanageId = win.connect('unmanaged', () => {
                    win.disconnect(shownId);
                    win.disconnect(unmanageId);
                });
            }
        });
    }

    disable() {
        if (this._winCreatedId) {
            global.display.disconnect(this._winCreatedId);
            this._winCreatedId = null;
        }
    }
}
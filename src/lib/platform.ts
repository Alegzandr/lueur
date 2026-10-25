// What the system the interface runs on can do, and how it names things.

const nav = typeof navigator !== "undefined" ? navigator : undefined;

const WINDOWS = !!nav && /windows/i.test(nav.userAgent);

/** Whether fullscreen can be detected, which only Windows offers (see
 *  src-tauri/src/auto.rs): automatic mode exists nowhere else. */
export const DETECTS_FULLSCREEN = WINDOWS;

/** Whether the page draws the title bar itself, which it does on Windows
 *  (see src-tauri/src/titlebar.rs); elsewhere the system's own stays. */
export const OWN_TITLEBAR = WINDOWS;

/** How keyboard shortcuts are written here: ⌘ on a Mac, Ctrl+ elsewhere. */
export const MOD_KEY = nav?.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

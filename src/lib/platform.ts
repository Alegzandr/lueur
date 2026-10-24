// What the system the interface runs on can do, and how it names things.

const nav = typeof navigator !== "undefined" ? navigator : undefined;

/** Whether fullscreen can be detected, which only Windows offers (see
 *  src-tauri/src/auto.rs): automatic mode exists nowhere else. */
export const DETECTS_FULLSCREEN = !!nav && /windows/i.test(nav.userAgent);

/** How keyboard shortcuts are written here: ⌘ on a Mac, Ctrl+ elsewhere. */
export const MOD_KEY = nav?.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

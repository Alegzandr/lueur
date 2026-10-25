<script lang="ts">
  // The title bar on Windows, drawn here so it takes the colour of what lies
  // under it (see src-tauri/src/titlebar.rs). The empty strip drags the window,
  // and a double-click maximises it, through the system. The buttons are the
  // system's own glyphs and sizes, out of the Tab order as theirs are.
  import { onMount } from "svelte";
  import { closeWindow, minimizeWindow, onMaximized, toggleMaximizeWindow } from "../ipc";
  import { t } from "../i18n";
  import UpdateButton from "./UpdateButton.svelte";

  let maximized = $state(false);

  onMount(() => {
    const unlisten = onMaximized((m) => (maximized = m));
    return () => void unlisten.then((u) => u());
  });
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="extra"><UpdateButton /></div>
  <div class="caption">
    <button tabindex="-1" aria-label={t("window_minimize")} title={t("window_minimize")} onclick={minimizeWindow}>
      {""}
    </button>
    <button
      tabindex="-1"
      aria-label={maximized ? t("window_restore") : t("window_maximize")}
      title={maximized ? t("window_restore") : t("window_maximize")}
      onclick={toggleMaximizeWindow}
    >
      {maximized ? "" : ""}
    </button>
    <button class="close" tabindex="-1" aria-label={t("window_close")} title={t("window_close")} onclick={closeWindow}>
      {""}
    </button>
  </div>
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex: none;
    height: 32px;
  }
  .extra {
    display: flex;
    align-items: center;
    padding-right: var(--s-2);
  }
  .caption {
    display: flex;
    align-self: stretch;
  }
  .caption button {
    width: 46px;
    font-family: "Segoe Fluent Icons", "Segoe MDL2 Assets";
    font-size: 10px;
    color: var(--text);
    transition: background-color var(--dur) var(--ease);
  }
  .caption button:hover {
    background: var(--surface-hover);
  }
  .caption button:active {
    background: var(--line);
  }
  /* The system's own red. */
  .caption .close:hover {
    background: #c42b1c;
    color: #fff;
  }
  .caption .close:active {
    background: #c84031;
    color: #fff;
  }
</style>

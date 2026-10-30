<script lang="ts">
  import { app, bindingFor, deleteScene, recallScene, saveScene, sceneNamed, showsScene } from "../../state.svelte";
  import { t } from "../../i18n";
  import type { SavedScene } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import { distinctLooks, lookOfScene } from "./pictures";

  // The scenes the user saved, first on the page: the whole room as it was,
  // back in one click.

  const scenes = $derived(app.config.scenes ?? []);

  /** A scene as the room it makes: the photograph of each thing its lights
   *  do, side by side, in the order of the room. */
  const looksOf = (scene: SavedScene) =>
    distinctLooks(
      app.config.room.lights.flatMap((l) => {
        const s = scene.lights[l.id];
        return s && bindingFor(l.id) ? [lookOfScene(s)] : [];
      }),
      3,
    );

  // Saving asks for a name in place; the name of a scene already saved
  // replaces it.
  let naming = $state(false);
  let name = $state("");
  const replacing = $derived(name.trim() ? sceneNamed(name) : undefined);
  let saveButton = $state<HTMLButtonElement>();

  function startNaming() {
    naming = true;
    name = "";
  }

  function stopNaming() {
    naming = false;
    // The field that had the keyboard is gone: back to where it came from.
    queueMicrotask(() => saveButton?.focus());
  }

  function save(e: SubmitEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    saveScene(name);
    stopNaming();
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  // Deleting asks first, in the scene's own place, with the keyboard on
  // the answer; Escape or Cancel keeps it.
  let confirming = $state<string | null>(null);

  function keep(id: string) {
    confirming = null;
    queueMicrotask(() => document.getElementById(`scene-del-${id}`)?.focus());
  }

  function escapeKeeps(e: KeyboardEvent, id: string) {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    keep(id);
  }
</script>

<section class="group">
  <div class="group-head">
    <h2 class="group-title">{t("scenes")}</h2>
    {#if !naming && scenes.length > 0}
      <button class="btn plain sm" title={t("scene_save_tip")} bind:this={saveButton} onclick={startNaming}>
        <Icon name="plus" size={13} />
        {t("scene_save")}
      </button>
    {/if}
  </div>

  {#if naming}
    <form class="card naming" onsubmit={save}>
      <input
        type="text"
        maxlength="40"
        placeholder={t("scene_name_hint")}
        aria-label={t("scene_name")}
        bind:value={name}
        use:focusOnMount
        onkeydown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            stopNaming();
          }
        }}
      />
      <button type="submit" class="btn sm" disabled={!name.trim()}>
        {replacing ? t("scene_replace") : t("scene_save_confirm")}
      </button>
      <button type="button" class="btn plain sm" onclick={stopNaming}>{t("sleep_cancel")}</button>
    </form>
    <p class="hint">{replacing ? t("scene_replace_tip", { name: replacing.name }) : t("scene_save_tip")}</p>
  {:else if scenes.length === 0}
    <!-- No scene yet: the place one goes, which saves the first. -->
    <button class="empty" bind:this={saveButton} onclick={startNaming}>
      <span class="empty-mark" aria-hidden="true"><Icon name="plus" size={14} /></span>
      <span class="empty-text">
        <span class="empty-title">{t("scene_save")}</span>
        <span class="empty-hint">{t("scenes_empty")}</span>
      </span>
    </button>
  {/if}

  {#if scenes.length > 0}
    <ul class="scenes" class:spaced={naming}>
      {#each scenes as s (s.id)}
        {@const on = showsScene(s)}
        {#if confirming === s.id}
          <li class="scene confirm">
            <span class="confirm-text">{t("scene_delete_ask", { name: s.name })}</span>
            <span class="confirm-actions">
              <button
                class="btn sm danger"
                use:focusOnMount
                onkeydown={(e) => escapeKeeps(e, s.id)}
                onclick={() => {
                  confirming = null;
                  deleteScene(s.id);
                }}>{t("delete")}</button
              >
              <button class="btn plain sm" onkeydown={(e) => escapeKeeps(e, s.id)} onclick={() => keep(s.id)}
                >{t("sleep_cancel")}</button
              >
            </span>
          </li>
        {:else}
          <li class="scene" class:active={on} class:pending={app.pending === s.id}>
            <button
              class="recall"
              aria-pressed={on}
              disabled={app.busy && app.pending !== s.id}
              onclick={() => recallScene(s)}
            >
              <span class="slices" aria-hidden="true">
                {#each looksOf(s) as look (look.key)}
                  <span class="slice">
                    <img src={look.src} alt="" draggable="false" />
                    {#if look.tint}<span class="tint" style="background:{look.tint}"></span>{/if}
                  </span>
                {/each}
              </span>
              <!-- Showing now: an amber ring and a lamp, not the colour alone. -->
              <span class="scene-name">
                {#if on}<span class="lamp" aria-hidden="true"></span>{/if}<span class="label">{s.name}</span>
              </span>
            </button>
            <button
              class="del"
              id="scene-del-{s.id}"
              title={t("scene_delete", { name: s.name })}
              aria-label={t("scene_delete", { name: s.name })}
              onclick={() => (confirming = s.id)}
            >
              <Icon name="close" size={11} />
            </button>
          </li>
        {/if}
      {/each}
    </ul>
  {/if}
</section>

<style>
  .naming {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-2);
  }
  .naming input {
    flex: 1;
    min-width: 0;
  }
  .hint {
    margin: var(--s-2) var(--s-1) 0;
    font-size: var(--t-sm);
    color: var(--text-3);
    line-height: 1.4;
  }
  .empty {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    width: 100%;
    min-height: 90px;
    padding: var(--s-3);
    border-radius: var(--r-lg);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    background-image: none;
    text-align: left;
    transition: background var(--dur) var(--ease);
  }
  .empty:hover {
    background: var(--surface-hover);
  }
  .empty-mark {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1.5px dashed var(--line-strong);
    color: var(--text-2);
  }
  .empty-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .empty-title {
    font-weight: 600;
  }
  .empty-hint {
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .naming + .hint {
    margin-top: var(--s-1);
  }
  /* A scene is a card like the modes: the photographs of what its lights
     do side by side, its name on the card beneath them. */
  .scenes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--s-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .scenes.spaced {
    margin-top: var(--s-3);
  }
  .scene {
    position: relative;
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
    transition:
      box-shadow 200ms var(--ease),
      background var(--dur) var(--ease);
  }
  .scene:has(.recall:hover:not(:disabled)) {
    background: var(--surface-hover);
  }
  .scene.active {
    box-shadow: inset 0 0 0 2px var(--active);
  }
  .recall {
    display: flex;
    flex-direction: column;
    gap: 7px;
    width: 100%;
    padding: 5px 5px 9px;
    border-radius: inherit;
    text-align: left;
  }
  .scene:not(.pending) .recall:disabled {
    opacity: 0.6;
  }
  .slices {
    position: relative;
    display: flex;
    gap: 2px;
    height: 48px;
    overflow: hidden;
    border-radius: 6px;
    background: var(--sunken);
  }
  .slices::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.14);
  }
  .slice {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .slice img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 50% 42%;
    transition: transform 500ms var(--ease);
  }
  .recall:hover:not(:disabled) .slice img {
    transform: scale(1.05);
  }
  .tint {
    position: absolute;
    inset: 0;
    mix-blend-mode: color;
    opacity: 0.9;
  }
  .scene-name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 0 4px;
    font-weight: 600;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scene-name .lamp {
    --lamp: 6px;
  }
  .pending .slices {
    animation: breathe 800ms var(--ease) infinite alternate;
  }
  @keyframes breathe {
    to {
      opacity: 0.45;
    }
  }
  /* Delete waits out of sight until the scene is hovered or reached by
     the keyboard: a small round mark on the corner of its pictures. */
  .del {
    position: absolute;
    top: 9px;
    right: 9px;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: var(--control-edge);
    color: var(--text-2);
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }
  .scene:hover .del,
  .del:focus-visible {
    opacity: 1;
  }
  .confirm {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--s-2);
    min-height: 90px;
    padding: 0 var(--s-2) 0 var(--s-3);
  }
  .confirm-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .confirm-actions {
    display: flex;
    gap: var(--s-1);
    margin-left: -6px;
  }
  @media (prefers-reduced-motion: reduce) {
    .pending .slices {
      animation: none;
      opacity: 0.5;
    }
    .slice img {
      transition: none;
    }
    .recall:hover:not(:disabled) .slice img {
      transform: none;
    }
  }
</style>

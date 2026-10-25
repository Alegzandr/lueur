<script lang="ts">
  import { tick } from "svelte";
  import { app, dismiss, holdToast, releaseToast, type Toast } from "../state.svelte";
  import Icon from "./Icon.svelte";
  import { t as tr } from "../i18n";

  // Two live regions, always present, so each toast is announced once as it
  // arrives (or comes again): errors at once (alert), information when the
  // reader is free (status). The toasts on screen carry no live role of
  // their own, so closing one or opening its details is not read out again.
  let alertText = $state("");
  let statusText = $state("");
  let lastSaid = 0;
  // Messages arriving together are queued and read as one, so a second
  // error in the same moment does not cut the first off.
  const queued: Record<Toast["kind"], string[]> = { error: [], info: [] };
  let flush: ReturnType<typeof setTimeout> | null = null;

  function announce(kind: Toast["kind"], msg: string): void {
    // Emptied first, so the same words coming again are read again.
    if (queued[kind].length === 0) {
      if (kind === "error") alertText = "";
      else statusText = "";
    }
    queued[kind].push(msg);
    flush ??= setTimeout(() => {
      if (queued.error.length > 0) alertText = queued.error.join(" ");
      if (queued.info.length > 0) statusText = queued.info.join(" ");
      queued.error = [];
      queued.info = [];
      flush = null;
    }, 80);
  }

  $effect(() => {
    const fresh = app.toasts.filter((t) => t.said > lastSaid).sort((a, b) => a.said - b.said);
    for (const t of fresh) announce(t.kind, t.msg);
    if (fresh.length > 0) lastSaid = fresh[fresh.length - 1].said;
  });

  // Where focus was before it came into the toasts, to give it back.
  let before: HTMLElement | null = null;
  function noteBefore(e: FocusEvent) {
    const from = e.relatedTarget as HTMLElement | null;
    if (from && !(e.currentTarget as HTMLElement).contains(from)) before = from;
  }

  /** Close a toast. When focus was in it, it moves to the next toast (or the
   *  one before), else back where it was before the toasts, never to the
   *  page itself. */
  async function close(id: number, el: HTMLElement) {
    const hadFocus = el.contains(document.activeElement);
    const i = app.toasts.findIndex((t) => t.id === id);
    const neighbour = app.toasts[i + 1] ?? app.toasts[i - 1];
    dismiss(id);
    if (!hadFocus) return;
    await tick();
    const next = neighbour && document.querySelector<HTMLElement>(`[data-toast="${neighbour.id}"] .x`);
    if (next) next.focus();
    else if (before?.isConnected) before.focus();
  }

  /** The toasts whose technical details are unfolded. */
  let open = $state<Record<number, boolean>>({});
  /** The toast whose text was just copied, to say so for a moment. */
  let copied = $state<number | null>(null);

  async function copy(t: Toast): Promise<void> {
    const text = t.detail ? `${t.msg}\n${t.detail}` : t.msg;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // No clipboard access: fall back on the selection, which webviews allow.
      const area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.append(area);
      area.select();
      document.execCommand("copy");
      area.remove();
    }
    copied = t.id;
    setTimeout(() => {
      if (copied === t.id) copied = null;
    }, 2000);
  }
</script>

<div class="sr-only" role="alert">{alertText}</div>
<div class="sr-only" role="status">{statusText}</div>

<div class="toasts" role="presentation" onfocusin={noteBefore}>
  {#each app.toasts as t (t.id)}
    <!-- Pointing at or focusing an information toast keeps it up until you
         move on; errors stay until closed anyway. -->
    <div
      class="toast"
      data-toast={t.id}
      class:error={t.kind === "error"}
      role="presentation"
      onmouseenter={() => holdToast(t.id, "hover")}
      onmouseleave={() => releaseToast(t.id, "hover")}
      onfocusin={() => holdToast(t.id, "focus")}
      onfocusout={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) releaseToast(t.id, "focus");
      }}
    >
      {#if t.kind === "error"}<span class="ic"><Icon name="alert" size={15} /></span>{/if}
      <div class="body">
        <p>
          {t.msg}{#if t.count && t.count > 1}<span class="count num">&nbsp;×{t.count}</span>{/if}
        </p>
        {#if t.kind === "error"}
          <div class="tools">
            {#if t.detail}
              <button
                class="link"
                aria-expanded={open[t.id] ?? false}
                aria-controls="toast-detail-{t.id}"
                onclick={() => (open[t.id] = !open[t.id])}
              >
                {open[t.id] ? tr("hide_details") : tr("details")}
              </button>
            {/if}
            <button class="link" title={tr("copy_tip")} onclick={() => copy(t)}>
              {copied === t.id ? tr("copied") : tr("copy")}
            </button>
          </div>
          {#if t.detail && open[t.id]}
            <pre id="toast-detail-{t.id}" class="detail">{t.detail}</pre>
          {/if}
        {/if}
      </div>
      {#if t.action}
        {@const action = t.action}
        <button
          class="btn sm"
          onclick={(e) => {
            action.run();
            void close(t.id, e.currentTarget.closest(".toast") as HTMLElement);
          }}>{action.label}</button
        >
      {/if}
      <button class="x" aria-label={tr("dismiss")} onclick={(e) => close(t.id, e.currentTarget.closest(".toast") as HTMLElement)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: var(--s-5);
    bottom: var(--s-5);
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: var(--s-2);
    z-index: 50;
    pointer-events: none;
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: var(--s-2);
    max-width: 400px;
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: color-mix(in oklab, var(--surface) 88%, transparent);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow: var(--float);
    pointer-events: auto;
    animation: rise 160ms var(--ease);
  }
  /* Line the icon, button and close up with the first line of text. */
  .toast > .ic,
  .toast > .x,
  .toast > :global(.btn) {
    margin-top: 2px;
  }
  .ic {
    color: var(--danger);
  }
  .body {
    flex: 1;
    min-width: 0;
    padding: 2px 0;
  }
  p {
    margin: 0;
    text-wrap: pretty;
  }
  .count {
    color: var(--text-3);
  }
  .tools {
    display: flex;
    gap: var(--s-3);
    margin-top: var(--s-1);
  }
  .link {
    font-size: var(--t-sm);
    color: var(--text-2);
    text-decoration: underline;
    text-underline-offset: 2px;
    border-radius: var(--r-sm);
  }
  .link:hover {
    color: var(--text);
  }
  .detail {
    margin: var(--s-2) 0 0;
    padding: var(--s-2);
    max-height: 140px;
    overflow: auto;
    border-radius: var(--r-sm);
    background: var(--surface-hover);
    font-size: var(--t-sm);
    color: var(--text-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .x {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    border-radius: var(--r-sm);
    color: var(--text-3);
  }
  .x:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>

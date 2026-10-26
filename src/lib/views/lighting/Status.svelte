<script lang="ts">
  import { t } from "../../i18n";

  // What plays now on an effect or mood: all the lights it goes to, some of
  // them, or none (nothing shown).

  let { all, share, total }: { all: boolean; share: number; total: number } = $props();
</script>

<!-- What plays now: amber, a lamp and words, so colour is never the only sign. -->
{#if all}
  <span class="status"><span class="lamp" aria-hidden="true"></span>{t("playing")}</span>
{:else if share > 0}
  <span class="status partial">{t("share_of", { n: share, total })}</span>
{/if}

<style>
  .status {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 1px 7px 1px 6px;
    border-radius: var(--r-full);
    background: var(--active-soft);
    color: var(--active-text);
    font-size: var(--t-xs);
    font-weight: 600;
    line-height: 16px;
    white-space: nowrap;
  }
  .status.partial {
    padding-left: 7px;
    background: none;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--text-2);
    font-weight: 500;
  }
  .status .lamp {
    --lamp: 6px;
    animation: glow 1.6s ease-in-out infinite alternate;
  }
  @keyframes glow {
    to {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .status .lamp {
      animation: none;
    }
  }
</style>

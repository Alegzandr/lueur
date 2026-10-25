<script lang="ts">
  // The mark of a screen zone: its colour with its letter on it, so the zone
  // reads without telling colours apart. The letter is hidden from screen
  // readers; with `labelled`, the zone is spelled out for them instead.
  import { EDGE_COLOR, EDGE_KIND, EDGE_LETTER, type Edge } from "../geometry";
  import { t } from "../i18n";

  interface Props {
    edge: Edge;
    /** A light left out of effects: grey, the letter still there. */
    off?: boolean;
    labelled?: boolean;
  }

  let { edge, off = false, labelled = false }: Props = $props();
</script>

<span class="zone-badge" class:off style="--c:{EDGE_COLOR[edge]}" aria-hidden="true">{t(EDGE_LETTER[edge])}</span
>{#if labelled}<span class="sr-only">{t("zone_of", { letter: t(EDGE_LETTER[edge]), zone: t(EDGE_KIND[edge]) })}</span>{/if}

<style>
  .zone-badge {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    background: var(--c);
    color: var(--zone-ink);
    font-size: 9.5px;
    font-weight: 700;
    line-height: 1;
  }
  .off {
    background: var(--line-strong);
    color: var(--text);
  }
</style>

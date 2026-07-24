<script module lang="ts">
  // One icon family for the whole app: 24px grid, 1.5 stroke, round caps,
  // currentColor. Anything that needs a glyph comes from here so the vocabulary
  // stays consistent across panels.
  export type IconName =
    | "room"
    | "bulb"
    | "settings"
    | "play"
    | "stop"
    | "power"
    | "target"
    | "trash"
    | "plus"
    | "search"
    | "check"
    | "reset";
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    size?: number;
  }

  let { name, size = 16 }: Props = $props();
</script>

<svg
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === "room"}
    <path d="M3 10.5 12 4l9 6.5V20H3z" />
    <path d="M9.5 20v-5.5h5V20" />
  {:else if name === "bulb"}
    <path d="M12 3a6 6 0 0 0-3.5 10.9c.6.5.9 1.2.9 1.9v.2h5.2v-.2c0-.7.3-1.4.9-1.9A6 6 0 0 0 12 3Z" />
    <path d="M9.6 19h4.8" />
    <path d="M10.6 21.5h2.8" />
  {:else if name === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M12 2.8v2.4M12 18.8v2.4M21.2 12h-2.4M5.2 12H2.8M18.5 5.5l-1.7 1.7M7.2 16.8l-1.7 1.7M18.5 18.5l-1.7-1.7M7.2 7.2 5.5 5.5"
    />
  {:else if name === "play"}
    <path d="M8 5.5v13l10.5-6.5z" fill="currentColor" stroke-width="1" />
  {:else if name === "stop"}
    <rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke-width="1" />
  {:else if name === "power"}
    <path d="M12 3.5v7.5" />
    <path d="M17.3 6.7a7.5 7.5 0 1 1-10.6 0" />
  {:else if name === "target"}
    <circle cx="12" cy="12" r="8" />
    <circle cx="12" cy="12" r="2.5" fill="currentColor" stroke="none" />
  {:else if name === "trash"}
    <path d="M4.5 6.5h15" />
    <path d="M9.5 6.5V4.8h5v1.7" />
    <path d="M6.5 6.5 7.4 20h9.2l.9-13.5" />
    <path d="M10.3 10v6.4M13.7 10v6.4" />
  {:else if name === "plus"}
    <path d="M12 5.5v13M5.5 12h13" />
  {:else if name === "search"}
    <circle cx="10.8" cy="10.8" r="6.3" />
    <path d="m15.4 15.4 4.1 4.1" />
  {:else if name === "check"}
    <path d="m5 12.5 4.5 4.5L19 7" />
  {:else if name === "reset"}
    <path d="M4.6 9.2A8 8 0 1 1 4 12.9" />
    <path d="M4 4.2v5h5" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>

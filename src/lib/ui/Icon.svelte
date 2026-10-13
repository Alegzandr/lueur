<script module lang="ts">
  // One icon family for the whole app: 20px grid, 1.5 stroke, round joins,
  // currentColor. Every glyph comes from here so the vocabulary stays coherent.
  export type IconName =
    | "home"
    | "plan"
    | "bulb"
    | "sliders"
    | "power"
    | "plus"
    | "search"
    | "check"
    | "trash"
    | "flash"
    | "reset"
    | "close"
    | "back"
    | "chevron"
    | "screen"
    | "corner"
    | "alert"
    | "info"
    | "refresh"
    | "link"
    | "unlink";
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    size?: number;
  }

  let { name, size = 16 }: Props = $props();
</script>

<svg
  viewBox="0 0 20 20"
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
  {#if name === "home"}
    <circle cx="10" cy="10" r="3.2" />
    <path d="M10 2.5v1.8M10 15.7v1.8M2.5 10h1.8M15.7 10h1.8M4.7 4.7l1.3 1.3M14 14l1.3 1.3M4.7 15.3 6 14M14 6l1.3-1.3" />
  {:else if name === "plan"}
    <path d="M3 3.5h14v13H3z" />
    <path d="M3 10h5.5M11.5 3.5v5M11.5 12v4.5" />
  {:else if name === "bulb"}
    <path d="M10 2.75a5 5 0 0 0-2.9 9.08c.5.36.8.92.8 1.53v.39h4.2v-.39c0-.61.3-1.17.8-1.53A5 5 0 0 0 10 2.75Z" />
    <path d="M8.1 16.25h3.8" />
  {:else if name === "sliders"}
    <path d="M3.5 6h7M14.5 6h2M3.5 14h2M9.5 14h7" />
    <circle cx="12.5" cy="6" r="2" />
    <circle cx="7.5" cy="14" r="2" />
  {:else if name === "power"}
    <path d="M10 3v6.5" />
    <path d="M14.4 5.6a6.25 6.25 0 1 1-8.8 0" />
  {:else if name === "plus"}
    <path d="M10 4.5v11M4.5 10h11" />
  {:else if name === "search"}
    <circle cx="9" cy="9" r="5.25" />
    <path d="m13 13 3.5 3.5" />
  {:else if name === "check"}
    <path d="m4.5 10.5 3.5 3.5 7.5-8" />
  {:else if name === "trash"}
    <path d="M4 5.75h12M8 5.75V4.25h4v1.5M5.6 5.75l.7 10.5h7.4l.7-10.5" />
  {:else if name === "flash"}
    <path d="M10 6.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7Z" fill="currentColor" stroke="none" opacity="0.9" />
    <path d="M10 2.5v1.5M10 16v1.5M2.5 10H4M16 10h1.5" />
  {:else if name === "reset"}
    <path d="M4.2 8A6 6 0 1 1 4 11.5" />
    <path d="M4 3.75V8h4.25" />
  {:else if name === "close"}
    <path d="m5.5 5.5 9 9M14.5 5.5l-9 9" />
  {:else if name === "back"}
    <path d="M12 4.5 6.5 10l5.5 5.5" />
  {:else if name === "chevron"}
    <path d="m8 5 5 5-5 5" />
  {:else if name === "screen"}
    <rect x="2.75" y="4" width="14.5" height="9.5" rx="1.25" />
    <path d="M7.5 16.5h5" />
  {:else if name === "corner"}
    <path d="M4 16V4h12" />
    <rect x="2.25" y="2.25" width="3.5" height="3.5" rx="0.75" fill="currentColor" stroke="none" />
  {:else if name === "alert"}
    <path d="M10 3.25 17.25 16H2.75z" />
    <path d="M10 8.25v3.25" />
    <circle cx="10" cy="13.75" r="0.6" fill="currentColor" />
  {:else if name === "info"}
    <circle cx="10" cy="10" r="7.25" />
    <path d="M10 9v4.5" />
    <circle cx="10" cy="6.5" r="0.6" fill="currentColor" />
  {:else if name === "refresh"}
    <path d="M15.8 8.5A6 6 0 0 0 4.6 7M4.2 11.5a6 6 0 0 0 11.2 1.5" />
    <path d="M15.75 4v4.5h-4.5M4.25 16v-4.5h4.5" />
  {:else if name === "link"}
    <path d="M8.5 11.5 11.5 8.5" />
    <path d="M9.5 6.5 11 5a3.2 3.2 0 0 1 4.5 4.5L14 11M11 14l-1.5 1.5A3.2 3.2 0 0 1 5 11l1.5-1.5" />
  {:else if name === "unlink"}
    <path d="M9.5 6.5 11 5a3.2 3.2 0 0 1 4.5 4.5L14 11M11 14l-1.5 1.5A3.2 3.2 0 0 1 5 11l1.5-1.5" />
    <path d="M3.5 3.5l13 13" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>

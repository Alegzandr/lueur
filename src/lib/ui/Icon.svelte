<script module lang="ts">
  // One icon family for the whole app: Phosphor, whose rounded, even strokes
  // sit with the interface face. Every glyph comes from here, by the name the
  // interface gives it, so the vocabulary stays coherent and a family can be
  // swapped in one place.
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
    | "unlink"
    | "undo"
    | "redo"
    | "clock";
</script>

<script lang="ts">
  import type { Component } from "svelte";
  import ArrowClockwise from "phosphor-svelte/lib/ArrowClockwise";
  import ArrowCounterClockwise from "phosphor-svelte/lib/ArrowCounterClockwise";
  import ArrowUUpLeft from "phosphor-svelte/lib/ArrowUUpLeft";
  import ArrowUUpRight from "phosphor-svelte/lib/ArrowUUpRight";
  import CaretLeft from "phosphor-svelte/lib/CaretLeft";
  import CaretRight from "phosphor-svelte/lib/CaretRight";
  import Check from "phosphor-svelte/lib/Check";
  import Clock from "phosphor-svelte/lib/Clock";
  import CornersOut from "phosphor-svelte/lib/CornersOut";
  import Couch from "phosphor-svelte/lib/Couch";
  import GearSix from "phosphor-svelte/lib/GearSix";
  import Info from "phosphor-svelte/lib/Info";
  import Lightbulb from "phosphor-svelte/lib/Lightbulb";
  import Link from "phosphor-svelte/lib/Link";
  import LinkBreak from "phosphor-svelte/lib/LinkBreak";
  import MagnifyingGlass from "phosphor-svelte/lib/MagnifyingGlass";
  import Monitor from "phosphor-svelte/lib/Monitor";
  import Plus from "phosphor-svelte/lib/Plus";
  import Power from "phosphor-svelte/lib/Power";
  import Sparkle from "phosphor-svelte/lib/Sparkle";
  import SunDim from "phosphor-svelte/lib/SunDim";
  import Trash from "phosphor-svelte/lib/Trash";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import X from "phosphor-svelte/lib/X";

  interface Props {
    name: IconName;
    size?: number;
    /** The filled cut, for the one thing chosen among its siblings (the
     *  current page in the sidebar). */
    filled?: boolean;
  }

  let { name, size = 16, filled = false }: Props = $props();

  // `any` props: the generated components share one prop type.
  const GLYPHS: Record<IconName, Component<any>> = {
    home: SunDim,
    plan: Couch,
    bulb: Lightbulb,
    sliders: GearSix,
    power: Power,
    plus: Plus,
    search: MagnifyingGlass,
    check: Check,
    trash: Trash,
    flash: Sparkle,
    reset: ArrowCounterClockwise,
    close: X,
    back: CaretLeft,
    chevron: CaretRight,
    screen: Monitor,
    corner: CornersOut,
    alert: WarningCircle,
    info: Info,
    refresh: ArrowClockwise,
    link: Link,
    unlink: LinkBreak,
    undo: ArrowUUpLeft,
    redo: ArrowUUpRight,
    clock: Clock,
  };

  const Glyph = $derived(GLYPHS[name]);
  // Small glyphs a step heavier, so their strokes keep their weight.
  const weight = $derived(filled ? "fill" : size <= 12 ? "bold" : "regular");
</script>

<span class="icon" style="--size:{size}px" aria-hidden="true">
  <Glyph {size} {weight} focusable="false" />
</span>

<style>
  .icon {
    display: inline-grid;
    flex: none;
    width: var(--size);
    height: var(--size);
  }
  .icon :global(svg) {
    display: block;
  }
</style>

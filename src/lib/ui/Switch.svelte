<script lang="ts">
  // An on/off switch. The track turns amber when on, since every switch in
  // Lueur decides whether something gives light.
  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
    onchange: (v: boolean) => void;
  }

  let { checked, label, disabled = false, onchange }: Props = $props();
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    width: 32px;
    height: 19px;
    flex: none;
    border-radius: var(--r-full);
    background: var(--line-strong);
    /* The edge reads against the surface whether the switch is off or on. */
    box-shadow:
      inset 0 0 0 1px var(--control-line),
      inset 0 0.5px 1.5px oklch(0 0 0 / 0.18);
    transition:
      background 180ms var(--ease),
      box-shadow 180ms var(--ease);
  }
  .switch.on {
    background: var(--glow);
    box-shadow:
      inset 0 0 0 1px var(--glow-text),
      inset 0 0.5px 1.5px oklch(0 0 0 / 0.18);
  }
  .switch:disabled {
    opacity: 0.5;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    background: #fff;
    box-shadow:
      0 0 0 0.5px oklch(0 0 0 / 0.08),
      0 1px 2.5px oklch(0 0 0 / 0.28);
    transition: transform 220ms var(--ease);
  }
  .switch:active:not(:disabled) .knob {
    width: 18px;
  }
  .on .knob {
    transform: translateX(13px);
  }
  .on:active:not(:disabled) .knob {
    transform: translateX(10px);
  }
</style>

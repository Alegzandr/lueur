<script lang="ts">
  // A classic on/off switch. The track turns to the glow colour when on, since
  // every switch in Lueur ultimately decides whether something gives light.
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
    width: 34px;
    height: 20px;
    flex: none;
    border-radius: var(--r-full);
    background: var(--line-strong);
    transition: background var(--dur) var(--ease);
  }
  .switch.on {
    background: var(--glow);
  }
  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px oklch(0 0 0 / 0.25);
    transition: transform var(--dur) var(--ease);
  }
  .on .knob {
    transform: translateX(14px);
  }
</style>

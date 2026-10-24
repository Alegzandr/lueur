<script lang="ts">
  // The mark from assets/logo.svg without its app tile: a warm core and the
  // zone-tinted bounce around it. Kept in step with the icon by hand.
  interface Props {
    size?: number;
  }

  let { size = 20 }: Props = $props();

  // Gradient and filter ids are document-global; keep each instance's own.
  const uid = $props.id();
</script>

<svg viewBox="0 0 512 512" width={size} height={size} aria-hidden="true">
  <defs>
    <radialGradient id="{uid}-core" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="#f6d48e" />
      <stop offset="0.6" stop-color="#f4ae7c" />
      <stop offset="1" stop-color="#ec8e6f" />
    </radialGradient>
    <filter id="{uid}-soft" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="40" />
    </filter>
  </defs>
  <g filter="url(#{uid}-soft)" opacity="0.9">
    <circle cx="176" cy="282" r="120" fill="#e78a6e" />
    <circle cx="236" cy="170" r="112" fill="#e0b062" />
    <circle cx="336" cy="266" r="124" fill="#6ba9c7" />
  </g>
  <circle cx="256" cy="256" r="124" fill="url(#{uid}-core)" />
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>

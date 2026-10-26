<script lang="ts">
  export let size: number = 32;
  export let alt: string = "Tidyr";
</script>

<svg
  class="topbar-logo"
  width={size}
  height={size}
  viewBox="0 0 1024 1024"
  role="img"
  aria-label={alt}
>
  <title>{alt}</title>
  <!-- Master Binder Crossbar -->
  <rect x="140" y="180" width="744" height="200" rx="48" fill="var(--brand-bar, #1d4e30)"/>
  <!-- Indicator Marker -->
  <circle cx="780" cy="280" r="44" fill="var(--warn, #e09a5a)"/>
  <!-- Stack Layer 1 (Mint) -->
  <rect x="372" y="412" width="280" height="124" rx="32" fill="var(--accent, #5cb87f)"/>
  <!-- Stack Layer 2 (Primary Sage) -->
  <rect x="372" y="562" width="280" height="124" rx="32" fill="var(--accent-hover, #2f7d4f)"/>
  <!-- Stack Layer 3 (Forest Foundation) -->
  <rect x="372" y="712" width="280" height="136" rx="32" fill="var(--brand-bar, #1d4e30)"/>
</svg>

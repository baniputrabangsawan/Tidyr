<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { id } from "./lib/i18n/id";
  import {
    screen,
    scanOutcome,
    plan,
    applyResult,
    settings,
    onboarded,
    markOnboarded,
  } from "./lib/store";
  import Home from "./lib/components/Home.svelte";
  import Scan from "./lib/components/Scan.svelte";
  import PlanView from "./lib/components/PlanView.svelte";
  import Applying from "./lib/components/Applying.svelte";
  import Done from "./lib/components/Done.svelte";
  import History from "./lib/components/History.svelte";
  import SettingsView from "./lib/components/SettingsView.svelte";

  let onboardStep = 0;

  onMount(async () => {
    try {
      settings.set(await api.getSettings());
      applyTheme($settings?.theme ?? "system");
      // Purge expired quarantine on startup (retention from settings).
      await api.purgeQuarantine($settings?.retention_days ?? 30);
    } catch {
      /* first run: defaults apply */
    }
  });

  function applyTheme(theme: string) {
    const root = document.documentElement;
    if (theme === "light" || theme === "dark") root.dataset.theme = theme;
    else delete root.dataset.theme;
  }

  $: if ($settings) applyTheme($settings.theme);

  const tabs: Array<{ id: typeof $screen; label: string }> = [
    { id: "home", label: id.nav.home },
    { id: "plan", label: id.nav.plan },
    { id: "history", label: id.nav.history },
    { id: "settings", label: id.nav.settings },
  ];
</script>

<header class="topbar">
  <h1>{id.appName}</h1>
  <span class="muted">{id.tagline}</span>
</header>

{#if !$onboarded}
  <main>
    <div class="card" role="dialog" aria-label="Pengenalan">
      <h2>{id.onboarding[onboardStep].title}</h2>
      <p>{id.onboarding[onboardStep].body}</p>
      <div class="row">
        {#if onboardStep < id.onboarding.length - 1}
          <button class="primary" on:click={() => (onboardStep += 1)}>
            {id.onboardingNext}
          </button>
        {:else}
          <button class="primary" on:click={markOnboarded}>
            {id.onboardingStart}
          </button>
        {/if}
      </div>
    </div>
  </main>
{:else}
  <nav class="tabs" aria-label="Navigasi">
    {#each tabs as t}
      <button
        aria-current={$screen === t.id ? "page" : undefined}
        on:click={() => screen.set(t.id)}
      >
        {t.label}
      </button>
    {/each}
  </nav>

  <main>
    {#if $screen === "home"}
      <Home />
    {:else if $screen === "scan"}
      <Scan />
    {:else if $screen === "plan"}
      <PlanView />
    {:else if $screen === "applying"}
      <Applying />
    {:else if $screen === "done"}
      <Done />
    {:else if $screen === "history"}
      <History />
    {:else if $screen === "settings"}
      <SettingsView />
    {/if}
  </main>
{/if}

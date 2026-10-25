<script lang="ts">
  import { onMount } from "svelte";
  import { api, formatBytes, isTauri } from "./lib/api";
  import type { SessionInfo } from "./lib/api";
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
  let purgeNotice = "";
  let interruptedSessions: SessionInfo[] = [];
  let showRecoveryModal = false;
  let recoveryError = "";
  let updateBanner: { version: string; body: string } | null = null;
  onMount(async () => {
    try {
      settings.set(await api.getSettings());
      applyTheme($settings?.theme ?? "system");
      // Purge expired quarantine on startup and report results
      const report = await api.purgeQuarantine($settings?.retention_days ?? 30);
      if (report && report.count > 0) {
        purgeNotice = `${report.count} ${id.history.purgedNotice} (${formatBytes(report.bytes)})`;
      }
      // Check for interrupted crash sessions
      const crashed = await api.interruptedSessions();
      if (crashed && crashed.length > 0) {
        interruptedSessions = crashed;
        showRecoveryModal = true;
      }
      // 24-hour automatic update check
      const nowSecs = Math.floor(Date.now() / 1000);
      if ($settings?.auto_update_check && (nowSecs - ($settings.last_update_check_secs ?? 0) >= 86400)) {
        try {
          const { check } = await import("@tauri-apps/plugin-updater");
          const update = await check();
          settings.update((s) => (s ? { ...s, last_update_check_secs: nowSecs } : s));
          if ($settings) await api.saveSettings($settings);

          if (update && update.version !== $settings?.skipped_version) {
            updateBanner = {
              version: update.version,
              body: update.body ?? "",
            };
          }
        } catch {
          // Offline or placeholder pubkey - graceful, no panic
        }
      }
    } catch {
      /* first run: defaults apply */
    }
  });

  async function rollbackAllInterrupted() {
    recoveryError = "";
    try {
      for (const s of interruptedSessions) {
        await api.undoSession(s.id);
      }
      interruptedSessions = [];
      showRecoveryModal = false;
      purgeNotice = id.history.recoverySuccess;
    } catch (e) {
      recoveryError = String(e);
    }
  }

  function dismissRecovery() {
    showRecoveryModal = false;
  }

  async function installUpdate() {
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) {
        await update.downloadAndInstall();
      }
    } catch (e) {
      purgeNotice = `${id.common.error}: ${String(e)}`;
    }
  }

  async function skipThisVersion() {
    if (updateBanner && $settings) {
      const ver = updateBanner.version;
      settings.update((s) => (s ? { ...s, skipped_version: ver } : s));
      await api.saveSettings($settings);
    }
    updateBanner = null;
  }

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

<svelte:window
  on:keydown={(e) => {
    if (e.key === "Escape") {
      if (updateBanner) updateBanner = null;
      if (showRecoveryModal) showRecoveryModal = false;
    }
  }}
/>
<header class="topbar">
  <h1>{id.appName}</h1>
  <span class="muted">{id.tagline}</span>
</header>

{#if !isTauri()}
  <div class="card" role="note" style="margin: 0 0 16px 0; padding: 10px 16px; font-size: 0.9rem; background: var(--surface); border-left: 4px solid var(--accent, #3182ce);">
    <span>🌐 <strong>Mode Pratinjau Web:</strong> Berjalan di browser biasa. Semua aksi dan data disimulasikan agar Anda dapat menguji seluruh antarmuka (Pilih folder, Rencana, Riwayat, dll).</span>
  </div>
{/if}

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
    {#if updateBanner}
      <div class="card" role="alert" style="margin-bottom: 12px; border-left: 4px solid var(--accent, #3182ce);">
        <h3>{id.settings.updateAvailable}: v{updateBanner.version}</h3>
        {#if updateBanner.body}
          <p class="muted" style="margin-top: 4px;">{updateBanner.body}</p>
        {/if}
        <div class="row" style="margin-top: 10px; gap: 8px;">
          <button class="primary" on:click={installUpdate}>{id.settings.updateNow}</button>
          <button class="ghost" on:click={() => (updateBanner = null)}>{id.settings.updateLater}</button>
          <button class="ghost" on:click={skipThisVersion}>{id.settings.updateSkip}</button>
        </div>
      </div>
    {/if}

    {#if purgeNotice}
      <div class="card" role="status" style="margin-bottom: 12px; display: flex; justify-content: space-between; align-items: center;">
        <span>{purgeNotice}</span>
        <button class="ghost" on:click={() => (purgeNotice = "")}>{id.common.close}</button>
      </div>
    {/if}

    {#if showRecoveryModal && interruptedSessions.length > 0}
      <div class="card" role="alertdialog" aria-label={id.history.crashTitle} style="margin-bottom: 16px;">
        <h2>{id.history.crashTitle}</h2>
        <p>{id.history.crashBody}</p>
        <ul>
          {#each interruptedSessions as s}
            <li><strong>{s.label}</strong> ({s.started_at}) — {s.files_moved} dipindah, {s.files_quarantined} dikarantina</li>
          {/each}
        </ul>
        {#if recoveryError}
          <p class="error" role="alert">{recoveryError}</p>
        {/if}
        <div class="row" style="margin-top: 12px; gap: 8px;">
          <button class="primary" on:click={rollbackAllInterrupted}>{id.history.recoveryRollbackAll}</button>
          <button class="ghost" on:click={dismissRecovery}>{id.history.recoveryLater}</button>
        </div>
      </div>
    {/if}

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

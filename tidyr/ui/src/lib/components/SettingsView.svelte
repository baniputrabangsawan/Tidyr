<script lang="ts">
  import { onMount } from "svelte";
  import { api, formatBytes } from "../api";
  import type { QuarantineInfo } from "../api";
  import { id } from "../i18n/id";
  import { settings } from "../store";

  let error = "";
  let saved = false;
  let quarantineInfo: QuarantineInfo | null = null;
  let newExcluded = "";

  onMount(async () => {
    try {
      quarantineInfo = await api.getQuarantineInfo();
    } catch {
      /* ignore if not yet available */
    }
  });

  async function openQuarantine() {
    try {
      await api.openQuarantineFolder();
    } catch (e) {
      error = String(e);
    }
  }

  async function addExcluded() {
    error = "";
    const trimmed = newExcluded.trim();
    if (!trimmed) return;
    if (!$settings) return;
    if ($settings.excluded_folders.includes(trimmed)) return;
    try {
      const validPath = await api.validateFolder(trimmed);
      $settings.excluded_folders = [...$settings.excluded_folders, validPath];
      newExcluded = "";
      saved = false;
    } catch (e) {
      error = String(e);
    }
  }

  function removeExcluded(idx: number) {
    if (!$settings) return;
    $settings.excluded_folders = $settings.excluded_folders.filter((_, i) => i !== idx);
    saved = false;
  }

  async function save() {
    error = "";
    saved = false;
    if (!$settings) return;
    try {
      await api.saveSettings($settings);
      saved = true;
    } catch (e) {
      error = String(e);
    }
  }

  async function checkUpdate() {
    error = "";
    saved = false;
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) {
        error = `${id.settings.updateAvailable}: v${update.version}`;
      } else {
        saved = true;
      }
    } catch {
      // With placeholder pubkey/endpoint or offline, gracefully inform user without crashing
      error = id.settings.updaterPlaceholderNotice;
    }
  }
</script>

{#if $settings}
  <div class="card">
    <h2>{id.settings.title}</h2>

    <div class="row" style="flex-direction: column; align-items: flex-start; gap: 4px; margin-bottom: 16px;">
      <strong>{id.settings.quarantineDir}</strong>
      <code>{quarantineInfo?.path ?? id.common.loading}</code>
      <span class="muted">{id.settings.quarantineSize}: {formatBytes(quarantineInfo?.total_bytes ?? 0)} ({quarantineInfo?.count ?? 0} file)</span>
      <button class="ghost" style="margin-top: 6px;" on:click={openQuarantine}>{id.settings.openFolder}</button>
    </div>

    <div class="row" style="margin-bottom: 16px;">
      <label>
        {id.settings.retention}:
        <input type="number" min="1" max="365" bind:value={$settings.retention_days} />
      </label>
    </div>

    <div style="margin-bottom: 16px;">
      <strong>{id.settings.excluded}</strong>
      <div class="row" style="margin-top: 6px; gap: 8px;">
        <input
          type="text"
          placeholder={id.settings.excludedPlaceholder}
          bind:value={newExcluded}
          style="flex: 1;"
          on:keydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addExcluded();
            }
          }}
        />
        <button class="ghost" on:click={addExcluded}>{id.settings.addExcluded}</button>
      </div>

      {#if ($settings.excluded_folders ?? []).length === 0}
        <p class="muted" style="margin-top: 6px; font-size: 0.9em;">{id.settings.excludedEmpty}</p>
      {:else}
        <ul style="margin-top: 8px; padding-left: 20px;">
          {#each $settings.excluded_folders as folder, idx}
            <li style="margin-bottom: 4px;">
              <code>{folder}</code>
              <button
                class="ghost"
                style="padding: 2px 6px; margin-left: 8px; font-size: 0.85em;"
                on:click={() => removeExcluded(idx)}
              >
                {id.settings.removeExcluded}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="row" style="margin-bottom: 16px;">
      <label>
        <input type="checkbox" bind:checked={$settings.auto_update_check} />
        {id.settings.autoUpdate}
      </label>
    </div>

    <div class="row" style="margin-bottom: 16px;">
      <label>
        {id.settings.theme}:
        <select bind:value={$settings.theme}>
          <option value="system">{id.settings.themeSystem}</option>
          <option value="light">{id.settings.themeLight}</option>
          <option value="dark">{id.settings.themeDark}</option>
        </select>
      </label>
    </div>

    <p class="muted" style="margin-bottom: 16px;">{id.settings.privacyNote}</p>

    <div class="row">
      <button class="primary" on:click={save}>{id.settings.save}</button>
      <button class="ghost" on:click={checkUpdate}>{id.settings.checkUpdate}</button>
    </div>

    {#if saved}<p role="status">{id.settings.saved}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/if}

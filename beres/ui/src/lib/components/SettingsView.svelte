<script lang="ts">
  import { api } from "../api";
  import { id } from "../i18n/id";
  import { settings } from "../store";

  let error = "";
  let saved = false;

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
    // Handled via the updater plugin; this button triggers a manual check.
    const { check } = await import("@tauri-apps/plugin-updater");
    try {
      const update = await check();
      if (update) {
        error = `${id.settings.updateAvailable}: ${update.version}`;
      } else {
        saved = true;
      }
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if $settings}
  <div class="card">
    <h2>{id.settings.title}</h2>
    <div class="row">
      <label>
        {id.settings.retention}:
        <input type="number" min="1" max="365" bind:value={$settings.retention_days} />
      </label>
    </div>
    <div class="row">
      <label>
        <input type="checkbox" bind:checked={$settings.auto_update_check} />
        {id.settings.autoUpdate}
      </label>
    </div>
    <div class="row">
      <label>
        {id.settings.theme}:
        <select bind:value={$settings.theme}>
          <option value="system">{id.settings.themeSystem}</option>
          <option value="light">{id.settings.themeLight}</option>
          <option value="dark">{id.settings.themeDark}</option>
        </select>
      </label>
    </div>
    <p class="muted">{id.settings.privacyNote}</p>
    <div class="row">
      <button class="primary" on:click={save}>{id.settings.save}</button>
      <button class="ghost" on:click={checkUpdate}>{id.settings.checkUpdate}</button>
    </div>
    {#if saved}<p role="status">{id.settings.saved}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/if}

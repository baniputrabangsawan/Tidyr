<script lang="ts">
  import { api } from "../api";
  import { id } from "../i18n/id";
  import { screen, scanOutcome, plan } from "../store";

  let folder = "";
  let mode: "by-type" | "by-date" = "by-type";
  let error = "";
  let busy = false;
  let dragOver = false;
  let recent: string[] = [];

  try {
    recent = JSON.parse(localStorage.getItem("beres-recent") ?? "[]");
  } catch {
    recent = [];
  }

  function remember(p: string) {
    recent = [p, ...recent.filter((r) => r !== p)].slice(0, 5);
    try {
      localStorage.setItem("beres-recent", JSON.stringify(recent));
    } catch {
      /* ignore */
    }
  }

  async function choose() {
    error = "";
    try {
      const picked = await api.pickFolder();
      if (picked) folder = picked;
    } catch (e) {
      error = String(e);
    }
  }

  async function analyze() {
    error = "";
    if (!folder) return;
    busy = true;
    try {
      const valid = await api.validateFolder(folder);
      const outcome = await api.scanFolder(valid, mode);
      scanOutcome.set(outcome);
      plan.set(outcome.plan);
      remember(valid);
      screen.set("plan");
    } catch (e) {
      const msg = String(e);
      error = msg.includes("dilindungi") ? id.home.protectedError : msg;
    } finally {
      busy = false;
    }
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragOver = false;
    const files = e.dataTransfer?.files;
    if (files && files.length > 0) {
      // Tauri exposes dropped folder paths via the file API.
      const path = (files[0] as unknown as { path?: string }).path;
      if (path) folder = path;
    }
  }
</script>

<div class="card">
  <div class="row">
    <button class="ghost" on:click={choose} aria-label={id.home.chooseFolder}>
      {id.home.chooseFolder}
    </button>
    {#if folder}
      <code>{folder}</code>
    {/if}
  </div>
  <div
    class="dropzone"
    class:over={dragOver}
    role="button"
    tabindex="0"
    aria-label={id.home.dropHint}
    on:dragover={(e) => {
      e.preventDefault();
      dragOver = true;
    }}
    on:dragleave={() => (dragOver = false)}
    on:drop={onDrop}
  >
    {id.home.dropHint}
  </div>
  <div class="row" style="margin-top: 12px">
    <label>
      {id.home.mode}:
      <select bind:value={mode} aria-label={id.home.mode}>
        <option value="by-type">{id.home.modeByType}</option>
        <option value="by-date">{id.home.modeByDate}</option>
      </select>
    </label>
    <button class="primary" disabled={!folder || busy} on:click={analyze}>
      {id.home.analyze}
    </button>
  </div>
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</div>

{#if recent.length > 0}
  <div class="card">
    <h2>{id.home.recent}</h2>
    {#each recent as r}
      <div class="row">
        <button
          class="ghost"
          on:click={() => {
            folder = r;
          }}
        >
          {r}
        </button>
      </div>
    {/each}
  </div>
{/if}

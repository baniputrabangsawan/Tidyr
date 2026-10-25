<script lang="ts">
  import { api } from "../api";
  import { id } from "../i18n/id";
  import { screen, applyResult } from "../store";

  let error = "";
  let undone = false;

  async function undo() {
    error = "";
    if (!$applyResult) return;
    try {
      await api.undoSession($applyResult.session_id);
      undone = true;
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if $applyResult}
  <div class="card">
    <h2>{id.done.title}</h2>
    <p>
      {$applyResult.files_moved} {id.done.moved}, {$applyResult.files_quarantined}
      {id.done.quarantined}.
    </p>
    {#if $applyResult.cancelled}
      <p class="muted">{id.applying.cancelled}</p>
    {/if}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    <div class="row">
      {#if !undone}
        <button class="primary" on:click={undo}>{id.done.undo}</button>
      {:else}
        <span class="badge">{id.history.undone}</span>
      {/if}
      <button class="ghost" on:click={() => screen.set("home")}>{id.done.backHome}</button>
    </div>
  </div>
{:else}
  <div class="card"><p class="muted">{id.common.loading}</p></div>
{/if}

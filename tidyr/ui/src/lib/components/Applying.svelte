<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { id } from "../i18n/id";
  import { screen, applyResult } from "../store";

  let error = "";

  onMount(async () => {
    error = "";
    try {
      const res = await api.applyPlan();
      applyResult.set(res);
      screen.set("done");
    } catch (e) {
      error = String(e);
    }
  });

  async function cancel() {
    await api.cancelApply();
  }
</script>

<div class="card" role="status" aria-live="polite">
  <h2>{id.applying.title}</h2>
  {#if error}
    <p class="error" role="alert">
      {error}
      <button
        class="ghost"
        on:click={async () => {
          try {
            await navigator.clipboard.writeText(error);
          } catch {
            /* clipboard unavailable */
          }
        }}
      >
        {id.common.copyDetails}
      </button>
    </p>
    <button class="ghost" on:click={() => screen.set("plan")}>{id.common.retry}</button>
  {:else}
    <div class="progress"><div style="width: 50%"></div></div>
    <div class="row" style="margin-top: 12px">
      <button class="ghost" on:click={cancel}>{id.applying.cancel}</button>
    </div>
  {/if}
</div>

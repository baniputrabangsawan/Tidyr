<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import type { SessionInfo, QuarantineRecord } from "../api";
  import { id } from "../i18n/id";

  let sessions: SessionInfo[] = [];
  let crashed: SessionInfo[] = [];
  let quarantined: QuarantineRecord[] = [];
  let error = "";
  let notice = "";

  onMount(async () => {
    try {
      sessions = await api.listSessions();
      crashed = await api.interruptedSessions();
      quarantined = await api.listQuarantined();
    } catch (e) {
      error = String(e);
    }
  });

  async function undo(sid: string) {
    error = "";
    notice = "";
    try {
      await api.undoSession(sid);
      sessions = await api.listSessions();
      crashed = await api.interruptedSessions();
      quarantined = await api.listQuarantined();
      notice = id.history.undone;
    } catch (e) {
      error = String(e);
    }
  }

  async function restore(qid: number) {
    error = "";
    notice = "";
    try {
      const dest = await api.restoreQuarantined(qid);
      quarantined = await api.listQuarantined();
      notice = `${id.history.restoredTo} ${dest}`;
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if crashed.length > 0}
  <div class="card" role="alert">
    <h2>{id.history.crashTitle}</h2>
    <p>{id.history.crashBody}</p>
    {#each crashed as c}
      <div class="row">
        <span>{c.label}</span>
        <button class="primary" on:click={() => undo(c.id)}>{id.history.crashRollback}</button>
      </div>
    {/each}
  </div>
{/if}

<div class="card">
  <h2>{id.history.title}</h2>
  {#if sessions.length === 0}
    <p class="muted">{id.history.empty}</p>
  {:else}
    <table class="list">
      <tbody>
        {#each sessions as s}
          <tr>
            <td>
              {s.label}
              <div class="muted">
                {s.started_at} · {s.files_moved} {id.history.moved}, {s.files_quarantined} {id.history.quarantined}
              </div>
            </td>
            <td>
              {#if s.status === "undone"}
                <span class="badge">{id.history.undone}</span>
              {:else}
                <button class="ghost" on:click={() => undo(s.id)}>{id.history.undo}</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div class="card">
  <h2>{id.history.quarantinedFiles} ({quarantined.length})</h2>
  {#if quarantined.length === 0}
    <p class="muted">{id.history.empty}</p>
  {:else}
    <table class="list">
      <tbody>
        {#each quarantined.slice(0, 100) as q}
          <tr>
            <td>{q.original_path}</td>
            <td>
              <button class="ghost" on:click={() => restore(q.id)}>{id.history.restore}</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

{#if notice}<p class="card" role="status">{notice}</p>{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { api } from "../api";
  import { id } from "../i18n/id";
  import { screen, scanOutcome } from "../store";

  let files = 0;
  let phase: string = id.scan.phase_scan;
  let unlisten: UnlistenFn | null = null;

  onMount(async () => {
    unlisten = await listen<{ files: number; bytes: number; phase: string }>(
      "tidyr://scan-progress",
      (e) => {
        files = e.payload.files;
        const key =
          e.payload.phase === "mencari-duplikat"
            ? id.scan.phase_hash
            : e.payload.phase === "menyusun-rencana"
              ? id.scan.phase_plan
              : id.scan.phase_scan;
        phase = key;
      },
    );
  });

  onDestroy(() => {
    unlisten?.();
  });

  async function cancel() {
    await api.cancelScan();
    screen.set("home");
  }
</script>

<div class="card" role="status" aria-live="polite">
  <h2>{id.scan.title}</h2>
  <p>{phase}…</p>
  <p class="muted">{files} {id.scan.files}</p>
  {#if $scanOutcome}
    <p class="muted">
      {$scanOutcome.unreadable_count} {id.scan.unreadable} ·
      {$scanOutcome.symlink_count} {id.scan.symlinks} ·
      {$scanOutcome.empty_count} {id.scan.emptyFiles}
    </p>
  {/if}
  <button class="ghost" on:click={cancel}>{id.scan.cancel}</button>
</div>

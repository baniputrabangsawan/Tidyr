<script lang="ts">
  import { api } from "../api";
  import type { Category } from "../api";
  import { id } from "../i18n/id";
  import { screen, plan } from "../store";
  import { formatBytes } from "../api";

  let tab: Category | "semua" = "semua";
  let confirming = false;
  let error = "";

  const tabDefs: Array<{ id: Category | "semua"; label: string }> = [
    { id: "semua", label: "Semua" },
    { id: "duplikat", label: id.plan.tabs.duplikat },
    { id: "rapikan", label: id.plan.tabs.rapikan },
    { id: "installer", label: id.plan.tabs.installer },
    { id: "folder_kosong", label: id.plan.tabs.folder_kosong },
  ];

  $: visibleActions = ($plan?.actions ?? []).filter(
    (a) => tab === "semua" || a.category === tab,
  );

  function describeTarget(a: (typeof visibleActions)[number]): string {
    const act = a.action;
    switch (act.kind) {
      case "move":
      case "rename":
        return `${act.from} → ${act.to}`;
      case "quarantine":
        return act.path;
      case "delete_empty_dir":
        return act.path;
    }
  }

  async function toggle(actionId: string, enabled: boolean) {
    try {
      const summary = await api.setActionEnabled(actionId, enabled);
      plan.update((p) => (p ? { ...p, summary } : p));
      // Refresh local enabled flag without full reload.
      plan.update((p) => {
        if (!p) return p;
        const actions = p.actions.map((a) =>
          a.id === actionId ? { ...a, enabled } : a,
        );
        return { ...p, actions };
      });
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleAll(enabled: boolean) {
    try {
      const summary = await api.setAllEnabled(
        tab === "semua" ? null : (tab as Category),
        enabled,
      );
      plan.update((p) => {
        if (!p) return p;
        const actions = p.actions.map((a) =>
          tab === "semua" || a.category === tab ? { ...a, enabled } : a,
        );
        return { ...p, actions, summary };
      });
    } catch (e) {
      error = String(e);
    }
  }

  async function chooseKeeper(groupId: string, idx: number) {
    try {
      const updated = await api.setKeeper(groupId, idx);
      plan.set(updated);
    } catch (e) {
      error = String(e);
    }
  }

  async function onApply() {
    error = "";
    try {
      const needs = await api.planNeedsConfirmation();
      if (needs && !confirming) {
        confirming = true;
        return;
      }
      confirming = false;
      screen.set("applying");
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if $plan}
  <div class="card">
    <p>
      {$plan.summary.files_found} {id.plan.summaryFiles}. {$plan.summary
        .duplicate_files} duplikat (hemat {formatBytes(
        $plan.summary.bytes_reclaimable,
      )}), {$plan.summary.photos_to_move} foto dipindah, {$plan.summary
        .installers_archived} installer diarsipkan.
    </p>
    <p class="muted">
      {$plan.summary.enabled_actions} / {$plan.summary.total_actions} aksi aktif
    </p>
  </div>

  {#if $plan.dup_groups.length > 0 && (tab === "semua" || tab === "duplikat")}
    <div class="card">
      <h2>{id.plan.tabs.duplikat} ({$plan.dup_groups.length})</h2>
      {#each $plan.dup_groups.slice(0, 50) as g}
        <div style="margin-bottom: 16px">
          <p>{g.reason} <span class="muted">(hemat {formatBytes(g.reclaimable_bytes)})</span></p>
          <table class="list">
            <tbody>
              {#each g.files as f, i}
                <tr>
                  <td>
                    {f.path}
                    <span class="muted"> · {formatBytes(f.size)}</span>
                  </td>
                  <td>
                    {#if i === g.keeper}
                      <span class="badge">{id.plan.keeper}</span>
                    {:else}
                      <button class="ghost" on:click={() => chooseKeeper(g.id, i)}>
                        {id.plan.setKeeper}
                      </button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/each}
      {#if $plan.dup_groups.length > 50}
        <p class="muted">… + {$plan.dup_groups.length - 50} grup lainnya</p>
      {/if}
    </div>
  {/if}

  <nav class="tabs" aria-label="Kategori">
    {#each tabDefs as t}
      <button
        aria-current={tab === t.id ? "page" : undefined}
        on:click={() => (tab = t.id)}
      >
        {t.label}
      </button>
    {/each}
  </nav>

  <div class="card">
    <div class="row">
      <button class="ghost" on:click={() => toggleAll(true)}>{id.plan.selectAll}</button>
      <button class="ghost" on:click={() => toggleAll(false)}>{id.plan.deselectAll}</button>
    </div>
    {#if visibleActions.length === 0}
      <p class="muted">{id.plan.empty}</p>
    {:else}
      <table class="list">
        <tbody>
          {#each visibleActions.slice(0, 200) as a}
            <tr>
              <td>
                <input
                  type="checkbox"
                  checked={a.enabled}
                  aria-label={a.id}
                  on:change={(e) => toggle(a.id, e.currentTarget.checked)}
                />
              </td>
              <td>
                {describeTarget(a)}
                <div class="muted">{a.reason}</div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if visibleActions.length > 200}
        <p class="muted">… + {visibleActions.length - 200} aksi lainnya</p>
      {/if}
    {/if}
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if confirming}
    <div class="card" role="alertdialog" aria-label={id.plan.confirmTitle}>
      <h2>{id.plan.confirmTitle}</h2>
      <p>{id.plan.confirmBody}</p>
      <div class="row">
        <button class="primary" on:click={onApply}>{id.plan.confirmYes}</button>
        <button class="ghost" on:click={() => (confirming = false)}>{id.plan.confirmNo}</button>
      </div>
    </div>
  {:else}
    <button class="primary" on:click={onApply}>{id.plan.apply}</button>
  {/if}
{:else}
  <div class="card"><p class="muted">{id.plan.empty}</p></div>
{/if}

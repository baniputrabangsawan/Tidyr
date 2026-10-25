<script lang="ts">
  import { api } from "../api";
  import type { Category } from "../api";
  import { id } from "../i18n/id";
  import { screen, plan } from "../store";
  import { formatBytes } from "../api";

  let tab: Category | "semua" = "semua";
  let confirming = false;
  let error = "";

  // Virtual windowing state for actions table
  const ACTION_ROW_HEIGHT = 56;
  const ACTION_VIEWPORT_HEIGHT = 450;
  const ACTION_BUFFER = 5;
  let actionsScrollTop = 0;

  // Virtual windowing state for duplicate groups
  const GROUP_ROW_HEIGHT = 140;
  const GROUP_VIEWPORT_HEIGHT = 450;
  const GROUP_BUFFER = 3;
  let groupsScrollTop = 0;

  const tabDefs: Array<{ id: Category | "semua"; label: string }> = [
    { id: "semua", label: id.plan.all },
    { id: "duplikat", label: id.plan.tabs.duplikat },
    { id: "rapikan", label: id.plan.tabs.rapikan },
    { id: "installer", label: id.plan.tabs.installer },
    { id: "folder_kosong", label: id.plan.tabs.folder_kosong },
  ];

  $: visibleActions = ($plan?.actions ?? []).filter(
    (a) => tab === "semua" || a.category === tab,
  );

  // Actions virtual window calculation
  $: actionVisibleCount = Math.ceil(ACTION_VIEWPORT_HEIGHT / ACTION_ROW_HEIGHT) + ACTION_BUFFER * 2;
  $: actionStartIndex = Math.max(0, Math.floor(actionsScrollTop / ACTION_ROW_HEIGHT) - ACTION_BUFFER);
  $: actionEndIndex = Math.min(visibleActions.length, actionStartIndex + actionVisibleCount);
  $: actionTopSpacer = actionStartIndex * ACTION_ROW_HEIGHT;
  $: actionBottomSpacer = Math.max(0, (visibleActions.length - actionEndIndex) * ACTION_ROW_HEIGHT);
  $: renderedActions = visibleActions.slice(actionStartIndex, actionEndIndex);

  // Duplicate groups virtual window calculation
  $: dupGroups = $plan?.dup_groups ?? [];
  $: groupVisibleCount = Math.ceil(GROUP_VIEWPORT_HEIGHT / GROUP_ROW_HEIGHT) + GROUP_BUFFER * 2;
  $: groupStartIndex = Math.max(0, Math.floor(groupsScrollTop / GROUP_ROW_HEIGHT) - GROUP_BUFFER);
  $: groupEndIndex = Math.min(dupGroups.length, groupStartIndex + groupVisibleCount);
  $: groupTopSpacer = groupStartIndex * GROUP_ROW_HEIGHT;
  $: groupBottomSpacer = Math.max(0, (dupGroups.length - groupEndIndex) * GROUP_ROW_HEIGHT);
  $: renderedGroups = dupGroups.slice(groupStartIndex, groupEndIndex);

  function describeTarget(a: (typeof visibleActions)[number]): string {
    const act = a.action;
    switch (act.kind) {
      case "move":
      case "rename":
        return `${id.plan.from} ${act.from} → ${id.plan.to} ${act.to}`;
      case "quarantine":
      case "delete_empty_dir":
        return act.path;
    }
  }

  function isGroupEnabled(groupId: string): boolean {
    if (!$plan) return false;
    const groupActions = $plan.actions.filter((a) => a.group_id === groupId);
    return groupActions.length > 0 && groupActions.every((a) => a.enabled);
  }

  async function toggleGroup(groupId: string, enabled: boolean) {
    if (!$plan) return;
    const groupActions = $plan.actions.filter((a) => a.group_id === groupId);
    for (const a of groupActions) {
      await toggle(a.id, enabled);
    }
  }

  async function toggle(actionId: string, enabled: boolean) {
    try {
      const summary = await api.setActionEnabled(actionId, enabled);
      plan.update((p) => (p ? { ...p, summary } : p));
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
      {$plan.summary.enabled_actions} / {$plan.summary.total_actions} {id.plan.activeActions}
    </p>
  </div>

  {#if dupGroups.length > 0 && (tab === "semua" || tab === "duplikat")}
    <div class="card">
      <div class="row" style="justify-content: space-between; align-items: center; margin-bottom: 8px;">
        <h2>{id.plan.tabs.duplikat} ({dupGroups.length})</h2>
        <span class="muted">
          {id.plan.showing} {renderedGroups.length} {id.plan.of} {dupGroups.length} {id.plan.groupsTotal}
        </span>
      </div>

      <div
        class="virtual-groups-viewport"
        style="max-height: 450px; overflow-y: auto;"
        on:scroll={(e) => (groupsScrollTop = e.currentTarget.scrollTop)}
      >
        <div style="height: {groupTopSpacer}px;"></div>

        {#each renderedGroups as g}
          <div style="margin-bottom: 16px; padding-bottom: 8px; border-bottom: 1px solid var(--border, #eee);">
            <div class="row" style="align-items: center; gap: 8px;">
              <input
                type="checkbox"
                checked={isGroupEnabled(g.id)}
                aria-label="{id.plan.toggleGroup}: {g.id}"
                on:change={(e) => toggleGroup(g.id, e.currentTarget.checked)}
              />
              <strong>{g.reason}</strong>
              <span class="muted">(hemat {formatBytes(g.reclaimable_bytes)})</span>
            </div>

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

        <div style="height: {groupBottomSpacer}px;"></div>
      </div>
    </div>
  {/if}

  <nav class="tabs" aria-label="Kategori">
    {#each tabDefs as t}
      <button
        aria-current={tab === t.id ? "page" : undefined}
        on:click={() => {
          tab = t.id;
          actionsScrollTop = 0;
        }}
      >
        {t.label}
      </button>
    {/each}
  </nav>

  <div class="card">
    <div class="row" style="justify-content: space-between; align-items: center; margin-bottom: 12px;">
      <div class="row" style="gap: 8px;">
        <button class="ghost" on:click={() => toggleAll(true)}>{id.plan.selectAll}</button>
        <button class="ghost" on:click={() => toggleAll(false)}>{id.plan.deselectAll}</button>
      </div>
      {#if visibleActions.length > 0}
        <span class="muted">
          {id.plan.showing} {renderedActions.length} {id.plan.of} {visibleActions.length} {id.plan.actionsTotal}
        </span>
      {/if}
    </div>

    {#if visibleActions.length === 0}
      <p class="muted">{id.plan.empty}</p>
    {:else}
      <div
        class="virtual-actions-viewport"
        style="max-height: 450px; overflow-y: auto;"
        on:scroll={(e) => (actionsScrollTop = e.currentTarget.scrollTop)}
      >
        <table class="list">
          <tbody>
            {#if actionTopSpacer > 0}
              <tr style="height: {actionTopSpacer}px;"><td colspan="2" style="padding:0; border:none;"></td></tr>
            {/if}

            {#each renderedActions as a (a.id)}
              <tr style="height: {ACTION_ROW_HEIGHT}px;">
                <td style="width: 40px; vertical-align: middle;">
                  <input
                    type="checkbox"
                    checked={a.enabled}
                    aria-label={a.id}
                    on:change={(e) => toggle(a.id, e.currentTarget.checked)}
                  />
                </td>
                <td style="vertical-align: middle;">
                  <div>{describeTarget(a)}</div>
                  <div class="muted" style="font-size: 0.85em;">{a.reason}</div>
                </td>
              </tr>
            {/each}

            {#if actionBottomSpacer > 0}
              <tr style="height: {actionBottomSpacer}px;"><td colspan="2" style="padding:0; border:none;"></td></tr>
            {/if}
          </tbody>
        </table>
      </div>
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

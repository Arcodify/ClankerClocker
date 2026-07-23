<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let open = false;
  export let title = "";
  export let body = "";
  export let confirmLabel = "OK";
  export let cancelLabel = "Cancel";
  export let showInput = false;
  export let inputPlaceholder = "";
  export let danger = false;

  // "cancel" fires only from the cancel button; Escape / overlay clicks fire
  // "dismiss" so callers whose cancel action has side effects (e.g. clocking
  // out) don't trigger it accidentally.
  const dispatch = createEventDispatcher<{
    confirm: { reason: string };
    cancel: void;
    dismiss: void;
  }>();
  let reason = "";

  function confirm() {
    dispatch("confirm", { reason: reason.trim() });
    reason = "";
  }

  function cancel() {
    dispatch("cancel");
    reason = "";
  }

  function dismiss() {
    dispatch("dismiss");
    reason = "";
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") dismiss();
  }
</script>

<svelte:window on:keydown={onKeydown} />

{#if open}
  <div class="dialog-overlay" on:click={dismiss} role="presentation"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={title}>
    <div class="dialog-title">{title}</div>
    <div class="dialog-body">{body}</div>
    {#if showInput}
      <!-- svelte-ignore a11y-autofocus -->
      <textarea
        class="dialog-input"
        bind:value={reason}
        placeholder={inputPlaceholder}
        rows="2"
        autofocus
      ></textarea>
    {/if}
    <div class="dialog-actions">
      <button class="dlg-btn dlg-cancel" on:click={cancel}>{cancelLabel}</button>
      <button class="dlg-btn dlg-confirm" class:danger on:click={confirm}>{confirmLabel}</button>
    </div>
  </div>
{/if}

<style>
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 90;
  }
  .dialog {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 100;
    width: calc(100vw - 48px);
    max-width: 320px;
    background: #16161e;
    border: 1px solid #2a2a38;
    border-radius: 12px;
    padding: 18px 16px 14px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .dialog-title {
    font-size: 14px;
    font-weight: 700;
    color: #e8e8ec;
  }
  .dialog-body {
    font-size: 12px;
    color: #9090a8;
    line-height: 1.5;
    white-space: pre-line;
  }
  .dialog-input {
    background: #0e0e16;
    border: 1px solid #2a2a38;
    border-radius: 8px;
    color: #e0e0ec;
    font-size: 12px;
    font-family: inherit;
    padding: 8px 10px;
    resize: none;
  }
  .dialog-input:focus {
    outline: none;
    border-color: #4a4a68;
  }
  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 2px;
  }
  .dlg-btn {
    padding: 8px 16px;
    border-radius: 7px;
    border: none;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
  }
  .dlg-cancel {
    background: #1e1e2c;
    color: #9090a8;
    border: 1px solid #2a2a38;
  }
  .dlg-cancel:hover {
    color: #c0c0d0;
    background: #252532;
  }
  .dlg-confirm {
    background: #22c55e;
    color: #080c10;
  }
  .dlg-confirm:hover {
    background: #16a34a;
  }
  .dlg-confirm.danger {
    background: #ef4444;
    color: white;
  }
  .dlg-confirm.danger:hover {
    background: #dc2626;
  }
</style>

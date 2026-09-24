<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { authToken, errorMessage } from "../lib/stores";

  const dispatch = createEventDispatcher();

  let provider = "plane";
  let customProvider = "";
  let apiKey = "";
  let saving = false;
  let saved = false;

  $: keyName = provider === "custom" ? customProvider.trim() : provider;
  $: canSave = !!$authToken && keyName.length > 0 && apiKey.trim().length > 0 && !saving;

  async function saveApiKey() {
    if (!canSave) return;

    saving = true;
    saved = false;
    try {
      await invoke("add_api_key", {
        key: keyName,
        value: apiKey.trim(),
      });
      apiKey = "";
      saved = true;
      setTimeout(() => (saved = false), 1800);
    } catch (e) {
      errorMessage.set("API key save failed: " + String(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="api-keys">
  <header>
    <button class="back-btn" on:click={() => dispatch("back")}>← Back</button>
    <div class="title-wrap">
      <span class="title">API Keys</span>
      <span class="subtitle">Connect project management tools</span>
    </div>
  </header>

  <section class="panel">
    <label>
      <span>Provider</span>
      <select bind:value={provider}>
        <option value="plane">Plane</option>
        <option value="jira">Jira</option>
        <option value="trello">Trello</option>
        <option value="clickup">ClickUp</option>
        <option value="custom">Custom</option>
      </select>
    </label>

    {#if provider === "custom"}
      <label>
        <span>Key Name</span>
        <input bind:value={customProvider} placeholder="linear" autocomplete="off" />
      </label>
    {/if}

    <label>
      <span>API Key</span>
      <input bind:value={apiKey} placeholder="Paste API key" type="password" autocomplete="off" />
    </label>

    <button class="btn-save" on:click={saveApiKey} disabled={!canSave}>
      {#if saved}Saved{:else if saving}Saving...{:else}Save API Key{/if}
    </button>

    {#if !$authToken}
      <p class="note">Sign in to PocketBase before saving integration keys.</p>
    {/if}
  </section>
</div>

<style>
  .api-keys {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow-y: auto;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid #1e1e24;
    position: sticky;
    top: 0;
    background: #0d0d0f;
  }

  .back-btn {
    background: none;
    border: none;
    color: #6b7280;
    font-size: 13px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .back-btn:hover { color: #e8e8ec; background: #1e1e24; }

  .title-wrap { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title { font-size: 14px; font-weight: 600; color: #c0c0cc; }
  .subtitle { font-size: 11px; color: #7c8aa6; }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 20px 18px;
  }

  label { display: flex; flex-direction: column; gap: 5px; }
  label span {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: #5a5a72;
  }

  input,
  select {
    background: #13131a;
    border: 1px solid #2a2a36;
    border-radius: 7px;
    padding: 9px 12px;
    color: #e0e0ec;
    font-size: 13px;
    outline: none;
    width: 100%;
  }
  input:focus,
  select:focus { border-color: #6366f1; }

  .btn-save {
    background: #6366f1;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 10px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    margin-top: 4px;
  }
  .btn-save:hover:not(:disabled) { background: #4f46e5; }
  .btn-save:disabled { opacity: 0.5; cursor: not-allowed; }

  .note {
    color: #7c8aa6;
    font-size: 12px;
    line-height: 1.45;
  }
</style>

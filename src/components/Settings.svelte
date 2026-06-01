<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { createEventDispatcher } from "svelte";
  import {
    authToken,
    errorMessage,
    isAdmin,
    settings,
    userId,
    userName,
    view,
  } from "../lib/stores";

  const dispatch = createEventDispatcher();

  let pbUrl = $settings.pb_url;
  let pbEmail = $settings.pb_email;
  let pbPassword = "";
  let saving = false;
  let saveOk = false;

  async function save() {
    if (!pbUrl || !pbEmail || !pbPassword) {
      errorMessage.set("All fields required");
      setTimeout(() => errorMessage.set(""), 2000);
      return;
    }
    saving = true;
    try {
      const result = await invoke<{ token: string; user_id: string }>(
        "authenticate_pb",
        {
          pbUrl,
          pbEmail,
          pbPassword,
        },
      );
      settings.update((s) => ({ ...s, pb_url: pbUrl, pb_email: pbEmail }));
      authToken.set(result.token);
      userId.set(result.user_id);
      saveOk = true;
      setTimeout(() => {
        saveOk = false;
        dispatch("back");
      }, 1000);
    } catch (e) {
      errorMessage.set("Auth failed: " + String(e));
      setTimeout(() => errorMessage.set(""), 3000);
    } finally {
      saving = false;
    }
  }
</script>

<div class="settings">
  <header>
    <button class="app-button app-button-ghost app-button-sm back-btn" on:click={() => dispatch("back")}>
      ← Back
    </button>
    <span class="text-title">Settings</span>
  </header>

  <div class="app-card form">
    <label class="app-field">
      <span class="app-label">PocketBase URL</span>
      <input
        class="app-input"
        bind:value={pbUrl}
        placeholder="https://your-pb.example.com"
        type="url"
      />
    </label>
    <label class="app-field">
      <span class="app-label">Email</span>
      <input
        class="app-input"
        bind:value={pbEmail}
        placeholder="you@example.com"
        type="email"
        autocomplete="off"
      />
    </label>
    <label class="app-field">
      <span class="app-label">Password</span>
      <input
        class="app-input"
        bind:value={pbPassword}
        placeholder="••••••••"
        type="password"
        autocomplete="off"
      />
    </label>

    <button
      class="app-button app-button-primary app-button-lg app-button-block"
      on:click={save}
      disabled={saving}
    >
      {#if saveOk}✓ Saved{:else if saving}Authenticating…{:else}Save & Login{/if}
    </button>

    {#if $authToken}
      <div class="auth-row">
        <span class="text-success auth-status"
          >✓ {$userName || $settings.pb_email}</span
        >
        <button
          class="app-button app-button-destructive-outline app-button-sm"
          on:click={() => {
            authToken.set("");
            userId.set("");
            isAdmin.set(false);
            userName.set("");
            view.set("login");
          }}>Sign out</button
        >
      </div>
    {/if}
  </div>

  <div class="info">
    <div class="text-caption info-title">Platform Notes For End Users</div>
    <div class="app-card app-card-compact text-small notes">
      <p>
        <strong>macOS:</strong> needs Accessibility permission granted in System
        Settings → Privacy &amp; Security → Accessibility
      </p>
      <p>
        <strong>Windows:</strong> run as administrator if the tray icon or input
        hooks get blocked by antivirus
      </p>
      <p>
        <strong>Linux:</strong> user must be in the input group (<code
          >sudo usermod -aG input $USER</code
        >, then log out/in)
      </p>
    </div>
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow-y: auto;
    padding: 0 12px 12px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 4px;
    border-bottom: 1px solid hsl(var(--border) / 0.72);
    position: sticky;
    top: 0;
    background: hsl(var(--background));
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 12px;
    padding: 16px;
  }
  .auth-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }
  .auth-status {
    font-size: 12px;
  }

  .info {
    padding: 14px 0 0;
  }
  .info-title {
    margin-bottom: 8px;
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .notes strong {
    color: hsl(var(--secondary-foreground));
  }
  .notes code {
    font-size: 11px;
    color: hsl(var(--foreground));
  }
</style>

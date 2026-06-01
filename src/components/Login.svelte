<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { createEventDispatcher, onMount } from "svelte";
  import {
    authToken,
    errorMessage,
    isAdmin,
    settings,
    userId,
    userName,
  } from "../lib/stores";

  const dispatch = createEventDispatcher();

  let pbUrl = $settings.pb_url || "";
  let pbEmail = $settings.pb_email || "";
  let pbPassword = "";
  let loading = false;
  let showAdvanced = !pbUrl;

  onMount(async () => {
    // Load default URL from Rust config constant if nothing saved yet
    if (!pbUrl) {
      try {
        const s = await invoke<{ default_pb_url: string }>("get_settings");
        if (s.default_pb_url) pbUrl = s.default_pb_url;
      } catch (_) {}
    }
    showAdvanced = !pbUrl;
  });

  async function connect() {
    if (!pbUrl || !pbEmail || !pbPassword) {
      if (!pbUrl) showAdvanced = true;
      errorMessage.set("All fields are required");
      return;
    }
    loading = true;
    try {
      const result = await invoke<{
        token: string;
        user_id: string;
        user_name: string;
        user_email: string;
        is_admin: boolean;
      }>("authenticate_pb", { pbUrl, pbEmail, pbPassword });

      settings.update((s) => ({ ...s, pb_url: pbUrl, pb_email: pbEmail }));
      authToken.set(result.token);
      userId.set(result.user_id);
      userName.set(result.user_name || result.user_email);
      isAdmin.set(result.is_admin);
      dispatch("done");
    } catch (e) {
      errorMessage.set("Connection failed: " + String(e));
    } finally {
      loading = false;
    }
  }
</script>

<div class="login">
  <div class="logo">
    <img class="logo-mark" src="/arcodify-logo.svg" alt="Arcodify" />
    <div class="text-h2 logo-name">ClankerClocker</div>
  </div>

  <div class="app-card form">
    <label class="app-field">
      <span class="app-label">Email</span>
      <input
        class="app-input"
        bind:value={pbEmail}
        placeholder="you@company.com"
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
        autocomplete="new-password"
        on:keydown={(e) => e.key === "Enter" && connect()}
      />
    </label>

    <button
      class="app-button app-button-ghost app-button-sm advanced-toggle"
      type="button"
      on:click={() => (showAdvanced = !showAdvanced)}
    >
      {showAdvanced ? "Hide advanced" : "Advanced"}
    </button>

    {#if showAdvanced}
      <label class="app-field">
        <span class="app-label">Server</span>
        <input
          class="app-input"
          bind:value={pbUrl}
          placeholder="https://pb.yourcompany.com"
          type="url"
          autocomplete="off"
        />
      </label>
    {/if}

    <button
      class="app-button app-button-primary app-button-lg app-button-block"
      on:click={connect}
      disabled={loading}
    >
      {loading ? "Connecting…" : "Sign In"}
    </button>

    <div class="divider">or</div>

    <button
      class="app-button app-button-outline app-button-md app-button-block"
      on:click={() => dispatch("skip")}
    >
      Work offline — sync later
    </button>

    <div class="app-help offline-note">
      Activity is saved locally and synced when you sign in.
    </div>
  </div>
</div>

<style>
  .login {
    display: flex;
    flex-direction: column;
    height: 100vh;
    align-items: center;
    justify-content: center;
    padding: 32px 24px;
    gap: 22px;
  }
  .logo {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .logo-mark {
    display: block;
    height: 58px;
    max-width: 200px;
    object-fit: contain;
    width: 200px;
  }

  .form {
    width: min(100%, 360px);
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px;
  }
  .advanced-toggle {
    align-self: flex-start;
    padding-left: 0;
    padding-right: 0;
  }
  .divider {
    text-align: center;
    font-size: 11px;
    color: hsl(var(--muted-foreground) / 0.6);
    position: relative;
    margin: 2px 0;
  }
  .divider::before,
  .divider::after {
    content: "";
    position: absolute;
    top: 50%;
    width: 44%;
    height: 1px;
    background: hsl(var(--border));
  }
  .divider::before {
    left: 0;
  }
  .divider::after {
    right: 0;
  }

  .offline-note {
    text-align: center;
  }
</style>

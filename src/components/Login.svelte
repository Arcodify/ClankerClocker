<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, authToken, userId, isAdmin, userName, errorMessage } from "../lib/stores";

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
    <div class="logo-icon">⏱</div>
    <div class="logo-name">ClankerClocker</div>
    <div class="logo-sub">by @arcodify</div>
  </div>

  <div class="form">
    <label>
      <span>Email</span>
      <input bind:value={pbEmail} placeholder="you@company.com" type="email" autocomplete="off" />
    </label>
    <label>
      <span>Password</span>
      <input
        bind:value={pbPassword}
        placeholder="••••••••"
        type="password"
        autocomplete="new-password"
        on:keydown={(e) => e.key === "Enter" && connect()}
      />
    </label>

    <button class="advanced-toggle" type="button" on:click={() => (showAdvanced = !showAdvanced)}>
      {showAdvanced ? "Hide advanced" : "Advanced"}
    </button>

    {#if showAdvanced}
      <label>
        <span>Server</span>
        <input
          bind:value={pbUrl}
          placeholder="https://pb.yourcompany.com"
          type="url"
          autocomplete="off"
        />
      </label>
    {/if}

    <button class="btn-connect" on:click={connect} disabled={loading}>
      {loading ? "Connecting…" : "Sign In"}
    </button>

    <div class="divider">or</div>

    <button class="btn-skip" on:click={() => dispatch("skip")}>
      Work offline — sync later
    </button>

    <div class="offline-note">
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
    padding: 28px 24px;
    gap: 28px;
  }
  .logo { display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .logo-icon { font-size: 38px; line-height: 1; }
  .logo-name { font-size: 20px; font-weight: 700; color: #f0f0f5; letter-spacing: 0.5px; }
  .logo-sub { font-size: 12px; color: #4a4a62; }

  .form { width: 100%; display: flex; flex-direction: column; gap: 11px; }
  label { display: flex; flex-direction: column; gap: 4px; }
  label span { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.7px; color: #4a4a62; }
  input {
    background: #13131a;
    border: 1px solid #252530;
    border-radius: 7px;
    padding: 9px 12px;
    color: #e0e0ec;
    font-size: 13px;
    outline: none;
    width: 100%;
    transition: border-color 0.15s;
  }
  input:focus { border-color: #6366f1; }

  .btn-connect {
    background: #6366f1;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 11px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    margin-top: 4px;
    transition: background 0.15s;
  }
  .btn-connect:hover:not(:disabled) { background: #4f46e5; }
  .btn-connect:disabled { opacity: 0.5; cursor: not-allowed; }

  .advanced-toggle {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: #6b7280;
    font-size: 12px;
    padding: 0;
    cursor: pointer;
  }
  .advanced-toggle:hover { color: #a0a0bc; }

  .divider {
    text-align: center;
    font-size: 11px;
    color: #2e2e42;
    position: relative;
    margin: 2px 0;
  }
  .divider::before, .divider::after {
    content: '';
    position: absolute;
    top: 50%;
    width: 44%;
    height: 1px;
    background: #1e1e28;
  }
  .divider::before { left: 0; }
  .divider::after { right: 0; }

  .btn-skip {
    background: transparent;
    border: 1px solid #252530;
    border-radius: 8px;
    padding: 10px;
    color: #5a5a72;
    font-size: 13px;
    cursor: pointer;
    transition: all 0.15s;
  }
  .btn-skip:hover { border-color: #3a3a52; color: #8080a0; }

  .offline-note { font-size: 11px; color: #2e2e42; text-align: center; line-height: 1.5; }
</style>

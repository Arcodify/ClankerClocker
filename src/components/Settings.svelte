<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, authToken, userId, isAdmin, userName, errorMessage, view } from "../lib/stores";

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
      const result = await invoke<{ token: string; user_id: string }>("authenticate_pb", {
        pbUrl,
        pbEmail,
        pbPassword,
      });
      settings.update((s) => ({ ...s, pb_url: pbUrl, pb_email: pbEmail }));
      authToken.set(result.token);
      userId.set(result.user_id);
      saveOk = true;
      setTimeout(() => { saveOk = false; dispatch("back"); }, 1000);
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
    <button class="back-btn" on:click={() => dispatch("back")}>← Back</button>
    <span class="title">Settings</span>
  </header>

  <div class="form">
    <label>
      <span>PocketBase URL</span>
      <input bind:value={pbUrl} placeholder="https://your-pb.example.com" type="url" />
    </label>
    <label>
      <span>Email</span>
      <input bind:value={pbEmail} placeholder="you@example.com" type="email" autocomplete="off" />
    </label>
    <label>
      <span>Password</span>
      <input bind:value={pbPassword} placeholder="••••••••" type="password" autocomplete="off" />
    </label>

    <button class="btn-save" on:click={save} disabled={saving}>
      {#if saveOk}✓ Saved{:else if saving}Authenticating…{:else}Save & Login{/if}
    </button>

    {#if $authToken}
      <div class="auth-row">
        <span class="auth-status">✓ {$userName || $settings.pb_email}</span>
        <button class="btn-logout" on:click={() => {
          authToken.set(""); userId.set(""); isAdmin.set(false); userName.set("");
          view.set("login");
        }}>Sign out</button>
      </div>
    {/if}
  </div>

  <div class="info">
    <div class="info-title">Platform Notes For End Users</div>
    <div class="notes">
      <p><strong>macOS:</strong> needs Accessibility permission granted in System Settings → Privacy &amp; Security → Accessibility</p>
      <p><strong>Windows:</strong> run as administrator if the tray icon or input hooks get blocked by antivirus</p>
      <p><strong>Linux:</strong> user must be in the input group (<code>sudo usermod -aG input $USER</code>, then log out/in)</p>
    </div>
  </div>
</div>

<style>
  .settings { display: flex; flex-direction: column; height: 100vh; overflow-y: auto; }

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
    background: none; border: none; color: #6b7280;
    font-size: 13px; cursor: pointer; padding: 4px 8px;
    border-radius: 4px;
  }
  .back-btn:hover { color: #e8e8ec; background: #1e1e24; }
  .title { font-size: 14px; font-weight: 600; color: #c0c0cc; }

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 20px 18px;
  }
  label { display: flex; flex-direction: column; gap: 5px; }
  label span { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.6px; color: #5a5a72; }
  input {
    background: #13131a;
    border: 1px solid #2a2a36;
    border-radius: 7px;
    padding: 9px 12px;
    color: #e0e0ec;
    font-size: 13px;
    outline: none;
    width: 100%;
  }
  input:focus { border-color: #6366f1; }

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

  .auth-row { display: flex; align-items: center; justify-content: center; gap: 10px; }
  .auth-status { font-size: 12px; color: #22c55e; }
  .btn-logout {
    background: none; border: 1px solid #3a1a1a; color: #ef4444;
    font-size: 11px; padding: 3px 10px; border-radius: 5px; cursor: pointer;
  }
  .btn-logout:hover { background: #1a0a0a; }

  .info { padding: 0 18px 20px; }
  .info-title { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.6px; color: #5a5a72; margin-bottom: 8px; }
  .notes {
    background: #0a0a10;
    border: 1px solid #1e1e28;
    border-radius: 8px;
    padding: 12px;
    color: #505068;
    display: flex;
    flex-direction: column;
    gap: 10px;
    line-height: 1.5;
    font-size: 12px;
  }
  .notes strong { color: #c0c0cc; }
  .notes code { font-size: 11px; color: #d0d0e8; }
</style>

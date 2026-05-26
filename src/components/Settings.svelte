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
    <div class="info-title">Required PocketBase Collections</div>
    <pre class="schema">{`work_sessions
  user_id    (text)
  clock_in   (date)
  clock_out  (date)
  status     (text)
  total_break_seconds (number)
  break_count (number)

breaks
  session_id (relation)
  start_time (date)
  end_time   (date)
  type       (text)

activity_snapshots
  session_id (relation)
  timestamp  (date)
  keystrokes (number)
  mouse_clicks (number)
  active_app (text)
  active_window (text)
  idle_seconds (number)

network_connections
  session_id   (relation)
  timestamp    (date)
  process_name (text)
  remote_host  (text)
  remote_ip    (text)
  remote_port  (number)

break_configs
  name               (text)
  type_key           (text)
  duration_minutes   (number)
  sort_order         (number)
  is_active          (bool)
  auto_start_enabled (bool)
  auto_start_time    (text, HH:MM in Nepal time)
  auto_end_time      (text, HH:MM in Nepal time)`}</pre>
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
  .schema {
    background: #0a0a10;
    border: 1px solid #1e1e28;
    border-radius: 8px;
    padding: 12px;
    font-size: 10px;
    color: #6060808;
    color: #505068;
    line-height: 1.6;
    overflow-x: auto;
    white-space: pre;
    font-family: "JetBrains Mono", "Fira Code", monospace;
  }
</style>

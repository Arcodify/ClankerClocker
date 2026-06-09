<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings, authToken, userId, isAdmin, userName, errorMessage, view } from "../lib/stores";

  const dispatch = createEventDispatcher();

  let pbUrl = $settings.pb_url;
  let pbEmail = $settings.pb_email;
  let pbPassword = "";
  let clockInTime = $settings.clock_in_time;
  let clockOutTime = $settings.clock_out_time;
  let autoClockOutEnabled = $settings.auto_clock_out_enabled;
  let saving = false;
  let savingSchedule = false;
  let saveOk = false;

  async function save() {
    if (!pbUrl || !pbEmail || !pbPassword) {
      errorMessage.set("All fields required");
      setTimeout(() => errorMessage.set(""), 2000);
      return;
    }
    saving = true;
    try {
      const result = await invoke<{
        token: string;
        user_id: string;
        user_name: string;
        user_email: string;
        is_admin: boolean;
      }>("authenticate_pb", {
        pbUrl,
        pbEmail,
        pbPassword,
      });
      settings.update((s) => ({ ...s, pb_url: pbUrl, pb_email: pbEmail, is_admin: result.is_admin }));
      authToken.set(result.token);
      userId.set(result.user_id);
      isAdmin.set(result.is_admin);
      userName.set(result.user_name || result.user_email);
      saveOk = true;
      setTimeout(() => { saveOk = false; dispatch("back"); }, 1000);
    } catch (e) {
      errorMessage.set("Auth failed: " + String(e));
      setTimeout(() => errorMessage.set(""), 3000);
    } finally {
      saving = false;
    }
  }

  async function saveSchedule() {
    savingSchedule = true;
    try {
      const result = await invoke<{
        clock_in_time: string;
        clock_out_time: string;
        auto_clock_out_enabled: boolean;
      }>("save_work_schedule", {
        clockInTime,
        clockOutTime,
        autoClockOutEnabled,
      });

      settings.update((s) => ({
        ...s,
        clock_in_time: result.clock_in_time,
        clock_out_time: result.clock_out_time,
        auto_clock_out_enabled: result.auto_clock_out_enabled,
      }));
      saveOk = true;
      setTimeout(() => { saveOk = false; }, 1000);
    } catch (e) {
      errorMessage.set("Schedule save failed: " + String(e));
      setTimeout(() => errorMessage.set(""), 3000);
    } finally {
      savingSchedule = false;
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

    {#if $isAdmin}
      <div class="schedule-card">
        <div class="schedule-title">Company Work Schedule</div>
        <label>
          <span>Clock In Time</span>
          <input bind:value={clockInTime} placeholder="09:00" type="time" />
        </label>
        <label>
          <span>Clock Out Time</span>
          <input bind:value={clockOutTime} placeholder="18:00" type="time" />
        </label>
        <label class="switch-row">
          <span>Auto clock out after clock-out time</span>
          <input bind:checked={autoClockOutEnabled} type="checkbox" />
        </label>
        <button class="btn-save btn-schedule" on:click={saveSchedule} disabled={savingSchedule}>
          {#if savingSchedule}Updating policy…{:else}Update Company Policy{/if}
        </button>
      </div>
    {/if}

    {#if $authToken}
      <div class="auth-row">
        <span class="auth-status">✓ {$userName || $settings.pb_email}</span>
        <button class="btn-logout" on:click={async () => {
          await invoke("clear_auth").catch(() => {});
          authToken.set(""); userId.set(""); isAdmin.set(false); userName.set("");
          view.set("login");
        }}>Sign out</button>
      </div>
    {/if}
  </div>

  <div class="info">
    <div class="info-title">Platform Notes For End Users</div>
    <div class="notes">
      <p><strong>macOS:</strong> needs Accessibility and Input Monitoring permission granted in System Settings → Privacy &amp; Security</p>
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

  .schedule-card {
    margin-top: 10px;
    padding: 14px;
    border: 1px solid #1f2432;
    border-radius: 10px;
    background: #0a0c12;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .schedule-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.7px;
    color: #7c8aa6;
  }
  .switch-row {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .switch-row input {
    width: auto;
  }
  .btn-schedule {
    margin-top: 0;
    background: #1f2937;
  }
  .btn-schedule:hover:not(:disabled) { background: #273244; }

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

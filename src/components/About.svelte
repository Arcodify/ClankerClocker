<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { APP_CREDITS, APP_LAST_UPDATED, APP_NAME, APP_VERSION } from "../lib/app-meta";

  const dispatch = createEventDispatcher();
</script>

<div class="about">
  <header>
    <button class="back-btn" on:click={() => dispatch("back")}>← Back</button>
    <div class="title-wrap">
      <span class="eyebrow">About</span>
      <span class="title">{APP_NAME}</span>
    </div>
    <span class="version">v{APP_VERSION}</span>
  </header>

  <section class="hero">
    <div class="hero-copy">
      <p class="label">Application Information</p>
      <h1>{APP_NAME}</h1>
      <p class="summary">
        A focused desktop tool for attendance tracking, activity summaries, and reminder workflows for the Arcodify team.
      </p>
    </div>
  </section>

  <section class="card">
    <div class="card-title">Release</div>
    <div class="meta-grid">
      <div>
        <span class="meta-label">Version</span>
        <span class="meta-value">v{APP_VERSION}</span>
      </div>
      <div>
        <span class="meta-label">Last updated</span>
        <span class="meta-value">{APP_LAST_UPDATED}</span>
      </div>
      <div>
        <span class="meta-label">Company</span>
        <span class="meta-value">Arcodify</span>
      </div>
    </div>
  </section>

  <section class="card">
    <div class="card-title">Credits</div>
    <div class="credits">
      {#each APP_CREDITS as credit}
        <div class="credit-row">
          <div class="credit-role">{credit.role}</div>
          <div class="credit-name">{credit.name}</div>
          {#if credit.detail}
            <div class="credit-detail">{credit.detail}</div>
          {/if}
        </div>
      {/each}
    </div>
  </section>

  <section class="card">
    <div class="card-title">What It Records</div>
    <div class="notes-list">
      <p>Session timing and break duration for attendance logs.</p>
      <p>Counts of keystrokes, mouse clicks, and mouse movement for activity summaries.</p>
      <p>The active app and window title so reports can show context.</p>
      <p>Network connection metadata such as process name, host, IP, and port for review and visibility.</p>
      <p>It does not store the text you type or screenshots.</p>
    </div>
  </section>

  <section class="card">
    <div class="card-title">Idle And Meetings</div>
    <div class="notes-list">
      <p>If there is no input for a while, the app treats the session as idle and can show a warning before auto clock-out.</p>
      <p>When it detects a meeting app or a meeting window title, idle auto clock-out is paused so active calls are not interrupted.</p>
    </div>
  </section>
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    overflow-y: auto;
    padding-bottom: 20px;
    background:
      radial-gradient(circle at top left, rgba(99, 102, 241, 0.22), transparent 30%),
      radial-gradient(circle at top right, rgba(34, 197, 94, 0.14), transparent 28%),
      #0d0d0f;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid #1e1e24;
    position: sticky;
    top: 0;
    background: rgba(13, 13, 15, 0.94);
    backdrop-filter: blur(12px);
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
  .title-wrap { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .eyebrow { font-size: 10px; text-transform: uppercase; letter-spacing: 0.12em; color: #7c8aa6; }
  .title { font-size: 14px; font-weight: 700; color: #e8e8ec; }
  .version {
    border: 1px solid #283044;
    background: #101521;
    color: #bfdbfe;
    border-radius: 999px;
    padding: 6px 10px;
    font-size: 12px;
    white-space: nowrap;
  }

  .hero { padding: 18px 18px 6px; }
  .hero-copy {
    border: 1px solid #1f2432;
    border-radius: 18px;
    padding: 18px;
    background:
      linear-gradient(135deg, rgba(17, 24, 39, 0.98), rgba(10, 12, 18, 0.98));
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.25);
  }
  .label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: #7c8aa6;
    margin-bottom: 10px;
  }
  h1 {
    font-size: 28px;
    line-height: 1.1;
    margin-bottom: 10px;
    color: #f8fafc;
  }
  .summary {
    color: #a9b2c7;
    line-height: 1.6;
    font-size: 13px;
    max-width: 62ch;
  }

  .card {
    margin: 14px 18px 0;
    padding: 16px;
    border: 1px solid #1f2432;
    border-radius: 16px;
    background: rgba(10, 12, 18, 0.94);
  }
  .card-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #7c8aa6;
    margin-bottom: 12px;
  }
  .meta-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;
  }
  .meta-label {
    display: block;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #5f6b84;
    margin-bottom: 6px;
  }
  .meta-value {
    display: block;
    color: #e8e8ec;
    font-size: 13px;
    font-weight: 600;
    line-height: 1.4;
  }
  .credits {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }
  .credit-row {
    padding: 12px;
    border-radius: 12px;
    background: #0f131d;
    border: 1px solid #1e2433;
  }
  .credit-role {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #7c8aa6;
    margin-bottom: 6px;
  }
  .credit-name {
    color: #f8fafc;
    font-weight: 700;
    font-size: 14px;
    margin-bottom: 4px;
  }
  .credit-detail {
    font-size: 12px;
    color: #a9b2c7;
  }
  .notes-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    color: #a9b2c7;
    line-height: 1.6;
    font-size: 13px;
  }

  @media (max-width: 700px) {
    .meta-grid,
    .credits {
      grid-template-columns: 1fr;
    }
  }
</style>

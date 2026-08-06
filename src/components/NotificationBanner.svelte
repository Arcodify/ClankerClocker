<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { AppNotification } from "../lib/types";

  export let notification: AppNotification;

  const dispatch = createEventDispatcher();

  // Same three-bucket severity the sound picker (audio.rs) uses, so the
  // banner's color always agrees with what the user just heard.
  const SEVERITY: Record<string, "critical" | "warning" | "info"> = {
    idle_clockout: "critical",
    scheduled_clockout: "critical",
    idle_clockout_warning: "warning",
    scheduled_clockout_warning: "warning",
    clock_in_reminder: "info",
    info: "info",
  };
  $: severity = SEVERITY[notification.kind] ?? "info";
</script>

<div class="banner {severity}" on:introend={() => {}}>
  <div class="flash"></div>
  <div class="content">
    <span class="title">{notification.title}</span>
    <span class="body">{notification.body}</span>
  </div>
  <button class="close" on:click={() => dispatch("dismiss")} aria-label="Dismiss">✕</button>
</div>

<style>
  .banner {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 200;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    animation: slide-in 0.25s ease-out;
  }
  @keyframes slide-in {
    from { transform: translateY(-100%); }
    to { transform: translateY(0); }
  }

  /* A few bright pulses on arrival grab the eye without staying loud —
     it decays into the banner's flat resting color. */
  .flash {
    position: absolute;
    inset: 0;
    pointer-events: none;
    animation: pulse 0.5s ease-out 3;
  }
  @keyframes pulse {
    0% { background: rgba(255, 255, 255, 0.55); }
    100% { background: rgba(255, 255, 255, 0); }
  }

  .banner.critical { background: #3a0d0d; }
  .banner.warning { background: #3a2a08; }
  .banner.info { background: #0d2438; }

  .content { flex: 1; display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title {
    font-size: 13px;
    font-weight: 700;
    text-transform: capitalize;
  }
  .banner.critical .title { color: #fca5a5; }
  .banner.warning .title { color: #fcd34d; }
  .banner.info .title { color: #93c5fd; }

  .body {
    font-size: 12px;
    color: #d8d8ec;
    line-height: 1.4;
    white-space: pre-line;
  }

  .close {
    background: none;
    border: none;
    color: #9a9ab0;
    font-size: 13px;
    cursor: pointer;
    padding: 2px 4px;
    line-height: 1;
    flex-shrink: 0;
  }
  .close:hover { color: #e8e8ec; }
</style>

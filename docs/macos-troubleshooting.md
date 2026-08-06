# ClankerClocker macOS install & permission troubleshooting

ClankerClocker is currently distributed **unsigned** (no Apple Developer ID
certificate yet), so macOS Gatekeeper and TCC (the permissions system behind
Accessibility / Automation / Input Monitoring) both need a bit of manual
help. Most of that is automated by the `clanker-fix` command.

## `clanker-fix`

The app installs this command to `~/.local/bin/clanker-fix` automatically
the first time it launches (see `install_clanker_fix_cli` in
`src-tauri/src/lib.rs`), and adds `~/.local/bin` to `PATH` via `~/.zprofile`
if it isn't there already. **Open a new terminal window** after installing
or updating the app for the `PATH` change to take effect the first time.

Run it with no arguments for an interactive menu:

```
$ clanker-fix

ClankerClocker permission troubleshooter
=========================================
  1) Relaunch app (de-quarantine + reopen)
  2) Reset permissions (last resort — asks for admin password)
  3) Open Privacy & Security settings
  4) Exit
Choose an option [1-4]:
```

Or skip the menu with a flag: `clanker-fix --relaunch`, `clanker-fix --reset-tcc`,
`clanker-fix --open-privacy`.

Option 2 (reset permissions) resets ClankerClocker's own TCC grants only —
it does **not** touch Accessibility/Automation/Input-Monitoring permissions
for any other app on the machine, unlike a bare `sudo tccutil reset
Accessibility` (which wipes that permission for every app).

If `clanker-fix` isn't on your `PATH` yet (fresh install, haven't opened a
new terminal), run it from a checkout of this repo instead:
`bash scripts/clanker-fix.sh` or `npm run fix:macos`.

## Manual walkthrough (what `clanker-fix` automates)

1. **Download** the latest ClankerClocker installer from the official source.
2. **Install** — open the installer and drag `ClankerClocker.app` into
   `/Applications`.
3. **Launch** `ClankerClocker.app` from Applications. If it refuses to open,
   continue below.
4. **Remove extended attributes** (clears the quarantine flag macOS attaches
   to downloaded/unsigned apps) — `clanker-fix` option 1, or manually:
   ```bash
   xattr -cr /Applications/ClankerClocker.app
   ```
5. **Launch again** and grant every permission it requests — Accessibility,
   Apple Events (Automation), Input Monitoring, or anything else macOS asks
   for. `clanker-fix` option 3 jumps straight to the relevant Privacy &
   Security panes.
6. **Restart the app**: quit ClankerClocker completely, then open it again.
   Permissions sometimes only take effect after a fresh launch.
7. **If permissions still don't work**: reboot the Mac. This clears TCC's
   in-memory identity-verifier cache. After rebooting, launch ClankerClocker
   once — don't re-sign or reinstall first.
8. **Last resort — reset TCC permissions** — `clanker-fix` option 2, or
   manually (note the bundle ID scoping, which avoids resetting every other
   app's grants too):
   ```bash
   sudo tccutil reset ListenEvent com.arcodify.clankerclocker
   sudo tccutil reset Accessibility com.arcodify.clankerclocker
   sudo tccutil reset AppleEvents com.arcodify.clankerclocker
   ```
9. **Reboot again**, then launch ClankerClocker. macOS should prompt for the
   required permissions again — approve all of them.

## Why this is necessary at all

macOS's TCC database keys permission grants to the app's **code signature**,
not just its bundle ID. Since ClankerClocker isn't signed with a stable
Apple Developer ID certificate yet, every build has a different signature,
so macOS can treat each update as a brand-new, unapproved app — while still
holding a stale record pointing at the old binary, which is why a plain
re-grant in System Settings doesn't always work.

The permanent fix is signing (and notarizing) release builds with a
Developer ID Application certificate, so the signing identity — and
therefore the TCC grants — stay stable across updates. That requires an
Apple Developer Program account and isn't set up yet; `clanker-fix` is the
interim workaround until it is.

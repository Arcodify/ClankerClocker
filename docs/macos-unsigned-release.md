# macOS unsigned release

This project can be distributed unsigned, but macOS will keep treating the app bundle as quarantined if it comes from a browser download or a zip file.

## Build

Build the app bundle first:

```bash
npm run tauri build
```

The macOS app bundle is expected at:

```text
src-tauri/target/release/bundle/macos/ClankerClocker.app
```

## Install once

Use the helper script to copy the app into `~/Applications`, remove quarantine, and launch it:

```bash
npm run install:macos:unsigned
```

Or pass an explicit app bundle path:

```bash
npm run install:macos:unsigned -- /path/to/ClankerClocker.app
```

When replacing an existing unsigned build, the installer offers to reset
ClankerClocker's bundle-scoped TCC entries. Accept the reset so Accessibility,
Input Monitoring, and Automation are requested for the new binary instead of
remaining attached to the previous unsigned binary. For unattended installs,
set `CLANKER_RESET_TCC_ON_UPDATE=1`.

## Notes

- This avoids manually running `xattr -cr` every time.
- It is still an unsigned distribution flow, so macOS may re-quarantine a fresh replacement bundle.
- For input tracking to work, the app still needs Accessibility permission in System Settings -> Privacy & Security -> Accessibility.

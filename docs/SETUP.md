# ZillaOS Setup — first-run experience (OOBE) v0.1

**Status: code in `main`, 2026-10-28. Not yet compiled or verified in VMware/QEMU.**

The first visible screen in the **`vm-demo` ISO** is now Zilla Setup, an
interactive first-run configuration wizard with a dark navy/graphite
background, teal accent, central translucent-looking (flat-color)
surface, step sidebar, progress indicator, large selection cards,
navigation buttons and a software mouse cursor.

> **This is not yet an operating-system installer.** There is no VFS,
> block-device/partition driver, user database, RTC writer or persistent
> configuration. It does not touch any virtual or physical disk. All
> choices live in RAM and reset on reboot. It launches the desktop *in
> the same already-booted kernel*, without creating an OS installation.

## User flow

| Step | Screen | Choice / behavior |
| --- | --- | --- |
| 1 | Welcome / Вітаємо | Branding, safe preview, Enter or Continue |
| 2 | Language / Мова | Ukrainian (default) or English; UI responds immediately |
| 3 | Region / Регіон | Ukraine (default), Poland, United States |
| 4 | Time Zone / Часовий пояс | Europe/Kyiv (default), Europe/Warsaw, America/New_York, UTC |
| 5 | Device / Назва комп'ютера | Default `zilla-pc`; ASCII letters, numbers, dash; 3–24 characters |
| 6 | Appearance / Оформлення | Dark (default) or light theme |
| 7 | Review / Перевірка | Review choices; Enter launches Zilla Desktop |

**Keyboard:** Enter = next/confirm; Escape = back; Tab and arrow keys
cycle choices; Backspace removes hostname characters; alphanumeric keys
and '-' edit the hostname on the device page.

**Mouse:** click option cards, Back or Continue; software cursor shares
the existing QEMU/VMware PS/2 polling driver. If mouse initialization
fails, keyboard navigation is still available.

The interface is rendered with a built-in 5×7 bitmap font. It now
supports the uppercase Ukrainian Cyrillic letters needed by the setup
text as well as Latin/ASCII; glyph quality and international
typography are provisional. Long strings may clip on small screens.
This is not full Unicode shaping or locale/keyboard-layout support yet.

## Important limitations

- **Language:** only Ukrainian/English interface selection, not
  system-wide internationalization. Cyrillic names cannot yet be entered
  in hostnames, consistent with ASCII hostname policy.
- **Time:** timezone preference only. The RTC, calendar clock and
  timezone conversion are not implemented or adjusted; the on-screen
  time is not synced.
- **Name:** hostname preference only; no network stack exists to
  advertise a computer name to a LAN.
- **Credentials:** no account creation, authentication, password
  storage or system encryption.
- **Disk:** no disk discovery, storage selection, partitioning,
  formatting, EFI boot entry writes or installation.
- **Persistence:** all choices lost after VM reset/shutdown. The same
  welcome screen appears on every new `demo` boot until a safe
  configuration service and storage abstraction exist.

## Technical structure

```text
Limine UEFI boot -> CPU/memory checks -> RGB32 framebuffer
    |
Wizard::new(layout)                         src/setup.rs
    |
render_setup()                              src/graphics.rs
    |-- proprietary-free bitmap Cyrillic fallback glyphs
    |-- centered card, sidebar, progress, selectable rows, buttons
    |
Input::initialize()                         src/input.rs
    |-- PS/2 scancodes -> Key::Confirm / Text / Backspace / navigation
    |-- PS/2 mouse    -> click geometry / pointer motion
    |
Wizard::handle_key()/handle_mouse()         pure, host-testable state
    |
Step::Finished -> log volatile settings    src/main.rs
    |
Desktop::with_layout -> apply dark theme    graphical desktop demo
```

Boot logs (serial COM1 in VMware) add:
```text
ZILLAOS_SETUP_READY
ZILLAOS_SETUP_INPUT_POLL
ZILLAOS_SETUP_STEP=0x0000000000000001
...
ZILLAOS_SETUP_STEP=0x0000000000000007
ZILLAOS_SETUP_COMPLETE
ZILLAOS_SETUP_LANGUAGE=uk
ZILLAOS_SETUP_TIMEZONE=Europe/Kyiv
ZILLAOS_SETUP_HOSTNAME=zilla-pc
ZILLAOS_SETUP_VOLATILE_ONLY
ZILLAOS_DESKTOP_READY
```

### QEMU automated check

```bash
git switch main
git pull origin main
cargo test --lib --target x86_64-unknown-linux-gnu --offline
python3 -m unittest discover -s tests -v
bash scripts/build.sh demo
python3 scripts/desktop_smoke.py --timeout 90
```

The updated desktop smoke test:
1. Waits for `ZILLAOS_SETUP_READY`.
2. Saves **`build/logs/desktop-qemu-setup.ppm`** before advancing.
3. Sends **seven Enter presses**, asserting each exact step marker.
4. Requires `ZILLAOS_SETUP_COMPLETE` and Desktop readiness.
5. Continues previous window, mouse, cursor and screenshot tests.

The CI artifact includes setup, desktop, and post-cursor screenshots.
A screenshot or passing host unit test **does not prove** settings
persist across reboot. VMware testing is a separate acceptance gate;
see [VMWARE.md](VMWARE.md).

## Next required milestones

1. Verify Rust compilation, keyboard/mouse flow and real framebuffer
   appearance inside VMware and QEMU. Tweak card spacing/font scaling
   based on actual screenshots, not mockups.
2. Replace the 5×7 bitmap font with a scalable antialiased, Unicode
   capable font subsystem and implement proper Ukrainian/English
   localization across all applications.
3. Add user accounts and passwords **only after** secure storage and
   process-isolation decisions; no plaintext secrets or fake login.
4. Add capability-limited configuration IPC and an on-disk settings
   format with atomic updates once VFS/block-device services exist.
5. Build a real install path (disk discovery, volume plan, confirmations,
   filesystem, boot entries, rollback/recovery) **with explicit data-loss
   warnings** after safe virtual-disk tests. Never autoformat disks
   during preview.

This OOBE is a product UX milestone, not an OS installer release.

# Zilla Task Manager — boot desktop UI and truthful process state

This milestone adds a working task-switching **surface manager** to the
existing early single-core Desktop demo. It is **not** yet a Windows-style
list of independent processes: built-in Files, Terminal, Settings, App Center,
Notepad and Task Manager are embedded inside the kernel's single event loop.

## User-facing behavior

- Windows-style **Z START** button at left side of the full-width taskbar.
  Opens the existing animated app launcher.
- **TASKS** button next to Start for displays >=960px, toggle opens/minimizes
  Task Manager. On narrower monitors use Dock icon `P`, top menu or hotkey.
- Centered macOS-style Dock still shows installed built-in app windows and
  focused/minimized indicators. Task Manager is its sixth app.
- Top menu names now identify their **actual click actions**:
  Apps / Files / Theme / Desktop / Tasks (three languages), sharing one source
  of truth for glyph widths and pointer hit-testing.
- `Ctrl+Shift+Esc` invokes Task Manager (inspired by the standard Windows
  shortcut); `Esc` closes the focused surface.
- Task Manager lists embedded GUI windows and their **real WindowManager
  states** (FOCUSED, VISIBLE, MINIMIZED, CLOSED); clicking a row calls the
  common `ApplicationManager` to focus or restore the window.
- Task Manager shows `KERNEL: RING 0 / BOOT CPU`,
  `SMP OFF | RING3: TEST ISO ONLY`, and
  `CPU / RAM / THREAD METRICS: N/A`. It never invents PIDs, CPU percentages
  or memory counters for embedded windows.
- Microphone has no guest driver; the status says `MIC N/A` or `MIC --`.
  The header time remains an RTC readout, not a verified locale-adjusted clock.
- The cluttered long keyboard-help line at the screen bottom is removed.

## Architecture boundary

```text
ZillaOS bootstrap CPU / Ring 0
  Desktop event loop
    WindowManager
      Files / Terminal / Settings / App Center / Notepad / Task Manager
    ApplicationManager (builtin window targets)
    Taskbar + Start Menu / clickable real actions

Opt-in experimental Ring 3 fixture (separate QEMU ISO)
  own PML4, GDT/TSS, INT 0x80, contained user #PF
  NOT registered as a persistent Desktop process
```

The normal `BuildRun` ISO never enables the Ring 3 experimental feature.
A genuine process view will consume an authenticated, read-only kernel
task-information service only after real process lifecycle, context
switching, signal/fault recovery and CPU scheduling are integrated. A
future `End Task` must terminate only its selected unprivileged process
and never treat a GUI close operation as a process kill.

## Verification

```powershell
cd M:\projects\ZillaOs
git pull --ff-only origin main
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

The CI runs host tests for the click bounds, `Ctrl+Shift+Esc`, task-window
focus/restore, installed registry + paths, no_std release builds and the
existing QEMU GUI regression.

Avoid **Suspend**: fully power down the VMware guest before loading a new
ISO. COM1 can be inspected in
`build\vmware\ZillaOS\zillaos-com1.log`.

## Follow-up milestones

1. Pure-thread model with TIDs, lifecycle, affinity and per-core runqueue
   selection, explicitly without claiming hardware context switching.
2. APIC/x2APIC enumeration and guarded CPU-online bring-up; idle threads.
3. Per-CPU GDT/TSS/IST, timer IRQ and preemptive context switching.
4. Syscall ABI and kernel IPC for real user-space GUI app processes.
5. Live process telemetry, real task termination, thread list and scheduler
   history, then dynamic CPU/memory charts.
6. A separate graphics subsystem milestone for GPU FIFO and real vsync.

## M2 — ThreadTable groundwork

`src/kernel/process/thread.rs` introduces a separate TID namespace,
fixed 64-record thread table, Created/Ready/Running/Blocked/Exited
transitions, no reuse of live TIDs, parent PID checks and executable/stack
mapping checks. It contains a deterministic Round Robin selection cursor
per possible CPU, but defaults to `online_cpu_mask = 1` and rejects all
CPU1+ affinity until a real CPU-online mechanism exists. All transitions
are **metadata-only**. The production ProcessManager exposes
`reserve_thread`, `prepare_thread`, `next_ready_thread`, `thread`,
and `cpu_mask` to prepare for future user task execution.

This does not make GUI built-ins independent processes, start an AP, create
an OS-level thread, enable multitasking or activate kernel preemption.

## RTC date and microphone status

The top bar adds a Gregorian `YYYY-MM-DD` readout from the x86 CMOS RTC
day, month, year, and optional century registers. CMOS is sampled twice and
validated, including leap days. When the optional century field is absent,
it shows `DATE N/A` rather than guessing 20xx. This is **firmware RTC
date**, not a timezone-adjusted calendar or NTP-synchronized value.

Microphone indicators say `MIC N/A` / `MIC --`, because no microphone
input or privacy permission handling is implemented. These status labels
must not be interpreted as audio capture or hardware mute controls.

The graphics CI uses normalized serial lines to tolerate QEMU CRLF and
diagnoses which required markers are missing, while still requiring a
real screenshot-driven boot of the graphical desktop.

## M3 — validated read-only process/thread snapshot

`src/services/task_snapshot.rs` supplies bounded `ProcessInfo` and
`ThreadInfo` records from **real** `ProcessTable` and `ThreadTable`
metadata, counting threads by parent PID. `ProcessManager::snapshot()`
returns this immutable view without owning foreign pointers, executing
code, changing scheduling or manufacturing CPU/RAM metrics. When no
processes are registered, the snapshot is explicitly empty. The UI
currently lists built-in window surfaces rather than populating fake
`ProcessInfo` records. Later the compositor can consume these snapshots
through a permission-checked IPC boundary.

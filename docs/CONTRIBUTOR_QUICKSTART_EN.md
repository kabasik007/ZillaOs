# ZillaOS — English quick start for contributors

ZillaOS is an **experimental, independent x86-64 Rust operating system** with a `no_std` kernel, Limine/UEFI boot, a software-rendered Desktop and research models for processes, IPC and drivers. **It is not Linux or Windows and does not run ordinary Windows/Android apps.** Read the [current capability/evidence status](STATUS.md) before making claims.

> **Legal note:** The source repository is public but does **not yet contain a LICENSE**. Public access is not an open-source grant. Agree on the contribution/rights policy with the maintainer before offering reusable code.

## Choose a first task

Start with [community starter tasks](COMMUNITY_STARTER_TASKS.md) or [open issues](https://github.com/kabasik007/ZillaOs/issues). Documentation, reproducibility reports and pure host-side tests are good starting points. Avoid hardware writes and changes to shared kernel/ABI contracts unless the maintainer approves a spec.

Comment on the issue first to coordinate, then read [CONTRIBUTING](../CONTRIBUTING.md), [AGENTS](../AGENTS.md), [architecture](ARCHITECTURE_CURRENT.md) and [security](SECURITY_ARCHITECTURE.md).

## Prepare a development checkout

Fork the repository in GitHub (when appropriate under the chosen contribution license), then:

```bash
git clone https://github.com/YOUR-USERNAME/ZillaOs.git
cd ZillaOs
git remote add upstream https://github.com/kabasik007/ZillaOs.git
git fetch upstream
git switch -c docs/my-first-contribution upstream/main
```

Use a small task-specific branch. AI agents must follow the `agent/<domain>-im2-<task>` naming/ownership protocol in [AGENTS.md](../AGENTS.md). The project pins Rust **1.90.0**. A Linux/WSL2 development host with Python 3 is expected.

## Verify

Run the checks relevant to your change; these are the established host/bare-metal baseline:

```bash
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
python3 -m unittest discover -s tests -v
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
```

These require dependencies and the configured Rust targets; if a prerequisite is unavailable, report **NOT RUN**, not PASS. Follow [DEVELOPMENT](DEVELOPMENT.md) for QEMU/OVMF/VMware and safety restrictions. Do not run physical-disk writing or PCI/GPU hardware experiments to complete a starter task.

## Open a pull request

Push your topic branch to your fork and open a **draft PR to `kabasik007/ZillaOs:main`**. Explain the issue, exact commit, paths, what changed, negative/failure tests, security/hardware boundaries and PASS / FAIL / NOT RUN results. Use the repository's PR template. The maintainer/Chief Architect reviews shared contracts, and merges only after explicit approval. Please be considerate of other contributors and do not force-update their work.

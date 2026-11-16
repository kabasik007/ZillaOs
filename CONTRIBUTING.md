# Contributing to ZillaOS

Thank you for helping build ZillaOS, an **experimental** independent Rust `no_std` operating system for x86-64.

> **Legal status (important):** The repository is public, but there is currently **no project LICENSE file**. Public visibility does not grant permission to copy, redistribute or reuse code. The maintainers must choose and publish a license before calling ZillaOS open source. Until then, discuss code contributions and their licensing with the maintainer first. Do not import code from another project without a compatible license and explicit attribution.

## Where to start

1. For a practical English walkthrough, start with [contributor quick start](docs/CONTRIBUTOR_QUICKSTART_EN.md). Then read [README.md](README.md), [current status](docs/STATUS.md), [architecture](docs/ARCHITECTURE_CURRENT.md), [security architecture](docs/SECURITY_ARCHITECTURE.md), and [AGENTS.md](AGENTS.md).
2. Browse [open issues](https://github.com/kabasik007/ZillaOs/issues), the [I-M2 workstreams](docs/NEXT_WAVE_I_M2.md), and [community starter tasks](docs/COMMUNITY_STARTER_TASKS.md).
3. Before a non-trivial change, comment on an issue describing the goal, intended files, failure cases and test plan. Ask for the maintainer's agreement for shared architecture or safety-sensitive code.
4. Fork the repository and create a focused branch **from the current `main`**; for automated engineering agents follow the required `agent/<domain>-im2-<task>` convention in AGENTS.md.
5. Make a small change with appropriate tests and documentation. Open a **draft pull request** early; use the PR template and link the issue. Keep the branch up to date without force-pushing over someone else's work.
6. A maintainer/Chief Architect reviews architecture and integration. **No automatic merging into `main`**, and no direct contributor pushes to `main`.

## Contributor boundaries

- Follow [single-writer module ownership](AGENTS.md) and [agent coordination](docs/AGENT_COORDINATION.md). Do not modify shared kernel APIs, `src/main.rs`, `src/lib.rs`, `Cargo.toml`, cross-subsystem interfaces or shared workflows without prior agreement.
- For changes to lifecycle, ABI, IPC, capabilities, memory, driver/resource access or observable behavior, update a [spec](docs/specs/README.md) first; describe invariants, negative tests and non-goals.
- Do not claim that host tests or QEMU fixtures demonstrate production-grade isolation, hardware GPU acceleration, SMP or an active `zillad`.
- Never enable unreviewed physical-disk writes, BAR/MMIO/DMA, GPU/PCI writes, IRQ/firmware loading, host NVRAM writes or privileged execution of untrusted binaries. Device bring-up requires a separate approved disposable-lab plan.
- No secrets, personal information, copyrighted third-party sources without permission, opaque binary blobs or unexplained vendored code.
- Be constructive in issues and reviews; criticize code and evidence, not people.

## Local validation

The established baseline from the repository root on Linux/WSL2 is:

```bash
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
python3 -m unittest discover -s tests -v
```

Additional QEMU / OVMF or feature-specific tests depend on the scope; see [AGENTS.md](AGENTS.md) and [build instructions](docs/DEVELOPMENT.md). Report **PASS / FAIL / NOT RUN** for each relevant check and link CI results on the **exact PR commit**. Never invent test results.

## PR acceptance checklist

A reviewable PR has a clear scope, linked issue/spec if needed, relevant positive and negative tests, risk analysis, docs for behavioral changes and a verification report. The maintainer may request changes, pause a high-risk proposal or decline it. Attribution belongs to each contributor. A future license decision will be announced explicitly rather than silently assumed.

Security-sensitive vulnerabilities should not be published with weaponizable details in a public issue. Check GitHub's private vulnerability reporting option if available and contact a maintainer privately before public disclosure.

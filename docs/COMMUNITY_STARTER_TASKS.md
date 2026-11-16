# ZillaOS — Community starter tasks

This is a **contributor-oriented task map**, not a claim that the OS is production-ready. Please comment on the linked issue before starting and read [CONTRIBUTING.md](../CONTRIBUTING.md).

## Recommended first contributions

| Area | Low-risk first step | Source of truth | Evidence to provide |
| --- | --- | --- | --- |
| Documentation / reproducibility | Reproduce and document the existing QEMU/UEFI quick-start without changing boot code | [Boot issue #1](https://github.com/kabasik007/ZillaOs/issues/1), [development](DEVELOPMENT.md) | Exact environment, commands and outcomes |
| Desktop regression | Add a deterministic host-side regression or document reproducible QEMU keyboard/mouse behavior | [Desktop issue #3](https://github.com/kabasik007/ZillaOs/issues/3), [status](STATUS.md) | Test/logs and clearly scoped assertions |
| Process / capabilities | Add negative model tests for stale handles and denied rights, without expanding runtime privileges | [Integration issue #21](https://github.com/kabasik007/ZillaOs/issues/21) | New test, expected failure, exact host checks |
| Device inventory | Strengthen read-only snapshots / bounds tests with no device writes | [Mission control #25](https://github.com/kabasik007/ZillaOs/issues/25), [ownership](AGENT_COORDINATION.md) | Pure model test and safety note |
| Documentation accuracy | Find and fix a stale status/README claim with source-backed evidence | [Status](STATUS.md), [evidence matrix](verification/EVIDENCE_MATRIX.md) | Exact changed claim and source commit |

**Not beginner tasks:** physical-disk installation, GPU MMIO/DMA, untrusted EXE execution, kernel privilege boundary, raw firmware changes, and changes to shared process/IPC/device contracts. These require architecture-owner review and a separate lab/test plan.

## How to ask for a task

Open an issue with the smallest observable goal, suggested files, test commands, and scope exclusions. Maintainers can label/assign it as appropriate. Do not assume any item is unclaimed: check open PRs and current `main`.

**Project license is still undecided.** Before submitting redistributable code, first align with the maintainer on the license/rights policy.

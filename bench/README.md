# ibde-bench

Floor-profile performance harness. Definitions and budgets: ADR-0002.

- `vm/fetch-image.sh` — download Debian 13 generic qcow2 (once)
- `vm/user-data`, `vm/meta-data` — NoCloud seed (served over user-net)
- `vm/run-bench.sh` — boot QEMU @ 1 vCPU / 512 MB, capture serial.log,
  print results
- `vm/parse.sh` — extract measurements between markers

Current baseline report: `docs/bench/BASELINE.md`.

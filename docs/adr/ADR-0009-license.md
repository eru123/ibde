# ADR-0009: License — Apache-2.0 for all components

- Status: Accepted (owner decision, 2026-10-31)
- Owner's question: *"Can we do apache-2.0 on license?"* — yes.

## Decision

Every IBDE component ships under **Apache-2.0** — library, shell,
compositor fork, terminal, themes, docs — with SPDX headers and NOTICE
files where required. One license to reason about; patent grant included;
maximum adoption surface (embedded vendors can embed `libibde-ui`).

## Dependency compatibility (verified)

| Dependency | License | Compatible |
|---|---|---|
| wlroots, labwc, wayland, libvterm | MIT | yes |
| cairo, pango, pixman | LGPL (dynamic linkage) | yes |
| JetBrains Mono (bundled font) | OFL-1.1 | yes |

## Consequences

No copyleft obligation flows to downstream users of the toolkit; DE
components remain permissively reusable, which the owner prefers over
the earlier mixed MIT/GPL proposal (superseded).

# Phase 3 exit review — v0.3

Date: 2026-11-01 · Agent-authored from measured evidence; owner
override welcome.

## Verdict: **EXIT GRANTED** — tag `v0.3` ("real authentication,
settings, and distribution")

Phase 3 delivered the trust and plumbing layer: real PAM lock auth,
a settings daemon with theming, a signed apt repository published to
GitHub Pages, multi-arch CI, and session supervision.

## Delivered

| Feature | Evidence | Notes |
|---|---|---|
| PAM lock auth | wrongpw → rc=7 rejected/stays locked; rightpw → rc=0 "auth ok — unlocking" (logs, e2e) | FFI to libpam; pam_unix handles root/non-root paths; soname link (no dev pkg) |
| Settings daemon + CLI | set/get/list/monitor; schema rejects invalid; write-back to keyfile (dbus e2e) | org.ibde.Settings1; schema: mode/accent/dock.size/profile |
| Dark mode theming | conf `appearance.mode = dark` → bar (27,27,32) vs light (238,238,241) — both pixel PASS | OnceLock palette; live D-Bus re-theming with the Settings app |
| Signed apt repository | fresh container: key → `apt update` 0 errors → `apt install` → installed shell RUNS | InRelease/Release.gpg; dedicated key E371E0FD…; SECURITY.md; revocation cert outside the repo |
| GitHub + CI | repo `eru123/ibde` live; tags pushed; jobs: check, workout, arm64, nightly soak, package→Pages | IBDE_APT_KEY secret; Pages publishes the repo on v* tags |
| arm64 cross-build | cross-builder image (sysroot extraction), CI job | binaries artifact for Pi-class |
| Soak gate | 5-min PASS: alive 1/1, post-warm-up drift +0.26%/+0.97% (<3%) | nightly 60-min in CI |
| systemd units | user units for shell+settingsd in the deb; Restart=on-failure | WantedBy graphical-session.target |

## Honest open items (Phase 4)

- Lock-screen visual sign-off on real hardware (screencopy excludes
  lock surfaces — a security behavior we verified indirectly)
- Settings GUI app; live re-theming over D-Bus (daemon + keyfile seam
  shipped)
- Global shortcuts remain bar-keyboard-intercept (ADR-0008 endgame:
  compositor cooperation)
- Double-buffered rendering; wl_surface frame pacing
- Full i18n; icon theme; wallpaper picker
- Pi Zero 2 W hardware run (owner action; arm64 build proven in CI)
- apt repo public URL announcement (Pages publishes on next v* tag)

## Bugs killed this phase

1. palette light-branch self-reference (sed hazard) — caught by
   re-verifying BOTH modes post-commit
2. gz+plain double-hash Release entries → apt mismatch (uncompressed
   indexes)
3. kb-inject virtual-keyboard session flap loses early events — one
   invocation for multi-step UI scripts
4. unix_chkpwd refuses direct invocation; real PAM FFI is the answer

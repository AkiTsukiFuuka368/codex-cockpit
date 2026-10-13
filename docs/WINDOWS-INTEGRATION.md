# Windows layout integration

This update builds on the latest community/platform integration, retaining its
native device identity, pricing, sync behavior, fonts, icons and dependency locks.
The existing Windows legacy-session migration and deduplication fix is retained.

- Usage donut numbers and captions reuse actual font measurement to stay inside
  the ring. Text, font and size changes trigger a new fit; values are not truncated.
- Long trend peak labels occupy their own row above the plot and can wrap inside
  the card instead of extending beyond its left edge.
- Regression tests cover the full long task readout at 100%, 150% and 200%, and
  keep unknown cost distinct from zero.

The same layout changes were checked at those text sizes in an earlier native
Windows test build. That does not constitute native acceptance of a new binary
built from this final source integration. A future release still needs its own
Windows packaging, Defender scan and native acceptance.

Windows source validation: 551 frontend tests passed; Rust formatting, tests,
check and strict clippy passed. Frontend build, version consistency and public
source/history privacy scans passed. No dependencies or release version changed.

This is a source-only update: no version tag, Release asset, updater manifest or
server deployment is created. The desktop release workflow runs on version tags
or explicit manual dispatch; a main-branch update does not publish a release.

# Visual insights

A correspondence triage desk: open-message age distribution, status register, site workload and daily arrival calendar. Every visual links to a real filtered inbox. Rolling age is based on received time, not an SLA or response-time measure.

Use existing React, React Router, CSS and SVG; inspected the existing query, hooks and routing APIs. No chart dependency is needed. Keep useful existing pages and administration workflows.

- [x] Add matching API aggregates and filters with tenant isolation.
- [x] Test filter boundaries, invalid values, counts and CSV consistency.
- [x] Build responsive interactive visual components, empty/loading/error states.
- [x] Verify full Rust/frontend checks and actual Chrome desktop/mobile/dark workflows.
- [x] Refresh README, editable cover and screenshots.

## Interaction and accessibility

All drilldowns are ordinary links with shareable query parameters. Chart values are available as text and accessible names, not color alone. Focus indicators are visible; mobile layouts retain the controls; reduced-motion preferences disable decorative transitions. Empty and failed requests explain the next action and provide a retry button. Existing setup, login, theme, administration and data workflows remain available.

## Validation

- `cargo fmt --check`, `CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings`, `cargo test`: 26 unit + 16 integration tests passed.
- `pnpm test`: 12 tests passed; `pnpm build` passed.
- The new API integration test first failed on the missing `open_age` aggregate, then passed with matching queue/CSV counts, oldest-first ordering, invalid parameter rejection and member visibility. A fixed-clock SQL test covers both sides of the 24-hour and 7-day boundaries.
- `docs/smoke-test.sh` passed against a fresh temporary database and the native binary.
- `docs/verify-insights.py` passed in actual Chrome: all three age queues and CSV counts; calendar date drilldown; desktop/mobile and light/dark operation with no page errors or horizontal overflow.
- `docs/capture-screenshots.py` refreshed all README images from an isolated seeded demo, including the complete desktop and mobile worktable.

## Data definitions

`open_age` counts only `new` and `in_progress`: fresh < 24 hours, waiting ≥ 24 hours and < 7 days, overdue ≥ 7 days. The API key `overdue` is an age bucket, not a promised response deadline. Both list and export use the same predicate and validated sort. The status register includes all non-spam messages, while its center reports open messages. Site bars compare absolute non-spam volumes against the largest site. The arrival calendar uses the browser's local days; rolling age uses UTC elapsed time. Counts can change as live time advances or records change.

## Reproduction and scope

Use a new data directory with the existing `demo` command; never seed or reset a production database. With Playwright and Chrome installed, run `DEMO_URL=... DEMO_PASSWORD=... python docs/verify-insights.py`, then the capture script with the same environment. A synthetic demo account is required; credentials are supplied through environment variables and are not committed.

No new packages, schema migration, external analytics, delivery integration or network service was added. This iteration verified native application workflows, not Docker image rebuilding or external SMTP/webhook delivery. Hosted CI was unavailable during the preceding iteration because of an account billing/spending-limit gate; local evidence is recorded here rather than claiming hosted checks passed.

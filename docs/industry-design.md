# Correspondence Desk

The original questionnaire application organizes a feedback message by sender, company, site, received date and extra fields. Its administration includes site membership, date filters, search and export. The redesign keeps those working concepts visible in a mailroom: incoming letters, routing tabs, receipt stamps and a separate team processing record.

The three envelopes are ordinary links into the existing age queues. Their counts come from `stats.open_age`; opening one preserves the age, open-status and oldest-first filters used by the list and CSV export. A postmark reports `new + in_progress`, and the status register opens each actual status. The date stamps and site ledger keep their original API definitions. Received age is not a response deadline or SLA.

The inbox remains a practical reading surface with sender details, a two-line message preview, site routing, status, date, selection and CSV. The individual letter shows its ID and received date above the original message; status controls and internal notes retain their existing endpoints and permissions. No external notification action was added.

Native CSS paper, envelope folds and postmarks provide the visual identity. The existing self-hosted Noto Sans TC font is paired with native serif display fonts; no dependency or external asset host was added. Dark paper tokens, visible whole-envelope keyboard focus, mobile layouts and reduced-motion behavior are included.

## Validation

- Frontend: 12 tests; production build.
- Rust: 26 unit and 16 integration tests; fmt and clippy with warnings denied.
- Fresh temporary database: complete existing API smoke script.
- Actual Chrome: age queue/API/CSV count equality, chronological order, date drilldown, keyboard envelope activation, desktop/mobile and both themes.
- Actual Chrome: detail status change and restore, internal note create and delete on isolated synthetic data.
- Every README UI image is captured anew from isolated demo data. Each image uses a fresh browser document, final viewport, loaded fonts and settled paint; the editable SVG cover was redrawn to match the envelopes.

No backend, schema, permission, delivery or notification behavior changed. Docker image rebuilding and external SMTP/webhook delivery are outside this visual iteration's validation.

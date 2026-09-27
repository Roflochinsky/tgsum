# Connector review: <platform / acquisition method>

Copy into a dated file in `docs/research/`; link it from Beads and the connector
registry. This template is an evidence record, not a completed review. Keep task
status and work assignments in Beads. Follow [review-policy.md](review-policy.md).

## Scope

- Connector/profile ID and Beads issue:
- Review owner:
- Attempted at (UTC), reviewer and trigger/change:
- Proposed implementation revision, API/SDK/client version, OS and architecture:
- Account/tenant/app-distribution type:
- Selected conversations vs authorization scope:
- Bootstrap, refresh, attachments and retention:
- AI handoff (local/cloud inference/training distinguished):
- Existing support and precisely proposed support claim:

## Primary sources

| Claim/question | Primary URL | Accessed at | API/client/commit version | Outcome: read / unavailable / changed | Fact and section |
| --- | --- | --- | --- | --- | --- |
| Archive/API capability | | | | | |
| Authorization and distribution | | | | | |
| Limits, pagination and retry | | | | | |
| Storage, content and AI restrictions | | | | | |
| OS/client automation contract | | | | | |

Separate documented facts, engineering inferences and unknowns. Record the
reason for an unavailable source and the unresolved claim. An HTTP 200, a valid
URL or the existence of an API method is insufficient evidence of support.
Do not replace missing primary evidence with an assumed permission or a
secondary summary. Source retrieval itself must not authenticate a real account.

## Acquisition contract

- Official acquisition method and exact scope boundary:
- Credentials owner/storage; what TGSUM can read:
- Grant/scopes/admin consent, app registration/distribution requirements:
- Pagination/cursor semantics, quotas, backoff and cancellation:
- Completion signal vs partial/in-progress export; timeout and user action:
- Stable IDs, account namespace, ordering, edited/deleted/missing semantics:
- Coverage bounds, attachment references/bytes and path validation:
- Retry, expired/revoked credentials, offline gap and data removal:
- GUI session/lock/accessibility requirements and manual fallback:
- Technical account risks and platform constraints affecting the integration:
- Unknown/unverified points and how they affect the support claim:

## Verification

| Assertion | Fixture or simulator | Command/run ID and source revision | OS/client version | Observed outcome | Evidence path |
| --- | --- | --- | --- | --- | --- |
| | | | | | |

State which boundary was simulated. Format-contract checks, synthetic-client
checks and real-client checks have distinct `qualifications.scope` values.
Do not label synthetic fixtures as user-account evidence. Real account/client
tests require the user's control. Newly discovered steps needing the user go
to backlog with the exact action and affected release; this does not qualify
the deferred scope.

## Decision and registry update

- Decision: keep / narrow / implement / defer / exclude; reasons:
- Exact operations supported by registered code and compatible versions:
- Qualification evidence, successful checks and remaining limits:
- Platform capability vs implemented TGSUM capability vs snapshot coverage:
- Changes needed to UI technical claims or release documentation:
- Unresolved questions and linked Beads work:
- Last **successful** policy review date:
- Next review due: at most 30 days for network/client automation, 90 for file/local:

Advance `last_policy_reviewed_at` only after the required primary sources were
actually reviewed. If a required source is unavailable, record this attempt and
keep the last successful date and its deadline. Restore access or narrow the
proposed claim with evidence; never reset the clock to make a release check pass.
The inventory validator detects evidence/date inconsistencies, but cannot
establish the truth of prose or the platform's permission for a use case.

Record the decision/revision/checks/next action in Beads and update the registry
together. Review expiry prompts maintainer release work; it does not adjudicate
the user's rights to content, disable local import or remove stored data.

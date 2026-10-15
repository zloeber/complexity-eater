export const SAMPLE_TITLE = "Reliability proposal: checkout retries for Q4";

export const SAMPLE_MARKDOWN = `# Reliability proposal: checkout retries for Q4

Prepared for Jordan Hale by Avery Chen's agent. This is the manager's proposal. It is not a task list Jordan has already accepted.

## Why this is on your desk

Checkout fails closed when the payment provider times out. Support saw 46 tickets last week. The spinner ends in "try again later" and no receipt is issued. Finance will not accept a retry that can double-charge.

Avery wants a decision this week on whether engineering takes a bounded retry project in the current cycle. The cycle ends on 31 October 2026. A slip past 7 November misses the holiday freeze.

## What Avery wants you to understand

- The customer-visible failure is a lost receipt, not a slow page.
- A retry without an idempotency key is a finance incident, not a reliability fix.
- The 7 November holiday freeze is the date that matters. Detail that does not move that date can wait.
- Avery wants the reply to lead with the decision, the date, and the single blocking question.

## Decisions needed

- Decide whether the retry budget is 2 attempts or 3.
- Decide who signs the "no double charge" acceptance test: payments or checkout.
- Decide if the 7 November holiday freeze is a hard stop or a target.

## Action items

- [ ] Draft the acceptance criteria for a safe retry before estimating the work.
- [ ] Name the owner for the provider timeout metric.
- [ ] Confirm the dependency on the payments idempotency key, which is not shipped.
- [ ] Write the customer-facing copy for a delayed receipt. Product has not supplied it.

## Assumptions

- Assuming the payments team can review an idempotency design within five business days.
- Assuming the 46 support tickets are the same failure mode. The export was not attached.
- Assuming wallet payments are out of this cycle.

## Dependencies

- Depends on the payments idempotency key. It is on their board and not scheduled.
- Depends on a product decision for the delayed-receipt copy.
- The status-page vendor change is out of scope, but the rollback plan assumes that vendor stays as it is.

## Open questions

- Is a partial rollout to 10 percent acceptable, or does finance want this dark until the acceptance test passes?
- Which clock defines the 7 November freeze: UTC, or the support team's local date?
- Should Jordan estimate the work before the attempt-count decision, or is that estimate blocked?

## Out of scope

- Rewriting the checkout service.
- Changing the payment provider.
- A general retry framework for every outbound call.

## Notes from Avery's agent

Avery's working style is decisions, dates, and one blocking question. Jordan's agent should not inherit Avery's private notes, calendar, or finance thread. Those were not included in this handoff. The work below is the whole authorized context.
`;

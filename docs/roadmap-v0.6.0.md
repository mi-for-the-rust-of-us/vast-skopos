# Roadmap: 0.6.0, a first draft

**Not affiliated with vast.ai.** This page collects candidates for 0.6.0, started on 2026-11-08 before
0.5.0 was released. It is a draft, not a plan yet, and not a promise: what ships is what the
[CHANGELOG](../CHANGELOG.md) says. The earlier roadmaps ([0.2.0](roadmap-v0.2.0.md),
[0.3.0](roadmap-v0.3.0.md), [0.4.0](roadmap-v0.4.0.md), [0.5.0](roadmap-v0.5.0.md)) are kept as they were
written.

## Candidates

### 1. What a box costs now, stopped boxes included

**What a rental showed (2026-11-08, an RTX 3070 Ti with a 200 GB disk, box 59725087).** The box's record
in the API (`GET /instances/{id}/`), saved while it ran and again once it was stopped:

| Field | Running | Stopped |
|---|---|---|
| `actual_status` | `running` | `exited` |
| `dph_base` (the GPU) | 0.186667 | 0.186667 |
| `storage_total_cost` (the disk: 200 GB at $0.20/GB/month over 720 h) | 0.055556 | 0.055556 |
| `dph_total` (the sum) | 0.242222 | 0.242222, unchanged |
| `instance.gpuCostPerHour` | 0.186667 | 0 |
| `instance.totalHour` | 0.242222 | 0.055556, the disk alone |

The console's Price Breakdown said the same before the rent: GPU $0.187/hr + disk $0.056/hr = $0.243/hr,
the internet billed per TB on top. So `dph_total` is the box's rental price, whatever its state, and the
`instance` object is what the box costs now. vast.ai's API reference documents neither the object's
fields nor this difference.

**What it means for `vas` 0.5.0:**

- **The burn leaves out stopped boxes,** which still cost their disk. The money line's runway is too
  long for an account that keeps stopped boxes, and the docs say so without a number.
- **`COST~` is `dph_total` times the box's age,** so it counts the GPU for every hour a box was stopped.

**The candidate:** read `instance.totalHour` (falling back to `dph_total` when it is missing or not a
number, as an undocumented field can be) for the burn, and count each stopped box's disk. The money line
would say how much of the burn is disks (`burn $0.30/h over 1 running + 2 stopped`, say).

**Open questions:**

- Does the credit fall at `instance.totalHour`, stopped and running? The rental's `--credit-log` curve
  answers it: one hour running, then one hour stopped (results to come).
- `COST~`: today's rate times the age stays wrong for a box that changed state. Is there a figure for the
  cost so far (an endpoint of charges per instance), or should `COST~` say it assumes the box always ran?
- What does `instance` hold for a box `loading`, `offline` or being created? The burn's `unsure` boxes
  depend on the answer.
- `--compact`'s `dph` key: keep it as `dph_total`, and add the current rate as a key of its own?

### Still open from earlier versions

These are to be seen at a rental when they turn up, as listed in the [0.5.0 plan](roadmap-v0.5.0.md#not-in-050):
`UNREACHABLE` and the direct-route fallback on a real refusing proxy; `100%?` on an idle box; the
`<575`-style mark on a board below its reference; note 4 of the 2026-10-23 report.

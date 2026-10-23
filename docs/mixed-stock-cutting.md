# One cutting job for roll lining and finite hides or remnants

`ftx-cutting-job/1` allocates a complete garment across both existing cutting planners. Every source
piece has one explicit route. The roll planner handles source-qualified rectangular stock; the finite
planner handles individually identified hides, sheets and remnants with their existing defects, prior
cuts and placement regions. Both keep the original pattern and instance identities. The job publishes
a plan only after every physical instance and complete-garment copy appears exactly once.

This supports, for example, leather shell panels cut from individually identified hides together with
lining cut from a roll. The whole-pattern roll planner correctly refuses hide material, and the
whole-pattern finite planner correctly refuses lining without matching finite inventory. A mixed job
uses their actual geometry implementations on explicitly scoped original pieces, without deleting
pieces, manufacturing a partial garment, substituting rectangle area for a hide, or changing the source.

## Request and replay

```json
{
  "schema": "ftx-cutting-job/1",
  "routes": [
    {"piece": "shell", "stock": "finite_stock"},
    {"piece": "lining", "stock": "roll"}
  ],
  "roll": {
    "copies": 2, "gap_mm": 2, "edge_margin_mm": 2, "end_margin_mm": 2
  },
  "finite_stock": {
    "schema": "ftx-finite-stock-request/1",
    "stocks": [],
    "options": {
      "copies": 2, "gap_mm": 2, "boundary_margin_mm": 2,
      "step_mm": 2, "work_limit": 1000000
    }
  }
}
```

This schema sketch needs real source-qualified `stocks`; the complete synthetic runnable documents are
[pattern.json](../examples/mixed-cutting/pattern.json) and [study.json](../examples/mixed-cutting/study.json).
The example has a mirrored shell pair and an unsimulated lining cut on the fold. Two garment copies
produce six physical cut parts. Its numerical dimensions, density and stock outlines are illustrative,
not measurements or a physically qualified garment.

For a material study, place the job in `cutting_job`. This is exclusive with the separate `cutting`,
`finite_stock` and `purchasing` study fields, avoiding two overlapping manufacturing inventories.
Existing sewing, mass, vapor and physical scenario consumers remain independently selectable. Each
material alternative starts with the same inventory and calls the complete job afresh. A material change
must still match a qualified stock snapshot; a new material label does not relabel an old physical hide.

```sh
ftx-materials study examples/mixed-cutting/pattern.json examples/mixed-cutting/study.json

# With the cutting_job object saved as job.json:
ftx-materials cut-job examples/mixed-cutting/pattern.json job.json
ftx materials cut-job examples/mixed-cutting/pattern.json job.json --json
```

The source-piece routes must be complete, unique and exact. Use original IDs, including namespaced
IDs such as `jacket::front` in a composed outfit. A route expands all of that piece's mirrors, quantities,
fold unfolding and garment copies. `sim:false` does not remove a physical cut piece. Supply planner
options exactly for the routes used; roll-only and finite-only jobs are allowed. Both routes must agree
on garment copies and the effective export seam allowance. Grain permissions, finite-stock region
constraints and material/estimate policies keep their existing exact-source requirements. The combined
job is limited to 4096 expanded physical parts, not 4096 for each route separately. Study finite-search
budgets are bounded across all alternatives.

The result contains the canonical `roll` and `finite_stock` subplans, an explicit complete `inventory`,
and the whole frozen `pattern_json`. Each subplan's hash still names that whole pattern; its
`source_piece_ids` identifies the route's scope. Retain the job's exact `replay_request_json` string and
`pattern_json` string to replay through `cut-job`. A scoped finite subplan's inner stock request alone
does not encode the routing and must not be replayed as a whole-pattern request.

## Material substitution can change the stock route

Use `ftx-cutting-job/2` when the same source piece may be cut from a roll for one material and from
an individually inventoried hide or remnant for another. Declare exact material snapshots and their
permitted routes in `material_routes`. The ordinary material resolver selects the route after each
assignment; neither fiber labels nor the existence of nearby inventory selects a route.

```json
{
  "schema": "ftx-cutting-job/2",
  "routes": [{"piece": "lining", "stock": "roll"}],
  "material_routes": [{
    "piece": "shell",
    "choices": [
      {"material_sha256": "<exact resolved linen SHA-256>", "stock": "roll"},
      {"material_sha256": "<exact resolved leather SHA-256>", "stock": "finite_stock"},
      {"material_sha256": "<exact resolved knit SHA-256>", "stock": "roll"}
    ]
  }],
  "roll": {"copies": 1, "gap_mm": 2, "edge_margin_mm": 2, "end_margin_mm": 2},
  "finite_stock": {
    "schema": "ftx-finite-stock-request/1",
    "stocks": [],
    "options": {
      "copies": 1, "gap_mm": 2, "boundary_margin_mm": 2,
      "step_mm": 2, "work_limit": 1000000
    }
  }
}
```

Replace the placeholders with exact lowercase resolver hashes and the stock array with actual
source-locked physical items. Each source piece must occur exactly once across `routes` and
`material_routes`; a material rule needs nonempty, unique snapshot choices. An unlisted revision
refuses even when its material ID or fiber name is unchanged. The complete request permits at most
4,096 rules and 4,096 material choices. Fixed routes continue to express an intentional route for
every subsequently assigned material; their ordinary planner still checks its supported stock form.

Provide planner options for every route used by any declared alternative, including currently dormant
ones. Copies and effective seam allowance must agree across both planners before any allocation.
Only the selected planners run. A roll-only alternative does not manufacture a finite-stock plan,
charge an unused hide or require that hide to fit the roll garment. Finite material selections are
frozen for replay even while dormant; invalid material locks or duplicate physical stock IDs refuse.
The selected planner remains responsible for geometric and process-option validation. A successful
roll alternative makes no claim that a dormant finite-stock allocation is feasible.

In `/2`, each planner's `grain_reversal_materials` is an explicit permission pool over the snapshots
declared for that planner. Unknown or duplicate permissions refuse. Before calling the ordinary
planner, the job selects exactly the permitted hashes used on that route by the current alternative.
This supports a study containing several independently authorized nap/print orientations without
granting reversal to a replacement lot. Finite `piece_zones` likewise applies only when that piece
takes a finite route; its keys must name pieces having at least one declared finite alternative.
The standalone roll/finite planners and `/1` job retain their strict existing permission behavior.

The `/2` plan adds `resolved_routes`, sorted by source-piece ID, with each selected `stock` and
`material_sha256`. Its `request` and exact `replay_request_json` retain the complete choice and
permission pools, while each canonical subplan retains the selected scope. Material studies require
no extra per-variant override: place this job in the existing `cutting_job` field and change only
material assignments. The same whole-pattern inventory, source locks, purchased marker dimensions,
finite-item pricing and comparison gates are recomputed for every alternative.

## Complete purchasing from the same market

Optional job `purchasing` contains the existing `quotes` book, one `market`, and `finite_bindings` using
the existing exact-stock `stock_id`, `stock_sha256` and `offer_ids` contract. The job's
`finite_stock_identities` supplies those canonical physical identities, including the whole stock area,
exact material hash and whether the item was used. Follow
[finite-stock purchasing](cutting-plans.md) for the required indivisible-item quote normalization.

Roll procurement uses actual gross markers, summed per material into whole orders with their existing
minimums, increments and longest continuous-cut checks. Finite procurement buys each used physical item
once, using its complete polygon area and exact stock lock, including defects and prior cuts. An offer
explicitly bound to a physical finite item is excluded from roll procurement even when both routes use
the same material snapshot: one remnant cannot also become a second roll supply.

The job's purchasing report retains both canonical outcomes and their source exclusions. Missing,
stale, unavailable or mismatched prices do not erase geometric allocation. The complete total remains
unknown unless every used roll material and finite item has an admitted offer in the common market.
The selected total does not pool suppliers' inventory, infer shipping discounts, reserve stock or
authorize a purchase. All alternatives use the same dated inputs; no wall-clock source refresh occurs
inside a solve.

For `/2` alternatives with no engaged finite route, the finite purchasing outcome is `not_requested`.
Its dormant bindings remain in the replay request and their offers remain excluded from roll supply.
Only engaged orders contribute to the complete total; inactive inventory is neither charged nor
declared price-qualified. When a later material choice engages finite stock, its complete ordinary
stock-binding and offer checks apply again.
Dormancy does not bypass the canonical binding, offer-link and quote budgets or namespace checks;
malformed shared purchasing inputs preserve geometric allocation but cannot publish a complete cost.

For finite-stock `minimize_purchase_cost`, top-level job purchasing supplies the canonical finite
pricing request before geometry search. If the finite request already contains pricing, it must be
identical. Search limits and incumbent/optimality reporting retain the ordinary finite planner's
meaning. Unknown prices cannot act as free candidate stock. Other geometric searches may be priced
after allocation without pretending that their selected allocation minimizes cost.

## Study objectives and interpretation

The study exposes `cutting_job_external_cut_area_m2`, `cutting_job_stock_area_m2`,
`cutting_job_stock_mass_kg` and `cutting_job_observed_order_cost`. Required consumers are `cutting_job`
for complete geometry and `cutting_job_purchasing` for a complete observed order total. Cost uses the
job's declared market currency. Nominal outputs are not propagated confidence intervals.

`stock_area_m2` is gross roll area plus the whole area of used finite items. It includes waste, defects
and previously occupied portions of engaged inventory; it is not newly consumed area. New external
cutter area is reported separately. `stock_mass_kg` is available only when every used route has admitted
mass evidence. These scopes differ from retained garment assembly mass, which remains a separate
consumer.

Synthetic known-area and whole-order answers, exact source/copy coverage, source-preserving replay,
canonical planner/procurement equality, material-substitution refusals and legacy planner regressions
check this software connection. No new empirical textile parameters, process qualification, machine
frames, physical folding/sewing feasibility, garment fit or hardware execution follow from the plan.

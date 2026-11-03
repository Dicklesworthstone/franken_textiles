# Offline textile catalog and frozen material selections

The domain types and physics contracts remain `ftx-pattern::materials` and
`docs/spec/06-textile-materials.md`. `ftx-pattern::material_catalog` adds the
bundled data, dimensional normalization and immutable selection receipts; it does
not introduce a competing material or constitutive model.

`Catalog::builtin()` loads the canonical source tranches (432 records after the
2026-11-03 Wang2011 addition); `textile_import::catalog()` also imports 11 legacy
observations into that same authority, for 443 discoverable records. The initial
seed contains six product/composition references and 24 family identities. The family
records deliberately contain no guessed mechanics. The seed includes linen,
cotton muslin, silk charmeuse, garment cowhide, DreamKnit composition and APEX
insulation, plus wool, silk, nylon, polyester, blends, bamboo-derived viscose,
lyocell, felt, nonwovens, down and other family IDs. It is a starter corpus, not
complete coverage or laboratory validation.

A `Selection` names an exact ID, optional frozen `record`, source-bearing
`overrides`, additional `sources`, and `allow_estimates` (false by default).
Overrides cannot rebind an existing source ID. `Catalog::resolve` returns an
immutable `ResolvedMaterial` containing the authored data and a normalized copy
of the **same** `Material` type. Mechanics and thermal resistance both use its
selected evidence policy. Its SHA-256 binds the values, original units, bounds,
source metadata, test conditions, policy and normalized result.

`ResolvedMaterial::frozen_selection()` embeds the authored record and a digest
lock. A saved pattern therefore does not depend on a future catalog release or
network lookup. Changing a locked record without deliberately updating/removing
the lock is refused, not silently accepted. Hashes establish identity, not trust
in the author of the measurements.

An embedded selection enters `textile_import::resolve_fabric` without rebuilding
the unrelated bundled catalog. The same canonical resolver still validates the
record, overrides, source bindings, units, evidence policy and digest. Unknown or
malformed embedded records never fall back to a similarly named catalog entry.
Identifier-only selection continues to load the merged catalog, and explicit
`Catalog::resolve` callers still validate their supplied collection.

A 2026-11-03 focused debug measurement compared the same 32-piece binding/freeze
workflow before and after this change. Five warmed repetitions on a shared AMD
EPYC 9V74 host with an eight-CPU quota and pinned Rust 1.100 nightly gave medians
of 2.053665 s and 0.134182 s, respectively; ranges were 1.4790–2.3916 s and
0.09217–0.18694 s. Complete report SHA-256 was identical:
`3424c63fa9bcc44d49ef24f71edaa4a3e3e2977f2f9848ad94f56456648a2130`.
This measures that debug workload on a busy host, not release solver performance.

The normalization registry accepts explicit conversions such as g/m2 and
avoirdupois oz/yd2 to kg/m2, mm/inches to metres, kN/m to N/m, tex/denier to kg/m,
thread counts per inch to per metre, and clo/tog to m2*K/W. Constituent Young's
modulus in Pa cannot become sheet stiffness in N/m without an explicit validated
model. Bounds convert with their values; authored observations remain intact.
Unknown extension properties are preserved, not automatically used by physics.

Supplier intervals without a nominal remain sourced `structure` observations,
not fabricated midpoints. This matters for the muslin's reported areal-weight
range and the leather's thickness range. They require an explicit specimen value
before the corresponding physics can run. The silk supplier's `mm` means momme,
not millimetres; no thickness is inferred. APEX's ambiguous thermal-efficiency
unit in the retrieved source is not interpreted as absolute clo. DreamKnit is a
composition reference, not a raw-fabric SKU or calibrated constitutive law.

Each record preserves its source URLs, retrieval date, data limits and rights
notes. The sources are factual supplier snapshots, not copied pages or images;
retrieval date does not prove a cached search result is current. No garment retail
price is treated as raw textile cost. No cut/fire/child-sleepwear certification is
inferred from composition or a material family name.

Design inspiration: frankensim `fs-matdb`, inspected at
`24fc0ddf85a4ba25606b921be690029088dbc775`, particularly source-bearing immutable
observations and consumer receipts. No source code or new dependency graph was
copied. The remaining work is in the comprehensive plan and material beads.

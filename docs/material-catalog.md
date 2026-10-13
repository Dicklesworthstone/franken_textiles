# Offline textile catalog and frozen material selections

The domain types and physics contracts remain `ftx-pattern::materials` and
`docs/spec/06-textile-materials.md`. `ftx-pattern::material_catalog` adds the
bundled data, dimensional normalization and immutable selection receipts; it does
not introduce a competing material or constitutive model.

`Catalog::builtin()` loads 30 records: six source-bearing product/composition
references and 24 fiber, yarn, fabric and fill family identities. The family
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

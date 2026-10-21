# Material optical faces follow the textile

An optional `record.optical_faces` declaration binds named optical observations to the
two sides of an oriented source sheet. With `radiation.automatic_material_face: true`,
the existing heat solvers select the applicable observation from the frozen material.
Changing that material or its face assignment changes radiation without editing solver code.

## Run the synthetic example

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- \
  thermal-steady tests/fixtures/materials/synthetic-asymmetric-exterior.json \
  > asymmetric-exterior-result.json
```

The fixture's resistance, two different emissivities and environment are **synthetic**.
They demonstrate numerical behavior and do not characterize a real textile.
Inspect `exterior_exchange.material_face`: it reports `side`, the exact `face` and
`emissivity_property`, `mapping_source`, and `material_sha256`. The same report retains
the solved surface temperature, signed convective/radiative fluxes and equation residual.
Its `request` is frozen for replay. Swapping the fixture's positive and negative mappings
selects the other coefficient and changes heat flow and the frozen request hash.

## Declare the physical assignment

The example material contains this mapping alongside its observations and sources:

```json
{
  "optical_faces": {
    "positive": {"face": "finish", "emissivity_property": "finish_emissivity"},
    "negative": {"face": "reverse", "emissivity_property": "reverse_emissivity"},
    "source": "synthetic",
    "evidence": "estimate",
    "method": "Explicit synthetic assignment to the canonical source-sheet normal",
    "specimen": "synthetic-asymmetric-coupon",
    "conditioning": "synthetic-dry"
  }
}
```

`source` must identify an entry in this material's `sources`. Each mapped property must
exist and retain its own source, with exactly matching `face`, `specimen` and
`conditioning` conditions. The orientation assignment and optical measurements may have
different sources when they describe that same specimen and conditioning. Either side
may be omitted when uncharacterized; requesting it refuses instead of borrowing the
opposite observation. At least one side must be declared.

Positive means the vertex-order normal of the retained thermal source triangle;
negative means its opposite. Observation names such as `finish` and `reverse` carry no
implicit geometric meaning. The assignment must describe the actual specimen and
source-sheet convention. Mirrored `.R`/`.L` pattern instances retain that convention;
their names do not introduce another side reversal.

## Enable automatic selection

Within an otherwise qualified `exterior_exchange`, use:

```json
{
  "radiation": {
    "ambient_c": -10,
    "automatic_material_face": true,
    "allow_opaque_gray_approximation": true
  }
}
```

The example radiant temperature is synthetic. Omit the explicit `exposed_face` and
`emissivity_property` strings in this mode; supplying both automatic and explicit
selection refuses. Existing explicit-face requests remain supported. Omitting radiation
still selects convection alone, without inventing a zero emissivity.

| Consumer | Physical face selected automatically |
|---|---|
| Independent steady/transient stack or source surface | Canonical positive outside face of the outermost material |
| Layered body coverage | Positive or negative side of the active outermost cloth triangle, according to its current normal relative to the body normal |
| Explicitly exposed cloth during motion | Canonical positive source face; the negative face retains its explicitly supplied inside film |

Covered selection and motion depth remapping use the same orientation authority.
Grazing or unresolved covered normals cannot qualify an automatic face. Exposed cloth
does not infer a nearest body, a second ambient face or two-sided radiation. Bare-body
fractions retain their separately declared films; textile optics never supply skin optics.

## Preserve the observation and temperature history

Automatic assignment does not relax optical qualification: the consumed emissivity must
have unit `1`, value and bounds within [0,1], a `total_hemispherical` spectral basis,
matching specimen/conditioning and a temperature domain covering the required exchange.
Camera-band or apparent assembly measurements can remain stored without becoming a
qualified coefficient. Estimated assignments and observations require the material's
`allow_estimates`; estimated convection has its separate boundary consent.

Transient coverage binds each layer's temperature order to its physical side. A direct
coverage advance refuses an orientation reversal of stored temperatures; use the existing
motion remapper to reverse depth order and transfer reference enthalpy. Covered/exposed
transitions and checkpoint restoration preserve that authority. Valid next-interval
weather changes preserve old temperatures. Editing a face map, its source or the material
changes its identity and requires freshly bound geometry/state. Pressure-conditioned
preparation removes and reports the previous `optical_faces` assignment; any replacement
needs separate characterization.

See [layered coverage](thermal-coverage.md#exterior-convection-and-radiation),
[moving coverage](thermal-motion.md#covered-and-exposed-exterior-exchange), and
[exterior qualification](transient-exterior-heat.md#qualify-the-exterior-boundary) for
boundary arrays, area/reference-mass accounting and input limits. Real matched optical
coupons and independent garment validation remain required; numerical fixtures do not
establish measured warmth or finished-material performance.

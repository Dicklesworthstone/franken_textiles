# ftx-measure

Measurement evidence in millimetres (degrees only for angle names), per spec 02 Part A.
No images, personal data, body-model dependency, I/O or runtime Python. Catalogue and chart
constants come from the specification, not a new oracle. Records retain provenance,
definition and one-sigma uncertainty. Repeats of the same method are correlated: they never
reduce uncertainty. Incompatible definitions are excluded, not silently fused. Disagreement
above z=3 requires remeasurement; a chart cannot override out-of-bounds evidence.

The library exposes the 35-item catalogue, record validation/unit parsing, normalization,
public-data priors, single-measurement and whole-document fusion, and the explicitly fictional
reference wearer. Built-in priors cover 800–1280 mm stature. Outside that interval, whole-file
fusion retains measured evidence and supplied priors and reports the absent chart bounds.
Disabling automatic priors does not disable sanity bounds or discard supplied evidence.
A numeric wearer.stature_mm is retained as evidence even without an age/automatic stature prior.
Original records are not changed, and a stored fused cache is always recomputed.

Tests pin every value/sigma/bound/method in spec 02 D.2, all five D.6 size columns, the worked
D.5 fusion examples, and invalid-input/precedence cases. These are specification regressions,
not a full oracle parity certificate or a real-wearer fit claim. Garment readiness and photo
measurement remain separate work. JSON maps are sorted; numerical reductions use input order;
formatting uses ftx-core's Python rounding. Invalid/non-finite inputs get typed refusals.

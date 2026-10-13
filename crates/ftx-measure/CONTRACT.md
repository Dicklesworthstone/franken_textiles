# ftx-measure

Measurement evidence in millimetres (degrees only for angle names), per spec 02 Part A.
No images, personal data, body-model dependency, I/O or runtime Python. Catalogue constants
come from the specification, not a new oracle. Measurement records retain provenance,
definition and one-sigma uncertainty. Repeats of the same method are correlated: they never
reduce uncertainty. Incompatible definitions are excluded, not silently fused. Disagreement
above z=3 requires remeasurement; a chart cannot override out-of-bounds evidence.

The first slice exposes the catalogue, record validation/unit parsing, normalization and
single-measurement fusion, including supplied prior bounds. Chart generation, whole-document
fusion and garment readiness follow separately. This is not a fit claim or a parity certificate.
Tests pin the worked fusion examples in spec 02 D.5, boundary cases and invalid input.
JSON maps are sorted, numerical reductions use input order, formatting uses ftx-core's Python
rounding. Public operations refuse malformed/non-finite input with typed ftx-core refusals.

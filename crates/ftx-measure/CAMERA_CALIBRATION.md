# Multi-view pinhole calibration

`photo::intrinsics::calibrate_camera` implements plan §10.2's shared-camera
calibration boundary. It estimates fx, fy, cx and cy from a rigid planar reference
in at least three fit views, using Zhang's orthogonality/equal-length constraints
(MSR-TR-98-71 §2.3/3.1). Zero skew is fixed, not estimated. B33=1 fixes the conic's
arbitrary scale; positive definiteness is checked before extracting intrinsics.

This is an algebraic solution, not nonlinear maximum likelihood or lens-distortion
estimation. Input must explicitly declare `pinhole_zero_skew` and provide lens,
pinhole and physical-reference evidence. Raw distorted pixels, changing zoom,
focus, stabilization/crop geometry or lens switches need a different calibration.
Those assumptions cannot be verified from the supplied coordinate arrays.

Inputs use sheet-local metres (x/z in the plane), already oriented pixels and
unique decoded timestamps from one stream. The sheet may move between images;
returned `camera_in_sheet_frame` poses therefore are NOT automatically a common
world frame. Reuse intrinsics with a stationary reference to establish that frame.
The principal point must lie inside the stated image; cropped optical centers
outside it are deliberately unsupported. The existing pinhole's normalized
radius limit of 2 applies to pose recovery and downstream projection.

Budgets are 128 controls, 32 views and 128 corners per view, checked before cloning.
Normalization removes pixel units and homography scale. A pivoted, twice
reorthogonalized four-column QR rank check (minimum pivot ratio 1e-6) rejects weak
orientation constraints before the existing Jacobi least-squares solver is called.
The pivot ratio is a numerical gate, not a statistical precision estimate.

Held-out views contribute no intrinsic constraints; their fit corners estimate
only their own extrinsics. Held-out corners never enter a homography or pose fit.
Every corner in every view must reproject within 3 px and every pose must pass the
existing rigid-sheet/depth/lens checks. No outliers are dropped. Missing independent
checks are advisories. Rejected or unobservable inputs publish no intrinsics or
poses, only diagnostics and the canonical original input. No sigma, camera-accuracy,
body measurement or garment-fit claim follows from acceptance.

Ten synthetic integration tests check known intrinsics/poses, ordering, modest
pixel noise, translation/in-plane-rotation degeneracy, insufficient/collinear
controls, both held-out partitions, malformed input, budgets, nonprojective warps
and metric-reference scaling. They are not a real-camera qualification.

Formula source: Zhengyou Zhang, *A Flexible New Technique for Camera Calibration*,
https://www.microsoft.com/en-us/research/publication/a-flexible-new-technique-for-camera-calibration/ .

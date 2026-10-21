# Body-model pin

| Field | Value |
|---|---|
| Model | Anny (NAVER LABS Europe), arXiv 2511.03589, https://github.com/naver/anny |
| Package | `anny==0.6.1` (PyPI, uploaded 2026-09-28) |
| Wheel | `anny-0.6.1-py3-none-any.whl`, 47,719,091 bytes |
| Wheel SHA-256 | `9dbd3d6c2e5dae20a4f5e0b80e104f50fd13d2e15e42c90fc288e00d86e60e09` |
| sdist SHA-256 | `b3eeb81f439f6e331d57383f624d0a7de903456d06a82636ce73a303dc105456` |
| Licences | Code Apache-2.0; MakeHuman/MPFB2-derived assets CC0 1.0. The optional `smplx` topology download is non-commercial and is never used |
| Checked | 2026-10-12, CPU only: 6 phenotype parameters, 256 local changes (20 `measure-*`), 13,718-vertex quad mesh, 163-bone default rig, ~8 ms per forward pass after load |

The converter (`convert_anny.py`) checks that the installed package is this version and that every installed
file matches the wheel's RECORD hashes, builds into a fresh cache, checks every array against spec 05 §8.1, and writes the
forward-model data into a versioned `*.ftxbody` file outside the repository. Converted data is never
committed. Moving the pin means new parity fixtures and keeping the old results.

Fixtures from this pin: `capture_anny_reference.py` (forward model, 16 cases) and `capture_anny_poses.py`
(drape poses, nappy and skin shell, 10 cases), both into `tests/fixtures/oracle/body/`. Re-running
`capture_anny_poses.py` gave identical bytes.

## Reproduced conversion environment (2026-10-20, `ftx-eju`)

The unchanged converter reproduced every pinned array and label hash on macOS 27.0.1 arm64 with
Python 3.12.12, `torch==2.14.1`, `numpy==2.5.4`, `roma==1.6.1`, `trimesh==5.1.1` and
`safetensors==0.8.0`, using the wheel above and pure Torch LBS. The resulting `.ftxbody` is
152,042,752 bytes with SHA-256
`8f0bb151ba19975437147d239b7ff237642a7fa63659a6fac49fa23c299a0915`.
Keep the converter's RECORD, array/label and sparse-reconstruction checks enabled. Conversion is a
build-time operation; the admitted file can then be copied to native/WASM consumers outside Git.

A fresh Linux/Python 3.14 build with Torch 2.13.0 and NumPy 2.4.3 passed package RECORD checks but
failed nine array hashes, including faces. Selected vertices across all sixteen reference cases
differed by at most 8.89e-16 m; that limited diagnostic does not admit different arrays or topology.
Pinning Anny alone is therefore insufficient to reproduce the byte-exact converted asset on every
host. This is not permission to widen hashes, skip checks or move the oracle pin.

The topology difference is twelve triangle rows from six quad diagonals. Upstream
`anny/utils/mesh_utils.py::_split_quad` chooses the smaller diagonal using a strict floating-point
comparison. The two template arrays differ by at most 5.56e-17 m. Evaluating both arrays on the
same Torch 2.14.1 runtime changes all six choices: the admitted template's diagonal differences
are -6.94e-18 to -1.22e-17 m, whereas the Linux template gives ties and selects the other diagonal.
Thus small coordinate differences can change topology; selected forward-vertex agreement alone
cannot justify admitting that conversion. Use the reproduced environment and full hash checks.

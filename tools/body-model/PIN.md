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

## Full-surface reference and portable reuse

The admitted converted file can be copied to another platform without rebuilding it. Verify its
size and complete SHA-256 above before setting `FTX_ANNY_MODEL`; unchanged internal array, label
and sparse-reconstruction checks still apply. A successful copy does not qualify body anatomy or
make an intersecting posed mesh a valid cloth collider.

The companion `anny-0.6.1-full-vertices.bin` contains every rest and posed vertex for the existing
16 reference inputs: 438,976 vertices across 32 surfaces. It was captured twice from the pinned
Python environment above with identical bytes: **10,541,975 bytes**, SHA-256
`8d019f3aca6597bb2be99629676fd746a0385ca33ff7ed703c05e38141a10782`.
It is derived model data and stays in the external model cache, alongside the converted file.
The original sampled JSON oracle remains unchanged.

```sh
python tools/body-model/capture_anny_reference.py NEW_FULL_REFERENCE.bin \
  --full-vertices-from tests/fixtures/oracle/body/anny-0.6.1-reference.json
FTX_ANNY_MODEL=/path/to/anny-0.6.1-default.ftxbody \
FTX_ANNY_FULL_REFERENCE=/path/to/anny-0.6.1-full-vertices.bin \
  cargo test -p ftx-body --release --test anny_parity -- --include-ignored --nocapture
```

The capture checks the installed wheel's RECORD and exact reference array/label hashes before
calling upstream Anny on the JSON's unchanged inputs. It creates a new output exclusively.
The binary starts with `FTXANNY1`, a little-endian u64 JSON-header length, and that header's bytes.
The remaining f64 little-endian payload is ordered by case, rest/posed state, vertex, then xyz,
in metres. The header locks the sampled JSON's full SHA-256, every input, state order and model
hashes. The Rust test additionally locks the complete binary digest, compares every vertex at
the existing 1e-9 m tolerance and checks that an unsampled coordinate mutation fails. If no
explicit full-reference path is supplied, it looks next to the converted model. This additional
ignored test requires both external files; missing data is a failed invocation, never a pass.

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

The converter (planned, `convert_anny.py`) installs exactly this wheel, verifies the hash, and writes the
forward-model data into a versioned `*.ftxbody` file outside the repository. Converted data is never
committed. Moving the pin means new parity fixtures and keeping the old results.

//! Reading converted Anny data (`.ftxbody`, written by `tools/body-model/convert_anny.py`).
//!
//! Layout (`ftxbody/1`, little-endian): `b"FTXBODY\0"`, u32 format version 1, u32 reserved, u64 header
//! length H, H bytes of JSON header (space-padded to 8-byte alignment), then the data section holding each
//! array at its header offset. The reader checks the magic, the version, every array's dtype, shape and
//! bounds against the counts, the index ranges, the bone order (`parent < child`) and, on request, each
//! array's SHA-256. Anything wrong is a refusal, never a panic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ftx_core::receipt::Sha256;
use ftx_core::{Mat3, Refusal, RefusalCode, Vec3};
use serde::Deserialize;

/// The converted model file this crate reads by default: `$FTX_ANNY_MODEL`, else
/// `$HOME/.cache/franken_textiles/anny-0.6.1-default.ftxbody`.
#[must_use]
pub fn default_model_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FTX_ANNY_MODEL") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cache/franken_textiles/anny-0.6.1-default.ftxbody"))
}

/// Counts recorded by the converter.
#[derive(Clone, Debug, Deserialize)]
pub struct Counts {
    /// V, mesh vertices (13,718).
    pub vertices: usize,
    /// F, triangles (27,420).
    pub faces: usize,
    /// J, bones (163).
    pub bones: usize,
    /// S, blend shapes (1,136).
    pub blendshapes: usize,
    /// P, phenotype blend shapes (624, the first P rows).
    pub phenotype_blendshapes: usize,
    /// L, local changes (256; rows P + 2j and P + 2j + 1).
    pub local_changes: usize,
    /// K, bone influences per vertex (9).
    pub influences: usize,
}

/// Phenotype metadata: the 26 components and the anchors of each feature, as the package built them.
#[derive(Clone, Debug, Deserialize)]
pub struct PhenotypeMeta {
    /// Features in order (race, gender, age, muscle, weight, height, proportions, cupsize, firmness).
    pub features: Vec<String>,
    /// `(feature, component)` pairs in mask-column order.
    pub components: Vec<(String, String)>,
    /// Anchor values per feature (the package's exact floats; race has none).
    pub anchors: BTreeMap<String, Vec<f64>>,
    /// The features exposed as parameters (gender, age, muscle, weight, height, proportions).
    pub exposed: Vec<String>,
    /// The value of every feature that is not exposed (0.5).
    pub fixed_value: f64,
}

/// Label lists.
#[derive(Clone, Debug, Deserialize)]
pub struct Labels {
    /// S blend-shape labels (`block:components`).
    pub blendshapes: Vec<String>,
    /// J bone labels.
    pub bones: Vec<String>,
    /// J parent indices (−1 for the root).
    pub bone_parents: Vec<i64>,
    /// L local-change labels (the positive target of each pair).
    pub local_changes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ArrayEntry {
    dtype: String,
    shape: Vec<usize>,
    offset: u64,
    nbytes: u64,
    sha256: String,
}

/// The JSON header.
#[derive(Clone, Debug, Deserialize)]
pub struct Header {
    /// `ftxbody/1`.
    pub schema: String,
    /// `anny`.
    pub model: String,
    /// The package version the data was converted from.
    pub anny_version: String,
    /// The model call it reproduces.
    pub configuration: String,
    /// Licence of the data.
    pub licence: String,
    /// The package's own array hashes, checked by the converter.
    pub package_sha256: BTreeMap<String, String>,
    /// Sizes.
    pub counts: Counts,
    /// Phenotype components and anchors.
    pub phenotype: PhenotypeMeta,
    /// Ŷ for the rest-frame construction (0, 1, 0).
    pub y_axis: [f64; 3],
    /// The rotation used when a bone points exactly along ±Ŷ.
    pub degenerate_rotation: [[f64; 3]; 3],
    /// Labels.
    pub labels: Labels,
    /// The 46 final vertex indices of the waist loop.
    pub waist_loop_final: Vec<usize>,
    arrays: BTreeMap<String, ArrayEntry>,
}

/// The arrays of spec 05 §1.2, typed.
#[derive(Clone, Debug)]
pub struct AnnyData {
    /// Header metadata.
    pub header: Header,
    /// T, template vertices (V).
    pub template_vertices: Vec<Vec3>,
    /// Triangles (F), outward winding.
    pub faces: Vec<[u32; 3]>,
    /// CSR row pointers over the S blend-shape rows (S + 1).
    pub blendshape_row_ptr: Vec<u32>,
    /// Vertex index of each non-zero blend-shape entry.
    pub blendshape_vertex: Vec<u32>,
    /// Offset of each non-zero blend-shape entry (metres).
    pub blendshape_delta: Vec<Vec3>,
    /// P × 26 phenotype mask (0/1), row-major.
    pub phenotype_mask: Vec<u8>,
    /// Template bone heads (J).
    pub template_bone_heads: Vec<Vec3>,
    /// Template bone tails (J).
    pub template_bone_tails: Vec<Vec3>,
    /// S × J bone-head blend shapes, row-major.
    pub bone_heads_blendshapes: Vec<Vec3>,
    /// S × J bone-tail blend shapes, row-major.
    pub bone_tails_blendshapes: Vec<Vec3>,
    /// Bone roll matrices (J), float32 values widened to f64 as stored.
    pub bone_rolls: Vec<Mat3>,
    /// V × K influencing bones, row-major.
    pub vertex_bone_indices: Vec<u32>,
    /// V × K weights, row-major.
    pub vertex_bone_weights: Vec<f64>,
    /// Final → MakeHuman base-mesh vertex index (V).
    pub base_mesh_vertex_indices: Vec<u32>,
}

fn bad(msg: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, msg)
        .repair("re-run tools/body-model/convert_anny.py against anny 0.6.1 and point FTX_ANNY_MODEL at its output")
}

struct Reader<'a> {
    data: &'a [u8],
    arrays: &'a BTreeMap<String, ArrayEntry>,
    verify: bool,
}

impl Reader<'_> {
    fn raw(&self, name: &str, dtype: &str, shape: &[usize]) -> Result<&[u8], Refusal> {
        let e = self
            .arrays
            .get(name)
            .ok_or_else(|| bad(format!("array '{name}' is missing")))?;
        if e.dtype != dtype || e.shape != shape {
            return Err(bad(format!(
                "array '{name}' is {} {:?}, expected {dtype} {shape:?}",
                e.dtype, e.shape
            )));
        }
        let width = match dtype {
            "f64" => 8,
            "u32" => 4,
            _ => 1,
        };
        let expect = shape.iter().product::<usize>() * width;
        let start = usize::try_from(e.offset).map_err(|_| bad(format!("array '{name}' offset")))?;
        let len = usize::try_from(e.nbytes).map_err(|_| bad(format!("array '{name}' size")))?;
        if len != expect
            || start
                .checked_add(len)
                .is_none_or(|end| end > self.data.len())
        {
            return Err(bad(format!("array '{name}' does not fit the file")));
        }
        let bytes = &self.data[start..start + len];
        if self.verify {
            let mut h = Sha256::new();
            h.update(bytes);
            let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if hex != e.sha256 {
                return Err(bad(format!("array '{name}' fails its SHA-256 check")));
            }
        }
        Ok(bytes)
    }

    fn f64s(&self, name: &str, shape: &[usize]) -> Result<Vec<f64>, Refusal> {
        let bytes = self.raw(name, "f64", shape)?;
        Ok(bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| f64::from_le_bytes(*c))
            .collect())
    }

    fn u32s(&self, name: &str, shape: &[usize]) -> Result<Vec<u32>, Refusal> {
        let bytes = self.raw(name, "u32", shape)?;
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect())
    }

    /// An array whose last axis is 3, read as points; `shape` is the full stored shape.
    fn vec3s(&self, name: &str, shape: &[usize]) -> Result<Vec<Vec3>, Refusal> {
        let v = self.f64s(name, shape)?;
        Ok(v.as_chunks::<3>()
            .0
            .iter()
            .map(|c| Vec3::from(*c))
            .collect())
    }
}

impl AnnyData {
    /// Reads and checks a `.ftxbody` file. With `verify`, every array's SHA-256 is checked too
    /// (about 150 MB of hashing).
    ///
    /// # Errors
    /// [`RefusalCode::Io`] if the file cannot be read; [`RefusalCode::InvalidDocument`] for anything
    /// malformed or inconsistent.
    pub fn read(path: &Path, verify: bool) -> Result<Self, Refusal> {
        let bytes = std::fs::read(path).map_err(|e| {
            Refusal::new(RefusalCode::Io, format!("cannot read {}: {e}", path.display()))
                .repair("run tools/body-model/convert_anny.py (it writes the default path), or set FTX_ANNY_MODEL")
        })?;
        Self::from_bytes(&bytes, verify)
    }

    /// Parses a `.ftxbody` image already in memory.
    ///
    /// # Errors
    /// As [`AnnyData::read`].
    pub fn from_bytes(bytes: &[u8], verify: bool) -> Result<Self, Refusal> {
        if bytes.len() < 24 || &bytes[..8] != b"FTXBODY\0" {
            return Err(bad("not a .ftxbody file"));
        }
        let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if version != 1 {
            return Err(bad(format!(
                "format version {version}; this build reads version 1"
            )));
        }
        let hlen = u64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8]));
        let hend = usize::try_from(hlen)
            .ok()
            .and_then(|h| h.checked_add(24))
            .filter(|&e| e <= bytes.len());
        let hend = hend.ok_or_else(|| bad("header length exceeds the file"))?;
        let header: Header = serde_json::from_slice(&bytes[24..hend])
            .map_err(|e| bad(format!("header is not valid: {e}")))?;
        if header.schema != "ftxbody/1" || header.model != "anny" {
            return Err(bad(format!(
                "schema {} / model {}",
                header.schema, header.model
            )));
        }
        let c = header.counts.clone();
        let r = Reader {
            data: &bytes[hend..],
            arrays: &header.arrays,
            verify,
        };
        let row_ptr = r.u32s("blendshape_row_ptr", &[c.blendshapes + 1])?;
        let nnz = *row_ptr.last().unwrap_or(&0) as usize;
        let rolls = r.f64s("bone_rolls", &[c.bones, 3, 3])?;
        let data = Self {
            template_vertices: r.vec3s("template_vertices", &[c.vertices, 3])?,
            faces: r.u32s("faces", &[c.faces, 3])?.as_chunks::<3>().0.to_vec(),
            blendshape_row_ptr: row_ptr,
            blendshape_vertex: r.u32s("blendshape_vertex", &[nnz])?,
            blendshape_delta: r.vec3s("blendshape_delta", &[nnz, 3])?,
            phenotype_mask: r
                .raw(
                    "phenotype_mask",
                    "u8",
                    &[c.phenotype_blendshapes, header.phenotype.components.len()],
                )?
                .to_vec(),
            template_bone_heads: r.vec3s("template_bone_heads", &[c.bones, 3])?,
            template_bone_tails: r.vec3s("template_bone_tails", &[c.bones, 3])?,
            bone_heads_blendshapes: r
                .vec3s("bone_heads_blendshapes", &[c.blendshapes, c.bones, 3])?,
            bone_tails_blendshapes: r
                .vec3s("bone_tails_blendshapes", &[c.blendshapes, c.bones, 3])?,
            bone_rolls: rolls
                .as_chunks::<9>()
                .0
                .iter()
                .map(|m| {
                    Mat3::from_rows([[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]])
                })
                .collect(),
            vertex_bone_indices: r.u32s("vertex_bone_indices", &[c.vertices, c.influences])?,
            vertex_bone_weights: r.f64s("vertex_bone_weights", &[c.vertices, c.influences])?,
            base_mesh_vertex_indices: r.u32s("base_mesh_vertex_indices", &[c.vertices])?,
            header,
        };
        data.check()?;
        Ok(data)
    }

    /// Index ranges and orders the forward model relies on.
    fn check(&self) -> Result<(), Refusal> {
        let c = &self.header.counts;
        let l = &self.header.labels;
        if l.blendshapes.len() != c.blendshapes
            || l.bones.len() != c.bones
            || l.bone_parents.len() != c.bones
            || l.local_changes.len() != c.local_changes
            || c.phenotype_blendshapes + 2 * c.local_changes != c.blendshapes
        {
            return Err(bad("label counts disagree with the array counts"));
        }
        for (j, &p) in l.bone_parents.iter().enumerate() {
            let ok = if j == 0 {
                p == -1
            } else {
                p >= 0 && (p as usize) < j
            };
            if !ok {
                return Err(bad(format!(
                    "bone {j} has parent {p}; bones must follow their parents"
                )));
            }
        }
        let v = c.vertices as u32;
        if self.faces.iter().flatten().any(|&i| i >= v)
            || self.blendshape_vertex.iter().any(|&i| i >= v)
        {
            return Err(bad("a vertex index is out of range"));
        }
        if self.blendshape_row_ptr.windows(2).any(|w| w[0] > w[1])
            || self.blendshape_row_ptr[0] != 0
        {
            return Err(bad("blend-shape rows are not in order"));
        }
        if self
            .vertex_bone_indices
            .iter()
            .any(|&b| b as usize >= c.bones)
        {
            return Err(bad("a skinning bone index is out of range"));
        }
        if self
            .header
            .waist_loop_final
            .iter()
            .any(|&i| i >= c.vertices)
        {
            return Err(bad("a waist-loop vertex is out of range"));
        }
        let comps = &self.header.phenotype.components;
        if comps.len() != 26 || self.phenotype_mask.iter().any(|&m| m > 1) {
            return Err(bad("the phenotype mask must be P × 26 of 0/1"));
        }
        for (feature, _) in comps {
            if feature != "race" && !self.header.phenotype.anchors.contains_key(feature) {
                return Err(bad(format!("no anchors for phenotype feature '{feature}'")));
            }
        }
        let all_finite = self.template_vertices.iter().all(|p| p.is_finite())
            && self.blendshape_delta.iter().all(|p| p.is_finite())
            && self.vertex_bone_weights.iter().all(|w| w.is_finite());
        if !all_finite {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the model data holds a non-finite number",
            ));
        }
        Ok(())
    }
}

//! Raster operations for the trace, each ported from what the reference calls (Pillow 12.3, OpenCV 5.0.0,
//! numpy 2.5) and checked against oracle captures (`tools/oracle/capture_raster.py`).

pub mod color;
pub mod image;

pub use color::{bgr_saturation, bgr_to_gray, bgr_to_lab, lab, lab_distance};
pub use image::{Bgr, Gray, load_png};

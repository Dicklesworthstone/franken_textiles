//! Writes the drape poses of a reference wearer as OBJ files for a visual check:
//! `cargo run --release -p ftx-body --example posed_skins -- OUT_DIR [years] [female 0|1]`
//! (default: a 2.5-year-old girl). Needs the converted Anny model.

use ftx_body::{
    AnnyModel, Phenotype, Pose, age_parameter_for_years, default_model_path, nappy_local_changes,
    posed_skin,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(
        args.get(1)
            .expect("usage: posed_skins OUT_DIR [years] [0|1]"),
    );
    let years: f64 = args.get(2).map_or(2.5, |s| s.parse().expect("years"));
    let gender: f64 = args
        .get(3)
        .map_or(1.0, |s| s.parse().expect("gender 0 = male, 1 = female"));
    let model = AnnyModel::load(&default_model_path().expect("model path"), false)
        .unwrap_or_else(|r| panic!("{r}"));
    let params = model.params(Phenotype {
        gender,
        age: age_parameter_for_years(years),
        ..Phenotype::default()
    });
    std::fs::create_dir_all(&out).expect("output directory");
    let cases = [
        ("rest", Pose::rest(), 0.0),
        ("a35", Pose::a_pose(Pose::A_POSE_DEG), 0.0),
        ("a50", Pose::a_pose(50.0), 0.0),
        ("arms_up", Pose::arms_up(), 0.0),
        ("seated", Pose::seated(), 0.0),
        ("squat", Pose::squat(), 0.0),
        ("nappy_a35", Pose::a_pose(Pose::A_POSE_DEG), 1.0),
    ];
    for (name, pose, nappy) in cases {
        let p = nappy_local_changes(&model, &params, nappy).unwrap_or_else(|r| panic!("{r}"));
        let skin = posed_skin(&model, &p, &pose, 1.0).unwrap_or_else(|r| panic!("{r}"));
        let path = out.join(format!("{name}.obj"));
        std::fs::write(&path, skin.to_obj()).expect("write OBJ");
        println!(
            "{}: {:.3} L, {:.4} m²",
            path.display(),
            skin.report.volume * 1e3,
            skin.report.area
        );
    }
}

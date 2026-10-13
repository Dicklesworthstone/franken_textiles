//! Controls and presentation for the paired drape; numerical decisions remain in ftx-drape.

use ftx_core::Refusal;
use ftx_drape::{run::DrapeOptions, two_start::TwoStartOptions};
use ftx_export::text::xml_escape;
use serde_json::Value;

use super::{Args, bad_input};

pub(super) const OPTIONS: &[&str] = &[
    "two-start-sew",
    "spread-vertex-mm",
    "spread-hem-mm",
    "spread-clearance-mm",
    "spread-seam-pp",
];

pub(super) fn options(args: &Args, run: &DrapeOptions) -> Result<Option<TwoStartOptions>, Refusal> {
    let Some(sew) = args.options.get("two-start-sew") else {
        if let Some(key) = OPTIONS[1..].iter().find(|&&k| args.options.contains_key(k)) {
            return Err(bad_input(format!("--{key} requires --two-start-sew N")));
        }
        return Ok(None);
    };
    let mut policy = TwoStartOptions {
        alternate_sew_steps: sew
            .parse()
            .map_err(|_| bad_input("--two-start-sew must be a nonnegative integer"))?,
        ..TwoStartOptions::default()
    };
    for (key, value) in [
        ("spread-vertex-mm", &mut policy.vertex_mm),
        ("spread-hem-mm", &mut policy.hem_mm),
        ("spread-clearance-mm", &mut policy.clearance_mm),
        ("spread-seam-pp", &mut policy.seam_strain_pp),
    ] {
        if let Some(s) = args.options.get(key) {
            *value = s
                .parse()
                .map_err(|_| bad_input(format!("--{key} must be a number")))?;
        }
    }
    policy.validate(run)?;
    Ok(Some(policy))
}

pub(super) fn summary(report: &Value) -> String {
    let Some(pair) = report.get("two_start") else {
        return String::new();
    };
    format!(
        "two-start: {}; max vertex spread {:.3} mm, RMS {:.3} mm ({} vs {} sewing steps)\n",
        pair["status"].as_str().unwrap_or("UNKNOWN"),
        pair["positions"]["max_mm"].as_f64().unwrap_or(f64::NAN),
        pair["positions"]["rms_mm"].as_f64().unwrap_or(f64::NAN),
        pair["primary"]["sew_steps"],
        pair["alternate"]["sew_steps"],
    )
}

pub(super) fn review_html(report: &Value) -> String {
    let Some(pair) = report.get("two_start") else {
        return String::new();
    };
    let mut out = format!(
        "<h3>Two sewing paths</h3><p>{}</p><p class=\"muted\">{}</p>",
        xml_escape(&summary(report)),
        xml_escape(pair["no_claim"].as_str().unwrap_or(""))
    );
    out.push_str("<div class=\"scroll\"><table><thead><tr><th>Metric</th><th>Primary</th><th>Alternate</th><th>Spread</th><th>Bound</th><th>Units</th><th>Result</th></tr></thead><tbody>");
    for row in pair["metrics"].as_array().into_iter().flatten() {
        out.push_str(if row["passed"] == true {
            "<tr class=\"pass\">"
        } else {
            "<tr class=\"fail\">"
        });
        let id = format!(
            "{}/{}",
            row["group"].as_str().unwrap_or("?"),
            row["id"].as_str().unwrap_or("?")
        );
        out.push_str(&format!("<td>{}</td>", xml_escape(&id)));
        for key in ["primary", "alternate", "spread", "bound"] {
            let value = row[key]
                .as_f64()
                .filter(|n| n.is_finite())
                .map_or_else(|| "unavailable".into(), |n| format!("{n:.3}"));
            out.push_str(&format!("<td>{value}</td>"));
        }
        out.push_str(&format!(
            "<td>{}</td><td>{}</td></tr>",
            xml_escape(row["units"].as_str().unwrap_or("?")),
            if row["passed"] == true {
                "PASS"
            } else {
                "REVIEW"
            }
        ));
    }
    out.push_str("</tbody></table></div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::RefusalCode;
    use serde_json::json;

    fn args(extra: &[&str]) -> Args {
        let mut a = vec!["drape".into()];
        a.extend(extra.iter().map(|s| s.to_string()));
        crate::parse(&a).unwrap()
    }

    #[test]
    fn explicit_paths_and_bounds_preserve_zero_and_default_single_run() {
        let run = DrapeOptions::default();
        assert_eq!(options(&args(&[]), &run).unwrap(), None);
        let p = options(
            &args(&["--two-start-sew", "0", "--spread-hem-mm", "4.5"]),
            &run,
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.alternate_sew_steps, 0);
        assert_eq!(p.hem_mm, 4.5);
        assert_eq!(p.vertex_mm, TwoStartOptions::default().vertex_mm);
    }

    #[test]
    fn malformed_policy_fails_before_input_or_output_io() {
        for bad in [
            vec!["--two-start-sew", "12"],
            vec!["--two-start-sew", "no"],
            vec!["--two-start-sew", "18", "--spread-seam-pp", "NaN"],
            vec!["--two-start-sew", "18", "--spread-vertex-mm", "0"],
            vec!["--spread-clearance-mm", "2"],
        ] {
            let r = crate::cmd_drape(&args(&bad)).unwrap_err();
            assert_ne!(r.code, RefusalCode::Io, "{bad:?}: {r}");
            assert!(!r.message.contains("pattern file"), "{bad:?}: {r}");
        }
    }

    #[test]
    fn review_escapes_names_and_shows_unavailable_readings() {
        let report = json!({"two_start": {"status":"REVIEW_REQUIRED", "no_claim":"not <fit>",
            "metrics":[{"group":"seams", "id":"<script>", "units":"<mm>", "spread":null, "passed":false}]}});
        let html = review_html(&report);
        assert!(html.contains("Two sewing paths"));
        assert!(html.contains("unavailable"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(review_html(&json!({})).is_empty());
        assert!(summary(&json!({})).is_empty());
    }

    #[test]
    fn output_reservation_never_reuses_existing_evidence() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!("ftx-paired-{}-{stamp}", std::process::id()));
        crate::reserve_drape_output(&out).unwrap();
        std::fs::write(out.join("sim_report.json"), b"old evidence").unwrap();
        assert!(crate::reserve_drape_output(&out).is_err());
        assert_eq!(
            std::fs::read(out.join("sim_report.json")).unwrap(),
            b"old evidence"
        );
    }
}

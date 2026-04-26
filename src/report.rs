use crate::detect::DetectionReport;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_detection_json(report: &DetectionReport, path: &Path) -> Result<(), String> {
    let mut file =
        File::create(path).map_err(|err| format!("cannot create {}: {err}", path.display()))?;
    file.write_all(detection_json(report).as_bytes())
        .map_err(|err| format!("write failed for {}: {err}", path.display()))
}

pub fn write_detection_markdown(report: &DetectionReport, path: &Path) -> Result<(), String> {
    let mut file =
        File::create(path).map_err(|err| format!("cannot create {}: {err}", path.display()))?;
    file.write_all(detection_markdown(report).as_bytes())
        .map_err(|err| format!("write failed for {}: {err}", path.display()))
}

fn detection_json(report: &DetectionReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"image_count\": {},\n", report.image_count));
    out.push_str(&format!("  \"block_size\": {},\n", report.block_size));
    out.push_str(&format!(
        "  \"sampled_blocks\": {},\n",
        report.sampled_blocks
    ));
    out.push_str("  \"type_stats\": {\n");
    out.push_str(&format!(
        "    \"mirrored_blocks\": {},\n",
        report.type_stats.mirrored_blocks
    ));
    out.push_str(&format!(
        "    \"parity_blocks\": {},\n",
        report.type_stats.parity_blocks
    ));
    out.push_str(&format!(
        "    \"unassigned_blocks\": {}\n",
        report.type_stats.unassigned_blocks
    ));
    out.push_str("  },\n");
    out.push_str("  \"candidates\": [\n");
    for (idx, candidate) in report.candidates.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"raid_type\": \"{}\",\n",
            escape(&candidate.raid_type)
        ));
        match candidate.stripe_size {
            Some(size) => out.push_str(&format!("      \"stripe_size\": {},\n", size)),
            None => out.push_str("      \"stripe_size\": null,\n"),
        }
        out.push_str(&format!("      \"score\": {:.6},\n", candidate.score));
        out.push_str("      \"evidence\": {\n");
        out.push_str(&format!(
            "        \"type_score\": {:.6},\n",
            candidate.evidence.type_score
        ));
        out.push_str(&format!(
            "        \"stripe_votes\": {},\n",
            candidate.evidence.stripe_votes
        ));
        out.push_str(&format!(
            "        \"stripe_vote_ratio\": {:.6},\n",
            candidate.evidence.stripe_vote_ratio
        ));
        out.push_str("        \"notes\": [");
        for (note_idx, note) in candidate.evidence.notes.iter().enumerate() {
            if note_idx > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("\"{}\"", escape(note)));
        }
        out.push_str("]\n");
        out.push_str("      }\n");
        out.push_str("    }");
        if idx + 1 != report.candidates.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn detection_markdown(report: &DetectionReport) -> String {
    let mut out = String::new();
    out.push_str("# RAID-reassemble Detection Report\n\n");
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Images: {}\n", report.image_count));
    out.push_str(&format!("- Block size: {}\n", report.block_size));
    out.push_str(&format!("- Sampled blocks: {}\n\n", report.sampled_blocks));
    out.push_str("## Type Evidence\n\n");
    out.push_str("| Metric | Count |\n| --- | ---: |\n");
    out.push_str(&format!(
        "| Mirrored blocks | {} |\n",
        report.type_stats.mirrored_blocks
    ));
    out.push_str(&format!(
        "| Parity blocks | {} |\n",
        report.type_stats.parity_blocks
    ));
    out.push_str(&format!(
        "| Unassigned blocks | {} |\n\n",
        report.type_stats.unassigned_blocks
    ));
    out.push_str("## Candidates\n\n");
    out.push_str("| Rank | RAID | Stripe size | Score | Stripe votes | Notes |\n");
    out.push_str("| ---: | --- | ---: | ---: | ---: | --- |\n");
    for (idx, candidate) in report.candidates.iter().enumerate() {
        let stripe = candidate
            .stripe_size
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        out.push_str(&format!(
            "| {} | {} | {} | {:.3} | {} | {} |\n",
            idx + 1,
            candidate.raid_type,
            stripe,
            candidate.score,
            candidate.evidence.stripe_votes,
            candidate.evidence.notes.join("; ")
        ));
    }
    out.push_str("\n## Risk Notes\n\n");
    out.push_str("- This result is heuristic evidence, not absolute proof.\n");
    out.push_str("- High-entropy encrypted or randomly filled images can defeat entropy boundary detection.\n");
    out.push_str("- RAID-0 disk order inference remains weak without filesystem validation.\n");
    out
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

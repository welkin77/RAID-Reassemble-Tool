use crate::features::{entropy, is_zero, xor_is_zero};
use crate::image::{BlockReader, ImageSet};
use std::cmp::Ordering;

#[derive(Clone)]
pub struct DetectionOptions {
    pub block_size: usize,
    pub max_blocks: usize,
    pub top: usize,
    pub forced_raid: String,
    pub forced_stripe: Option<usize>,
}

#[derive(Clone)]
pub struct DetectionReport {
    pub image_count: usize,
    pub block_size: usize,
    pub sampled_blocks: usize,
    pub type_stats: TypeStats,
    pub candidates: Vec<RaidCandidate>,
}

#[derive(Clone, Default)]
pub struct TypeStats {
    pub mirrored_blocks: usize,
    pub parity_blocks: usize,
    pub unassigned_blocks: usize,
}

#[derive(Clone)]
pub struct RaidCandidate {
    pub raid_type: String,
    pub stripe_size: Option<usize>,
    pub score: f32,
    pub evidence: CandidateEvidence,
}

#[derive(Clone)]
pub struct CandidateEvidence {
    pub type_score: f32,
    pub stripe_votes: usize,
    pub stripe_vote_ratio: f32,
    pub notes: Vec<String>,
}

pub fn detect_raid(
    images: &ImageSet,
    options: DetectionOptions,
) -> Result<DetectionReport, String> {
    if images.images.len() < 2 {
        return Err("detection requires at least two images".to_string());
    }

    let type_stats = detect_type_stats(images, options.block_size, options.max_blocks)?;
    let stripe_votes = if let Some(stripe) = options.forced_stripe {
        vec![(stripe, options.max_blocks)]
    } else {
        detect_stripe_sizes(images, options.block_size, options.max_blocks)?
    };
    let raid_types = candidate_raid_types(&type_stats, images.images.len(), options.forced_raid);
    let mut candidates = Vec::new();
    for (raid_type, type_score, note) in raid_types {
        for (stripe_size, votes) in stripe_votes.iter().take(options.top.max(1)) {
            let max_votes = stripe_votes.first().map(|(_, v)| *v).unwrap_or(1).max(1);
            let vote_ratio = *votes as f32 / max_votes as f32;
            let score = (0.65 * type_score) + (0.35 * vote_ratio);
            candidates.push(RaidCandidate {
                raid_type: raid_type.clone(),
                stripe_size: Some(*stripe_size),
                score,
                evidence: CandidateEvidence {
                    type_score,
                    stripe_votes: *votes,
                    stripe_vote_ratio: vote_ratio,
                    notes: vec![note.clone()],
                },
            });
        }
    }
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    candidates.truncate(options.top.max(1));

    Ok(DetectionReport {
        image_count: images.images.len(),
        block_size: options.block_size,
        sampled_blocks: options.max_blocks,
        type_stats,
        candidates,
    })
}

impl DetectionReport {
    pub fn human_summary(&self) -> String {
        let mut out = String::new();
        out.push_str("RAID-reassemble detect\n");
        out.push_str(&format!("images: {}\n", self.image_count));
        out.push_str(&format!("block_size: {}\n", self.block_size));
        out.push_str(&format!(
            "type_stats: mirrored={} parity={} unassigned={}\n",
            self.type_stats.mirrored_blocks,
            self.type_stats.parity_blocks,
            self.type_stats.unassigned_blocks
        ));
        out.push_str("candidates:\n");
        for (idx, c) in self.candidates.iter().enumerate() {
            out.push_str(&format!(
                "  {}. {} stripe={} score={:.3} votes={}\n",
                idx + 1,
                c.raid_type,
                c.stripe_size
                    .map(format_size)
                    .unwrap_or_else(|| "unknown".to_string()),
                c.score,
                c.evidence.stripe_votes
            ));
        }
        out
    }
}

fn detect_type_stats(
    images: &ImageSet,
    block_size: usize,
    max_blocks: usize,
) -> Result<TypeStats, String> {
    let mut readers = images
        .images
        .iter()
        .map(|image| BlockReader::open(image, block_size))
        .collect::<Result<Vec<_>, _>>()?;
    let mut stats = TypeStats::default();

    for _ in 0..max_blocks {
        let mut blocks = Vec::with_capacity(readers.len());
        for reader in &mut readers {
            let Some(block) = reader.next_block()? else {
                return Ok(stats);
            };
            blocks.push(block);
        }

        if blocks.iter().all(|b| is_zero(b)) {
            continue;
        }

        if blocks.windows(2).all(|w| w[0] == w[1]) {
            stats.mirrored_blocks += 1;
            continue;
        }

        let refs = blocks.iter().map(|b| b.as_slice()).collect::<Vec<_>>();
        if xor_is_zero(&refs) {
            stats.parity_blocks += 1;
        } else {
            stats.unassigned_blocks += 1;
        }
    }
    Ok(stats)
}

fn detect_stripe_sizes(
    images: &ImageSet,
    block_size: usize,
    max_blocks: usize,
) -> Result<Vec<(usize, usize)>, String> {
    const LOW: f32 = 0.3;
    const HIGH: f32 = 7.3;
    const WINDOW: usize = 16;
    const CANDIDATES: &[usize] = &[
        1024 * 1024,
        512 * 1024,
        256 * 1024,
        128 * 1024,
        64 * 1024,
        32 * 1024,
        16 * 1024,
        8 * 1024,
        4 * 1024,
        2 * 1024,
        1024,
    ];

    let mut votes = CANDIDATES.iter().map(|s| (*s, 0usize)).collect::<Vec<_>>();
    for image in &images.images {
        let mut reader = BlockReader::open(image, block_size)?;
        let mut entropies = Vec::new();
        let mut offsets = Vec::new();
        for idx in 0..max_blocks {
            let Some(block) = reader.next_block()? else {
                break;
            };
            entropies.push(entropy(&block));
            offsets.push(idx * block_size);
        }
        let edges = entropy_edges(&entropies, &offsets, LOW, HIGH, WINDOW);
        for pair in edges.windows(2) {
            let distance = pair[1].abs_diff(pair[0]);
            if distance == 0 {
                continue;
            }
            for (stripe, count) in &mut votes {
                if distance % *stripe == 0 {
                    *count += 1;
                    break;
                }
            }
        }
    }
    votes.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(votes)
}

fn entropy_edges(
    entropies: &[f32],
    offsets: &[usize],
    low: f32,
    high: f32,
    window: usize,
) -> Vec<usize> {
    if entropies.len() < (window * 2) + 1 {
        return Vec::new();
    }
    let mut edges = Vec::new();
    for idx in window..(entropies.len() - window) {
        let before = &entropies[idx - window..idx];
        let after = &entropies[idx..idx + window];
        let low_to_high = before.iter().all(|v| *v <= low) && after.iter().all(|v| *v >= high);
        let high_to_low = before.iter().all(|v| *v >= high) && after.iter().all(|v| *v <= low);
        if low_to_high || high_to_low {
            edges.push(offsets[idx]);
        }
    }
    edges
}

fn candidate_raid_types(
    stats: &TypeStats,
    image_count: usize,
    forced: String,
) -> Vec<(String, f32, String)> {
    if forced != "auto" {
        return vec![(
            normalize_raid_name(&forced),
            1.0,
            "user-forced RAID type".to_string(),
        )];
    }

    let total =
        (stats.mirrored_blocks + stats.parity_blocks + stats.unassigned_blocks).max(1) as f32;
    let mirrored = stats.mirrored_blocks as f32 / total;
    let parity = stats.parity_blocks as f32 / total;
    let unassigned = stats.unassigned_blocks as f32 / total;

    let mut candidates = Vec::new();
    candidates.push((
        "raid1".to_string(),
        mirrored,
        "high mirrored block ratio indicates RAID-1".to_string(),
    ));
    if image_count >= 3 {
        candidates.push((
            "raid5".to_string(),
            parity,
            "XOR-zero rows indicate complete RAID-5 parity".to_string(),
        ));
        let degraded_score = ((mirrored + parity) * 0.5).min(0.8);
        candidates.push((
            "raid5-degraded".to_string(),
            degraded_score,
            "some mirrored/parity traces may indicate degraded RAID-5".to_string(),
        ));
    }
    candidates.push((
        "raid0".to_string(),
        unassigned,
        "few redundancy hits indicate RAID-0 or unknown striping".to_string(),
    ));
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    candidates
}

fn normalize_raid_name(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "0" | "raid0" | "raid-0" => "raid0".to_string(),
        "1" | "raid1" | "raid-1" => "raid1".to_string(),
        "5" | "raid5" | "raid-5" => "raid5".to_string(),
        other => other.to_string(),
    }
}

fn format_size(size: usize) -> String {
    if size % (1024 * 1024) == 0 {
        format!("{}M", size / 1024 / 1024)
    } else if size % 1024 == 0 {
        format!("{}K", size / 1024)
    } else {
        size.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detects_simple_raid1_images() {
        let dir = std::env::temp_dir().join(unique_name("raid1-detect"));
        fs::create_dir_all(&dir).unwrap();
        let disk0 = dir.join("disk0.img");
        let disk1 = dir.join("disk1.img");
        let mut data = Vec::new();
        for i in 0..128u8 {
            data.extend([i; 512]);
        }
        fs::write(&disk0, &data).unwrap();
        fs::write(&disk1, &data).unwrap();

        let images = ImageSet::open(vec![disk0, disk1]).unwrap();
        let report = detect_raid(
            &images,
            DetectionOptions {
                block_size: 512,
                max_blocks: 128,
                top: 3,
                forced_raid: "auto".to_string(),
                forced_stripe: Some(64 * 1024),
            },
        )
        .unwrap();
        assert_eq!(report.candidates[0].raid_type, "raid1");

        let _ = fs::remove_dir_all(dir);
    }

    fn unique_name(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{prefix}-{nanos}")
    }
}

use crate::features::xor_into;
use crate::image::{ImageSet, read_block_at};
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn assemble(
    images: &ImageSet,
    raid: &str,
    stripe_size: usize,
    order: Option<Vec<usize>>,
    layout: Option<&str>,
    output: &Path,
) -> Result<(), String> {
    let order = normalized_order(order, images.images.len())?;
    match normalize_raid(raid).as_str() {
        "raid0" => assemble_raid0(images, stripe_size, &order, output),
        "raid1" => assemble_raid1(images, output),
        "raid5" => assemble_raid5(
            images,
            stripe_size,
            &order,
            layout.unwrap_or("left-symmetric"),
            output,
        ),
        other => Err(format!("unsupported assemble RAID type '{other}'")),
    }
}

pub fn recover_raid5_missing(
    images: &ImageSet,
    stripe_size: usize,
    missing_disk: usize,
    order: Option<Vec<usize>>,
    layout: &str,
    output: &Path,
) -> Result<(), String> {
    let full_disk_count = images.images.len() + 1;
    if missing_disk >= full_disk_count {
        return Err("--missing is outside the full RAID disk count".to_string());
    }
    let order = normalized_order(order, full_disk_count)?;
    let mut out =
        File::create(output).map_err(|err| format!("cannot create {}: {err}", output.display()))?;
    let rows = images.min_size() as usize / stripe_size;
    let available_by_logical = available_disk_map(full_disk_count, missing_disk);

    for row in 0..rows {
        let parity = parity_disk(row, full_disk_count, layout)?;
        for logical_disk in data_disk_order(row, full_disk_count, parity, layout)? {
            let offset = (row * stripe_size) as u64;
            let physical_disk = order[logical_disk];
            if physical_disk == missing_disk {
                let recovered = xor_available_blocks(
                    images,
                    stripe_size,
                    offset,
                    full_disk_count,
                    missing_disk,
                    &available_by_logical,
                )?;
                out.write_all(&recovered)
                    .map_err(|err| format!("write failed: {err}"))?;
            } else {
                let image_idx = available_by_logical[physical_disk]
                    .ok_or_else(|| "internal available disk map error".to_string())?;
                let block = read_block_at(&images.images[image_idx].path, offset, stripe_size)?;
                out.write_all(&block)
                    .map_err(|err| format!("write failed: {err}"))?;
            }
        }
    }
    Ok(())
}

fn assemble_raid0(
    images: &ImageSet,
    stripe_size: usize,
    order: &[usize],
    output: &Path,
) -> Result<(), String> {
    let mut out =
        File::create(output).map_err(|err| format!("cannot create {}: {err}", output.display()))?;
    let rows = images.min_size() as usize / stripe_size;
    for row in 0..rows {
        let offset = (row * stripe_size) as u64;
        for disk in order {
            let block = read_block_at(&images.images[*disk].path, offset, stripe_size)?;
            out.write_all(&block)
                .map_err(|err| format!("write failed: {err}"))?;
        }
    }
    Ok(())
}

fn assemble_raid1(images: &ImageSet, output: &Path) -> Result<(), String> {
    std::fs::copy(&images.images[0].path, output)
        .map(|_| ())
        .map_err(|err| format!("copy failed: {err}"))
}

fn assemble_raid5(
    images: &ImageSet,
    stripe_size: usize,
    order: &[usize],
    layout: &str,
    output: &Path,
) -> Result<(), String> {
    let n = images.images.len();
    if n < 3 {
        return Err("RAID-5 assemble requires at least three images".to_string());
    }
    let mut out =
        File::create(output).map_err(|err| format!("cannot create {}: {err}", output.display()))?;
    let rows = images.min_size() as usize / stripe_size;
    for row in 0..rows {
        let parity = parity_disk(row, n, layout)?;
        let offset = (row * stripe_size) as u64;
        for logical_disk in data_disk_order(row, n, parity, layout)? {
            let physical_disk = order[logical_disk];
            let block = read_block_at(&images.images[physical_disk].path, offset, stripe_size)?;
            out.write_all(&block)
                .map_err(|err| format!("write failed: {err}"))?;
        }
    }
    Ok(())
}

fn xor_available_blocks(
    images: &ImageSet,
    stripe_size: usize,
    offset: u64,
    full_disk_count: usize,
    missing_disk: usize,
    available_by_logical: &[Option<usize>],
) -> Result<Vec<u8>, String> {
    let mut acc = vec![0u8; stripe_size];
    for logical_disk in 0..full_disk_count {
        if logical_disk == missing_disk {
            continue;
        }
        let image_idx = available_by_logical[logical_disk]
            .ok_or_else(|| "internal available disk map error".to_string())?;
        let block = read_block_at(&images.images[image_idx].path, offset, stripe_size)?;
        xor_into(&mut acc[..block.len()], &block);
    }
    Ok(acc)
}

fn available_disk_map(full_disk_count: usize, missing_disk: usize) -> Vec<Option<usize>> {
    let mut out = vec![None; full_disk_count];
    let mut image_idx = 0usize;
    for (logical_disk, slot) in out.iter_mut().enumerate() {
        if logical_disk == missing_disk {
            continue;
        }
        *slot = Some(image_idx);
        image_idx += 1;
    }
    out
}

fn normalized_order(order: Option<Vec<usize>>, disk_count: usize) -> Result<Vec<usize>, String> {
    let order = order.unwrap_or_else(|| (0..disk_count).collect());
    if order.len() != disk_count {
        return Err(format!("order must contain {disk_count} entries"));
    }
    let mut seen = vec![false; disk_count];
    for disk in &order {
        if *disk >= disk_count {
            return Err(format!("disk order entry {disk} is out of range"));
        }
        if seen[*disk] {
            return Err(format!("disk order entry {disk} is duplicated"));
        }
        seen[*disk] = true;
    }
    Ok(order)
}

fn normalize_raid(raid: &str) -> String {
    match raid.to_ascii_lowercase().as_str() {
        "0" | "raid0" | "raid-0" => "raid0".to_string(),
        "1" | "raid1" | "raid-1" => "raid1".to_string(),
        "5" | "raid5" | "raid-5" => "raid5".to_string(),
        other => other.to_string(),
    }
}

fn parity_disk(row: usize, disk_count: usize, layout: &str) -> Result<usize, String> {
    match layout {
        "left-symmetric" | "left-asymmetric" => Ok((disk_count - 1) - (row % disk_count)),
        "right-symmetric" | "right-asymmetric" => Ok(row % disk_count),
        other => Err(format!("unsupported RAID-5 layout '{other}'")),
    }
}

fn data_disk_order(
    row: usize,
    disk_count: usize,
    parity: usize,
    layout: &str,
) -> Result<Vec<usize>, String> {
    let mut disks = Vec::with_capacity(disk_count - 1);
    match layout {
        "left-symmetric" | "right-symmetric" => {
            let mut disk = (parity + 1) % disk_count;
            while disks.len() < disk_count - 1 {
                if disk != parity {
                    disks.push(disk);
                }
                disk = (disk + 1) % disk_count;
            }
        }
        "left-asymmetric" | "right-asymmetric" => {
            for disk in 0..disk_count {
                if disk != parity {
                    disks.push(disk);
                }
            }
            if row % 2 == 1 && layout.contains("right") {
                disks.reverse();
            }
        }
        other => return Err(format!("unsupported RAID-5 layout '{other}'")),
    }
    Ok(disks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn left_symmetric_parity_rotates_from_right() {
        assert_eq!(parity_disk(0, 4, "left-symmetric").unwrap(), 3);
        assert_eq!(parity_disk(1, 4, "left-symmetric").unwrap(), 2);
        assert_eq!(parity_disk(2, 4, "left-symmetric").unwrap(), 1);
        assert_eq!(parity_disk(3, 4, "left-symmetric").unwrap(), 0);
    }

    #[test]
    fn assembles_simple_raid0() {
        let dir = std::env::temp_dir().join(unique_name("raid0-assemble"));
        fs::create_dir_all(&dir).unwrap();
        let disk0 = dir.join("disk0.img");
        let disk1 = dir.join("disk1.img");
        let output = dir.join("logical.img");
        fs::write(&disk0, [b'A'; 4]).unwrap();
        fs::write(&disk1, [b'B'; 4]).unwrap();

        let images = ImageSet::open(vec![disk0, disk1]).unwrap();
        assemble_raid0(&images, 2, &[0, 1], &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"AABBAABB");

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

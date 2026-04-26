use crate::detect::{DetectionOptions, detect_raid};
use crate::image::ImageSet;
use crate::raid::{assemble, recover_raid5_missing};
use crate::report::{write_detection_json, write_detection_markdown};
use std::path::PathBuf;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.get(1).map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "scan" => scan_cmd(&args[2..]),
        "detect" => detect_cmd(&args[2..]),
        "assemble" => assemble_cmd(&args[2..]),
        "recover" => recover_cmd(&args[2..]),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

fn scan_cmd(args: &[String]) -> Result<(), String> {
    let opts = parse_common(args)?;
    let images = ImageSet::open(opts.images)?;
    let summary = images.scan_summary(opts.block_size, opts.max_blocks)?;
    println!("RAID-reassemble scan");
    println!("images: {}", summary.images.len());
    println!("block_size: {}", summary.block_size);
    println!(
        "sampled_blocks_per_image: {}",
        summary.sampled_blocks_per_image
    );
    for image in summary.images {
        println!(
            "{}\tsize={} bytes\tzero_blocks={}\tmean_entropy={:.4}",
            image.path.display(),
            image.size,
            image.zero_blocks,
            image.mean_entropy
        );
    }
    Ok(())
}

fn detect_cmd(args: &[String]) -> Result<(), String> {
    let opts = parse_common(args)?;
    let output = value_after(args, "--output").or_else(|| value_after(args, "-o"));
    let markdown = value_after(args, "--markdown");
    let top = value_after(args, "--top")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5);
    let stripe = value_after(args, "--stripe")
        .map(|v| parse_size(&v))
        .transpose()?;
    let raid = value_after(args, "--raid").unwrap_or_else(|| "auto".to_string());

    let images = ImageSet::open(opts.images)?;
    let detection = detect_raid(
        &images,
        DetectionOptions {
            block_size: opts.block_size,
            max_blocks: opts.max_blocks,
            top,
            forced_raid: raid,
            forced_stripe: stripe,
        },
    )?;

    println!("{}", detection.human_summary());
    if let Some(path) = output {
        write_detection_json(&detection, PathBuf::from(path).as_path())?;
    }
    if let Some(path) = markdown {
        write_detection_markdown(&detection, PathBuf::from(path).as_path())?;
    }
    Ok(())
}

fn assemble_cmd(args: &[String]) -> Result<(), String> {
    let opts = parse_common(args)?;
    let output = required_value(args, "--output")?;
    let raid = required_value(args, "--raid")?;
    let stripe = parse_size(&required_value(args, "--stripe")?)?;
    let order = value_after(args, "--order")
        .map(|v| parse_order(&v))
        .transpose()?;
    let layout = value_after(args, "--layout");

    let images = ImageSet::open(opts.images)?;
    assemble(
        &images,
        raid.as_str(),
        stripe,
        order,
        layout.as_deref(),
        PathBuf::from(output).as_path(),
    )
}

fn recover_cmd(args: &[String]) -> Result<(), String> {
    let opts = parse_common(args)?;
    let output = required_value(args, "--output")?;
    let missing = required_value(args, "--missing")?
        .parse::<usize>()
        .map_err(|_| "--missing must be a zero-based disk index".to_string())?;
    let stripe = parse_size(&required_value(args, "--stripe")?)?;
    let order = value_after(args, "--order")
        .map(|v| parse_order(&v))
        .transpose()?;
    let layout = value_after(args, "--layout").unwrap_or_else(|| "left-symmetric".to_string());

    let images = ImageSet::open(opts.images)?;
    recover_raid5_missing(
        &images,
        stripe,
        missing,
        order,
        layout.as_str(),
        PathBuf::from(output).as_path(),
    )
}

struct CommonOptions {
    images: Vec<PathBuf>,
    block_size: usize,
    max_blocks: usize,
}

fn parse_common(args: &[String]) -> Result<CommonOptions, String> {
    let block_size = value_after(args, "--block-size")
        .map(|v| parse_size(&v))
        .transpose()?
        .unwrap_or(512);
    let max_blocks = value_after(args, "--max-blocks")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1_500_000);

    let mut images = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with('-') {
            i += if option_takes_value(arg.as_str()) {
                2
            } else {
                1
            };
            continue;
        }
        images.push(PathBuf::from(arg));
        i += 1;
    }
    if images.is_empty() {
        return Err("at least one image path is required".to_string());
    }
    Ok(CommonOptions {
        images,
        block_size,
        max_blocks,
    })
}

fn option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--block-size"
            | "--max-blocks"
            | "--output"
            | "-o"
            | "--markdown"
            | "--top"
            | "--stripe"
            | "--raid"
            | "--order"
            | "--layout"
            | "--missing"
    )
}

fn value_after(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn required_value(args: &[String], name: &str) -> Result<String, String> {
    value_after(args, name).ok_or_else(|| format!("{name} is required"))
}

fn parse_order(value: &str) -> Result<Vec<usize>, String> {
    value
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .map_err(|_| format!("invalid disk order entry '{part}'"))
        })
        .collect()
}

fn parse_size(value: &str) -> Result<usize, String> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let (number, multiplier) = if let Some(num) = lower.strip_suffix("kb") {
        (num, 1024usize)
    } else if let Some(num) = lower.strip_suffix('k') {
        (num, 1024usize)
    } else if let Some(num) = lower.strip_suffix("mb") {
        (num, 1024usize * 1024)
    } else if let Some(num) = lower.strip_suffix('m') {
        (num, 1024usize * 1024)
    } else {
        (lower.as_str(), 1usize)
    };
    number
        .trim()
        .parse::<usize>()
        .map(|n| n * multiplier)
        .map_err(|_| format!("invalid size '{value}'"))
}

fn print_help() {
    println!(
        "RAID-reassemble\n\n\
Usage:\n  \
raid-reassemble scan <images...> [--block-size 512] [--max-blocks N]\n  \
raid-reassemble detect <images...> [--raid auto|raid0|raid1|raid5] [--stripe 256K] [--output result.json] [--markdown report.md]\n  \
raid-reassemble assemble <images...> --raid raid0|raid1|raid5 --stripe SIZE --output logical.img [--order 0,1,2,3] [--layout left-symmetric]\n  \
raid-reassemble recover <images...> --stripe SIZE --missing INDEX --output logical.img [--order 0,1,2,3] [--layout left-symmetric]\n"
    );
}

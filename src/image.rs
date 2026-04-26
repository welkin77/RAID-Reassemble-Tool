use crate::features::{entropy, is_zero};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub struct ImageSet {
    pub images: Vec<Image>,
}

pub struct Image {
    pub path: PathBuf,
    pub size: u64,
}

pub struct ScanSummary {
    pub block_size: usize,
    pub sampled_blocks_per_image: usize,
    pub images: Vec<ImageSummary>,
}

pub struct ImageSummary {
    pub path: PathBuf,
    pub size: u64,
    pub zero_blocks: usize,
    pub mean_entropy: f32,
}

impl ImageSet {
    pub fn open(paths: Vec<PathBuf>) -> Result<Self, String> {
        let mut images = Vec::new();
        for path in paths {
            let meta = std::fs::metadata(&path)
                .map_err(|err| format!("cannot read metadata for {}: {err}", path.display()))?;
            if !meta.is_file() {
                return Err(format!("{} is not a file", path.display()));
            }
            images.push(Image {
                path,
                size: meta.len(),
            });
        }
        Ok(Self { images })
    }

    pub fn scan_summary(
        &self,
        block_size: usize,
        max_blocks: usize,
    ) -> Result<ScanSummary, String> {
        let mut summaries = Vec::new();
        for image in &self.images {
            let mut reader = BlockReader::open(image, block_size)?;
            let mut zero_blocks = 0usize;
            let mut entropy_sum = 0f32;
            let mut count = 0usize;
            while count < max_blocks {
                let Some(block) = reader.next_block()? else {
                    break;
                };
                if is_zero(&block) {
                    zero_blocks += 1;
                }
                entropy_sum += entropy(&block);
                count += 1;
            }
            summaries.push(ImageSummary {
                path: image.path.clone(),
                size: image.size,
                zero_blocks,
                mean_entropy: if count == 0 {
                    0.0
                } else {
                    entropy_sum / count as f32
                },
            });
        }
        Ok(ScanSummary {
            block_size,
            sampled_blocks_per_image: max_blocks,
            images: summaries,
        })
    }

    pub fn min_size(&self) -> u64 {
        self.images.iter().map(|i| i.size).min().unwrap_or(0)
    }
}

pub struct BlockReader {
    file: File,
    block_size: usize,
    buffer: Vec<u8>,
}

impl BlockReader {
    pub fn open(image: &Image, block_size: usize) -> Result<Self, String> {
        if block_size == 0 {
            return Err("block size must be greater than zero".to_string());
        }
        let file = File::open(&image.path)
            .map_err(|err| format!("cannot open {}: {err}", image.path.display()))?;
        Ok(Self {
            file,
            block_size,
            buffer: vec![0u8; block_size],
        })
    }

    pub fn next_block(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut read = 0usize;
        while read < self.block_size {
            let n = self
                .file
                .read(&mut self.buffer[read..])
                .map_err(|err| format!("read failed: {err}"))?;
            if n == 0 {
                break;
            }
            read += n;
        }
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(self.buffer[..read].to_vec()))
    }
}

pub fn read_block_at(path: &PathBuf, offset: u64, size: usize) -> Result<Vec<u8>, String> {
    let mut file =
        File::open(path).map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("seek failed for {}: {err}", path.display()))?;
    let mut buf = vec![0u8; size];
    let read = file
        .read(&mut buf)
        .map_err(|err| format!("read failed for {}: {err}", path.display()))?;
    buf.truncate(read);
    Ok(buf)
}

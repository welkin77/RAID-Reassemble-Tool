pub fn entropy(block: &[u8]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    if block.iter().all(|b| *b == 0) {
        return 0.0;
    }

    let mut counts = [0usize; 256];
    for byte in block {
        counts[*byte as usize] += 1;
    }

    let len = block.len() as f32;
    counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let p = *count as f32 / len;
            -p * p.log2()
        })
        .sum()
}

pub fn is_zero(block: &[u8]) -> bool {
    block.iter().all(|b| *b == 0)
}

pub fn xor_into(dst: &mut [u8], src: &[u8]) {
    for (left, right) in dst.iter_mut().zip(src.iter()) {
        *left ^= *right;
    }
}

pub fn xor_is_zero(blocks: &[&[u8]]) -> bool {
    let Some(first) = blocks.first() else {
        return false;
    };
    let mut acc = vec![0u8; first.len()];
    for block in blocks {
        if block.len() != acc.len() {
            return false;
        }
        xor_into(&mut acc, block);
    }
    is_zero(&acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_zero_block_is_zero() {
        assert_eq!(entropy(&[0u8; 512]), 0.0);
    }

    #[test]
    fn entropy_uniform_bytes_is_high() {
        let mut block = Vec::new();
        for _ in 0..2 {
            for byte in 0u8..=255 {
                block.push(byte);
            }
        }
        assert!((entropy(&block) - 8.0).abs() < 0.001);
    }

    #[test]
    fn xor_zero_detects_parity_row() {
        let a = [1u8, 2, 3];
        let b = [4u8, 5, 6];
        let p = [1 ^ 4, 2 ^ 5, 3 ^ 6];
        assert!(xor_is_zero(&[&a, &b, &p]));
    }
}

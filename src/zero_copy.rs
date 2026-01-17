use anyhow::{anyhow, Result};
use bytemuck::Pod;
use sha2::{Digest, Sha256};

pub const DISCRIMINATOR_LEN: usize = 8;

pub fn discriminator(input: &[u8]) -> [u8; DISCRIMINATOR_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    let digest = hasher.finalize();
    let mut out = [0u8; DISCRIMINATOR_LEN];
    out.copy_from_slice(&digest[..DISCRIMINATOR_LEN]);
    out
}

pub fn checked_from_bytes_with_discriminator<T: Pod>(
    data: &[u8],
    expected: [u8; DISCRIMINATOR_LEN],
) -> Result<&T> {
    if data.len() < DISCRIMINATOR_LEN {
        return Err(anyhow!("account data too short for discriminator"));
    }

    if data[..DISCRIMINATOR_LEN] != expected {
        return Err(anyhow!("discriminator mismatch"));
    }

    let body = &data[DISCRIMINATOR_LEN..];
    let expected_len = std::mem::size_of::<T>();
    if body.len() < expected_len {
        return Err(anyhow!("account data too short for struct"));
    }

    Ok(bytemuck::from_bytes(&body[..expected_len]))
}

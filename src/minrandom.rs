use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::mem;

pub fn fill_buf(bytes: &mut [u8]) {
    bytes
        .chunks_mut(mem::size_of::<u32>())
        .zip(random_numbers(random_seed() as u32).map(|n| n.to_be_bytes()))
        .for_each(|(chunk, rand)| chunk.copy_from_slice(&rand[..chunk.len()]))
}

// https://github.com/matklad/config/blob/b8ea0aad0f86d4575651a390a3c7aefb63229774/templates/snippets/src/lib.rs#L28L42
// See also: https://blog.orhun.dev/zero-deps-random-in-rust/
pub fn random_numbers(seed: u32) -> impl Iterator<Item = u32> {
    // https://github.com/rust-lang/rust/blob/1.55.0/library/core/src/slice/sort.rs#L559-L573
    // Pseudorandom number generator from the "Xorshift RNGs" paper by George Marsaglia.
    let mut random = seed;
    std::iter::repeat_with(move || {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        random
    })
}

pub fn random_seed() -> u64 {
    RandomState::new().build_hasher().finish()
}

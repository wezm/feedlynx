/*

Imported from: https://github.com/viz-rs/nano-id/blob/a9022772b2f1ce38929b5b81eccc670ac9d3ab23/src/lib.rs

MIT/Apache-2.0

*/

//! A tiny, secure, URL-friendly, unique string ID generator

use crate::minrandom;

#[macro_export]
macro_rules! gen {
    ($mod:tt, $len:tt, $alphabet:tt) => {
        #[doc = concat!(" Nanoid with alphabet table `", stringify!($alphabet), "`")]
        mod $mod {
            pub const MASK: usize = ($len as usize).next_power_of_two() - 1;
            pub const ALPHABET: &'static [u8; $len] = $alphabet;
        }

        #[doc = concat!(" Nanoid with ", stringify!($mod))]
        #[must_use]
        pub fn $mod<const N: usize>() -> String {
            // TODO: If https://github.com/rust-lang/rust/issues/76560 is stabilised this
            // can probably just be an array.
            let mut bytes = vec![0u8; 8 * N / 5];
            let mut id = String::with_capacity(N);

            loop {
                minrandom::fill_buf(&mut bytes);

                for byte in &bytes {
                    let idx = *byte as usize & $mod::MASK;
                    if idx < $len {
                        id.push($mod::ALPHABET[idx] as char)
                    }
                    if id.len() == N {
                        return id;
                    }
                }
            }
        }
    };
}

gen!(
    base62,
    62,
    b"ModuleSymbhasOwnPr0123456789ABCDEFGHNRVfgctiUvzKqYTJkLxpZXIjQW"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_base62() {
        let id = base62::<21>();
        println!("{}", &id);
        assert_eq!(id.len(), 21);
    }

    #[test]
    fn generates_uid() {
        gen!(
            uid,
            64,
            b"_-0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
        );

        let id = uid::<21>();
        println!("{}", &id);
        assert_eq!(id.len(), 21);
    }

    #[test]
    fn symbols() {
        use std::collections::BTreeMap;

        let mut counts = BTreeMap::new();

        for _ in 0..1_000_000 {
            let id = base62::<10>();
            for c in id.chars() {
                *counts.entry(c).or_insert(0) += 1;
            }
        }

        println!("{} symbols generated", counts.len());
        for (c, count) in &counts {
            println!("{}: {}", c, count);
        }

        assert_eq!(counts.len(), 62);
    }
}

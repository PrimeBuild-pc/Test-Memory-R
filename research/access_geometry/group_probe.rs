//! Isolated 256-bit code-generation comparison, not a TMR kernel or hardware benchmark.
//! Compile with nightly, -O -C target-cpu=x86-64-v3 --crate-type lib --emit asm.
//! Preconditions: aligned buffer, positive period/group, group divides period and length.
#![feature(portable_simd)]

use std::simd::{num::SimdUint, u64x4};

const WEYL: u64 = 0x9e37_79b9_7f4a_7c15;

#[inline(always)]
fn expected(lid: u64, key: u64) -> [u64x4; 2] {
    let base = u64x4::splat((lid << 3).wrapping_mul(WEYL).wrapping_add(key));
    std::array::from_fn(|v| {
        let offsets = u64x4::from_array(std::array::from_fn(|j| ((v * 4 + j) as u64).wrapping_mul(WEYL)));
        let x = base + offsets;
        x ^ (x >> u64x4::splat(32))
    })
}

#[inline(always)]
fn opaque(mut p: *const u64) -> *const u64 {
    unsafe { std::arch::asm!("/* {0} */", inout(reg) p, options(pure, readonly, nostack, preserves_flags)) };
    p
}

macro_rules! pair {
    ($p:expr, $at:expr, $key:expr, $pf:expr, $acc:expr) => {{
        let p = $p;
        // One pair uses all four accumulators, including in the runtime-group candidate.
        for j in 0..2 {
            let q = p.add(j * 8);
            core::arch::x86_64::_mm_prefetch::<2>(q.wrapping_add($pf).cast());
            let e = expected((q.addr().wrapping_sub($at)) as u64 >> 6, $key);
            $acc[j * 2] |= *q.cast::<u64x4>() ^ e[0];
            $acc[j * 2 + 1] |= *q.add(4).cast::<u64x4>() ^ e[1];
        }
    }};
}

macro_rules! group {
    ([$($offset:literal),*] $p:ident, $g:expr, $at:ident, $key:ident, $pf:ident, $acc:ident) => {
        $(pair!($p.add($offset), $at, $key, $pf, $acc);)*
    };
    (fixed $p:ident, $g:expr, $at:ident, $key:ident, $pf:ident, $acc:ident) => {
        for pair in 0..$g / 2 {
            pair!($p.add(pair * 16), $at, $key, $pf, $acc);
        }
    };
    (runtime $p:ident, $g:expr, $at:ident, $key:ident, $pf:ident, $acc:ident) => {{
        let mut tile = $p;
        let end = $p.add($g * 8);
        while tile < end {
            pair!(tile, $at, $key, $pf, $acc);
            tile = opaque(tile.add(16));
        }
    }};
}

macro_rules! walk {
    ($form:tt, $base:ident, $n:ident, $p:ident, $g:expr, $key:ident) => {{
        let g = $g;
        let period = $p * 8;
        let ahead = (128 / g).min($n / $p) * period;
        let at = $base.addr();
        let stop = $base.add($n * 8);
        let mut first = $base.add($p.min($n) * 8);
        let mut acc = [u64x4::splat(0); 4];
        for _ in 0..$p.min($n) / g {
            first = first.sub(g * 8);
            let mut p = first;
            let pf = ahead;
            while p.wrapping_add(ahead) < stop {
                group!($form p, $g, at, $key, pf, acc);
                p = opaque(p.wrapping_add(period));
            }
            let left = (stop.addr() - first.addr()) / 8;
            let pf = ahead.wrapping_sub(left.div_ceil(period) * period + g * 8);
            while p < stop {
                group!($form p, $g, at, $key, pf, acc);
                p = opaque(p.wrapping_add(period));
            }
        }
        ((acc[0] | acc[1]) | (acc[2] | acc[3])).reduce_or()
    }};
}

macro_rules! fixed {
    ($name:ident, $g:literal) => {
        #[unsafe(no_mangle)]
        #[target_feature(enable = "avx2")]
        pub unsafe fn $name(base: *const u64, n: usize, period: usize, key: u64) -> u64 {
            // SAFETY: the probe's caller contract supplies an aligned, live buffer and complete groups.
            unsafe { walk!(fixed, base, n, period, $g, key) }
        }
    };
}

fixed!(group2, 2);
fixed!(group4, 4);
fixed!(group8, 8);
fixed!(group16, 16);

#[unsafe(no_mangle)]
#[target_feature(enable = "avx2")]
pub unsafe fn expanded16(base: *const u64, n: usize, period: usize, key: u64) -> u64 {
    // SAFETY: the probe's caller contract supplies an aligned, live buffer and complete groups.
    unsafe { walk!([0, 16, 32, 48, 64, 80, 96, 112], base, n, period, 16, key) }
}

#[unsafe(no_mangle)]
#[target_feature(enable = "avx2")]
pub unsafe fn runtime_group(base: *const u64, n: usize, period: usize, group: usize, key: u64) -> u64 {
    // SAFETY: the probe's caller contract supplies an aligned, live buffer and complete groups.
    unsafe { walk!(runtime, base, n, period, group, key) }
}

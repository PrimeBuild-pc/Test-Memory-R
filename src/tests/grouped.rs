//! Grouped strided visits (TODO 126). The line offsets are macro tokens: no group-size dispatch inside the memory loop.

/// Descending offset passes, ascending lines within each visit. G divides both the period and chunk length; callers validate it before
/// allocating. Prefetch one line per visited line, at most 128 visited lines ahead and no further than the shortest offset pass.
macro_rules! grouped_walk {
    ($prefetch:expr, $base:expr, $start:expr, $end:expr, $period:expr, $lid0:expr,
     [$($j:literal),+], |$p:ident, $lid:ident, $index:ident| $body:block) => {{
        use $crate::tests::{LINE_WORDS, STRIDED_AHEAD, opaque_ptr};
        const WALK_GROUP: usize = [$($j),+].len();
        const _: () = {
            let offsets = [$($j),+];
            let mut i = 0;
            while i < WALK_GROUP {
                assert!(offsets[i] == i, "group offsets must be consecutive, starting at zero");
                i += 1;
            }
        };
        let (base, start, end, period, lid0): (*mut u64, usize, usize, usize, u64) = ($base, $start, $end, $period, $lid0);
        debug_assert!(period >= WALK_GROUP * LINE_WORDS && period.is_multiple_of(WALK_GROUP * LINE_WORDS));
        debug_assert!(start.is_multiple_of(LINE_WORDS) && (end - start).is_multiple_of(WALK_GROUP * LINE_WORDS));
        let ahead = (STRIDED_AHEAD / WALK_GROUP).min((end - start) / period) * period;
        let stop = base.wrapping_add(end);
        let at = (base.wrapping_add(start) as u64).wrapping_sub(lid0 << 6);
        let offsets = period.min(end - start);
        let mut first = base.wrapping_add(start + offsets);
        for _ in 0..offsets / (WALK_GROUP * LINE_WORDS) {
            first = first.wrapping_sub(WALK_GROUP * LINE_WORDS);
            let mut group = first;
            while group.wrapping_add(ahead) < stop {
                $({
                    let $p = group.add($j * LINE_WORDS);
                    let $lid = ($p as u64).wrapping_sub(at) >> 6;
                    let $index = $j;
                    $prefetch($p.wrapping_add(ahead));
                    $body
                })+
                group = opaque_ptr(group.wrapping_add(period));
            }
            let left = (stop as usize).saturating_sub(first as usize) / std::mem::size_of::<u64>();
            let back = ahead.wrapping_sub(left.div_ceil(period) * period + WALK_GROUP * LINE_WORDS);
            while group < stop {
                $({
                    let $p = group.add($j * LINE_WORDS);
                    let $lid = ($p as u64).wrapping_sub(at) >> 6;
                    let $index = $j;
                    $prefetch($p.wrapping_add(back));
                    $body
                })+
                group = opaque_ptr(group.wrapping_add(period));
            }
        }
    }};
}

pub(super) use grouped_walk;

#[cfg(test)]
mod tests {
    #[test]
    fn grouped_visits_and_prefetches_match_the_complete_order() {
        for n in [64, 192, 320, 4096] {
            for g in [1, 2, 4, 8, 16] {
                for period in [g, 3 * g, 16 * g, n - g, n, n + g, 2 * n + g] {
                    let pad = 3;
                    let mut memory = vec![std::simd::u64x8::splat(0); n + 2 * pad];
                    let base = memory.as_mut_ptr().cast::<u64>();
                    let lid0 = crate::pattern::line_base(7) + pad as u64;
                    let mut visited = Vec::new();
                    let mut prefetched = Vec::new();
                    let offset = |p: *const u64| (p.addr().wrapping_sub(base.addr()) as isize / 64) - pad as isize;
                    let mut prefetch = |p: *const u64| prefetched.push(offset(p));
                    macro_rules! run {
                        ([$($j:literal),+]) => {
                            // SAFETY: every full group is inside the aligned live buffer; the recorder never reads a prefetch address.
                            unsafe {
                                grouped_walk!(prefetch, base, pad * LINE_WORDS, (pad + n) * LINE_WORDS, period * LINE_WORDS, lid0,
                                              [$($j),+], |p, lid, _j| {
                                    visited.push((offset(p) as usize, lid));
                                });
                            }
                        };
                    }
                    match g {
                        1 => run!([0]),
                        2 => run!([0, 1]),
                        4 => run!([0, 1, 2, 3]),
                        8 => run!([0, 1, 2, 3, 4, 5, 6, 7]),
                        16 => run!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
                        _ => unreachable!(),
                    }
                    // Independent specification: sort all valid line numbers by offset-group descending, then period, then group lane.
                    let mut expected: Vec<_> = (0..n).collect();
                    expected.sort_unstable_by_key(|&line| (std::cmp::Reverse((line % period) / g), line / period, line % g));
                    assert_eq!(visited.iter().map(|&(line, _)| line).collect::<Vec<_>>(), expected, "{n}/{period}/{g}");
                    assert!(visited.iter().all(|&(line, lid)| lid == lid0 + line as u64));
                    let ahead = (128 / g).min(n / period) * g;
                    for i in 0..n - ahead {
                        assert_eq!(prefetched[i], expected[i + ahead] as isize, "{n}/{period}/{g}, prefetch {i}");
                    }
                    assert_eq!(prefetched.len(), n, "one prefetch per visited line");
                }
            }
        }
    }
}

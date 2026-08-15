#[inline]
pub(crate) fn lmr_reduction(depth: i32, move_index: i32) -> i32 {
    let d = depth as f64;
    let m = move_index as f64;
    (0.75 + (d.ln() * m.ln()) / 2.25) as i32
}

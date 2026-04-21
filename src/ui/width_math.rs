use crate::timing::TimingEntry;

pub(crate) fn max_text_width<F>(entries: &[TimingEntry], accessor: F) -> u16
where
    F: Fn(&TimingEntry) -> &str,
{
    entries
        .iter()
        .map(|entry| accessor(entry).chars().count())
        .max()
        .unwrap_or(1) as u16
}

pub(crate) fn distribute_extra_space<const N: usize>(widths: &mut [u16; N], mut extra: u16) {
    if extra == 0 {
        return;
    }
    let total: u32 = widths.iter().map(|w| *w as u32).sum();
    if total == 0 {
        return;
    }
    for width in widths.iter_mut() {
        let share = ((extra as u32 * *width as u32) / total) as u16;
        *width = width.saturating_add(share);
        extra = extra.saturating_sub(share);
    }
    let mut idx = 0usize;
    while extra > 0 {
        widths[idx] = widths[idx].saturating_add(1);
        extra -= 1;
        idx = (idx + 1) % N;
    }
}

pub(crate) fn reduce_widths_in_order<const N: usize>(
    widths: &mut [u16; N],
    minimums: &[u16; N],
    mut deficit: u16,
    indexes: &[usize],
) -> u16 {
    if deficit == 0 || indexes.is_empty() {
        return deficit;
    }
    let mut progressed = true;
    while deficit > 0 && progressed {
        progressed = false;
        for idx in indexes {
            if deficit == 0 {
                break;
            }
            if widths[*idx] > minimums[*idx] {
                widths[*idx] -= 1;
                deficit -= 1;
                progressed = true;
            }
        }
    }
    deficit
}

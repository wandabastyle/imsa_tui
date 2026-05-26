use crate::timing::TimingEntry;

pub fn max_text_width<F>(entries: &[TimingEntry], accessor: F) -> u16
where
   F: Fn(&TimingEntry) -> &str,
{
   let max_len = entries
      .iter()
      .map(|entry| accessor(entry).chars().count())
      .max()
      .unwrap_or(1);
   u16::try_from(max_len).unwrap_or(u16::MAX)
}

pub fn distribute_extra_space<const N: usize>(widths: &mut [u16; N], mut extra: u16) {
   if extra == 0 {
      return;
   }
   let total: u32 = widths.iter().map(|w| u32::from(*w)).sum();
   if total == 0 {
      return;
   }
   for width in widths.iter_mut() {
      let share_u32 = (u32::from(extra) * u32::from(*width)) / total;
      let share = u16::try_from(share_u32).unwrap_or(u16::MAX);
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

pub fn reduce_widths_in_order<const N: usize>(
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

use ratatui::{
   layout::{
      Constraint,
      Direction,
      Layout,
      Rect,
   },
   widgets::{
      Block,
      Borders,
      Paragraph,
      TableState,
   },
   Frame,
};

use crate::{
   timing::TimingEntry,
   ui::{
      grouping::selected_group_idx,
      render::RenderCtx,
      table::{
         build_table,
         TableRenderCtx,
      },
   },
};

const GROUP_TABLE_CHROME_HEIGHT: u16 = 3;
const GROUP_MIN_VISIBLE_CAR_ROWS: u16 = 3;
const GROUP_TABLE_PREFERRED_MIN_HEIGHT: u16 =
   GROUP_TABLE_CHROME_HEIGHT + GROUP_MIN_VISIBLE_CAR_ROWS;

pub fn render_grouped(
   f: &mut Frame<'_>,
   ctx: &RenderCtx<'_>,
   current_groups: &[(String, Vec<TimingEntry>)],
   area: Rect,
) {
   if current_groups.is_empty() {
      render_empty(f, area);
      return;
   }

   let selected_group_idx = selected_group_idx(ctx.selected_row, current_groups);
   let minimum_rows_per_group = ctx
      .config
      .grouped_min_rows
      .max(GROUP_TABLE_PREFERRED_MIN_HEIGHT);
   let visible_group_count =
      calculate_visible_group_count(area.height, current_groups.len(), minimum_rows_per_group);

   let start_group_idx = calculate_start_group_idx(
      selected_group_idx,
      current_groups.len(),
      visible_group_count,
   );
   let end_group_idx = start_group_idx + visible_group_count;
   let visible_groups = &current_groups[start_group_idx..end_group_idx];

   let group_chunks = create_group_layout(visible_groups, area, minimum_rows_per_group);
   let mut global_offset = calculate_global_offset(current_groups, start_group_idx);

   render_visible_groups(f, ctx, visible_groups, &group_chunks, &mut global_offset);
}

fn render_empty(f: &mut Frame<'_>, area: Rect) {
   let waiting = Paragraph::new("No grouped class data available yet.")
      .block(Block::default().title("Grouped").borders(Borders::ALL));
   f.render_widget(waiting, area);
}

fn calculate_start_group_idx(
   selected_group_idx: usize,
   total_groups: usize,
   visible_group_count: usize,
) -> usize {
   if total_groups <= visible_group_count {
      return 0;
   }
   let half = visible_group_count / 2;
   selected_group_idx
      .saturating_sub(half)
      .min(total_groups - visible_group_count)
}

fn calculate_visible_group_count(
   area_height: u16,
   total_groups: usize,
   minimum_rows_per_group: u16,
) -> usize {
   if total_groups == 0 {
      return 0;
   }

   let min_rows = minimum_rows_per_group.max(GROUP_TABLE_PREFERRED_MIN_HEIGHT);
   let max_by_min = usize::from((area_height / min_rows).max(1));
   total_groups.min(max_by_min).max(1)
}

fn create_group_layout(
   visible_groups: &[(String, Vec<TimingEntry>)],
   area: Rect,
   minimum_rows_per_group: u16,
) -> Vec<Rect> {
   let total_cars: usize = visible_groups
      .iter()
      .map(|(_, entries)| entries.len())
      .sum();

   let constraints: Vec<Constraint> = visible_groups
      .iter()
      .map(|(_, entries)| {
         let min_rows = group_min_height(entries.len(), minimum_rows_per_group).min(area.height);
         let target_rows =
            proportional_rows(entries.len(), total_cars, visible_groups.len(), area.height);
         Constraint::Length(target_rows.clamp(min_rows, area.height))
      })
      .collect();

   Layout::default()
      .direction(Direction::Vertical)
      .constraints(constraints)
      .split(area)
      .to_vec()
}

fn group_min_height(entries_len: usize, minimum_rows_per_group: u16) -> u16 {
   let available_car_rows = u16::try_from(entries_len)
      .unwrap_or(GROUP_MIN_VISIBLE_CAR_ROWS)
      .clamp(1, GROUP_MIN_VISIBLE_CAR_ROWS);
   let available_height = GROUP_TABLE_CHROME_HEIGHT + available_car_rows;

   minimum_rows_per_group
      .max(GROUP_TABLE_PREFERRED_MIN_HEIGHT)
      .min(available_height)
}

fn proportional_rows(
   entries_len: usize,
   total_cars: usize,
   group_count: usize,
   area_height: u16,
) -> u16 {
   let area_u128 = u128::from(area_height);
   if total_cars > 0 {
      let entries = u128::try_from(entries_len).unwrap_or(u128::MAX);
      let total = u128::try_from(total_cars).unwrap_or(u128::MAX);
      let rounded = (entries.saturating_mul(area_u128).saturating_add(total / 2)) / total;
      u16::try_from(rounded).unwrap_or(area_height)
   } else {
      let groups = u128::try_from(group_count).unwrap_or(1);
      let rounded = (area_u128.saturating_add(groups / 2)) / groups;
      u16::try_from(rounded).unwrap_or(area_height)
   }
}

fn calculate_global_offset(
   current_groups: &[(String, Vec<TimingEntry>)],
   start_group_idx: usize,
) -> usize {
   current_groups
      .iter()
      .take(start_group_idx)
      .map(|(_, entries)| entries.len())
      .sum::<usize>()
}

fn render_visible_groups(
   f: &mut Frame<'_>,
   ctx: &RenderCtx<'_>,
   visible_groups: &[(String, Vec<TimingEntry>)],
   group_chunks: &[Rect],
   global_offset: &mut usize,
) {
   for ((class_name, class_entries), area) in visible_groups.iter().zip(group_chunks.iter()) {
      let is_selected_group = ctx.selected_row >= *global_offset
         && ctx.selected_row < *global_offset + class_entries.len();
      let local_selected = if is_selected_group {
         ctx.selected_row
            .saturating_sub(*global_offset)
            .min(class_entries.len().saturating_sub(1))
      } else {
         0
      };
      let (visible_entries, start) = visible_slice(class_entries, local_selected, area.height);

      let highlight = calculate_highlight(
         ctx.selected_row,
         *global_offset,
         class_entries.len(),
         local_selected,
         start,
      );
      let mut state = TableState::default();
      state.select(highlight);

      let title = format!("{} ({} cars)", class_name, class_entries.len());
      let table_ctx = create_table_ctx(ctx, highlight);

      let table = build_table(
         title,
         visible_entries,
         &table_ctx,
         area.width,
         ctx.table_width_baselines,
      );
      f.render_stateful_widget(table, *area, &mut state);

      *global_offset += class_entries.len();
   }
}

const fn calculate_highlight(
   selected_row: usize,
   global_offset: usize,
   class_entries_len: usize,
   local_selected: usize,
   start: usize,
) -> Option<usize> {
   if selected_row >= global_offset && selected_row < global_offset + class_entries_len {
      Some(local_selected.saturating_sub(start))
   } else {
      None
   }
}

fn create_table_ctx<'a>(ctx: &'a RenderCtx<'_>, highlight: Option<usize>) -> TableRenderCtx<'a> {
   TableRenderCtx {
      favourites:           ctx.favourites,
      marked_stable_id:     ctx.marked_stable_id,
      active_series:        ctx.active_series,
      selected_row_in_view: highlight,
      marquee_tick:         ctx.marquee_tick,
      gap_anchor:           ctx.gap_anchor,
      pit_trackers:         ctx.pit_trackers,
      class_colors:         &ctx.header.class_colors,
      now:                  ctx.now,
      session_type_raw:     &ctx.header.session_type_raw,
      session_name:         &ctx.header.session_name,
      highlighted_cars:     ctx.highlighted_notice_cars,
   }
}

use super::render_utils::visible_slice as visible_slice_impl;

fn visible_slice(
   entries: &[TimingEntry],
   local_selected: usize,
   height: u16,
) -> (&[TimingEntry], usize) {
   visible_slice_impl(entries, local_selected, height)
}

#[cfg(test)]
mod tests {
   use super::*;

   fn test_entry(stable_id: &str) -> TimingEntry {
      TimingEntry {
         stable_id: stable_id.to_string(),
         ..TimingEntry::default()
      }
   }

   #[test]
   fn create_group_layout_handles_small_groups_without_panicking() {
      let groups = vec![
         ("GTP".to_string(), vec![test_entry("car-1")]),
         ("GTD".to_string(), vec![
            test_entry("car-2"),
            test_entry("car-3"),
         ]),
      ];

      let layout = create_group_layout(&groups, Rect::new(0, 0, 100, 8), 3);

      assert_eq!(layout.len(), groups.len());
   }

   #[test]
   fn create_group_layout_handles_zero_height_area() {
      let groups = vec![("GTP".to_string(), vec![test_entry("car-1")])];

      let layout = create_group_layout(&groups, Rect::new(0, 0, 80, 0), 3);

      assert_eq!(layout.len(), groups.len());
      assert_eq!(layout[0].height, 0);
   }

   #[test]
   fn grouped_view_prefers_three_visible_car_rows_per_group() {
      let area = Rect::new(0, 0, 120, 12);
      let visible_groups = calculate_visible_group_count(area.height, 10, 3);

      assert_eq!(visible_groups, 2);
   }

   #[test]
   fn create_group_layout_keeps_each_group_usable_when_space_allows() {
      let groups = vec![
         ("SP-9".to_string(), vec![test_entry("car-1")]),
         ("SP-X".to_string(), vec![test_entry("car-2")]),
         ("SP-10".to_string(), vec![test_entry("car-3")]),
      ];

      let layout = create_group_layout(
         &groups,
         Rect::new(0, 0, 120, 18),
         GROUP_TABLE_PREFERRED_MIN_HEIGHT,
      );

      assert_eq!(layout.len(), groups.len());
      assert!(layout
         .iter()
         .all(|chunk| chunk.height >= GROUP_TABLE_PREFERRED_MIN_HEIGHT));
   }

   #[test]
   fn visible_group_count_uses_requested_group_height_above_preferred_minimum() {
      let visible_groups = calculate_visible_group_count(20, 10, 10);
      assert_eq!(visible_groups, 2);
   }

   #[test]
   fn visible_group_count_stays_one_when_space_is_tight() {
      let visible_groups = calculate_visible_group_count(3, 10, 4);
      assert_eq!(visible_groups, 1);
   }
}

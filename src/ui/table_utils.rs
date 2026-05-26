use std::collections::HashSet;

pub fn normalize_car_number(value: &str) -> &str {
   let trimmed = value.trim();
   let without_zeroes = trimmed.trim_start_matches('0');
   if without_zeroes.is_empty() {
      trimmed
   } else {
      without_zeroes
   }
}

pub fn is_highlighted_car_number(car_number: &str, highlighted: &HashSet<String>) -> bool {
   let trimmed = car_number.trim();
   highlighted.contains(trimmed) || highlighted.contains(normalize_car_number(trimmed))
}

pub fn marquee_if_needed(text: &str, width_hint: usize, selected: bool, tick: usize) -> String {
   if !selected {
      return text.to_string();
   }

   let chars: Vec<char> = text.chars().collect();
   if chars.len() <= width_hint {
      return text.to_string();
   }

   let gap = 3;
   let cycle_len = chars.len() + gap;
   let offset = tick % cycle_len;

   if offset < chars.len() {
      let mut out = String::new();
      out.extend(chars[offset..].iter());
      out.push_str("   ");
      out.extend(chars[..offset].iter());
      out
   } else {
      let leading_spaces = offset - chars.len();
      let mut out = " ".repeat(leading_spaces);
      out.push_str(text);
      out
   }
}

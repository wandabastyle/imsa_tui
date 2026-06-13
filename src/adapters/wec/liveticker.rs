use std::hash::{
   Hash,
   Hasher,
};

use serde_json::Value;

use crate::timing::WecLivetickerEntry;

/// Build a unique ID for a liveticker entry.
#[cfg_attr(not(test), expect(dead_code))]
fn build_entry_id(elapsed_time_ms: i64, phrase: &str) -> String {
   let mut hasher = std::collections::hash_map::DefaultHasher::new();
   elapsed_time_ms.hash(&mut hasher);
   phrase.hash(&mut hasher);
   format!("{:016x}", hasher.finish())
}

/// Parse a commentator-phrase view from `SignalR` into a liveticker entry.
///
/// Returns `None` if the view cannot be parsed or is missing required fields.
#[must_use]
pub fn parse_commentator_phrase(view: &Value) -> Option<WecLivetickerEntry> {
   let view_obj = view.as_object()?;

   // Get elapsed time
   let elapsed_time_ms = view_obj.get("elapsedTimeMillis").and_then(Value::as_i64)?;

   // Get phrase text - try "phrase" first, then "phrases.en"
   let phrase = view_obj
      .get("phrase")
      .and_then(Value::as_str)
      .map(str::to_string)
      .or_else(|| {
         view_obj
            .get("phrases")
            .and_then(Value::as_object)
            .and_then(|phrases| phrases.get("en"))
            .and_then(Value::as_str)
            .map(str::to_string)
      })?;

   if phrase.trim().is_empty() {
      return None;
   }

   // Get optional audio URL
   let audio_url = view_obj
      .get("audioUrl")
      .and_then(Value::as_str)
      .filter(|url| !url.is_empty())
      .map(str::to_string);

   // Get timestamp
   let ts = view_obj
      .get("ts")
      .and_then(Value::as_str)
      .map(str::to_string);

   Some(WecLivetickerEntry {
      elapsed_time_ms,
      phrase,
      audio_url,
      ts,
   })
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn parse_commentator_phrase_extracts_all_fields() {
      let view = serde_json::json!({
         "phrase": "Leading car pits for fresh tires",
         "audioUrl": "https://example.com/audio/123.mp3",
         "ts": "2026-06-13T17:27:14.7309842+00:00",
         "elapsedTimeMillis": 12_345_678
      });

      let entry = parse_commentator_phrase(&view).expect("should parse");

      assert_eq!(entry.elapsed_time_ms, 12_345_678);
      assert_eq!(entry.phrase, "Leading car pits for fresh tires");
      assert_eq!(
         entry.audio_url,
         Some("https://example.com/audio/123.mp3".to_string())
      );
      assert_eq!(
         entry.ts,
         Some("2026-06-13T17:27:14.7309842+00:00".to_string())
      );
   }

   #[test]
   fn parse_commentator_phrase_uses_phrases_en_as_fallback() {
      let view = serde_json::json!({
         "phrases": {
            "en": "Safety car deployed",
            "fr": "Voiture de sécurité déployée"
         },
         "elapsedTimeMillis": 3_600_000
      });

      let entry = parse_commentator_phrase(&view).expect("should parse");

      assert_eq!(entry.phrase, "Safety car deployed");
      assert_eq!(entry.elapsed_time_ms, 3_600_000);
      assert!(entry.audio_url.is_none());
   }

   #[test]
   fn parse_commentator_phrase_prefers_phrase_over_phrases() {
      let view = serde_json::json!({
         "phrase": "Direct phrase",
         "phrases": {
            "en": "Phrases object",
            "fr": "French text"
         },
         "elapsedTimeMillis": 7_200_000
      });

      let entry = parse_commentator_phrase(&view).expect("should parse");

      assert_eq!(entry.phrase, "Direct phrase");
   }

   #[test]
   fn parse_commentator_phrase_skips_empty_phrase() {
      let view = serde_json::json!({
         "phrase": "   ",
         "elapsedTimeMillis": 1000
      });

      assert!(parse_commentator_phrase(&view).is_none());
   }

   #[test]
   fn parse_commentator_phrase_requires_elapsed_time() {
      let view = serde_json::json!({
         "phrase": "Some text"
         // Missing elapsedTimeMillis
      });

      assert!(parse_commentator_phrase(&view).is_none());
   }

   #[test]
   fn parse_commentator_phrase_requires_phrase() {
      let view = serde_json::json!({
         "elapsedTimeMillis": 1000
         // Missing phrase
      });

      assert!(parse_commentator_phrase(&view).is_none());
   }

   #[test]
   fn parse_commentator_phrase_no_audio_url() {
      let view = serde_json::json!({
         "phrase": "Text only",
         "elapsedTimeMillis": 5000
      });

      let entry = parse_commentator_phrase(&view).expect("should parse");

      assert!(entry.audio_url.is_none());
   }

   #[test]
   fn build_entry_id_is_deterministic() {
      let id1 = build_entry_id(12_345_678, "Test phrase");
      let id2 = build_entry_id(12_345_678, "Test phrase");

      assert_eq!(id1, id2);
   }

   #[test]
   fn build_entry_id_is_unique_per_input() {
      let id1 = build_entry_id(12_345_678, "Phrase one");
      let id2 = build_entry_id(12_345_678, "Phrase two");
      let id3 = build_entry_id(12_345_679, "Phrase one");

      assert_ne!(id1, id2);
      assert_ne!(id1, id3);
   }
}

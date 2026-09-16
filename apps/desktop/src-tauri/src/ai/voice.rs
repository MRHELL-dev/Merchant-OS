/// Voice transcript sanitization and normalization.
/// Invariant: Treats all voice transcripts strictly as untrusted text input.

pub struct VoiceTranscriptHandler;

impl VoiceTranscriptHandler {
    /// Sanitizes an audio transcription by removing audio filler words, speech artifacts, and excess punctuation.
    pub fn sanitize_transcript(raw_transcript: &str) -> String {
        // 1. Remove common punctuation added by STT engines
        let trimmed = raw_transcript
            .trim()
            .trim_end_matches(|c| c == '.' || c == '?' || c == '!' || c == ',');

        let fillers = [
            "you know", "can you", "could you", "uh", "um", "ah", "hmm", "err", "like", "please", "kindly",
        ];

        let mut padded = format!(" {} ", trimmed);
        for filler in fillers {
            loop {
                let lower = padded.to_lowercase();
                let target = format!(" {} ", filler);
                if let Some(idx) = lower.find(&target) {
                    padded.replace_range(idx..idx + target.len(), " ");
                } else {
                    break;
                }
            }
        }

        // 2. Condense multi-whitespace
        let parts: Vec<&str> = padded.split_whitespace().collect();
        parts.join(" ")
    }
}

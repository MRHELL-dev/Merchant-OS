pub mod error;
pub mod intents;
pub mod interpreter;
pub mod provider;
pub mod recommendations;
pub mod translation;
pub mod validation;
pub mod voice;

pub use error::AIError;
pub use intents::*;
pub use interpreter::{AIInterpreter, AIResponseDto, AIResponseMode};
pub use provider::{AIProvider, DeterministicLocalInterpreter, MockMode, MockProvider};
pub use recommendations::{DemandIntelligenceService, DemandRecommendationDto};
pub use translation::MultilingualNormalizer;
pub use validation::DeterministicIntentValidator;
pub use voice::VoiceTranscriptHandler;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIStatusDto {
    pub provider_name: String,
    pub is_available: bool,
    pub is_offline_capable: bool,
    pub voice_transcriber_available: bool,
    pub supported_languages: Vec<String>,
}

impl AIStatusDto {
    pub fn default_local() -> Self {
        Self {
            provider_name: "Deterministic Local Interpreter (V1 Offline)".to_string(),
            is_available: true,
            is_offline_capable: true,
            voice_transcriber_available: true, // Optional online capability via browser STT
            supported_languages: vec![
                "English".to_string(),
                "Hindi (हिंदी)".to_string(),
                "Hinglish".to_string(),
            ],
        }
    }
}

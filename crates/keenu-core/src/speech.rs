// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! The voice assistant's speech models: whisper.cpp's files, which `keenu-setup-voice` downloads
//! to [`MODELS_DIR`] in the user's data folder and `keenu-whisper` reads.

/// The model sizes offered, smallest first.
pub const MODELS: [&str; 4] = ["tiny", "base", "small", "medium"];

/// Where the models are kept, within the user's data folder (`~/.local/share`).
pub const MODELS_DIR: &str = "keenu/models";

/// Silero's voice activity detector, so Whisper only hears the parts where someone speaks.
pub const VAD_FILE: &str = "ggml-silero-v5.1.2.bin";

/// The file a model is kept in, if it is one of [`MODELS`]: 8-bit, about half the size of the
/// full model and as accurate.
pub fn model_file(model: &str) -> Option<String> {
    MODELS
        .contains(&model)
        .then(|| format!("ggml-{model}-q8_0.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_quantized_file() {
        assert_eq!(model_file("small").as_deref(), Some("ggml-small-q8_0.bin"));
        assert_eq!(model_file("tiny").as_deref(), Some("ggml-tiny-q8_0.bin"));
    }

    #[test]
    fn refuses_what_is_not_a_model() {
        assert_eq!(model_file("large"), None);
        assert_eq!(model_file(""), None);
        assert_eq!(model_file("../small"), None);
    }
}

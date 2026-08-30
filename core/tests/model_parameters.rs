//! Print what the model really is and pin it. Both bugs edge-ear found
//! this way came from trusting a model's shape without looking.

mod support;

use edge_stt_core::backend::whisper::WhisperBackend;
use edge_stt_core::{Config, Language};

#[test]
#[ignore = "needs a Whisper model"]
fn the_model_is_what_the_documentation_claims() {
    let spec = support::model_spec();
    let backend = WhisperBackend::load(&spec, &Config::local(spec.clone())).expect("a model");
    let facts = backend.facts();

    assert!(
        facts.vocabulary > 50_000,
        "a Whisper vocabulary, got {}",
        facts.vocabulary
    );
    assert!(facts.audio_context > 0);
    assert!(!facts.description.is_empty());
    assert!(
        facts.multilingual,
        "{} speaks only English; Korean needs a multilingual model",
        facts.description
    );
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_monolingual_model_is_refused_when_another_language_is_asked_for() {
    let spec = support::model_spec();
    let config = Config::local(spec.clone()).with_language(Language::new("ko"));

    // A multilingual model must accept this; the refusal is what the
    // English-only files get, and the message must name the language.
    match WhisperBackend::load(&spec, &config) {
        Ok(backend) => assert!(backend.facts().multilingual),
        Err(why) => assert!(why.to_string().contains("ko"), "{why}"),
    }
}

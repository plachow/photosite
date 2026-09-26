//! Asking a vision model to describe a photograph.
//!
//! The model runs on this machine, through [Ollama](https://ollama.com), or
//! — with a key somebody brought — at one of three providers over the wire:
//! OpenAI and everything that speaks its dialect, Anthropic, and Google's
//! Gemini. Either way a photograph goes as pixels and the answer comes back
//! as JSON. The local model is the default and the whole feature was built
//! round it; **a model on somebody else's computer is a different promise
//! entirely**, and choosing one is a choice the settings make explicit
//! rather than a URL that happens not to be localhost.
//!
//! Two things make the answers usable rather than merely impressive.
//!
//! **The reply is constrained to a schema.** Every provider will hold a
//! model to a JSON shape in some form, so the answer is always
//! machine-readable rather than prose that has to be picked apart with a
//! regular expression and an apology.
//!
//! **The place is told to the model, never asked of it.** A model shown a
//! coastline will name a country with total confidence and be wrong. So the
//! coordinates are resolved against the offline gazetteer first and the
//! model is handed the answer as a fact — and where the position is itself
//! doubtful, it is handed the doubt too.

pub mod base64;
mod cloud;
mod http;
mod ollama;
pub mod prompt;

pub use ollama::DEFAULT_ENDPOINT;
pub use prompt::Insights;

use photosite_core::Catalog;
use photosite_core::gazetteer::Nearby;
use std::time::Duration;

/// Who is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    /// A model on this machine. Nothing leaves it.
    #[default]
    Ollama,
    /// OpenAI, or any server that speaks its chat-completions dialect —
    /// OpenRouter, Groq, Mistral, LM Studio, and the rest.
    OpenAi,
    Anthropic,
    Gemini,
}

impl Provider {
    /// The settings' names for these, in the settings' order.
    pub const ALL: [Self; 4] = [Self::Ollama, Self::OpenAi, Self::Anthropic, Self::Gemini];

    /// The name the settings file holds.
    pub fn id(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    /// The settings' name read back; anything unknown is the local model,
    /// which is the one that can do no harm.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|provider| provider.id() == id.trim())
            .unwrap_or_default()
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Ollama => "ai-provider-ollama",
            Self::OpenAi => "ai-provider-openai",
            Self::Anthropic => "ai-provider-anthropic",
            Self::Gemini => "ai-provider-gemini",
        }
    }

    /// The name as it reads in a sentence.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
        }
    }

    pub fn default_endpoint(self) -> &'static str {
        match self {
            Self::Ollama => DEFAULT_ENDPOINT,
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
        }
    }

    /// Does the photograph leave this machine?
    pub fn remote(self) -> bool {
        self != Self::Ollama
    }
}

/// Where to ask, and how to be let in.
#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    pub provider: Provider,
    /// Empty means the provider's own address.
    pub endpoint: String,
    /// Empty for Ollama, which asks nobody.
    pub api_key: String,
}

// By hand, so that the key cannot reach a log through a `{:?}`.
impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "…" })
            .finish()
    }
}

/// The environment variable a key may be handed in through — for the
/// headless binary on a runner, and for whoever would rather not have it in
/// a settings file at all.
pub const KEY_VARIABLE: &str = "PHOTOSITE_AI_KEY";

impl Server {
    /// What the settings say, the key taken from the environment when the
    /// settings hold none.
    pub fn of(settings: &photosite_core::settings::Ai) -> Self {
        let provider = Provider::from_id(&settings.provider);
        let api_key = if provider.remote() {
            let from_settings = settings.api_key.trim();
            if from_settings.is_empty() {
                std::env::var(KEY_VARIABLE).unwrap_or_default()
            } else {
                from_settings.to_owned()
            }
        } else {
            String::new()
        };
        Self {
            provider,
            endpoint: if provider.remote() {
                settings.cloud_endpoint.clone()
            } else {
                settings.endpoint.clone()
            },
            api_key,
        }
    }

    pub fn endpoint_or_default(&self) -> &str {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty() {
            self.provider.default_endpoint()
        } else {
            endpoint
        }
    }

    /// Is there a key where one is needed?
    pub fn admitted(&self) -> bool {
        !self.provider.remote() || !self.api_key.trim().is_empty()
    }
}

/// The model the settings name for the provider they name.
pub fn model_of(settings: &photosite_core::settings::Ai) -> &str {
    if Provider::from_id(&settings.provider).remote() {
        &settings.cloud_model
    } else {
        &settings.model
    }
}

/// The same, to write into.
pub fn model_field(settings: &mut photosite_core::settings::Ai) -> &mut String {
    if Provider::from_id(&settings.provider).remote() {
        &mut settings.cloud_model
    } else {
        &mut settings.model
    }
}

/// The models a server offers. Ollama's come vision-capable first; a cloud
/// provider does not say which of its models can see, so those come as
/// named.
pub fn models(server: &Server, timeout: Duration) -> Result<Vec<String>, anyhow::Error> {
    match server.provider {
        Provider::Ollama => ollama::models(&server.endpoint, timeout),
        _ => cloud::models(server, timeout),
    }
}

/// Describes one photograph.
///
/// `place` is a fact told to the model, never a question asked of it — see
/// the crate documentation for why that matters more than it sounds.
#[allow(clippy::too_many_arguments)]
pub fn describe(
    server: &Server,
    model: &str,
    jpeg: &[u8],
    language: &str,
    english_too: bool,
    place: Option<&Nearby>,
    approximate: bool,
    direction: &str,
    timeout: Duration,
) -> Result<Insights, anyhow::Error> {
    anyhow::ensure!(!model.trim().is_empty(), "no model has been chosen");
    let context = place.and_then(|place| prompt::place_context(place, approximate, direction));
    let text = prompt::build(language, english_too, context.as_deref());
    let insights = match server.provider {
        Provider::Ollama => {
            ollama::describe(&server.endpoint, model, jpeg, &text, english_too, timeout)?
        }
        _ => cloud::describe(server, model, jpeg, &text, english_too, timeout)?,
    };
    Ok(prompt::with_place(insights, place, approximate))
}

/// What to do about a photograph that has already been described.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Fill in what is empty and leave the rest alone.
    ///
    /// The default, and the reason a run over a thousand photographs can be
    /// interrupted and started again: everything already described is
    /// skipped, so the second run picks up where the first stopped.
    #[default]
    FillEmpty,
    Overwrite,
}

impl Mode {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::FillEmpty => "ai-fill-empty",
            Self::Overwrite => "ai-overwrite",
        }
    }
}

/// Is this photograph already done?
pub fn should_skip(mode: Mode, title: Option<&str>, description: Option<&str>) -> bool {
    mode == Mode::FillEmpty
        && title.is_some_and(|title| !title.trim().is_empty())
        && description.is_some_and(|description| !description.trim().is_empty())
}

/// Writes what the model said into the catalogue, and queues the file.
///
/// The one place this is decided, for the window and the headless command
/// alike. Filling in means filling in: a title already there is kept unless
/// the mode says otherwise. Keywords are added and never replace what a
/// person wrote. The English copy is not conditional on the mode: nobody
/// typed it, so there is nothing of theirs to overwrite.
pub fn apply(
    catalog: &mut Catalog,
    photo: &photosite_core::Photo,
    insights: &Insights,
    mode: Mode,
    now: i64,
) -> Result<(), anyhow::Error> {
    let overwrite = mode == Mode::Overwrite;
    let blank = |value: Option<&str>| value.is_none_or(|value| value.trim().is_empty());

    if let Some(title) = &insights.title
        && (overwrite || blank(photo.organisation.title.as_deref()))
    {
        catalog.set_title(&[photo.id], Some(title))?;
    }

    if let Some(description) = &insights.description
        && (overwrite || blank(photo.organisation.description.as_deref()))
    {
        catalog.set_description(&[photo.id], Some(description))?;
    }

    if let Some(english) = &insights.description_en {
        catalog.set_description_en(photo.id, Some(english))?;
    }

    if !insights.keywords.is_empty() {
        catalog.add_keywords(&[photo.id], &insights.keywords)?;
    }

    catalog.enqueue(&[photo.id], now)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photograph_already_described_is_skipped_when_filling_in_the_empty_ones() {
        assert!(should_skip(Mode::FillEmpty, Some("A title"), Some("Words")));
        assert!(!should_skip(Mode::FillEmpty, Some("A title"), None));
        assert!(!should_skip(Mode::FillEmpty, None, Some("Words")));
        assert!(!should_skip(Mode::FillEmpty, None, None));
    }

    #[test]
    fn a_title_of_spaces_is_not_a_title() {
        assert!(!should_skip(Mode::FillEmpty, Some("   "), Some("Words")));
    }

    #[test]
    fn overwriting_means_overwriting() {
        assert!(!should_skip(Mode::Overwrite, Some("A title"), Some("Words")));
    }

    /// The settings hold the provider by name, and the names must be the
    /// ones the settings offer as choices.
    #[test]
    fn the_providers_are_the_settings_choices() {
        let ids: Vec<&str> = Provider::ALL.iter().map(|provider| provider.id()).collect();
        assert_eq!(ids, photosite_core::settings::AI_PROVIDER_IDS);
        for provider in Provider::ALL {
            assert_eq!(Provider::from_id(provider.id()), provider);
        }

        assert_eq!(Provider::from_id("something else"), Provider::Ollama);
    }

    #[test]
    fn the_server_follows_the_provider_chosen() {
        let mut settings = photosite_core::settings::Ai {
            endpoint: "localhost:11434".to_owned(),
            cloud_endpoint: "https://openrouter.ai/api/v1".to_owned(),
            model: "llava".to_owned(),
            cloud_model: "gpt-4o".to_owned(),
            api_key: "sk-secret".to_owned(),
            ..Default::default()
        };

        let local = Server::of(&settings);
        assert_eq!(local.provider, Provider::Ollama);
        assert_eq!(local.endpoint, "localhost:11434");
        assert!(local.api_key.is_empty(), "the key stays home");
        assert_eq!(model_of(&settings), "llava");

        settings.provider = "openai".to_owned();
        let remote = Server::of(&settings);
        assert_eq!(remote.provider, Provider::OpenAi);
        assert_eq!(remote.endpoint, "https://openrouter.ai/api/v1");
        assert_eq!(remote.api_key, "sk-secret");
        assert_eq!(model_of(&settings), "gpt-4o");
        assert!(remote.admitted());

        settings.cloud_endpoint.clear();
        assert_eq!(
            Server::of(&settings).endpoint_or_default(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn the_key_never_shows_in_a_debug_print() {
        let server = Server {
            provider: Provider::Anthropic,
            endpoint: String::new(),
            api_key: "sk-ant-secret".to_owned(),
        };
        let printed = format!("{server:?}");
        assert!(!printed.contains("secret"), "{printed}");
    }

    #[test]
    fn a_remote_provider_without_a_key_is_not_admitted() {
        let server = Server {
            provider: Provider::Gemini,
            endpoint: String::new(),
            api_key: String::new(),
        };
        assert!(!server.admitted());
        assert!(
            describe(
                &server,
                "gemini",
                b"x",
                "English",
                false,
                None,
                false,
                "",
                Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn what_the_model_said_lands_in_the_catalogue() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog
            .upsert(&photosite_core::catalog::NewPhoto {
                path: std::path::PathBuf::from("/a/b.jpg"),
                file_size: 1,
                modified_at: 1,
                taken_at: None,
                width: None,
                height: None,
                orientation: 1,
                camera: None,
                lens: None,
                place: None,
                verdict: photosite_core::Verdict::Nowhere,
                reason: None,
            })
            .unwrap();
        catalog.set_title(&[id], Some("Typed by hand")).unwrap();
        let photo = catalog.by_path(std::path::Path::new("/a/b.jpg")).unwrap().unwrap();

        let insights = Insights {
            title: Some("From the model".to_owned()),
            description: Some("Words.".to_owned()),
            keywords: vec!["hill".to_owned()],
            description_en: Some("Words in English.".to_owned()),
        };
        apply(&mut catalog, &photo, &insights, Mode::FillEmpty, 5).unwrap();
        let after = catalog.by_path(std::path::Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(after.organisation.title.as_deref(), Some("Typed by hand"));
        assert_eq!(after.organisation.description.as_deref(), Some("Words."));
        assert_eq!(after.description_en.as_deref(), Some("Words in English."));
        assert_eq!(catalog.keywords_of(id).unwrap(), ["hill"]);
        assert_eq!(catalog.outbox().unwrap().0, 1, "queued for the file");

        apply(&mut catalog, &photo, &insights, Mode::Overwrite, 6).unwrap();
        let after = catalog.by_path(std::path::Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(after.organisation.title.as_deref(), Some("From the model"));
    }
}

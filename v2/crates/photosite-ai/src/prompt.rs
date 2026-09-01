//! What the model is asked, and what is made of the answer.
//!
//! All of it is text in, text out, so all of it can be tested without a
//! model, a server or a network — which matters, because this is where the
//! quality of the descriptions actually lives.

use photosite_core::gazetteer::{NEARBY_KM, Nearby};

/// The metadata one call extracted from a photograph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Insights {
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    /// The same description in English, when the run's language was not.
    ///
    /// It lives in the catalogue and never in the file: the photograph
    /// carries one description, in the language somebody chose, and the
    /// second one is there so that searching works in both.
    pub description_en: Option<String>,
}

impl Insights {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// How many keywords are kept. Beyond this a model is padding, and a
/// photograph with sixty keywords is a photograph with none.
const MAX_KEYWORDS: usize = 30;

/// One sentence of verified place, or nothing worth saying.
///
/// A **nearby** place reads "in or near X"; a distant one is only a
/// reference point — "about 39 km east of X" — because claiming a photograph
/// was taken in a town two valleys away puts a wrong name in a caption and
/// the model will repeat it word for word. A doubtful position softens the
/// wording instead of pretending to a precision the fix never had.
pub fn place_context(place: &Nearby, approximate: bool, direction: &str) -> Option<String> {
    if place.name.trim().is_empty() {
        return None;
    }

    let mut parents: Vec<&str> = Vec::new();
    for name in [&place.subregion, &place.region, &place.country]
        .into_iter()
        .flatten()
    {
        if !name.eq_ignore_ascii_case(&place.name)
            && !parents.iter().any(|had| had.eq_ignore_ascii_case(name))
        {
            parents.push(name);
        }
    }

    let suffix = if parents.is_empty() {
        String::new()
    } else {
        format!(" ({})", parents.join(", "))
    };
    let where_ = if place.distance_km <= NEARBY_KM {
        format!("in or near {}{suffix}", place.name)
    } else {
        format!(
            "about {:.0} km {direction} of {}{suffix}",
            place.distance_km, place.name
        )
    };

    Some(if approximate {
        format!("probably {where_}; the GPS fix was imprecise, so treat the place as approximate")
    } else {
        where_
    })
}

/// What the model is told to do.
///
/// Written out rather than assembled from parts: it is the one thing in this
/// crate whose exact words decide whether the answers are worth having, and
/// a prompt spread over six functions is a prompt nobody can read as a
/// whole.
pub fn build(language: &str, english_too: bool, place: Option<&str>) -> String {
    let mut prompt = String::from(
        "You are an expert photo librarian. Analyze this photograph and \
         extract as much information as you can.\n",
    );

    if let Some(place) = place {
        prompt.push_str(&format!(
            "Verified place: the photograph was taken {place}. Work this \
             place into the description and the keywords, and into the title \
             when it fits naturally.\n"
        ));
    }

    prompt.push_str(
        "Return:\n\
         - \"title\": a short factual title, at most 8 words.\n\
         - \"description\": 2 to 5 sentences covering the main subject, the \
         setting and type of location, actions, the number of people, \
         notable objects, animals, plants, weather, light, season if \
         apparent, dominant colours, mood and composition. Quote any \
         readable text, signs or inscriptions exactly.\n\
         - \"keywords\": 10 to 25 keywords: subjects, objects, animals, \
         plants, type of location, activities, events, season, time of day, \
         weather, dominant colours, mood, photographic style. Use single \
         words or short phrases.\n",
    );

    if english_too {
        prompt.push_str("- \"description_en\": the same description written in English.\n");
    }

    prompt.push_str(&format!(
        "Write the title, the description and the keywords in {language}. "
    ));

    // The one instruction that keeps a confident model from inventing a
    // place name. Without it, a coastline becomes "Santorini" and a field
    // becomes "Tuscany", stated as fact.
    prompt.push_str(match place {
        None => {
            "Do not guess names of people or exact places unless visible \
             text makes them certain."
        }
        Some(_) => {
            "Do not guess names of people. Beyond the verified place, name a \
             more specific spot or landmark only if you clearly recognize it \
             in the photograph or readable text makes it certain."
        }
    });

    prompt
}

/// The JSON body of one `api/chat` request.
///
/// `format` is what holds the model to a shape. Without it the answer is
/// prose about a photograph, which is charming and useless.
pub fn request(
    model: &str,
    image: &[u8],
    language: &str,
    english_too: bool,
    place: Option<&str>,
    no_thinking: bool,
) -> serde_json::Value {
    let mut properties = serde_json::json!({
        "title": { "type": "string" },
        "description": { "type": "string" },
        "keywords": { "type": "array", "items": { "type": "string" } },
    });
    let mut required = vec!["title", "description", "keywords"];
    if english_too {
        properties["description_en"] = serde_json::json!({ "type": "string" });
        required.push("description_en");
    }

    let mut body = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": [{
            "role": "user",
            "content": build(language, english_too, place),
            "images": [crate::base64::encode(image)],
        }],
        "format": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
        // Low, because this is a description of what is in a photograph and
        // not a piece of writing. Invention is the failure mode here.
        "options": { "temperature": 0.2 },
    });

    if no_thinking {
        body["think"] = serde_json::Value::Bool(false);
    }

    body
}

/// Reads the answer out of a chat response.
///
/// Two layers of JSON: the chat reply, whose `message.content` is itself the
/// JSON the schema asked for. That is Ollama's shape, not ours.
pub fn read(reply: &str) -> anyhow::Result<Insights> {
    let outer: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| anyhow::anyhow!("the reply was not JSON"))?;
    let content = outer
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("the reply carried no message"))?;

    let inner: serde_json::Value = serde_json::from_str(content)
        .map_err(|_| anyhow::anyhow!("the model's answer was not the JSON it was asked for"))?;

    Ok(Insights {
        title: line(inner.get("title").and_then(serde_json::Value::as_str), 200),
        description: text(
            inner.get("description").and_then(serde_json::Value::as_str),
            4000,
        ),
        keywords: clean_keywords(
            inner
                .get("keywords")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        ),
        description_en: text(
            inner
                .get("description_en")
                .and_then(serde_json::Value::as_str),
            4000,
        ),
    })
}

/// The models a server offers, vision-capable first.
///
/// A model whose capabilities the server does not report is **kept**, at the
/// end: an older Ollama says nothing about capabilities at all, and a list
/// that came back empty would look like a server with no models on it.
pub fn read_models(reply: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(reply) else {
        return Vec::new();
    };

    let Some(models) = parsed.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };

    let mut vision = Vec::new();
    let mut unknown = Vec::new();
    for model in models {
        let Some(name) = model.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };

        match model
            .get("capabilities")
            .and_then(serde_json::Value::as_array)
        {
            None => unknown.push(name.to_owned()),
            Some(capabilities) => {
                if capabilities.iter().any(|capability| {
                    capability
                        .as_str()
                        .is_some_and(|word| word.eq_ignore_ascii_case("vision"))
                }) {
                    vision.push(name.to_owned());
                }
            }
        }
    }

    vision.extend(unknown);
    vision
}

/// Puts the resolved place names at the front of the keyword list.
///
/// Ahead of the model's own, so that a search for the region or the country
/// finds the photograph however the model chose to phrase things — and so
/// that the names that are *known* outrank the ones that were *described*.
pub fn with_place(insights: Insights, place: Option<&Nearby>, approximate: bool) -> Insights {
    let Some(place) = place else {
        return insights;
    };

    let mut keywords = place.keywords(!approximate);
    keywords.extend(insights.keywords);
    Insights {
        keywords: clean_keywords(keywords),
        ..insights
    }
}

/// Keywords already on the photograph stay; the new ones are added after
/// them, without regard to case.
///
/// Adding and never replacing, for the same reason the keyword box on a
/// selection adds: a description written by a model is not grounds for
/// throwing away what a person typed.
pub fn merge_keywords(existing: &[String], added: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = existing.to_vec();
    for keyword in added {
        if !merged.iter().any(|had| had.eq_ignore_ascii_case(keyword)) {
            merged.push(keyword.clone());
        }
    }

    merged
}

/// Tidies what a model returned into things that can be keywords.
///
/// Semicolons and commas go because a keyword list travels as one; a hash
/// goes because a model asked for keywords will sometimes answer with
/// hashtags. Duplicates go without regard to case.
pub fn clean_keywords(keywords: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for keyword in keywords {
        let cleaned = collapse(&keyword.replace([';', ','], " ").replace('#', ""));
        if cleaned.is_empty() || out.iter().any(|had| had.eq_ignore_ascii_case(&cleaned)) {
            continue;
        }

        out.push(cleaned);
        if out.len() == MAX_KEYWORDS {
            break;
        }
    }

    out
}

/// One line: whitespace collapsed, cut to length.
fn line(text: Option<&str>, most: usize) -> Option<String> {
    let cleaned = collapse(text?);
    if cleaned.is_empty() {
        return None;
    }

    Some(cut(&cleaned, most))
}

/// Several lines: the newlines kept, the ends trimmed, cut to length.
fn text(value: Option<&str>, most: usize) -> Option<String> {
    let cleaned = value?.replace("\r\n", "\n").trim().to_owned();
    if cleaned.is_empty() {
        return None;
    }

    Some(cut(&cleaned, most))
}

/// Cut by characters and never by bytes: cutting a string of Czech in the
/// middle of a letter is a panic, and it would happen only to somebody
/// else's language.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }

    text.chars()
        .take(most)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nearby(name: &str, distance_km: f64) -> Nearby {
        Nearby {
            name: name.to_owned(),
            subregion: Some("Okres Brno-mesto".to_owned()),
            region: Some("South Moravian".to_owned()),
            country: Some("Czechia".to_owned()),
            distance_km,
            bearing: 90,
        }
    }

    #[test]
    fn a_place_close_by_is_where_the_photograph_was_taken() {
        let said = place_context(&nearby("Brno", 1.2), false, "east").unwrap();
        assert!(said.starts_with("in or near Brno"), "{said}");
        assert!(said.contains("South Moravian"), "{said}");
    }

    /// Claiming a photograph was taken in a town two valleys away puts a
    /// wrong name in a caption, and the model repeats it word for word.
    #[test]
    fn a_distant_place_is_only_a_reference_point() {
        let said = place_context(&nearby("Brno", 39.4), false, "east").unwrap();
        assert!(said.starts_with("about 39 km east of Brno"), "{said}");
    }

    #[test]
    fn a_shaky_position_is_said_to_be_shaky() {
        let said = place_context(&nearby("Brno", 1.0), true, "east").unwrap();
        assert!(said.starts_with("probably in or near Brno"), "{said}");
        assert!(said.contains("approximate"), "{said}");
    }

    #[test]
    fn a_region_named_after_its_own_town_is_not_said_twice() {
        let place = Nearby {
            name: "Paris".to_owned(),
            subregion: Some("Paris".to_owned()),
            region: Some("Ile-de-France".to_owned()),
            country: Some("France".to_owned()),
            distance_km: 1.0,
            bearing: 0,
        };
        let said = place_context(&place, false, "east").unwrap();
        assert_eq!(said, "in or near Paris (Ile-de-France, France)");
    }

    #[test]
    fn a_place_with_no_name_says_nothing() {
        assert!(place_context(&nearby("  ", 1.0), false, "east").is_none());
    }

    /// The instruction that keeps a confident model from inventing a place
    /// name, and the one that tells it what the place actually is.
    #[test]
    fn the_prompt_says_what_to_write_and_what_not_to_guess() {
        let without = build("Czech", false, None);
        assert!(without.contains("in Czech"), "{without}");
        assert!(without.contains("Do not guess names of people or exact places"));
        assert!(!without.contains("description_en"));

        let with = build("Czech", true, Some("in or near Brno"));
        assert!(with.contains("Verified place: the photograph was taken in or near Brno"));
        assert!(with.contains("description_en"));
        assert!(with.contains("Beyond the verified place"));
    }

    #[test]
    fn the_request_holds_the_photograph_and_the_shape_of_the_answer() {
        let body = request("qwen", b"not really a jpeg", "English", false, None, true);
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["stream"], false);
        assert_eq!(body["think"], false);
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(
            body["messages"][0]["images"][0],
            crate::base64::encode(b"not really a jpeg")
        );
        assert!(
            body["format"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "keywords")
        );
    }

    #[test]
    fn asking_for_english_too_asks_for_it_in_the_schema() {
        let body = request("qwen", b"x", "Czech", true, None, false);
        assert!(body.get("think").is_none(), "an older server rejects it");
        assert!(body["format"]["properties"]["description_en"].is_object());
    }

    #[test]
    fn the_answer_is_read_out_of_the_two_layers_of_json() {
        let reply = serde_json::json!({
            "message": {
                "content": serde_json::json!({
                    "title": "  Sunrise over the bay  ",
                    "description": "A wide bay at first light.\nGulls on the water.",
                    "keywords": ["sea", "sunrise", "SEA", "#gulls", "a;b"],
                    "description_en": "A wide bay at first light."
                }).to_string()
            }
        })
        .to_string();

        let insights = read(&reply).unwrap();
        assert_eq!(insights.title.as_deref(), Some("Sunrise over the bay"));
        assert!(insights.description.unwrap().contains('\n'), "lines kept");
        assert_eq!(insights.keywords, ["sea", "sunrise", "gulls", "a b"]);
        assert!(insights.description_en.is_some());
    }

    #[test]
    fn a_reply_that_is_not_what_was_asked_for_is_an_error_and_not_a_guess() {
        assert!(read("this is not json").is_err());
        assert!(read(r#"{"message":{}}"#).is_err());
        assert!(read(r#"{"message":{"content":"sorry, I cannot"}}"#).is_err());
    }

    #[test]
    fn a_missing_field_is_missing_rather_than_fatal() {
        let reply = serde_json::json!({
            "message": { "content": r#"{"title":"Only a title"}"# }
        })
        .to_string();
        let insights = read(&reply).unwrap();
        assert_eq!(insights.title.as_deref(), Some("Only a title"));
        assert_eq!(insights.description, None);
        assert!(insights.keywords.is_empty());
    }

    /// Cutting a description in the middle of a letter is a panic, and it
    /// would only ever happen in somebody else's language.
    #[test]
    fn a_long_answer_is_cut_between_letters_and_not_inside_one() {
        let long: String = "ěščřžýáíé".repeat(1000);
        let reply = serde_json::json!({
            "message": { "content": serde_json::json!({ "description": long }).to_string() }
        })
        .to_string();
        let insights = read(&reply).unwrap();
        assert_eq!(insights.description.unwrap().chars().count(), 4000);
    }

    #[test]
    fn a_model_that_answers_with_sixty_keywords_gives_thirty() {
        let many: Vec<String> = (0..60).map(|n| format!("word{n}")).collect();
        assert_eq!(clean_keywords(many).len(), MAX_KEYWORDS);
    }

    #[test]
    fn the_place_names_lead_the_keywords() {
        let insights = Insights {
            keywords: vec!["sea".to_owned(), "Czechia".to_owned()],
            ..Default::default()
        };
        let with = with_place(insights, Some(&nearby("Brno", 1.0)), false);
        assert_eq!(
            with.keywords,
            [
                "Brno",
                "Okres Brno-mesto",
                "South Moravian",
                "Czechia",
                "sea"
            ],
            "the known names should come first, and not twice"
        );
    }

    #[test]
    fn a_shaky_position_keeps_the_country_and_drops_the_town() {
        let with = with_place(Insights::default(), Some(&nearby("Brno", 1.0)), true);
        assert!(!with.keywords.contains(&"Brno".to_owned()));
        assert!(with.keywords.contains(&"Czechia".to_owned()));
    }

    /// A description written by a model is not grounds for throwing away
    /// what a person typed.
    #[test]
    fn keywords_are_added_and_never_replaced() {
        let existing = vec!["holiday".to_owned(), "Jana".to_owned()];
        let added = vec!["sea".to_owned(), "HOLIDAY".to_owned()];
        assert_eq!(
            merge_keywords(&existing, &added),
            ["holiday", "Jana", "sea"]
        );
    }

    #[test]
    fn vision_models_come_first_and_an_older_server_still_lists_its_own() {
        let reply = serde_json::json!({
            "models": [
                { "name": "text-only", "capabilities": ["completion"] },
                { "name": "sees", "capabilities": ["completion", "vision"] },
                { "name": "who-knows" },
            ]
        })
        .to_string();
        assert_eq!(read_models(&reply), ["sees", "who-knows"]);
    }

    #[test]
    fn a_reply_that_is_not_a_model_list_is_an_empty_list() {
        assert_eq!(read_models("nonsense"), Vec::<String>::new());
        assert_eq!(read_models("{}"), Vec::<String>::new());
    }
}

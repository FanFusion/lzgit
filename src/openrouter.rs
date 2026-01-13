use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model: String,
    pub referer: Option<String>,
    pub title: Option<String>,
}

impl OpenRouterConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| "Missing OPENROUTER_API_KEY".to_string())?;
        let model =
            std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "openai/gpt-5.2".to_string());
        let referer = std::env::var("OPENROUTER_REFERER").ok();
        let title = std::env::var("OPENROUTER_TITLE").ok();
        Ok(Self {
            api_key,
            model,
            referer,
            title,
        })
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    temperature: f32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

pub fn generate_commit_message(
    cfg: &OpenRouterConfig,
    staged_diff: &str,
) -> Result<String, String> {
    let mut system = String::new();
    system.push_str("You write git commit messages. ");
    system.push_str("Output only the commit message text (no code fences, no quotes). ");
    system.push_str("Prefer 1 line summary, optionally blank line + short body. ");
    system.push_str("Use imperative mood.");

    let mut user = String::new();
    user.push_str("Write a commit message for this staged diff:\n\n");
    user.push_str(staged_diff);

    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system,
            },
            Message {
                role: "user".to_string(),
                content: user,
            },
        ],
        stream: false,
        temperature: 0.2,
    };

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(60))
        .build();

    let mut request = agent
        .post("https://openrouter.ai/api/v1/chat/completions")
        .set("Authorization", &format!("Bearer {}", cfg.api_key))
        .set("Content-Type", "application/json");

    if let Some(r) = &cfg.referer {
        request = request.set("HTTP-Referer", r);
    }
    if let Some(t) = &cfg.title {
        request = request.set("X-Title", t);
    }

    let res = request.send_json(ureq::json!(req));

    let ok = match res {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            return Err(format!("OpenRouter HTTP {}: {}", code, body));
        }
        Err(e) => return Err(e.to_string()),
    };

    let parsed: ChatResponse = ok.into_json().map_err(|e| e.to_string())?;
    let content = parsed
        .choices
        .get(0)
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();

    Ok(sanitize_message(&content))
}

fn sanitize_message(s: &str) -> String {
    let trimmed = s.trim();
    let mut out = String::new();
    for line in trimmed.lines() {
        let l = line.trim_end();
        if l == "```" {
            continue;
        }
        if l.starts_with("```") {
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_string()
}

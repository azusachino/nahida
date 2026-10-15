//! Nahida's file configuration, compatible with Pi's JSON record formats.
//! No HTTP, agent loop, terminal, executable credential source or Pi runtime.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
#[error("configuration: {0}")]
pub struct Error(&'static str);

pub type Result<T> = std::result::Result<T, Error>;

/// Provider-owned defaults, supplied by the consumer rather than duplicated
/// as a model/provider catalog here.
pub struct Defaults {
    pub api: &'static str,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub env_key: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaxTokensField {
    MaxTokens,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinkingFormat {
    Zai,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpenAiCompat {
    pub supports_store: Option<bool>,
    pub supports_developer_role: Option<bool>,
    pub supports_reasoning_effort: Option<bool>,
    pub max_tokens_field: Option<MaxTokensField>,
    pub thinking_format: Option<ThinkingFormat>,
    pub supports_strict_mode: Option<bool>,
    pub zai_tool_stream: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelOptions {
    pub reasoning: bool,
    pub thinking_level_map: Option<BTreeMap<ThinkingLevel, Option<String>>>,
    pub compat: Option<OpenAiCompat>,
}

// Neither model records nor the file store implement Debug: both may hold keys.
pub struct Model {
    pub provider: String,
    pub api: String,
    pub base_url: String,
    pub id: String,
    pub max_tokens: u32,
    key_source: Option<String>,
    env_key: Option<&'static str>,
    options: ModelOptions,
}

impl Model {
    /// Metadata options explicitly selected by this model record.
    pub fn options(&self) -> &ModelOptions {
        &self.options
    }
}

pub struct Config {
    directory: PathBuf,
    models: Value,
    settings: Value,
}

/// Nahida owns its default home. Sharing someone else's directory is an
/// explicit host choice via --config-dir, not automatic Pi discovery.
pub fn default_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(dir).join("nahida"));
    }
    std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .map(|home| PathBuf::from(home).join(".config/nahida"))
        .ok_or(Error("set --config-dir or HOME/XDG_CONFIG_HOME"))
}

fn read_json(path: &Path) -> Result<Value> {
    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({}));
        }
        Err(_) => return Err(Error("cannot read configuration file")),
    };
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| Error("cannot decode configuration file"))?;
    if text.len() as u64 > MAX_BYTES {
        return Err(Error("configuration file is too large"));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| Error("invalid JSON"))?;
    if !value.is_object() {
        return Err(Error("expected a JSON object"));
    }
    Ok(value)
}

fn string<'a>(value: &'a Value, field: &str) -> Result<Option<&'a str>> {
    match value.get(field) {
        None => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s)),
        _ => Err(Error("invalid string field")),
    }
}

fn unsupported_options(value: &Value, model: bool) -> Result<()> {
    for field in
        ["headers", "authHeader", "oauth", "samplingParams", "samplingParamsByThinkingLevel"]
    {
        if value.get(field).is_some() {
            return Err(Error("selected provider/model uses options not implemented yet"));
        }
    }
    if !model && value.get("compat").is_some() {
        return Err(Error("provider-level compat options are not implemented yet"));
    }
    if !model && value.get("thinkingLevelMap").is_some() {
        return Err(Error("provider-level thinkingLevelMap is not implemented yet"));
    }
    Ok(())
}

fn bool_option(value: &Value, field: &str) -> Result<Option<bool>> {
    match value.get(field) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(Error("invalid compat boolean")),
    }
}

fn parse_compat(value: &Value) -> Result<Option<OpenAiCompat>> {
    let Some(value) = value.get("compat") else {
        return Ok(None);
    };
    let object = value.as_object().ok_or(Error("compat must be an object"))?;
    for field in object.keys() {
        if ![
            "supportsStore",
            "supportsDeveloperRole",
            "supportsReasoningEffort",
            "maxTokensField",
            "thinkingFormat",
            "supportsStrictMode",
            "zaiToolStream",
        ]
        .contains(&field.as_str())
        {
            return Err(Error("selected model uses unsupported compat options"));
        }
    }
    let max_tokens_field = match value.get("maxTokensField") {
        None => None,
        Some(Value::String(field)) if field == "max_tokens" => Some(MaxTokensField::MaxTokens),
        _ => return Err(Error("unsupported maxTokensField")),
    };
    let thinking_format = match value.get("thinkingFormat") {
        None => None,
        Some(Value::String(format)) if format == "zai" => Some(ThinkingFormat::Zai),
        _ => return Err(Error("unsupported thinkingFormat")),
    };
    Ok(Some(OpenAiCompat {
        supports_store: bool_option(value, "supportsStore")?,
        supports_developer_role: bool_option(value, "supportsDeveloperRole")?,
        supports_reasoning_effort: bool_option(value, "supportsReasoningEffort")?,
        max_tokens_field,
        thinking_format,
        supports_strict_mode: bool_option(value, "supportsStrictMode")?,
        zai_tool_stream: bool_option(value, "zaiToolStream")?,
    }))
}

fn parse_options(value: &Value) -> Result<ModelOptions> {
    unsupported_options(value, true)?;
    let reasoning = match value.get("reasoning") {
        None => false,
        Some(Value::Bool(reasoning)) => *reasoning,
        _ => return Err(Error("invalid reasoning metadata")),
    };
    let thinking_level_map = match value.get("thinkingLevelMap") {
        None => None,
        Some(value) => {
            let map = value.as_object().ok_or(Error("thinkingLevelMap must be an object"))?;
            let mut result = BTreeMap::new();
            for (name, value) in map {
                let level = match name.as_str() {
                    "off" => ThinkingLevel::Off,
                    "minimal" => ThinkingLevel::Minimal,
                    "low" => ThinkingLevel::Low,
                    "medium" => ThinkingLevel::Medium,
                    "high" => ThinkingLevel::High,
                    "xhigh" => ThinkingLevel::XHigh,
                    "max" => ThinkingLevel::Max,
                    _ => return Err(Error("unsupported thinking level")),
                };
                let mapped = match value {
                    Value::Null => None,
                    Value::String(mapped) if !mapped.is_empty() => Some(mapped.clone()),
                    _ => return Err(Error("thinkingLevelMap values must be strings or null")),
                };
                result.insert(level, mapped);
            }
            Some(result)
        }
    };
    Ok(ModelOptions { reasoning, thinking_level_map, compat: parse_compat(value)? })
}

impl Config {
    /// Reads metadata only. No auth.json access, directory creation or HTTP.
    pub fn load(directory: &Path) -> Result<Self> {
        let models = read_json(&directory.join("models.json"))?;
        if models.get("providers").is_some_and(|value| !value.is_object()) {
            return Err(Error("providers must be an object"));
        }
        let settings = read_json(&directory.join("settings.json"))?;
        Ok(Self { directory: directory.to_owned(), models, settings })
    }

    pub fn default_provider(&self) -> Result<Option<&str>> {
        string(&self.settings, "defaultProvider")
    }

    pub fn default_model(&self) -> Result<Option<&str>> {
        string(&self.settings, "defaultModel")
    }

    pub fn has_provider(&self, provider: &str) -> bool {
        self.models.get("providers").and_then(|p| p.get(provider)).is_some()
    }

    /// Resolve an ordinary provider/model selection. Custom providers need a
    /// configured model; known providers can supply their existing defaults.
    pub fn model(
        &self,
        provider: &str,
        model: Option<&str>,
        defaults: Option<&Defaults>,
    ) -> Result<Model> {
        let entry =
            self.models.get("providers").and_then(|p| p.get(provider)).unwrap_or(&Value::Null);
        if self.has_provider(provider) && !entry.is_object() {
            return Err(Error("provider configuration must be an object"));
        }
        unsupported_options(entry, false)?;
        if entry.get("modelOverrides").is_some() {
            return Err(Error("modelOverrides are not implemented yet"));
        }
        let id = model
            .or_else(|| defaults.as_ref().map(|d| d.model.as_str()))
            .filter(|id| !id.is_empty())
            .ok_or(Error("select a model with --model or settings.json"))?;
        let models = match entry.get("models") {
            None => None,
            Some(Value::Array(models)) => Some(models),
            _ => return Err(Error("models must be an array")),
        };
        let mut selected = None;
        if let Some(models) = models {
            for item in models {
                if !item.is_object() || string(item, "id")?.is_none() {
                    return Err(Error("model entry requires an id"));
                }
                if string(item, "id")? == Some(id) && selected.replace(item).is_some() {
                    return Err(Error("duplicate model id"));
                }
            }
        }
        let selected = selected.unwrap_or(&Value::Null);
        if defaults.is_none() && selected.is_null() {
            return Err(Error("model is not configured for this provider"));
        }
        let options = parse_options(selected)?;
        let api = string(selected, "api")?
            .or(string(entry, "api")?)
            .or_else(|| defaults.as_ref().map(|d| d.api))
            .ok_or(Error("provider requires an API type"))?;
        let base_url = string(selected, "baseUrl")?
            .or(string(entry, "baseUrl")?)
            .or_else(|| defaults.as_ref().map(|d| d.base_url.as_str()))
            .ok_or(Error("provider requires a baseUrl"))?;
        let max_tokens = match selected.get("maxTokens") {
            None => defaults.as_ref().map_or(32_000, |d| d.max_tokens),
            Some(value) => value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or(Error("invalid maxTokens"))?,
        };
        Ok(Model {
            provider: provider.to_owned(),
            api: api.to_owned(),
            base_url: base_url.to_owned(),
            id: id.to_owned(),
            max_tokens,
            key_source: string(entry, "apiKey")?.map(str::to_owned),
            env_key: defaults.as_ref().map(|d| d.env_key),
            options,
        })
    }

    /// Read only the selected credential. Read-only API-key slice; no refresh
    /// or write. Stored OAuth is never downgraded to a configured/ambient key.
    pub fn api_key(
        &self,
        model: &Model,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<Option<String>> {
        let auth = read_json(&self.directory.join("auth.json"))?;
        let source = if let Some(stored) = auth.get(&model.provider) {
            if string(stored, "type")? != Some("api_key") {
                return Err(Error(
                    "stored OAuth refresh is not implemented yet; no fallback attempted",
                ));
            }
            if stored.get("env").is_some() {
                return Err(Error("credential-scoped env is not implemented yet"));
            }
            string(stored, "key")?.ok_or(Error("stored API key is missing"))?
        } else if let Some(source) = &model.key_source {
            source
        } else {
            // An already resolved ambient key is literal, not a command or
            // template. Leave legacy Bearer/OAuth handling to its consumer.
            return Ok(model.env_key.and_then(env).filter(|s| !s.is_empty()));
        };
        Ok(Some(resolve_key(source, env)?))
    }
}

fn resolve_key(source: &str, env: &impl Fn(&str) -> Option<String>) -> Result<String> {
    if source.starts_with('!') {
        return Err(Error("credential commands are not supported; no command executed"));
    }
    let mut result = String::new();
    let mut rest = source;
    while let Some((prefix, suffix)) = rest.split_once('$') {
        result.push_str(prefix);
        if let Some(tail) = suffix.strip_prefix('$').or_else(|| suffix.strip_prefix('!')) {
            result.push(suffix.chars().next().expect("escape character"));
            rest = tail;
            continue;
        }
        let (name, tail) = if let Some(braced) = suffix.strip_prefix('{') {
            if let Some(pair) = braced.split_once('}') {
                pair
            } else {
                result.push('$');
                rest = suffix;
                continue;
            }
        } else {
            let end = suffix
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(suffix.len());
            suffix.split_at(end)
        };
        if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            result.push('$');
            rest = suffix;
            continue;
        }
        result.push_str(
            &env(name)
                .filter(|s| !s.is_empty())
                .ok_or(Error("credential environment variable is unset"))?,
        );
        rest = tail;
    }
    result.push_str(rest);
    if result.is_empty() {
        return Err(Error("empty API key"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_does_not_execute_commands_or_reinterpret_env_values() {
        let env = |name: &str| (name == "KEY").then(|| "fake!$literal".to_owned());
        assert_eq!(
            resolve_key("pre-${KEY}-$KEY", &env).unwrap(),
            "pre-fake!$literal-fake!$literal"
        );
        assert!(resolve_key("!touch forbidden", &env).is_err());
        assert!(resolve_key("${MISSING}", &env).is_err());
        assert_eq!(
            resolve_key("$!literal-$$-${bad-name}-$3-${unclosed", &env).unwrap(),
            "!literal-$-${bad-name}-$3-${unclosed"
        );
    }

    #[test]
    fn defaults_create_no_files_or_auth_reads() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        let model = config
            .model(
                "local",
                None,
                Some(&Defaults {
                    api: "openai-completions",
                    base_url: "http://localhost:1234".into(),
                    model: "test-model".into(),
                    max_tokens: 1024,
                    env_key: "KEY",
                }),
            )
            .unwrap();
        assert_eq!(model.id, "test-model");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn configured_model(models_json: &str) -> Result<Model> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("models.json"), models_json).unwrap();
        Config::load(dir.path())?.model("zai-coding-cn", Some("glm-5.3-flash"), None)
    }

    #[test]
    fn selected_pi_model_options_preserve_supported_metadata_and_provider_overlay() {
        let model = configured_model(
            r#"{"providers":{"zai-coding-cn":{"api":"openai-completions","baseUrl":"http://localhost","models":[{"id":"glm-5.3-flash","reasoning":true,"thinkingLevelMap":{"off":null,"minimal":null,"low":"low","medium":null,"high":"high","xhigh":null,"max":"max"},"compat":{"supportsStore":false,"supportsDeveloperRole":false,"supportsReasoningEffort":true,"maxTokensField":"max_tokens","thinkingFormat":"zai","supportsStrictMode":true,"zaiToolStream":true}}]}}}"#,
        )
        .unwrap();
        assert!(model.options().reasoning);
        assert_eq!(
            model.options().thinking_level_map.as_ref().unwrap(),
            &BTreeMap::from([
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("low".into())),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high".into())),
                (ThinkingLevel::XHigh, None),
                (ThinkingLevel::Max, Some("max".into())),
            ])
        );
        assert_eq!(
            model.options().compat.as_ref().unwrap(),
            &OpenAiCompat {
                supports_store: Some(false),
                supports_developer_role: Some(false),
                supports_reasoning_effort: Some(true),
                max_tokens_field: Some(MaxTokensField::MaxTokens),
                thinking_format: Some(ThinkingFormat::Zai),
                supports_strict_mode: Some(true),
                zai_tool_stream: Some(true),
            }
        );
        assert_eq!(model.api, "openai-completions");
        assert_eq!(model.base_url, "http://localhost");
    }

    #[test]
    fn selected_model_options_keep_explicit_false_and_null_distinct_from_absence() {
        let model = configured_model(
            r#"{"providers":{"zai-coding-cn":{"api":"openai-completions","baseUrl":"http://localhost","models":[{"id":"glm-5.3-flash","compat":{"supportsStore":false},"thinkingLevelMap":{"off":null}}]}}}"#,
        )
        .unwrap();
        let options = model.options();
        assert!(!options.reasoning);
        assert_eq!(options.compat.as_ref().unwrap().supports_store, Some(false));
        assert_eq!(options.compat.as_ref().unwrap().supports_developer_role, None);
        assert_eq!(
            options.thinking_level_map.as_ref().unwrap().get(&ThinkingLevel::Off),
            Some(&None)
        );
        assert!(!options.thinking_level_map.as_ref().unwrap().contains_key(&ThinkingLevel::Low));
    }

    #[test]
    fn malformed_and_unsupported_selected_options_fail() {
        for model_fields in [
            r#""reasoning":"yes""#,
            r#""thinkingLevelMap":{"low":4}"#,
            r#""thinkingLevelMap":{"unknown":"low"}"#,
            r#""compat":{"headers":{"x":"y"}}"#,
            r#""compat":{"maxTokensField":"max_completion_tokens"}"#,
            r#""compat":{"thinkingFormat":"other"}"#,
            r#""headers":{"x":"y"}"#,
        ] {
            let json = format!(
                r#"{{"providers":{{"zai-coding-cn":{{"api":"openai-completions","baseUrl":"http://localhost","models":[{{"id":"glm-5.3-flash",{model_fields}}}]}}}}}}"#
            );
            assert!(configured_model(&json).is_err(), "accepted {model_fields}");
        }
    }

    #[test]
    fn provider_level_compat_options_are_rejected_instead_of_assumed_inherited() {
        let result = configured_model(
            r#"{"providers":{"zai-coding-cn":{"api":"openai-completions","baseUrl":"http://localhost","compat":{"supportsStore":false},"models":[{"id":"glm-5.3-flash"}]}}}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn invalid_json_does_not_echo_contents_and_size_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(&path, "{secret").unwrap();
        assert!(!Config::load(dir.path()).err().unwrap().to_string().contains("secret"));
        std::fs::write(path, " ".repeat(2 * 1024 * 1024 + 1)).unwrap();
        assert!(Config::load(dir.path()).err().unwrap().to_string().contains("too large"));
    }
}

//! Construct existing transports from resolved settings. Files and their
//! formats belong to nahida-config, not this provider layer.

use async_trait::async_trait;
use futures_util::StreamExt as _;

use crate::client::{Auth, Client};
use crate::{
    Dialect, Error, EventStream, OpenAiCompletionsProvider, Profile, Provider, Request, Result,
};

/// Validate transport metadata without resolving credentials or making HTTP.
pub fn profile(
    provider: &str,
    api: &str,
    base_url: &str,
    model: &str,
    max_tokens: u32,
) -> Result<Profile> {
    if !matches!(api, "anthropic-messages" | "openai-completions") {
        return Err(Error::Configuration(
            "API not implemented yet; supported: anthropic-messages, openai-completions",
        ));
    }
    let url = reqwest::Url::parse(base_url).map_err(|_| Error::Configuration("invalid baseUrl"))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Configuration(
            "baseUrl requires HTTPS (or loopback HTTP), without credentials/query/fragment",
        ));
    }
    Ok(Profile {
        name: provider.to_owned(),
        base_url: base_url.to_owned(),
        dialect: if api == "anthropic-messages" {
            Dialect::infer(base_url)
        } else {
            Dialect::Compat
        },
        default_model: model.to_owned(),
        default_max_tokens: max_tokens,
    })
}

pub fn resolve(
    provider: &str,
    api: &str,
    profile: Profile,
    key: String,
) -> Result<Box<dyn Provider>> {
    let provider: Box<dyn Provider> = match api {
        "openai-completions" => Box::new(OpenAiCompletionsProvider::new(key, profile)?),
        "anthropic-messages" => {
            let auth = if provider == "zai" {
                Auth::Bearer { token: key, oauth: false }
            } else {
                Auth::ApiKey(key)
            };
            Box::new(Client::new(auth, profile)?)
        }
        _ => return Err(Error::Configuration("API not implemented yet; no fallback attempted")),
    };
    Ok(Box::new(RedactedProvider(provider)))
}

// A configured endpoint can echo credentials in errors. Preserve retry and
// overflow classifications using fixed messages, not arbitrary server text.
struct RedactedProvider(Box<dyn Provider>);

fn redacted(error: Error) -> Error {
    let overflow = error.is_context_overflow();
    let message = if overflow { "prompt is too long" } else { "provider request failed" };
    match error {
        Error::Api { status, .. } => {
            Error::Api { status, kind: "provider_error".into(), message: message.into() }
        }
        Error::Transport(error) => Error::Transport(error.without_url()),
        Error::Stream { kind, .. } => Error::Stream {
            kind: if matches!(kind.as_str(), "overloaded_error" | "rate_limit_error" | "api_error")
            {
                kind
            } else {
                "provider_error".into()
            },
            message: message.into(),
        },
        Error::Decode { .. } => Error::Configuration("provider response could not be decoded"),
        other => other,
    }
}

#[async_trait]
impl Provider for RedactedProvider {
    fn profile(&self) -> &Profile {
        self.0.profile()
    }

    async fn stream(&self, request: &Request) -> Result<EventStream> {
        let stream = self.0.stream(request).await.map_err(redacted)?;
        Ok(Box::pin(stream.map(|event| event.map_err(redacted))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_redaction_keeps_retry_and_overflow_behavior() {
        for error in [
            Error::Api { status: 429, kind: "secret".into(), message: "secret".into() },
            Error::Api {
                status: 400,
                kind: "secret".into(),
                message: "prompt is too long: secret".into(),
            },
            Error::Stream { kind: "overloaded_error".into(), message: "secret".into() },
        ] {
            let retry = error.is_retryable();
            let overflow = error.is_context_overflow();
            let error = redacted(error);
            assert_eq!(error.is_retryable(), retry);
            assert_eq!(error.is_context_overflow(), overflow);
            assert!(!error.to_string().contains("secret"));
        }
    }
}

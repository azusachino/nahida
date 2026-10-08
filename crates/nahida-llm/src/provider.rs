//! Which provider to talk to at all, and the abstraction that lets the loop
//! not care.
//!
//! `nahida-agent` only ever depends on [`Provider`] — never on a concrete
//! HTTP client — so adding a provider never reaches into the loop. This is
//! deliberately small next to something like `earendil-works/pi`'s `pi-ai`
//! package: pluggable wire formats and a declarative registry, not pi-ai's
//! OAuth framework, model catalogs, dynamic model refresh, or cost tracking.
//! Two wire formats exist because two are needed (Anthropic Messages,
//! `OpenAI` Chat Completions); a third gets added the same way, not
//! speculatively.

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;

use crate::client::{self, Auth, Client, Dialect};
use crate::openai::OpenAiCompletionsProvider;
use crate::stream::StreamEvent;
use crate::types::Request;
use crate::{Error, Profile, Result};

pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

/// A wire format the loop can send a canonical [`Request`] through and get
/// canonical [`StreamEvent`]s back. Each implementation owns the translation
/// to and from its own provider's actual JSON shape entirely; the loop never
/// sees it.
#[async_trait]
pub trait Provider: Send + Sync {
    fn profile(&self) -> &Profile;

    async fn stream(&self, req: &Request) -> Result<EventStream>;
}

/// Lets [`resolve`]'s `Box<dyn Provider>` — the environment-chosen case,
/// which could be any wire format — go straight into `Agent::new` the same
/// way a concrete, known provider does.
#[async_trait]
impl Provider for Box<dyn Provider> {
    fn profile(&self) -> &Profile {
        (**self).profile()
    }

    async fn stream(&self, req: &Request) -> Result<EventStream> {
        (**self).stream(req).await
    }
}

/// Which wire format a registry entry's provider speaks. `AnthropicMessages`
/// carries its own [`Dialect`] — the feature-subset question is orthogonal to
/// which JSON shape is on the wire at all. `OpenAiCompletions` has no
/// `Dialect` of its own; see [`Profile::dialect`]'s doc comment for what a
/// non-Anthropic-wire profile puts there.
#[derive(Debug, Clone, Copy)]
enum Wire {
    AnthropicMessages(Dialect),
    OpenAiCompletions,
}

/// One Bearer-auth, single-env-var provider. First-party Anthropic isn't
/// here — it needs two possible env vars and an OAuth flag `resolve` handles
/// directly — but everything else (a coding-plan gateway, a compatible
/// endpoint with its own dedicated key) fits this shape.
struct ProviderEntry {
    name: &'static str,
    env_key: &'static str,
    base_url: &'static str,
    wire: Wire,
    default_model: &'static str,
    default_max_tokens: u32,
}

const REGISTRY: &[ProviderEntry] = &[
    ProviderEntry {
        name: "zai",
        env_key: "ZAI_API_KEY",
        base_url: "https://api.z.ai/api/anthropic",
        wire: Wire::AnthropicMessages(Dialect::Compat),
        // `glm-5.1` is the model `refs/crush` exercises in its own agent
        // tests, so it is known to work against an Anthropic-shaped coding
        // agent. `glm-5.2` is newer (1M context); `glm-5` caps output at ~20k.
        default_model: "glm-5.1",
        default_max_tokens: 32_000,
    },
    ProviderEntry {
        name: "zai-coding-cn",
        env_key: "ZAI_CODING_CN_API_KEY",
        // Confirmed against `earendil-works/pi`'s own `zai-coding-cn.ts`
        // provider — a *different domain* from the global `zai` entry above
        // (`open.bigmodel.cn`, not `api.z.ai`), speaking OpenAI Chat
        // Completions rather than Anthropic Messages. There is no evidence
        // this domain has a working Anthropic-compatible path at all; pi's
        // own maintainers don't use one for it.
        base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
        wire: Wire::OpenAiCompletions,
        default_model: "glm-5.3",
        default_max_tokens: 32_000,
    },
];

/// Resolve a provider from the environment. First match wins:
///
/// | Variable | Provider |
/// |---|---|
/// | `ANTHROPIC_API_KEY` | first-party Anthropic |
/// | `ANTHROPIC_AUTH_TOKEN` | first-party via OAuth, or a gateway |
/// | `ZAI_API_KEY` | Z.ai coding plan (GLM), global |
/// | `ZAI_CODING_CN_API_KEY` | Z.ai coding plan (GLM), China region |
///
/// `ANTHROPIC_BASE_URL` overrides the base URL and `NAHIDA_DIALECT`
/// (`anthropic` \| `compat`) overrides the inferred dialect — for any entry
/// whose wire format is Anthropic Messages; a registry entry speaking a
/// different wire format has no `Dialect` for the override to adjust, and the
/// override is skipped for it entirely.
pub fn resolve() -> Result<Box<dyn Provider>> {
    resolve_named(None)
}

/// Select a provider explicitly, or retain environment precedence with `None`.
/// A named provider never falls back to another provider's credentials.
/// `chatgpt` is reserved but unavailable until official sign-in is implemented.
pub fn resolve_named(name: Option<&str>) -> Result<Box<dyn Provider>> {
    let selected = select(name, &env_value)?;
    let profile = selected.profile(&env_value);
    let auth = selected.auth(&env_value)?;
    match selected {
        Selected::Registered(ProviderEntry { wire: Wire::OpenAiCompletions, .. }) => {
            let Auth::Bearer { token, .. } = auth else { unreachable!() };
            Ok(Box::new(OpenAiCompletionsProvider::new(token, profile)?))
        }
        _ => Ok(Box::new(Client::new(auth, profile)?)),
    }
}

/// Inspect effective defaults without constructing a client or resolving auth.
/// Explicit selection works without credentials. Auto selection checks which
/// credential variables are nonempty, preserving [`resolve`]'s precedence.
/// No token store, refresh, or network is involved. Endpoint overrides can
/// contain secrets: callers must not print `Profile::base_url` verbatim.
pub fn inspect_profile(name: Option<&str>) -> Result<Profile> {
    Ok(select(name, &env_value)?.profile(&env_value))
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

#[derive(Clone, Copy)]
enum Selected {
    Anthropic,
    Registered(&'static ProviderEntry),
}

fn select(name: Option<&str>, env: &impl Fn(&str) -> Option<String>) -> Result<Selected> {
    match name {
        Some("anthropic") => Ok(Selected::Anthropic),
        Some("chatgpt") => Err(Error::ProviderUnavailable("chatgpt")),
        Some(name) => REGISTRY
            .iter()
            .find(|entry| entry.name == name)
            .map(Selected::Registered)
            .ok_or_else(|| Error::UnknownProvider(name.to_string())),
        None if env("ANTHROPIC_API_KEY").is_some() || env("ANTHROPIC_AUTH_TOKEN").is_some() => {
            Ok(Selected::Anthropic)
        }
        None => REGISTRY
            .iter()
            .find(|entry| env(entry.env_key).is_some())
            .map(Selected::Registered)
            .ok_or(Error::NoCredentials),
    }
}

impl Selected {
    fn profile(self, env: &impl Fn(&str) -> Option<String>) -> Profile {
        let Self::Registered(entry) = self else {
            let mut profile = client::anthropic_profile();
            apply_overrides(env, &mut profile);
            return profile;
        };
        let mut profile = Profile {
            name: entry.name,
            base_url: entry.base_url.to_string(),
            // Non-Anthropic profiles retain the informational Compat placeholder.
            dialect: match entry.wire {
                Wire::AnthropicMessages(dialect) => dialect,
                Wire::OpenAiCompletions => Dialect::Compat,
            },
            default_model: entry.default_model.to_string(),
            default_max_tokens: entry.default_max_tokens,
        };
        if matches!(entry.wire, Wire::AnthropicMessages(_)) {
            apply_overrides(env, &mut profile);
        }
        profile
    }

    fn auth(self, env: &impl Fn(&str) -> Option<String>) -> Result<Auth> {
        match self {
            Self::Anthropic => {
                if let Some(key) = env("ANTHROPIC_API_KEY") {
                    return Ok(Auth::ApiKey(key));
                }
                env("ANTHROPIC_AUTH_TOKEN")
                    .map(|token| Auth::Bearer { token, oauth: env("ANTHROPIC_BASE_URL").is_none() })
                    .ok_or(Error::MissingProviderCredentials {
                        provider: "anthropic",
                        variables: "ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN",
                    })
            }
            Self::Registered(entry) => env(entry.env_key)
                .map(|token| Auth::Bearer { token, oauth: false })
                .ok_or(Error::MissingProviderCredentials {
                    provider: entry.name,
                    variables: entry.env_key,
                }),
        }
    }
}

/// `ANTHROPIC_BASE_URL`/`NAHIDA_DIALECT`, applied after whichever
/// Anthropic-Messages entry matched — the generic escape hatch for any other
/// Anthropic-compatible gateway, regardless of which registry entry (if any)
/// provided the key.
fn apply_overrides(env: &impl Fn(&str) -> Option<String>, profile: &mut Profile) {
    if let Some(url) = env("ANTHROPIC_BASE_URL") {
        profile.dialect = Dialect::infer(&url);
        profile.base_url = url;
        profile.name = "custom";
    }
    if let Some(d) = env("NAHIDA_DIALECT").as_deref().and_then(Dialect::parse) {
        profile.dialect = d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        |key| values.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_string())
    }

    #[test]
    fn named_cn_ignores_other_credentials_and_anthropic_overrides() {
        let env = env(&[
            ("ANTHROPIC_API_KEY", "fake-anthropic"),
            ("ZAI_API_KEY", "fake-global"),
            ("ANTHROPIC_BASE_URL", "https://secret.example/token"),
            ("NAHIDA_DIALECT", "anthropic"),
        ]);
        let selected = select(Some("zai-coding-cn"), &env).unwrap();
        let profile = selected.profile(&env);
        assert_eq!(profile.name, "zai-coding-cn");
        assert_eq!(profile.default_model, "glm-5.3");
        assert_eq!(profile.base_url, "https://open.bigmodel.cn/api/coding/paas/v4");
        assert_eq!(profile.dialect, Dialect::Compat);
        let error = selected.auth(&env).unwrap_err();
        assert!(matches!(
            error,
            Error::MissingProviderCredentials { provider: "zai-coding-cn", .. }
        ));
        assert!(!error.is_retryable());
    }

    #[test]
    fn auto_selection_preserves_each_precedence_level() {
        let keys =
            ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ZAI_API_KEY", "ZAI_CODING_CN_API_KEY"];
        let names = ["anthropic", "anthropic", "zai", "zai-coding-cn"];
        for (index, expected) in names.iter().enumerate() {
            let values: Vec<_> = keys[index..].iter().map(|key| (*key, "fake-key")).collect();
            let env = env(&values);
            assert_eq!(select(None, &env).unwrap().profile(&env).name, *expected);
        }
        assert!(matches!(select(None, &env(&[])), Err(Error::NoCredentials)));
    }

    #[test]
    fn explicit_metadata_does_not_read_credentials() {
        let metadata_env = |key: &str| {
            assert!(matches!(key, "ANTHROPIC_BASE_URL" | "NAHIDA_DIALECT"), "read {key}");
            None
        };
        for name in ["anthropic", "zai", "zai-coding-cn"] {
            let profile = select(Some(name), &metadata_env).unwrap().profile(&metadata_env);
            assert_eq!(profile.name, name);
        }
    }

    #[test]
    fn anthropic_auth_keeps_api_key_precedence_and_oauth_header_policy() {
        let both =
            env(&[("ANTHROPIC_API_KEY", "fake-key"), ("ANTHROPIC_AUTH_TOKEN", "fake-token")]);
        assert!(matches!(Selected::Anthropic.auth(&both).unwrap(), Auth::ApiKey(_)));
        let token = env(&[("ANTHROPIC_AUTH_TOKEN", "fake-token")]);
        assert!(matches!(
            Selected::Anthropic.auth(&token).unwrap(),
            Auth::Bearer { oauth: true, .. }
        ));
        let gateway = env(&[
            ("ANTHROPIC_AUTH_TOKEN", "fake-token"),
            ("ANTHROPIC_BASE_URL", "http://localhost:8080"),
        ]);
        assert!(matches!(
            Selected::Anthropic.auth(&gateway).unwrap(),
            Auth::Bearer { oauth: false, .. }
        ));
    }

    #[test]
    fn global_glm_keeps_anthropic_endpoint_and_dialect_overrides() {
        let env = env(&[
            ("ANTHROPIC_BASE_URL", "http://localhost:8080"),
            ("NAHIDA_DIALECT", "anthropic"),
        ]);
        let profile = select(Some("zai"), &env).unwrap().profile(&env);
        assert_eq!(profile.name, "custom");
        assert_eq!(profile.base_url, "http://localhost:8080");
        assert_eq!(profile.dialect, Dialect::Anthropic);
        assert_eq!(profile.default_model, "glm-5.1");
    }

    #[test]
    fn unavailable_and_unknown_providers_do_not_attempt_fallback() {
        let no_env_reads = |key: &str| panic!("unexpected env read: {key}");
        assert!(matches!(
            select(Some("chatgpt"), &no_env_reads),
            Err(Error::ProviderUnavailable("chatgpt"))
        ));
        assert!(matches!(select(Some("missing"), &no_env_reads), Err(Error::UnknownProvider(_))));
        for error in
            [Error::ProviderUnavailable("chatgpt"), Error::UnknownProvider("missing".into())]
        {
            assert!(!error.is_retryable());
            assert!(!error.is_context_overflow());
        }
    }
}

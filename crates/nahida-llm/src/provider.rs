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
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());

    // First-party Anthropic is special-cased: it's reachable via two
    // different env vars, and only the OAuth-token path ever sends the
    // `oauth-2025-04-20` beta header. Nothing in the declarative registry
    // below needs that nuance.
    let anthropic = if let Some(key) = env("ANTHROPIC_API_KEY") {
        Some(Auth::ApiKey(key))
    } else {
        env("ANTHROPIC_AUTH_TOKEN").map(|token| {
            let oauth = env("ANTHROPIC_BASE_URL").is_none();
            Auth::Bearer { token, oauth }
        })
    };

    if let Some(auth) = anthropic {
        let mut profile = client::anthropic_profile();
        apply_overrides(&env, &mut profile);
        return Ok(Box::new(Client::new(auth, profile)?));
    }

    for entry in REGISTRY {
        let Some(key) = env(entry.env_key) else { continue };
        let mut profile = Profile {
            name: entry.name,
            base_url: entry.base_url.to_string(),
            // A wire format with no `Dialect` of its own gets `Compat` as a
            // placeholder — informational only (the `--verbose` print, and
            // `Client`-specific field-stripping neither this profile nor its
            // provider ever runs) rather than a new `Option<Dialect>` shape
            // change rippling through every caller for one field nobody
            // outside `Client` reads on this path.
            dialect: match entry.wire {
                Wire::AnthropicMessages(dialect) => dialect,
                Wire::OpenAiCompletions => Dialect::Compat,
            },
            default_model: entry.default_model.to_string(),
            default_max_tokens: entry.default_max_tokens,
        };

        return match entry.wire {
            Wire::AnthropicMessages(_) => {
                apply_overrides(&env, &mut profile);
                Ok(Box::new(Client::new(Auth::Bearer { token: key, oauth: false }, profile)?))
            }
            Wire::OpenAiCompletions => Ok(Box::new(OpenAiCompletionsProvider::new(key, profile)?)),
        };
    }

    Err(Error::NoCredentials)
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

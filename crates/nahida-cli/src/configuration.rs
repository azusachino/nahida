//! Host composition: file configuration below, transport construction above.

use anyhow::{Result, bail};
use nahida_config::{Config, Defaults, Model};
use nahida_llm::{Profile, Provider, configured, provider};

use crate::Cli;

pub struct Selection {
    config: Config,
    provider: Option<String>,
    model: Option<Model>,
}

impl Selection {
    pub fn load(cli: &Cli) -> Result<Self> {
        let directory = cli.config_dir.clone().map_or_else(nahida_config::default_dir, Ok)?;
        let config = Config::load(&directory)?;
        let name = cli.provider.as_deref().or(config.default_provider()?);
        let model = if let Some(name) = name {
            let defaults = model_defaults(name);
            if defaults.is_none() && !config.has_provider(name) && name != "chatgpt" {
                return Err(nahida_llm::Error::UnknownProvider(name.to_owned()).into());
            }
            if config.has_provider(name) || config.default_provider()?.is_some() {
                let id = cli.model.as_deref().or(if config.default_provider()? == Some(name) {
                    config.default_model()?
                } else {
                    None
                });
                Some(config.model(name, id, defaults.as_ref())?)
            } else {
                None
            }
        } else {
            None
        };
        let provider = name.map(str::to_owned);
        Ok(Self { config, provider, model })
    }

    pub fn inspect(&self) -> Result<Profile> {
        if let Some(model) = &self.model {
            let mut profile = configured::profile(
                &model.provider,
                &model.api,
                &model.base_url,
                &model.id,
                model.max_tokens,
            )?;
            if let Some((defaults, api, _)) = provider::configuration_defaults(&model.provider)
                && api == model.api
                && defaults.base_url == model.base_url
            {
                profile.dialect = defaults.dialect;
            }
            Ok(profile)
        } else {
            Ok(provider::inspect_profile(self.provider.as_deref())?)
        }
    }

    pub fn resolve(&self) -> Result<Box<dyn Provider>> {
        // Only runtime resolution touches auth.json, including for a built-in
        // provider without a models.json entry. Metadata never stats the store.
        let built_in;
        let model = if let Some(model) = &self.model {
            Some(model)
        } else if let Some(name) = &self.provider
            && let Some(defaults) = model_defaults(name)
        {
            built_in = self.config.model(name, None, Some(&defaults))?;
            Some(&built_in)
        } else {
            None
        };
        if let Some(model) = model {
            let profile = self.inspect()?;
            if let Some(key) = self.config.api_key(model, &|name| std::env::var(name).ok())? {
                return Ok(configured::resolve(&model.provider, &model.api, profile, key)?);
            }
            if self.config.has_provider(&model.provider) {
                bail!("no API key for the selected configured provider; no fallback attempted");
            }
            // Preserve the existing named provider's Bearer policy and missing
            // key diagnostic; never fall back to another provider's key.
        }
        Ok(provider::resolve_named(self.provider.as_deref())?)
    }
}

fn model_defaults(name: &str) -> Option<Defaults> {
    provider::configuration_defaults(name).map(|(profile, api, env_key)| Defaults {
        api,
        env_key,
        base_url: profile.base_url,
        model: profile.default_model,
        max_tokens: profile.default_max_tokens,
    })
}

mod login_workflow;
mod oauth;
mod responses;

use crate::{
    config::Config,
    core::{
        AssistantMessageEvent, Context, InputModality, Model, ModelCost, StreamOptions, now_ms,
    },
    provider::{AuthError, Provider},
    ui::Ui,
};
use std::collections::BTreeMap;

pub use login_workflow::login_and_save;
pub use oauth::{ChatGptLogin, Credential, begin_login, exchange_callback, exchange_code, refresh};
pub use responses::stream;

pub struct OpenAiChatGpt {
    config: Config,
    credential: Option<Credential>,
}

impl OpenAiChatGpt {
    pub fn init(config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let credential = config
            .provider("openai")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        Ok(Self { config, credential })
    }
}

impl Provider for OpenAiChatGpt {
    fn auth_refresh(&mut self) -> Result<(), AuthError> {
        let Some(stored) = &self.credential else {
            return Err(AuthError::NotLoggedIn);
        };
        if stored.expires > now_ms() {
            return Ok(());
        }
        let credential = refresh(stored).map_err(|error| AuthError::Provider(Box::new(error)))?;
        self.config.set_provider(
            "openai",
            serde_json::to_value(&credential)
                .map_err(|error| AuthError::Provider(Box::new(error)))?,
        );
        self.config
            .save()
            .map_err(|error| AuthError::Provider(Box::new(error)))?;
        self.credential = Some(credential);
        Ok(())
    }

    fn auth_is_valid(&self) -> bool {
        self.credential
            .as_ref()
            .is_some_and(|credential| credential.expires > now_ms())
    }

    fn auth_login(&mut self, ui: &mut dyn Ui) -> Result<(), Box<dyn std::error::Error>> {
        self.credential = Some(login_and_save(&mut self.config, ui)?);
        Ok(())
    }

    fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<Vec<AssistantMessageEvent>, Box<dyn std::error::Error>> {
        responses::stream(
            &self
                .credential
                .as_ref()
                .expect("ChatGPT is not signed in")
                .access,
            model,
            context,
            options,
        )
    }

    fn get_models(&self) -> BTreeMap<String, Model> {
        let model = Model {
            id: "gpt-6-luna".into(),
            name: "GPT-6 Luna".into(),
            api: "openai-responses".into(),
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            input: vec![InputModality::Text, InputModality::Image],
            input_limits: None,
            cost: ModelCost::default(),
            model_type: None,
            reasoning: true,
            thinking_level_map: None,
            prompt_cache: None,
            context_window: 272_000,
            max_tokens: 128_000,
            headers: None,
            sampling_params: None,
            compat: None,
        };
        BTreeMap::from([(model.name.clone(), model)])
    }
}

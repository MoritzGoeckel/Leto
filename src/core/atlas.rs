use crate::{
    config::Config,
    core::{Context, Message, StreamOptions, UserContent, UserMessage, tools::ToolRuntime},
    plugins::PluginManager,
    provider::{AuthError, Provider, openai_chatgpt::OpenAiChatGpt},
    ui::{self, Ui},
};
use std::sync::{Arc, Mutex};

pub struct Atlas {
    ui: Arc<Mutex<ui::tui::Tui>>,
    plugins: Arc<Mutex<PluginManager>>,
    provider: OpenAiChatGpt,
    tools: ToolRuntime,
}

impl Atlas {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = Arc::new(Config::load()?);
        let ui = Arc::new(Mutex::new(ui::tui::Tui));
        let plugins = Arc::new(Mutex::new(PluginManager::new()));
        ui::tui::Tui::register_plugin_methods(Arc::clone(&ui), &mut plugins.lock().unwrap());
        plugins.lock().unwrap().start(&config)?;
        let mut provider = OpenAiChatGpt::init((*config).clone())?;
        if let Err(error) = provider.auth_refresh() {
            match error {
                AuthError::NotLoggedIn => provider.auth_login(&mut *ui.lock().unwrap())?,
                error => return Err(error.into()),
            }
        }
        let mut tools = ToolRuntime::new(Arc::clone(&ui), config, Arc::clone(&plugins));
        tools.add_tools(crate::core::tools::make_default_tools());
        Ok(Self {
            ui,
            plugins,
            provider,
            tools,
        })
    }

    pub fn start(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.plugins.lock().unwrap().init_plugins()?;
        self.plugins.lock().unwrap().notify_init()?;
        let model = self
            .provider
            .get_models()
            .into_values()
            .next()
            .expect("provider has no models");
        let mut context = Context {
            tools: Some(
                self.tools
                    .tools
                    .values()
                    .map(|tool| tool.definition.clone())
                    .collect(),
            ),
            ..Context::default()
        };
        self.plugins.lock().unwrap().notify_new_conversation()?;
        let notice_id = self
            .ui
            .lock()
            .unwrap()
            .inform_blocking("Signed in. Enter a message, or /exit to quit.");
        self.ui.lock().unwrap().close(notice_id);
        loop {
            let input = self.ui.lock().unwrap().get_input("you> ")?;
            if input == "/exit" {
                break;
            }
            if let Err(error) = self.provider.auth_refresh() {
                match error {
                    AuthError::NotLoggedIn => {
                        self.provider.auth_login(&mut *self.ui.lock().unwrap())?
                    }
                    error => return Err(error.into()),
                }
            }
            let user_message = UserMessage {
                content: UserContent::Text(input),
                timestamp: crate::core::now_ms(),
            };
            let user_message = Message::User(
                self.plugins
                    .lock()
                    .unwrap()
                    .transform_user_message(user_message)?,
            );
            self.ui.lock().unwrap().add_message(&user_message);
            context.messages.push(user_message);
            let events = self
                .provider
                .stream(&model, &context, &StreamOptions::default())?;
            let message = events
                .into_iter()
                .find_map(|event| match event {
                    crate::core::AssistantMessageEvent::Done { message, .. } => Some(message),
                    crate::core::AssistantMessageEvent::Error { error, .. } => Some(error),
                    _ => None,
                })
                .expect("Responses API returned no assistant message");
            self.plugins
                .lock()
                .unwrap()
                .notify_assistant_message(&message)?;
            let assistant_message = Message::Assistant(message.clone());
            self.ui.lock().unwrap().add_message(&assistant_message);
            context.messages.push(assistant_message);
            for result in self.tools.run_tool_calls(message) {
                let result = Message::ToolResult(result);
                // TODO: Notify tool result
                // TODO: Transform tool result
                self.ui.lock().unwrap().add_message(&result);
                context.messages.push(result);
            }
        }
        self.plugins.lock().unwrap().notify_exit()?;
        Ok(())
    }
}

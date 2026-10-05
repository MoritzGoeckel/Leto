use crate::{
    config::Config,
    core::{Context, Message, StreamOptions, UserContent, UserMessage, tools::ToolRuntime},
    plugins::PluginManager,
    provider::{AuthError, Provider, openai_chatgpt::OpenAiChatGpt},
    ui::{self, Ui},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct Loop {
    ui: Arc<Mutex<ui::tui::Tui>>,
    plugins: Arc<Mutex<PluginManager>>,
    provider: OpenAiChatGpt,
    tools: ToolRuntime,
    context: Context,
    exit: bool,
    commands: HashMap<String, fn(&mut Self) -> Result<(), Box<dyn std::error::Error>>>,
}

impl Loop {
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
        tools.add_tools(crate::core::tools::buildin::make_default_tools());
        let context = Context {
            tools: Some(
                tools
                    .tools
                    .values()
                    .map(|tool| tool.definition.clone())
                    .collect(),
            ),
            ..Context::default()
        };
        Ok(Self {
            ui,
            plugins,
            provider,
            tools,
            context,
            exit: false,
            commands: Self::make_commands(),
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
        self.plugins.lock().unwrap().notify_new_conversation()?;
        let notice_id = self
            .ui
            .lock()
            .unwrap()
            .inform_blocking("Signed in. Enter a message, or /exit to quit.");
        self.ui.lock().unwrap().close(notice_id);
        while !self.exit {
            let input = self.ui.lock().unwrap().get_input("you> ")?;
            if input.starts_with('/') {
                self.run_command(&input)?;
                continue;
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
            self.context.messages.push(user_message);
            loop {
                let events =
                    self.provider
                        .stream(&model, &self.context, &StreamOptions::default())?;
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
                let has_tool_calls = message
                    .content
                    .iter()
                    .any(|content| matches!(content, crate::core::AssistantContent::ToolCall(_)));
                let assistant_message = Message::Assistant(message.clone());
                self.ui.lock().unwrap().add_message(&assistant_message);
                self.context.messages.push(assistant_message);
                for result in self.tools.run_tool_calls(message) {
                    let result = self.plugins.lock().unwrap().transform_tool_result(result)?;
                    let result = Message::ToolResult(result);
                    self.ui.lock().unwrap().add_message(&result);
                    self.context.messages.push(result);
                }
                if !has_tool_calls {
                    break;
                }
            }
        }
        self.plugins.lock().unwrap().notify_exit()?;
        Ok(())
    }

    fn make_commands() -> HashMap<String, fn(&mut Self) -> Result<(), Box<dyn std::error::Error>>> {
        HashMap::from([
            ("exit".to_owned(), Self::exit_command as _),
            ("clear".to_owned(), Self::clear_command as _),
        ])
    }

    fn run_command(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let name = input[1..].split_whitespace().next().unwrap_or("");
        match self.commands.get(name) {
            Some(command) => command(self),
            None => {
                self.ui
                    .lock()
                    .unwrap()
                    .inform_blocking(&format!("Unknown command: /{name}"));
                Ok(())
            }
        }
    }

    fn exit_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.exit = true;
        Ok(())
    }

    fn clear_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.context.messages.clear();
        Ok(())
    }
}

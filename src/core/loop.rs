use crate::{
    core::{
        Context, Message, StreamOptions, UserContent, UserMessage,
        events::{EventLog, EventValue},
        tools::ToolRuntime,
    },
    plugins::PluginManager,
    provider::{AuthError, Provider, openai_chatgpt::OpenAiChatGpt},
    ui::{AskOptions, Ui},
};
use std::fs;
use std::io;
use std::sync::{Arc, Mutex};

pub struct Loop {
    pub(super) ui: Arc<Mutex<dyn Ui>>,
    pub(super) plugins: Arc<Mutex<PluginManager>>,
    pub(super) config: crate::config::Config,
    pub(super) provider: OpenAiChatGpt,
    pub(super) model: crate::core::Model,
    pub(super) tools: ToolRuntime,
    pub(super) context: Context,
    pub(super) exit: bool,
    pub(super) commands:
        std::collections::HashMap<String, fn(&mut Self) -> Result<(), Box<dyn std::error::Error>>>,
    pub(super) reasoning: Option<crate::core::ThinkingLevel>,
    pub(super) events: EventLog,
}

impl Loop {
    pub fn new(
        ui: Arc<Mutex<dyn Ui>>,
        config: Arc<crate::config::Config>,
        plugins: Arc<Mutex<PluginManager>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut provider = OpenAiChatGpt::init((*config).clone())?;
        if let Err(error) = provider.auth_refresh() {
            match error {
                AuthError::NotLoggedIn => {
                    let mut ui = ui.lock().unwrap();
                    provider.auth_login(&mut *ui)?;
                }
                error => return Err(error.into()),
            }
        }
        let models = provider.get_models();
        let model_config = config.model();
        let configured = model_config.is_some();
        let model = model_config
            .as_ref()
            .map(|config| models[&config.model].clone())
            .unwrap_or_else(|| models.into_values().next().expect("provider has no models"));
        let mut tools =
            ToolRuntime::new(Arc::clone(&ui), Arc::clone(&config), Arc::clone(&plugins));
        tools.add_tools(crate::core::tools::buildin::make_default_tools());
        let agents_md = Self::load_agents_md();
        if let Some((path, _)) = &agents_md {
            ui.lock()
                .unwrap()
                .append_message_str(&format!("Loaded {}", path.display()));
        }
        ui.lock().unwrap().append_message_str(&format!(
            "Using {}model: {} {} {}",
            if configured { "" } else { "default " },
            model.provider,
            model.id,
            model_config
                .as_ref()
                .and_then(|config| config.reasoning.as_ref())
                .map(|level| format!("{level:?}").to_lowercase())
                .unwrap_or_else(|| "default".to_owned())
        ));
        let mut loop_state = Self {
            ui,
            plugins,
            config: (*config).clone(),
            provider,
            model,
            tools,
            context: Context::default(),
            exit: false,
            commands: crate::core::commands::make_commands(),
            reasoning: model_config.and_then(|config| config.reasoning),
            events: EventLog::create()?,
        };
        crate::core::commands::clear(&mut loop_state)?;
        Ok(loop_state)
    }

    pub fn start(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.plugins.lock().unwrap().init_plugins()?;
        self.plugins.lock().unwrap().notify_init()?;
        self.plugins.lock().unwrap().notify_new_conversation()?;
        while !self.exit {
            let input = match self.ui.lock().unwrap().ask_styled(AskOptions {
                form_text: "instructions...".to_owned(),
                ..AskOptions::default()
            }) {
                Ok(input) => input,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
                Err(error) => return Err(error.into()),
            };
            if is_command(&input) {
                self.ui.lock().unwrap().append_command(&input);
                self.events
                    .append(EventValue::Command(input.clone()), false)?;
                self.run_command(&input)?;
                continue;
            }
            if let Err(error) = self.provider.auth_refresh() {
                match error {
                    AuthError::NotLoggedIn => {
                        let mut ui = self.ui.lock().unwrap();
                        self.provider.auth_login(&mut *ui)?;
                    }
                    error => return Err(error.into()),
                }
            }
            let user_message = UserMessage {
                content: UserContent::Text(input),
                timestamp: crate::core::now_ms(),
            };
            let user_message = self
                .plugins
                .lock()
                .unwrap()
                .transform_user_message(user_message)?;
            self.events.append(
                EventValue::UserMessage(serde_json::to_value(&user_message)?),
                true,
            )?;
            let user_message = Message::User(user_message);
            self.ui.lock().unwrap().append_message(&user_message);
            self.context.messages.push(user_message);
            self.ui.lock().unwrap().start_working();
            let mut options = StreamOptions::default();
            options.reasoning = self.reasoning.clone();
            loop {
                let events = self.provider.stream(
                    &self.model,
                    &self.context,
                    &options,
                    &mut *self.ui.lock().unwrap(),
                )?;
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
                self.events.append(
                    EventValue::AssistantMessage(serde_json::to_value(&message)?),
                    true,
                )?;
                let assistant_message = Message::Assistant(message.clone());
                self.ui.lock().unwrap().append_message(&assistant_message);
                self.context.messages.push(assistant_message);
                for result in self.tools.run_tool_calls(message) {
                    let result = self.plugins.lock().unwrap().transform_tool_result(result)?;
                    self.events
                        .append(EventValue::ToolResult(serde_json::to_value(&result)?), true)?;
                    let result = Message::ToolResult(result);
                    self.ui.lock().unwrap().append_message(&result);
                    self.context.messages.push(result);
                }
                if !has_tool_calls {
                    self.ui.lock().unwrap().stop_working();
                    break;
                }
            }
        }
        self.plugins.lock().unwrap().notify_exit()?;
        Ok(())
    }

    pub(super) fn load_agents_md() -> Option<(std::path::PathBuf, String)> {
        let mut directory = std::env::current_dir().ok()?;
        loop {
            let path = directory.join("AGENTS.md");
            if let Ok(content) = fs::read_to_string(&path) {
                return Some((path, content));
            }
            if !directory.pop() {
                return None;
            }
        }
    }

    fn run_command(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error>> {
        let name = input[1..].split_whitespace().next().unwrap_or("");
        match self.commands.get(name) {
            Some(command) => command(self),
            None => {
                self.ui
                    .lock()
                    .unwrap()
                    .append_message_str(&format!("Unknown command: /{name}"));
                Ok(())
            }
        }
    }
}

fn is_command(input: &str) -> bool {
    input
        .strip_prefix('/')
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase()))
}

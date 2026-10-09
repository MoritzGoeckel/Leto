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
use ratatui::style::Color;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::{Arc, Mutex};

pub struct Loop {
    ui: Arc<Mutex<dyn Ui>>,
    plugins: Arc<Mutex<PluginManager>>,
    provider: OpenAiChatGpt,
    tools: ToolRuntime,
    context: Context,
    exit: bool,
    commands: HashMap<String, fn(&mut Self) -> Result<(), Box<dyn std::error::Error>>>,
    events: EventLog,
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
        let mut tools = ToolRuntime::new(Arc::clone(&ui), config, Arc::clone(&plugins));
        tools.add_tools(crate::core::tools::buildin::make_default_tools());
        let agents_md = Self::load_agents_md();
        if let Some((path, _)) = &agents_md {
            ui.lock()
                .unwrap()
                .append_message_str(&format!("Loaded {}", path.display()));
        }
        let mut loop_state = Self {
            ui,
            plugins,
            provider,
            tools,
            context: Context::default(),
            exit: false,
            commands: Self::make_commands(),
            events: EventLog::create()?,
        };
        loop_state.clear_command()?;
        Ok(loop_state)
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
        while !self.exit {
            let input = match self.ui.lock().unwrap().ask(AskOptions {
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

    fn make_commands() -> HashMap<String, fn(&mut Self) -> Result<(), Box<dyn std::error::Error>>> {
        HashMap::from([
            ("exit".to_owned(), Self::exit_command as _),
            ("clear".to_owned(), Self::clear_command as _),
            ("resume".to_owned(), Self::resume_command as _),
        ])
    }

    fn load_agents_md() -> Option<(std::path::PathBuf, String)> {
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

    fn exit_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.exit = true;
        Ok(())
    }

    fn resume_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let conversations = crate::core::events::list_conversations()?
            .into_iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>();
        if conversations.is_empty() {
            self.ui
                .lock()
                .unwrap()
                .append_message_str("No saved conversations");
            return Ok(());
        }
        let names = conversations
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy())
            .collect::<Vec<_>>();
        let (answer, mut context, events) = loop {
            let mut ui = self.ui.lock().unwrap();
            ui.append_message_str(&format!("{}", names.join("\n")));
            let answer = ui.ask(AskOptions {
                background: Color::Rgb(45, 39, 26),
                form_text: "Conversation filename...".to_owned(),
                ..AskOptions::default()
            })?;
            drop(ui);
            let Some(path) = conversations
                .iter()
                .find(|path| path.file_name().unwrap().to_str().unwrap() == answer)
            else {
                self.ui
                    .lock()
                    .unwrap()
                    .append_message_str("Conversation not found");
                continue;
            };
            let loaded = EventLog::read_context(path)
                .and_then(|context| EventLog::open(path).map(|events| (context, events)));
            match loaded {
                Ok((context, events)) => break (answer, context, events),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.ui
                        .lock()
                        .unwrap()
                        .append_message_str("Conversation not found");
                }
                Err(error) => return Err(error.into()),
            }
        };
        context.tools = self.context.tools.clone();
        self.context = context;
        self.events = events;
        let mut ui = self.ui.lock().unwrap();
        ui.append_message_str(&format!("Resumed {answer}"));
        let earlier_messages = self.context.messages.len().saturating_sub(5);
        if earlier_messages > 0 {
            ui.append_message_str(&format!("{earlier_messages} earlier messages"));
        }
        for message in self.context.messages.iter().skip(earlier_messages) {
            ui.append_message(message);
        }
        Ok(())
    }

    fn clear_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let agents_md = Self::load_agents_md();
        self.context = Context {
            system_prompt: agents_md.map(|(_, content)| content),
            tools: Some(
                self.tools
                    .tools
                    .values()
                    .map(|tool| tool.definition.clone())
                    .collect(),
            ),
            ..Context::default()
        };
        if let Some(system_prompt) = &self.context.system_prompt {
            self.events
                .append(EventValue::SystemPrompt(system_prompt.clone()), false)?;
        }
        Ok(())
    }
}

fn is_command(input: &str) -> bool {
    input
        .strip_prefix('/')
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase()))
}

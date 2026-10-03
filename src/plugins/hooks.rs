macro_rules! hooks {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum Hook {
            $($variant),+
        }

        impl Hook {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub fn from_str(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

hooks! {
    OnInit => "on_init",
    OnNewConversation => "on_new_conversation",
    OnUserMessage => "on_user_message",
    OnAssistantMessage => "on_assistant_message",
    OnExit => "on_exit",
}

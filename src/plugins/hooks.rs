#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Hook {
    OnStart,
}

impl Hook {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnStart => "on_start",
        }
    }

    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "on_start" => Some(Self::OnStart),
            _ => None,
        }
    }
}

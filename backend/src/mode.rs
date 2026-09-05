use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Production,
    Development,
    Demo,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Development => "development",
            Self::Demo => "demo",
        }
    }

    pub const fn is_demo(self) -> bool {
        matches!(self, Self::Demo)
    }
}

impl Serialize for Mode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AiProvider { #[default] OpenAi, Claude, Gemini }
impl AiProvider {
    pub const ALL: [Self; 3] = [Self::OpenAi, Self::Claude, Self::Gemini];
    pub fn label(self) -> &'static str { match self { Self::OpenAi => "OpenAI", Self::Claude => "Claude", Self::Gemini => "Gemini" } }
}

/// Only explicit settings serialization and authentication may expose this value.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct ApiKey(Zeroizing<String>);
impl ApiKey {
    pub fn new(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.len() > 4096 || !value.bytes().all(|b| (33..=126).contains(&b)) {
            return Err("API keys must contain at most 4096 printable ASCII characters, without spaces.".into());
        }
        Ok(Self(Zeroizing::new(value.to_owned())))
    }
    pub fn expose(&self) -> &str { &self.0 }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
}
impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("[redacted]") }
}
impl Serialize for ApiKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(self.expose()) }
}
impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ProviderSettings {
    #[serde(skip_serializing_if = "ApiKey::is_empty")]
    pub api_key: ApiKey,
    pub model: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AiPreferences {
    pub default_provider: AiProvider,
    pub openai: ProviderSettings,
    pub claude: ProviderSettings,
    pub gemini: ProviderSettings,
}
impl AiPreferences {
    pub fn settings(&self, provider: AiProvider) -> &ProviderSettings {
        match provider { AiProvider::OpenAi => &self.openai, AiProvider::Claude => &self.claude, AiProvider::Gemini => &self.gemini }
    }
    pub fn settings_mut(&mut self, provider: AiProvider) -> &mut ProviderSettings {
        match provider { AiProvider::OpenAi => &mut self.openai, AiProvider::Claude => &mut self.claude, AiProvider::Gemini => &mut self.gemini }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_redacted_and_validation_does_not_repeat_input() {
        let key = ApiKey::new(" fake-secret-value\n").unwrap();
        assert_eq!(key.expose(), "fake-secret-value");
        assert!(!format!("{key:?}").contains("fake-secret"));
        assert!(ApiKey::new("header\r\ninjection").is_err());
        assert!(ApiKey::new(&"x".repeat(4097)).is_err());
    }
}

#[path = "ai_storage.rs"]
mod storage;
pub(crate) use storage::AiSettingsStore;
